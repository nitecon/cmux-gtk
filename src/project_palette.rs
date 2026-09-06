//! Searchable project-action palette backed by the shared resolver and execution path.

use crate::app_state::AppStateRef;
use gtk4::{glib, prelude::*};
use std::rc::Rc;
use uuid::Uuid;

#[derive(Clone)]
struct Item {
    id: String,
    fingerprint: String,
    title: String,
    subtitle: String,
    disclosure: String,
    workspace: Uuid,
}

/// Show one modal palette for the active window and resolve its local project actions off GTK.
pub fn show(parent: &gtk4::ApplicationWindow, state: &AppStateRef) {
    if let Some(existing) = parent
        .application()
        .into_iter()
        .flat_map(|app| app.windows())
        .find(|window| window.title().as_deref() == Some("Command Palette"))
    {
        existing.present();
        return;
    }
    let dialog = gtk4::Dialog::builder()
        .title("Command Palette")
        .transient_for(parent)
        .modal(true)
        .default_width(620)
        .default_height(480)
        .build();
    dialog.add_button("Close", gtk4::ResponseType::Close);
    dialog.connect_response(|dialog, _| dialog.close());
    let content = dialog.content_area();
    content.set_spacing(8);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    let search = gtk4::SearchEntry::builder()
        .placeholder_text("Search project actions")
        .build();
    content.append(&search);
    let status = gtk4::Label::new(Some("Loading project actions…"));
    status.set_xalign(0.0);
    status.add_css_class("dim-label");
    content.append(&status);
    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);
    content.append(
        &gtk4::ScrolledWindow::builder()
            .vexpand(true)
            .child(&list)
            .build(),
    );
    dialog.present();
    search.grab_focus();

    let (response, receiver) = tokio::sync::oneshot::channel();
    let trace_id = Uuid::new_v4().to_string();
    crate::diagnostics::record(
        "project.palette.open",
        serde_json::json!({"trace_id":trace_id.clone()}),
    );
    crate::socket::project::list(
        state,
        None,
        serde_json::Value::Null,
        response,
        Some(trace_id),
    );
    glib::MainContext::default().spawn_local({
        let dialog = dialog.downgrade();
        let search = search.downgrade();
        let status = status.downgrade();
        let list = list.downgrade();
        let state = Rc::downgrade(state);
        async move {
            let (Some(dialog), Some(search), Some(status), Some(list), Some(state)) = (
                dialog.upgrade(),
                search.upgrade(),
                status.upgrade(),
                list.upgrade(),
                state.upgrade(),
            ) else {
                return;
            };
            let Ok(reply) = receiver.await else {
                status.set_text("Project action lookup was cancelled.");
                return;
            };
            let Some(result) = reply.get("result") else {
                status.set_text(
                    reply
                        .pointer("/error/message")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("Project actions could not be loaded."),
                );
                return;
            };
            let Some(workspace) = result
                .get("workspace_id")
                .and_then(serde_json::Value::as_str)
                .and_then(|value| Uuid::parse_str(value).ok())
            else {
                status.set_text("Project action lookup returned an invalid workspace.");
                return;
            };
            let items = action_items(result, workspace);
            status.set_text(&format!("{} project actions", items.len()));
            let rows = Rc::new(
                items
                    .into_iter()
                    .map(|item| {
                        let (row, button) = action_row(&dialog, &state, &status, item.clone());
                        let haystack =
                            format!("{} {} {}", item.id, item.title, item.subtitle).to_lowercase();
                        list.append(&row);
                        (row, button, haystack)
                    })
                    .collect::<Vec<_>>(),
            );
            search.connect_search_changed({
                let rows = rows.clone();
                move |entry| {
                    let query = entry.text().trim().to_lowercase();
                    for (row, _, haystack) in rows.iter() {
                        row.set_visible(query.is_empty() || haystack.contains(&query));
                    }
                }
            });
            search.connect_activate(move |_| {
                if let Some((_, button, _)) = rows.iter().find(|(row, _, _)| row.is_visible()) {
                    button.emit_clicked();
                }
            });
        }
    });
}

/// Extract bounded, already validated action records intended for palette presentation.
fn action_items(result: &serde_json::Value, workspace: Uuid) -> Vec<Item> {
    let commands = result
        .pointer("/config/commands")
        .and_then(serde_json::Value::as_object);
    result
        .pointer("/config/actions")
        .and_then(serde_json::Value::as_object)
        .into_iter()
        .flat_map(|actions| actions.iter())
        .filter(|(_, action)| {
            action
                .pointer("/definition/palette")
                .and_then(serde_json::Value::as_bool)
                != Some(false)
                && action
                    .pointer("/intent/type")
                    .and_then(serde_json::Value::as_str)
                    != Some("metadata")
        })
        .filter_map(|(id, action)| {
            let definition = action.get("definition")?;
            let fingerprint = action.get("fingerprint")?.as_str()?.to_owned();
            let title = definition
                .get("title")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(id)
                .to_owned();
            let subtitle = definition
                .get("subtitle")
                .or_else(|| definition.get("description"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let source = action
                .get("source")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("project configuration");
            let named = action
                .pointer("/intent/name")
                .and_then(serde_json::Value::as_str)
                .and_then(|name| commands?.get(name));
            let disclosure = serde_json::to_string_pretty(&serde_json::json!({
                "action_id": id,
                "source": source,
                "definition": definition,
                "workspace_command": named,
            }))
            .unwrap_or_else(|_| format!("Run '{id}' from {source}?"));
            Some(Item {
                id: id.to_owned(),
                fingerprint,
                title,
                subtitle,
                disclosure,
                workspace,
            })
        })
        .collect()
}

/// Build a plain-text row; action definitions never become markup.
fn action_row(
    dialog: &gtk4::Dialog,
    state: &AppStateRef,
    status: &gtk4::Label,
    item: Item,
) -> (gtk4::ListBoxRow, gtk4::Button) {
    let row = gtk4::ListBoxRow::new();
    let button = gtk4::Button::new();
    let labels = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    for (text, dim) in [(&item.title, false), (&item.subtitle, true)] {
        if !text.is_empty() {
            let label = gtk4::Label::new(Some(text));
            label.set_xalign(0.0);
            label.set_wrap(true);
            if dim {
                label.add_css_class("dim-label");
            }
            labels.append(&label);
        }
    }
    button.set_child(Some(&labels));
    button.connect_clicked({
        let dialog = dialog.downgrade();
        let state = Rc::downgrade(state);
        let status = status.downgrade();
        move |_| submit(&dialog, &state, &status, item.clone(), false)
    });
    row.set_child(Some(&button));
    (row, button)
}

/// Submit through the same reviewed action runner used by the socket API and CLI.
fn submit(
    dialog: &glib::WeakRef<gtk4::Dialog>,
    state: &std::rc::Weak<std::cell::RefCell<crate::app_state::AppState>>,
    status: &glib::WeakRef<gtk4::Label>,
    item: Item,
    confirmed: bool,
) {
    let (Some(state), Some(status)) = (state.upgrade(), status.upgrade()) else {
        return;
    };
    status.set_text("Running action…");
    let (response, receiver) = tokio::sync::oneshot::channel();
    let trace_id = Uuid::new_v4().to_string();
    crate::socket::project::run(
        &state,
        crate::socket::project::RunRequest {
            workspace: Some(item.workspace),
            action_id: item.id.clone(),
            fingerprint: item.fingerprint.clone(),
            confirmed,
            req_id: serde_json::Value::Null,
            trace_id: Some(trace_id),
        },
        response,
    );
    glib::MainContext::default().spawn_local({
        let dialog = dialog.clone();
        let state = Rc::downgrade(&state);
        let status = status.downgrade();
        async move {
            let Ok(reply) = receiver.await else {
                return;
            };
            if reply.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
                if let Some(dialog) = dialog.upgrade() {
                    dialog.close();
                }
                return;
            }
            let code = reply
                .pointer("/error/code")
                .and_then(serde_json::Value::as_str);
            if code == Some("confirmation_required") && !confirmed {
                confirm(&dialog, &state, &status, item);
            } else if let Some(status) = status.upgrade() {
                status.set_text(
                    reply
                        .pointer("/error/message")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("Project action failed."),
                );
            }
        }
    });
}

/// Collect an explicit user decision without closing or replacing the palette state.
fn confirm(
    dialog: &glib::WeakRef<gtk4::Dialog>,
    state: &std::rc::Weak<std::cell::RefCell<crate::app_state::AppState>>,
    status: &glib::WeakRef<gtk4::Label>,
    item: Item,
) {
    let Some(parent) = dialog.upgrade() else {
        return;
    };
    let prompt = gtk4::MessageDialog::builder()
        .transient_for(&parent)
        .modal(true)
        .text("Confirm Project Action")
        .secondary_text(&item.disclosure)
        .build();
    prompt.add_button("Cancel", gtk4::ResponseType::Cancel);
    prompt.add_button("Run", gtk4::ResponseType::Accept);
    prompt.connect_response({
        let dialog = dialog.clone();
        let state = state.clone();
        let status = status.clone();
        move |prompt, response| {
            prompt.close();
            if response == gtk4::ResponseType::Accept {
                submit(&dialog, &state, &status, item.clone(), true);
            } else if let Some(status) = status.upgrade() {
                status.set_text("Action cancelled.");
            }
        }
    });
    prompt.present();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Palette extraction excludes metadata and explicitly hidden actions without interpreting markup.
    #[test]
    fn extracts_visible_executable_actions() {
        let workspace = Uuid::new_v4();
        let result = serde_json::json!({"config":{"actions":{
            "visible":{"source":"/tmp/cmux.json","fingerprint":"a".repeat(64),
                "definition":{"title":"Build","description":"Compile"},"intent":{"type":"command"}},
            "hidden":{"source":"/tmp/cmux.json","fingerprint":"b".repeat(64),
                "definition":{"palette":false},"intent":{"type":"command"}},
            "metadata":{"source":"/tmp/cmux.json","fingerprint":"c".repeat(64),
                "definition":{"title":"Label"},"intent":{"type":"metadata"}}
        }}});
        let items = action_items(&result, workspace);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "visible");
        assert_eq!(items[0].title, "Build");
        assert_eq!(items[0].subtitle, "Compile");
        assert_eq!(items[0].workspace, workspace);
    }
}
