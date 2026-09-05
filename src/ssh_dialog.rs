#[allow(deprecated)]
use gtk4::prelude::*;
use crate::app_state::AppState;
use std::cell::RefCell;
use std::rc::Rc;

/// Show the SSH connect dialog with autocomplete from saved hosts and SSH config.
pub fn show_ssh_dialog(app: &gtk4::Application, state: Rc<RefCell<AppState>>) {
    let window = app.windows().into_iter().next();

    let dialog = gtk4::Window::builder()
        .title("Connect to SSH Host")
        .modal(true)
        .default_width(400)
        .build();
    dialog.add_css_class("ssh-dialog");

    if let Some(ref parent) = window {
        dialog.set_transient_for(Some(parent));
    }

    // Entry with placeholder
    let entry = gtk4::Entry::new();
    entry.set_placeholder_text(Some("user@host or SSH alias"));

    // EntryCompletion with merged host list
    #[allow(deprecated)]
    {
        let store = gtk4::ListStore::new(&[gtk4::glib::Type::STRING]);
        for host in crate::ssh_hosts::all_hosts() {
            store.set(&store.append(), &[(0, &host)]);
        }
        let completion = gtk4::EntryCompletion::new();
        completion.set_model(Some(&store));
        completion.set_text_column(0);
        completion.set_minimum_key_length(1);
        entry.set_completion(Some(&completion));
    }

    // Layout: vertical box with padding
    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    vbox.set_margin_start(16);
    vbox.set_margin_end(16);
    vbox.set_margin_top(16);
    vbox.set_margin_bottom(16);
    vbox.append(&entry);
    let directory = gtk4::Entry::new();
    directory.set_placeholder_text(Some("Remote folder (optional, e.g. /opt/project)"));
    vbox.append(&directory);
    let error = gtk4::Label::new(None);
    error.set_wrap(true);
    error.add_css_class("error");
    vbox.append(&error);
    let connect = gtk4::Button::with_label("Connect");
    connect.add_css_class("suggested-action");
    connect.set_margin_top(12);
    vbox.append(&connect);
    dialog.set_child(Some(&vbox));

    for field in [&entry, &directory] {
        let connect = connect.downgrade();
        field.connect_activate(move |_| {
            if let Some(connect) = connect.upgrade() { connect.emit_clicked(); }
        });
    }
    connect.connect_clicked({
        let state = state.clone();
        let dialog = dialog.downgrade();
        let entry = entry.clone();
        move |_| {
            let Some(dialog) = dialog.upgrade() else { return; };
            let target = entry.text().to_string().trim().to_string();
            if !target.is_empty() {
                if let Err(message) = crate::workspace::validate_ssh_target(&target) {
                    error.set_text(&message);
                    return;
                }
                let directory = directory.text().trim().to_string();
                if !directory.is_empty() && (!directory.starts_with('/') || directory.contains('\0')) {
                    error.set_text("Use an absolute remote folder path.");
                    return;
                }
                trigger_ssh_connect(&state, target, (!directory.is_empty()).then_some(directory));
                dialog.close();
            }
        }
    });

    // Escape key: close dialog
    let esc_ctrl = gtk4::EventControllerKey::new();
    esc_ctrl.connect_key_pressed({
        let dialog = dialog.downgrade();
        move |_, key, _, _| {
            if key == gtk4::gdk::Key::Escape {
                if let Some(dialog) = dialog.upgrade() { dialog.close(); }
                gtk4::glib::Propagation::Stop
            } else {
                gtk4::glib::Propagation::Proceed
            }
        }
    });
    dialog.add_controller(esc_ctrl);

    // Restore terminal focus after dialog closes
    dialog.connect_destroy({
        let state = state.clone();
        move |_| {
            if let Some(engine) = state.borrow_mut().active_split_engine_mut() {
                engine.focus_active_surface();
            }
        }
    });

    dialog.present();
    entry.grab_focus();
}

/// Create an SSH workspace using the same pattern as the socket handler.
fn trigger_ssh_connect(state: &Rc<RefCell<AppState>>, target: String, directory: Option<String>) {
    let (write_tx, write_rx) = tokio::sync::mpsc::unbounded_channel();
    let (output_tx, _output_rx) = tokio::sync::mpsc::unbounded_channel();
    let bridge = std::sync::Arc::new(crate::ssh::bridge::SshBridge::new(write_tx, write_rx, output_tx));
    *bridge.directory.lock().unwrap() = directory.clone();
    let id = state.borrow_mut().create_remote_workspace(target.clone(), &bridge);
    {
        let mut s = state.borrow_mut();
        let index = s.workspaces.iter().position(|w| w.id == id).unwrap();
        s.workspaces[index].remote_directory = directory;
        if let Some(row) = s.sidebar_list.row_at_index(index as i32) {
            row.set_child(Some(&crate::sidebar::workspace_row_content(&s.workspaces[index])));
            crate::sidebar::style_workspace_row(&row, &s.workspaces[index]);
        }
        s.workspace_bridges.insert(id, bridge.clone());
        s.trigger_session_save();
    }
    let (list, app) = { let s = state.borrow(); (s.sidebar_list.clone(), s.gtk_app.clone()) };
    crate::sidebar::wire_latest_row(&list, state.clone(), &app);

    let ssh_tx = state.borrow().ssh_event_tx.clone();
    let rt_handle = state.borrow().runtime_handle.clone();
    if let (Some(tx), Some(rt)) = (ssh_tx, rt_handle) {
        let handle = rt.spawn(crate::ssh::tunnel::run_ssh_lifecycle(id, target, tx, bridge));
        state.borrow_mut().ssh_task_handles.insert(id, handle);
    }
}
