//! Preferences entry and visible task acceptance for existing interactive agent terminals.
use super::{model::*, submit};
use crate::app_state::AppStateRef;
use gtk4::prelude::*;
use serde_json::json;

/// Add gateway configuration access without changing ordinary terminal preference ownership.
pub fn append_preferences(content: &gtk4::Box, state: &AppStateRef) {
    let button = gtk4::Button::with_label("Gateway tasks…");
    let state = std::rc::Rc::downgrade(state);
    button.connect_clicked(move |button| {
        let Some(state) = state.upgrade() else {
            return;
        };
        let parent = button
            .root()
            .and_then(|r| r.downcast::<gtk4::Window>().ok());
        show(parent.as_ref(), &state);
    });
    content.append(&button);
}

/// Show bounded task state and explicit workspace mapping without moving terminal focus.
fn show(parent: Option<&gtk4::Window>, state: &AppStateRef) {
    let dialog = gtk4::Dialog::builder()
        .title("Gateway tasks")
        .default_width(620)
        .default_height(620)
        .build();
    dialog.set_transient_for(parent);
    dialog.add_button("Close", gtk4::ResponseType::Close);
    dialog.connect_response(|dialog, _| dialog.close());
    let content = dialog.content_area();
    content.set_spacing(12);
    for side in [
        gtk4::PositionType::Left,
        gtk4::PositionType::Right,
        gtk4::PositionType::Top,
        gtk4::PositionType::Bottom,
    ] {
        match side {
            gtk4::PositionType::Left => content.set_margin_start(16),
            gtk4::PositionType::Right => content.set_margin_end(16),
            gtk4::PositionType::Top => content.set_margin_top(16),
            _ => content.set_margin_bottom(16),
        }
    }
    let initial = state
        .borrow()
        .gateway
        .as_ref()
        .map(|g| g.view.borrow().clone())
        .unwrap_or_default();
    let enabled = gtk4::CheckButton::with_label("Enable gateway connection");
    enabled.set_active(initial.config.enabled);
    content.append(&enabled);
    let url = gtk4::Entry::builder()
        .placeholder_text("https://gateway.example.com")
        .text(&initial.config.url)
        .build();
    content.append(&url);
    let key = gtk4::PasswordEntry::builder()
        .placeholder_text("API key (leave blank to keep current key)")
        .show_peek_icon(true)
        .build();
    content.append(&key);
    let help = gtk4::Label::new(Some("Map each local workspace to its exact gateway project identity. Install cmux Claude/Codex hooks. Enable cmux execution for that project in the gateway. New tasks appear here; Send to agent requires confirmation that its prompt is empty."));
    help.set_wrap(true);
    help.set_xalign(0.0);
    content.append(&help);
    let error = gtk4::Label::new(None);
    error.set_wrap(true);
    error.set_xalign(0.0);
    let save = gtk4::Button::with_label("Save connection");
    save.connect_clicked({
        let state = std::rc::Rc::downgrade(state);
        let url = url.clone();
        let enabled = enabled.clone();
        let key = key.clone();
        let error = error.downgrade();
        move |_| {
            let Some(state) = state.upgrade() else {
                return;
            };
            let secret = key.text().to_string();
            let params = json!({"enabled":enabled.is_active(), "url":url.text().as_str(),
                "api_key":if secret.is_empty() { None } else { Some(secret) }});
            key.set_text("");
            respond(submit(&state, "gateway.configure", &params), error.clone());
        }
    });
    content.append(&save);
    let workspaces: Vec<(String, String)> = state
        .borrow()
        .workspaces
        .iter()
        .filter(|w| w.remote_target.is_none())
        .map(|w| (w.uuid.to_string(), w.name.clone()))
        .collect();
    let names: Vec<&str> = workspaces.iter().map(|(_, name)| name.as_str()).collect();
    let chooser = gtk4::DropDown::from_strings(&names);
    let active = state
        .borrow()
        .workspaces
        .get(state.borrow().active_index)
        .map(|w| w.uuid.to_string());
    let index = workspaces
        .iter()
        .position(|(id, _)| Some(id) == active.as_ref())
        .unwrap_or(0);
    chooser.set_selected(index as u32);
    let project = gtk4::Entry::builder()
        .placeholder_text("Gateway project identity (empty removes mapping)")
        .build();
    if let Some((workspace, _)) = workspaces.get(index) {
        if let Some(mapping) = initial
            .config
            .mappings
            .iter()
            .find(|m| &m.workspace_id == workspace)
        {
            project.set_text(&mapping.project_ident);
        }
    }
    chooser.connect_selected_notify({
        let workspaces = workspaces.clone();
        let project = project.clone();
        let state = std::rc::Rc::downgrade(state);
        move |chooser| {
            let Some(state) = state.upgrade() else {
                return;
            };
            let text = workspaces
                .get(chooser.selected() as usize)
                .and_then(|(id, _)| {
                    state
                        .borrow()
                        .gateway
                        .as_ref()?
                        .view
                        .borrow()
                        .config
                        .mappings
                        .iter()
                        .find(|m| &m.workspace_id == id)
                        .map(|m| m.project_ident.clone())
                })
                .unwrap_or_default();
            project.set_text(&text);
        }
    });
    content.append(&chooser);
    content.append(&project);
    let bind = gtk4::Button::with_label("Save workspace mapping");
    bind.connect_clicked({
        let state = std::rc::Rc::downgrade(state);
        let error = error.downgrade();
        move |_| {
            let Some(state) = state.upgrade() else {
                return;
            };
            let Some((workspace, _)) = workspaces.get(chooser.selected() as usize) else {
                return;
            };
            respond(
                submit(
                    &state,
                    "gateway.bind",
                    &json!({"workspace_id":workspace, "project_ident":project.text().as_str()}),
                ),
                error.clone(),
            );
        }
    });
    content.append(&bind);
    content.append(&error);
    let status = gtk4::Label::new(Some(&initial.connection));
    status.set_wrap(true);
    status.set_xalign(0.0);
    content.append(&status);
    let tasks = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    let scroll = gtk4::ScrolledWindow::builder()
        .vexpand(true)
        .min_content_height(160)
        .child(&tasks)
        .build();
    content.append(&scroll);
    let receiver = state.borrow().gateway.as_ref().map(|g| g.view.clone());
    if let Some(mut receiver) = receiver {
        let state = std::rc::Rc::downgrade(state);
        let tasks = tasks.downgrade();
        let status = status.downgrade();
        let weak_dialog = dialog.downgrade();
        let listener = glib::MainContext::default().spawn_local(async move {
            loop {
                let (Some(state), Some(tasks), Some(status), Some(dialog)) = (
                    state.upgrade(),
                    tasks.upgrade(),
                    status.upgrade(),
                    weak_dialog.upgrade(),
                ) else {
                    break;
                };
                if !dialog.is_visible() {
                    break;
                }
                let view = receiver.borrow_and_update().clone();
                status.set_text(&format!(
                    "{} · {} registered sessions",
                    view.connection,
                    view.sessions.len()
                ));
                while let Some(child) = tasks.first_child() {
                    tasks.remove(&child);
                }
                for run in &view.runs {
                    append_run(&tasks, &dialog, &state, run, &view);
                }
                drop((state, tasks, status, dialog));
                if receiver.changed().await.is_err() {
                    break;
                }
            }
        });
        dialog.connect_close_request(move |_| {
            listener.abort();
            glib::Propagation::Proceed
        });
    }
    dialog.present();
}

/// Render assignment status and offer submission only for the exact idle native target.
fn append_run(
    container: &gtk4::Box,
    parent: &gtk4::Dialog,
    state: &AppStateRef,
    run: &Run,
    view: &View,
) {
    let label = gtk4::Label::new(Some(&format!(
        "Task {}\n{} · {} · {}",
        run.assignment.task_id, run.assignment.project_ident, run.status, run.phase
    )));
    label.set_selectable(true);
    label.set_xalign(0.0);
    label.set_wrap(true);
    container.append(&label);
    if run.phase == "offered" && !run.terminal() {
        let send = gtk4::Button::with_label("Send to agent…");
        send.set_sensitive(
            view.connection == "Connected"
                && view
                    .sessions
                    .iter()
                    .any(|s| s.matches(&run.assignment) && s.state == "idle"),
        );
        send.connect_clicked({
            let state = std::rc::Rc::downgrade(state);
            let parent = parent.downgrade();
            let assignment = run.assignment.clone();
            move |_| {
                let (Some(state), Some(parent)) = (state.upgrade(), parent.upgrade()) else {
                    return;
                };
                confirm(&parent, &state, &assignment);
            }
        });
        container.append(&send);
    } else if !run.terminal() {
        let message = gtk4::Label::new(Some(if run.phase == "uncertain" {
            "Delivery needs reconciliation. This prompt will not be sent again. Inspect the terminal and gateway run before reporting an outcome."
        } else {
            "Respond to questions in the same terminal. Use cmux gateway report to record progress or the final outcome."
        }));
        message.set_wrap(true);
        message.set_xalign(0.0);
        container.append(&message);
    }
}

/// Require an explicit empty-prompt confirmation; no provider screen parsing or speculative input.
fn confirm(parent: &gtk4::Dialog, state: &AppStateRef, assignment: &Assignment) {
    let dialog = gtk4::Dialog::builder()
        .title("Send delegated task")
        .transient_for(parent)
        .modal(true)
        .default_width(560)
        .build();
    dialog.add_button("Cancel", gtk4::ResponseType::Cancel);
    let send = dialog.add_button("Send to agent", gtk4::ResponseType::Accept);
    send.set_sensitive(false);
    let preview = gtk4::Label::new(Some(&format!(
        "Project: {}\nSurface: {}\nNative session: {}\n\n{}",
        assignment.project_ident,
        assignment.surface_id,
        assignment.session_id,
        bounded(&assignment.prompt, 4096)
    )));
    preview.set_wrap(true);
    preview.set_selectable(true);
    preview.set_xalign(0.0);
    let scroll = gtk4::ScrolledWindow::builder()
        .max_content_height(300)
        .propagate_natural_height(true)
        .child(&preview)
        .build();
    dialog.content_area().append(&scroll);
    let ready = gtk4::CheckButton::with_label(
        "I confirm this agent is at an empty prompt and ready for this task",
    );
    ready.connect_toggled(move |ready| send.set_sensitive(ready.is_active()));
    dialog.content_area().append(&ready);
    let error = gtk4::Label::new(None);
    error.set_wrap(true);
    dialog.content_area().append(&error);
    dialog.connect_response({
        let state = std::rc::Rc::downgrade(state);
        let run = assignment.run_id.clone();
        let error = error.downgrade();
        move |dialog, response| {
            if response == gtk4::ResponseType::Accept && ready.is_active() {
                if let Some(state) = state.upgrade() {
                    respond(
                        submit(
                            &state,
                            "gateway.accept",
                            &json!({"run_id":run, "confirm_ready":true}),
                        ),
                        error.clone(),
                    );
                }
            }
            dialog.close();
        }
    });
    dialog.present();
}

/// Display durable enqueue results without retaining the originating dialog through async work.
fn respond(
    result: Result<tokio::sync::oneshot::Receiver<Result<serde_json::Value, String>>, String>,
    label: glib::WeakRef<gtk4::Label>,
) {
    glib::MainContext::default().spawn_local(async move {
        let message = match result {
            Ok(receiver) => match receiver.await {
                Ok(Ok(_)) => "Saved / queued".into(),
                Ok(Err(error)) => error,
                Err(_) => "Gateway worker stopped".into(),
            },
            Err(error) => error,
        };
        if let Some(label) = label.upgrade() {
            label.set_text(&message);
        }
    });
}
