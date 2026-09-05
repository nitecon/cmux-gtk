use crate::app_state::AppStateRef;
use gtk4::prelude::*;

/// Present the local workspace creation wizard.
///
/// The selected directory is validated before AppState creates any GTK or
/// Ghostty state, so cancelling or correcting an invalid path has no side
/// effects on the current workspace.
pub fn show_workspace_dialog(app: &gtk4::Application, state: AppStateRef) {
    if let Some(existing) = app
        .windows()
        .into_iter()
        .find(|window| window.title().as_deref() == Some("Create Workspace"))
    {
        existing.present();
        return;
    }

    let dialog = gtk4::Dialog::builder()
        .application(app)
        .title("Create Workspace")
        .modal(true)
        .default_width(560)
        .build();
    if let Some(parent) = app.active_window() {
        dialog.set_transient_for(Some(&parent));
    }

    dialog.add_button("Cancel", gtk4::ResponseType::Cancel);
    dialog.add_button("Create Workspace", gtk4::ResponseType::Accept);
    dialog.set_default_response(gtk4::ResponseType::Accept);

    let content = dialog.content_area();
    content.set_spacing(12);
    content.set_margin_top(18);
    content.set_margin_bottom(18);
    content.set_margin_start(18);
    content.set_margin_end(18);

    let intro = gtk4::Label::new(Some(
        "Choose the folder this workspace should use. New terminals in the workspace will start there.",
    ));
    intro.set_wrap(true);
    intro.set_xalign(0.0);
    content.append(&intro);

    let name_label = gtk4::Label::new(Some("Workspace name"));
    name_label.set_xalign(0.0);
    content.append(&name_label);

    let name_entry = gtk4::Entry::new();
    name_entry.set_placeholder_text(Some("Defaults to the folder name"));
    name_entry.set_activates_default(true);
    content.append(&name_entry);

    let folder_label = gtk4::Label::new(Some("Workspace folder"));
    folder_label.set_xalign(0.0);
    content.append(&folder_label);

    let folder_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let path_entry = gtk4::Entry::new();
    path_entry.set_hexpand(true);
    path_entry.set_placeholder_text(Some("Select or enter a folder path"));
    path_entry.set_activates_default(true);
    let browse_button = gtk4::Button::with_label("Browse…");
    folder_row.append(&path_entry);
    folder_row.append(&browse_button);
    content.append(&folder_row);

    let script_label = gtk4::Label::new(Some("Startup script (optional, POSIX shell)"));
    script_label.set_xalign(0.0);
    content.append(&script_label);
    let script_entry = gtk4::Entry::new();
    script_entry.set_placeholder_text(Some("/path/to/start-workspace.sh"));
    content.append(&script_entry);
    let script_help = gtk4::Label::new(Some("Runs in each terminal, including after restart. Leave empty for a regular shell."));
    script_help.set_wrap(true);
    script_help.set_xalign(0.0);
    script_help.add_css_class("dim-label");
    content.append(&script_help);

    let validation = gtk4::Label::new(Some("Choose an existing folder."));
    validation.set_xalign(0.0);
    validation.add_css_class("dim-label");
    content.append(&validation);

    let create_button = dialog
        .widget_for_response(gtk4::ResponseType::Accept)
        .expect("create workspace response button");
    create_button.set_sensitive(false);

    path_entry.connect_changed({
        let create_button = create_button.clone();
        let validation = validation.clone();
        move |entry| update_path_validation(entry, &create_button, &validation)
    });

    browse_button.connect_clicked({
        let path_entry = path_entry.clone();
        let name_entry = name_entry.clone();
        let dialog = dialog.downgrade();
        move |_| {
            let Some(dialog) = dialog.upgrade() else { return; };
            let chooser = gtk4::FileChooserNative::builder()
                .title("Choose Workspace Folder")
                .action(gtk4::FileChooserAction::SelectFolder)
                .accept_label("Choose")
                .cancel_label("Cancel")
                .modal(true)
                .transient_for(&dialog)
                .build();

            let typed_path = std::path::PathBuf::from(path_entry.text().as_str());
            if typed_path.is_dir() {
                let _ = chooser.set_current_folder(Some(&gtk4::gio::File::for_path(typed_path)));
            }

            chooser.connect_response({
                let path_entry = path_entry.clone();
                let name_entry = name_entry.clone();
                move |chooser, response| {
                    if response == gtk4::ResponseType::Accept {
                        if let Some(path) = chooser.file().and_then(|file| file.path()) {
                            path_entry.set_text(&path.to_string_lossy());
                            if name_entry.text().trim().is_empty() {
                                if let Some(name) =
                                    path.file_name().and_then(|value| value.to_str())
                                {
                                    name_entry.set_text(name);
                                }
                            }
                        }
                    }
                    chooser.destroy();
                }
            });
            chooser.show();
        }
    });

    dialog.connect_response({
        let path_entry = path_entry.clone();
        let name_entry = name_entry.clone();
        let validation = validation.clone();
        let app = app.clone();
        move |dialog, response| {
            if response != gtk4::ResponseType::Accept {
                dialog.close();
                return;
            }

            let path = std::path::PathBuf::from(path_entry.text().as_str());
            let script = script_entry.text();
            let result = if script.trim().is_empty() {
                state.borrow_mut().create_workspace_in(name_entry.text().to_string(), &path)
            } else {
                state.borrow_mut().create_script_workspace(name_entry.text().to_string(), &path, std::path::Path::new(script.as_str()))
            };
            match result {
                Ok(_) => {
                    let sidebar_list = state.borrow().sidebar_list.clone();
                    crate::sidebar::wire_latest_row(&sidebar_list, state.clone(), &app);
                    dialog.close();
                }
                Err(message) => {
                    validation.set_text(&message);
                    validation.add_css_class("error");
                }
            }
        }
    });

    dialog.present();
    browse_button.grab_focus();
}

fn update_path_validation(
    path_entry: &gtk4::Entry,
    create_button: &gtk4::Widget,
    validation: &gtk4::Label,
) {
    let path = std::path::PathBuf::from(path_entry.text().as_str());
    let valid = path.is_dir();
    create_button.set_sensitive(valid);
    validation.remove_css_class("error");
    validation.set_text(if valid {
        "New terminals will start in this folder."
    } else {
        "Choose an existing folder."
    });
}
