//! Global gateway configuration and explicit experimental terminal-injection consent.
use super::submit;
use crate::app_state::AppStateRef;
use gtk4::prelude::*;
use serde_json::json;

/// Build global preferences; the dialog owns its weak status listener and cancels it on close.
pub fn append_preferences(content: &gtk4::Box, state: &AppStateRef, dialog: &gtk4::Dialog) {
    let initial = state
        .borrow()
        .gateway
        .as_ref()
        .map(|g| g.view.borrow().clone())
        .unwrap_or_default();
    let enabled = gtk4::CheckButton::with_label("Enable Agent Gateway");
    enabled.set_active(initial.config.enabled);
    content.append(&enabled);
    let url = gtk4::Entry::builder()
        .placeholder_text("https://gateway.example.com")
        .text(&initial.config.url)
        .build();
    field(content, "Gateway address", &url);
    let key = gtk4::PasswordEntry::builder()
        .placeholder_text("API key (leave blank to keep current key)")
        .show_peek_icon(true)
        .build();
    field(content, "API key", &key);
    let approval = gtk4::CheckButton::with_label("Allow experimental terminal message injection");
    approval.set_active(initial.config.injection_approved);
    content.append(&approval);
    let help = gtk4::Label::new(Some("This approval allows task messages to be typed and submitted into active Claude/Codex terminals across matching workspaces. Projects will match automatically by Git upstream URL. Busy agents and unfinished input wait; terminals without an active agent receive nothing. No per-project setup or gateway hooks are required. The new stream connection is awaiting the gateway specification."));
    help.set_wrap(true);
    help.set_xalign(0.0);
    content.append(&help);
    let error = gtk4::Label::new(None);
    error.set_wrap(true);
    error.set_xalign(0.0);
    let save = gtk4::Button::with_label("Save gateway preferences");
    save.connect_clicked({
        let state = std::rc::Rc::downgrade(state);
        let error = error.downgrade();
        move |_| {
            let Some(state) = state.upgrade() else { return; };
            let secret = key.text().to_string();
            let params = json!({"enabled":enabled.is_active(), "url":url.text().as_str(),
                "injection_approved":approval.is_active(), "api_key":if secret.is_empty() { None } else { Some(secret) }});
            key.set_text("");
            respond(submit(&state, "gateway.configure", &params), error.clone());
        }
    });
    content.append(&save);
    content.append(&error);
    let status = gtk4::Label::new(Some(&initial.connection));
    status.set_wrap(true);
    status.set_xalign(0.0);
    content.append(&status);
    if let Some(mut receiver) = state.borrow().gateway.as_ref().map(|g| g.view.clone()) {
        let status = status.downgrade();
        let listener = glib::MainContext::default().spawn_local(async move {
            loop {
                let Some(status) = status.upgrade() else {
                    break;
                };
                let view = receiver.borrow_and_update().clone();
                status.set_text(&format!(
                    "{} · {} queued messages",
                    view.connection, view.pending
                ));
                drop(status);
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
}

/// Label saved fields independently of placeholders.
fn field(content: &gtk4::Box, text: &str, widget: &impl IsA<gtk4::Widget>) {
    let label = gtk4::Label::new(Some(text));
    label.set_xalign(0.0);
    content.append(&label);
    content.append(widget);
}

/// Show asynchronous save failures without retaining the preferences dialog.
fn respond(
    result: Result<tokio::sync::oneshot::Receiver<Result<serde_json::Value, String>>, String>,
    label: glib::WeakRef<gtk4::Label>,
) {
    glib::MainContext::default().spawn_local(async move {
        let message = match result {
            Ok(receiver) => match receiver.await {
                Ok(Ok(_)) => "Saved".into(),
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
