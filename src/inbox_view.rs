//! Live GTK inbox, refreshed from a coalesced signal and holding only weak model/widget references.
use crate::app_state::AppStateRef;
use crate::inbox::{Action, Scope};
use gtk4::{gio, glib, prelude::*};
use std::rc::Rc;
use uuid::Uuid;

/// Show one nonmodal panel per application state; reuse it when the menu is invoked again.
/// Closing the window aborts its listener, so no task retains hidden widgets or message copies.
pub fn show(parent: &gtk4::ApplicationWindow, state: &AppStateRef) {
    if let Some(window) = state.borrow().inbox_window.upgrade() {
        window.present();
        return;
    }
    let window = gtk4::Dialog::builder()
        .title("Notifications")
        .transient_for(parent)
        .default_width(580)
        .default_height(480)
        .build();
    window.add_button("Close", gtk4::ResponseType::Close);
    window.connect_response(|window, _| window.close());
    let content = window.content_area();
    content.set_spacing(8);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    let status = gtk4::Label::new(None);
    status.set_xalign(0.0);
    content.append(&status);
    let toolbar = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    for (label, action) in [
        ("_Jump to unread", "jump"),
        ("_Mark all read", "mark_all"),
        ("_Clear all", "clear_all"),
    ] {
        toolbar.append(&button(label, action, None));
    }
    content.append(&toolbar);
    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);
    let scroll = gtk4::ScrolledWindow::builder()
        .vexpand(true)
        .child(&list)
        .build();
    content.append(&scroll);
    let group = gio::SimpleActionGroup::new();
    for name in ["open", "dismiss", "jump", "mark_all", "clear_all"] {
        let action = gio::SimpleAction::new(
            name,
            matches!(name, "open" | "dismiss").then_some(glib::VariantTy::STRING),
        );
        action.connect_activate({
            let state = Rc::downgrade(state);
            let status = status.downgrade();
            let window = window.downgrade();
            move |_, parameter| {
                let Some(state) = state.upgrade() else {
                    return;
                };
                let id = parameter
                    .and_then(|value| value.str())
                    .and_then(|value| Uuid::parse_str(value).ok());
                let operation = match name {
                    "open" => id.map(Action::Open),
                    "dismiss" => id.map(|id| Action::Dismiss {
                        id: Some(id),
                        all_read: false,
                    }),
                    "jump" => Some(Action::JumpToUnread),
                    "mark_all" => Some(Action::MarkRead {
                        id: None,
                        scope: Scope::default(),
                        all: true,
                    }),
                    _ => Some(Action::Clear(Scope::default())),
                };
                let Some(operation) = operation else {
                    return;
                };
                let result = crate::inbox_actions::handle(&mut state.borrow_mut(), operation);
                match result {
                    Ok(value)
                        if value.get("opened").and_then(|value| value.as_bool()) == Some(true) =>
                    {
                        if let Some(window) = window.upgrade() {
                            window.close();
                        }
                    }
                    Err((_, message)) => {
                        if let Some(status) = status.upgrade() {
                            status.set_text(message);
                        }
                    }
                    _ => {}
                }
            }
        });
        group.add_action(&action);
    }
    window.insert_action_group("inbox", Some(&group));
    render(&list, &status, &state.borrow());
    let (sender, mut receiver) = tokio::sync::watch::channel(());
    {
        let mut state = state.borrow_mut();
        state.inbox_updates = Some(sender);
        state.inbox_window = window.downgrade();
    }
    let listener = glib::MainContext::default().spawn_local({
        let state = Rc::downgrade(state);
        let list = list.downgrade();
        let status = status.downgrade();
        async move {
            while receiver.changed().await.is_ok() {
                let (Some(state), Some(list), Some(status)) =
                    (state.upgrade(), list.upgrade(), status.upgrade())
                else {
                    break;
                };
                render(&list, &status, &state.borrow());
            }
        }
    });
    window.connect_close_request(move |_| {
        listener.abort();
        glib::Propagation::Proceed
    });
    window.present();
}

/// Construct a window-scoped action button; no row callback retains the application or record.
fn button(label: &str, action: &str, id: Option<Uuid>) -> gtk4::Button {
    let button = gtk4::Button::with_mnemonic(label);
    button.set_action_name(Some(&format!("inbox.{action}")));
    if let Some(id) = id {
        button.set_action_target_value(Some(&id.to_string().to_variant()));
    }
    button
}

/// Rebuild the bounded visible history newest-first, treating notification text as plain text.
fn render(list: &gtk4::ListBox, status: &gtk4::Label, state: &crate::app_state::AppState) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    let records = &state.inbox.records;
    let unread = records.iter().filter(|record| !record.is_read).count();
    if let Some(window) = list.root().and_downcast::<gtk4::Dialog>() {
        window.set_title(Some(&format!("Notifications — {unread} unread")));
    }
    status.set_text(&format!(
        "{} unread · {} messages",
        records.iter().filter(|record| !record.is_read).count(),
        records.len()
    ));
    for record in records.iter().rev() {
        let row = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        row.set_margin_top(8);
        row.set_margin_bottom(8);
        let workspace = state
            .workspaces
            .iter()
            .find(|workspace| workspace.uuid == record.workspace_id)
            .map(|workspace| workspace.name.as_str())
            .unwrap_or("Closed workspace");
        let heading = gtk4::Label::new(Some(&format!(
            "{}{} — {workspace}",
            if record.is_read { "" } else { "Unread · " },
            record.content.title
        )));
        heading.set_xalign(0.0);
        heading.set_wrap(true);
        row.append(&heading);
        for text in [&record.content.subtitle, &record.content.body] {
            if !text.is_empty() {
                let label = gtk4::Label::new(Some(text));
                label.set_xalign(0.0);
                label.set_wrap(true);
                label.set_selectable(true);
                row.append(&label);
            }
        }
        let actions = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        actions.append(&button(
            if record.surface_id.is_some() {
                "Open terminal"
            } else {
                "Open workspace"
            },
            "open",
            Some(record.id),
        ));
        actions.append(&button("Dismiss", "dismiss", Some(record.id)));
        row.append(&actions);
        list.append(&row);
    }
}
