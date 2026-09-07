use gtk4::gio;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

/// Register all GIO actions on the ApplicationWindow.
/// Actions are named "win.{action-name}" and can be invoked by buttons, menus, and shortcuts.
/// Per D-11, D-12: all menu items mirror existing keyboard shortcut actions.
pub fn register_actions(
    window: &gtk4::ApplicationWindow,
    state: Rc<RefCell<crate::app_state::AppState>>,
    sidebar: &gtk4::Box,
    app: &gtk4::Application,
) {
    if !crate::browser::agent_browser_available() {
        eprintln!(
            "cmux: browser tabs disabled; install agent-browser with: npm install -g agent-browser && agent-browser install"
        );
    }
    // --- File section actions ---

    // win.new-workspace (D-01, D-05)
    let action = gio::SimpleAction::new("new-workspace", None);
    action.connect_activate({
        let state = state.clone();
        let app = app.clone();
        move |_, _| {
            crate::shortcuts::handle_new_workspace(&state, &app);
        }
    });
    window.add_action(&action);

    // win.project-palette — searchable project-defined actions for the active workspace.
    let action = gio::SimpleAction::new("project-palette", None);
    action.connect_activate({
        let state = state.clone();
        let window = window.downgrade();
        move |_, _| {
            if let Some(window) = window.upgrade() {
                crate::project_palette::show(&window, &state);
            }
        }
    });
    window.add_action(&action);

    // win.new-terminal-tab — sibling surface in the focused pane.
    let action = gio::SimpleAction::new("new-terminal-tab", None);
    action.connect_activate({
        let state = state.clone();
        move |_, _| crate::shortcuts::handle_new_terminal_tab(&state)
    });
    window.add_action(&action);

    // win.focus-pane — keep pointer-selected pane state aligned with GTK focus.
    let action = gio::SimpleAction::new("focus-pane", Some(&u64::static_variant_type()));
    action.connect_activate({
        let state = state.clone();
        move |_, parameter| {
            let Some(pane_id) = parameter.and_then(|value| value.get::<u64>()) else {
                return;
            };
            crate::shortcuts::handle_focus_pane(&state, pane_id);
        }
    });
    window.add_action(&action);

    // win.new-browser-tab — sibling surface in the focused pane.
    let action = gio::SimpleAction::new("new-browser-tab", None);
    action.set_enabled(crate::browser::agent_browser_available());
    action.connect_activate({
        let state = state.clone();
        move |_, _| crate::shortcuts::handle_browser_open(&state)
    });
    window.add_action(&action);

    // win.new-ssh-workspace
    let action = gio::SimpleAction::new("new-ssh-workspace", None);
    action.connect_activate({
        let state = state.clone();
        let app = app.clone();
        move |_, _| {
            crate::shortcuts::handle_new_ssh_workspace(&state, &app);
        }
    });
    window.add_action(&action);

    // win.browser-open (D-07)
    let action = gio::SimpleAction::new("browser-open", None);
    action.set_enabled(crate::browser::agent_browser_available());
    action.connect_activate({
        let state = state.clone();
        move |_, _| {
            crate::shortcuts::handle_browser_open(&state);
        }
    });
    window.add_action(&action);

    // win.close-surface-tab — close one terminal/browser tab in its pane.
    let action = gio::SimpleAction::new("close-surface-tab", Some(&String::static_variant_type()));
    action.connect_activate({
        let state = state.clone();
        let app = app.clone();
        move |_, parameter| {
            let Some(uuid) = parameter
                .and_then(|value| value.str())
                .and_then(|value| uuid::Uuid::parse_str(value).ok())
            else {
                return;
            };
            crate::shortcuts::handle_close_surface_tab(&state, &app, uuid);
        }
    });
    window.add_action(&action);

    // GtkNotebook pointer reordering mutates its local surface model before asking the
    // application to persist the resulting workspace topology.
    let action = gio::SimpleAction::new("surface-tabs-changed", None);
    action.connect_activate({
        let state = state.clone();
        move |_, _| state.borrow().trigger_session_save()
    });
    window.add_action(&action);

    // Native tab drag destination. Payload is internal and contains only a stable UUID,
    // session-local pane id and a fixed direction selected by the pane drop target.
    let action = gio::SimpleAction::new("surface-drop", Some(&String::static_variant_type()));
    action.connect_activate({
        let state = state.clone();
        move |_, parameter| {
            let Some(payload) = parameter.and_then(|value| value.str()) else {
                return;
            };
            let mut fields = payload.split('|');
            let (Some(surface), Some(pane), Some(direction), Some(position), None) = (
                fields.next(),
                fields.next(),
                fields.next(),
                fields.next(),
                fields.next(),
            ) else {
                return;
            };
            let (Ok(surface), Ok(pane)) = (uuid::Uuid::parse_str(surface), pane.parse::<u64>())
            else {
                return;
            };
            let mut state = state.borrow_mut();
            let Some(source_index) = state
                .split_engines
                .iter()
                .position(|engine| engine.find_pane_id_by_uuid(&surface.to_string()).is_some())
            else {
                return;
            };
            let Some(destination_index) = state
                .split_engines
                .iter()
                .position(|engine| engine.contains_pane(pane))
            else {
                return;
            };
            let destination_workspace = state.workspaces[destination_index].uuid;
            let result = if direction == "center" {
                let Ok(position) = position.parse::<usize>() else {
                    return;
                };
                state
                    .move_surface_between_workspaces(
                        surface,
                        destination_workspace,
                        Some(pane),
                        Some(position),
                        true,
                    )
                    .map(|_| ())
            } else {
                if source_index != destination_index
                    && state
                        .move_surface_between_workspaces(
                            surface,
                            destination_workspace,
                            Some(pane),
                            None,
                            false,
                        )
                        .is_err()
                {
                    return;
                }
                let Some(index) = state
                    .workspaces
                    .iter()
                    .position(|workspace| workspace.uuid == destination_workspace)
                else {
                    return;
                };
                let direction = match direction {
                    "left" => crate::split_engine::FocusDirection::Left,
                    "right" => crate::split_engine::FocusDirection::Right,
                    "up" => crate::split_engine::FocusDirection::Up,
                    "down" => crate::split_engine::FocusDirection::Down,
                    _ => return,
                };
                state.split_engines[index]
                    .drag_surface_to_split(surface, pane, direction)
                    .map(|_| {
                        state.switch_to_index(index);
                        state.trigger_session_save();
                    })
            };
            if let Err(message) = result {
                crate::diagnostics::record(
                    "surface.drop_rejected",
                    serde_json::json!({"reason": message}),
                );
            }
        }
    });
    window.add_action(&action);

    // Sidebar workspace rows accept the same tab drag currency and resolve the target's
    // focused pane at drop time.
    let action = gio::SimpleAction::new(
        "surface-workspace-drop",
        Some(&String::static_variant_type()),
    );
    action.connect_activate({
        let state = state.clone();
        move |_, parameter| {
            let Some(payload) = parameter.and_then(|value| value.str()) else {
                return;
            };
            let Some((surface, workspace)) = payload.split_once('|') else {
                return;
            };
            let (Ok(surface), Ok(workspace)) = (
                uuid::Uuid::parse_str(surface),
                uuid::Uuid::parse_str(workspace),
            ) else {
                return;
            };
            if let Err(message) = state
                .borrow_mut()
                .move_surface_between_workspaces(surface, workspace, None, None, true)
            {
                crate::diagnostics::record(
                    "surface.workspace_drop_rejected",
                    serde_json::json!({"reason": message}),
                );
            }
        }
    });
    window.add_action(&action);

    // win.close-pane
    let action = gio::SimpleAction::new("close-pane", None);
    action.connect_activate({
        let state = state.clone();
        let app = app.clone();
        move |_, _| {
            crate::shortcuts::handle_close_pane(&state, &app);
        }
    });
    window.add_action(&action);

    // win.close-workspace
    let action = gio::SimpleAction::new("close-workspace", None);
    action.connect_activate({
        let state = state.clone();
        let app = app.clone();
        move |_, _| {
            crate::shortcuts::handle_close_workspace(&state, &app);
        }
    });
    window.add_action(&action);

    // --- Edit section actions ---

    // win.copy (Ctrl+Shift+C) -- invoke Ghostty's copy_to_clipboard binding action
    let action = gio::SimpleAction::new("copy", None);
    action.connect_activate({
        let state = state.clone();
        move |_, _| {
            let s = state.borrow();
            if let Some(engine) = s.active_split_engine() {
                if let Some(pane_id) = engine.root.find_active_pane_id() {
                    if let Some(surface) = engine.root.find_surface_for_pane(pane_id) {
                        let action_str = b"copy_to_clipboard";
                        unsafe {
                            crate::ghostty::ffi::ghostty_surface_binding_action(
                                surface,
                                action_str.as_ptr() as *const _,
                                action_str.len(),
                            );
                        }
                    }
                }
            }
        }
    });
    window.add_action(&action);

    // win.paste (Ctrl+Shift+V) -- invoke Ghostty's paste_from_clipboard binding action
    let action = gio::SimpleAction::new("paste", None);
    action.connect_activate({
        let state = state.clone();
        move |_, _| {
            let s = state.borrow();
            if let Some(engine) = s.active_split_engine() {
                if let Some(pane_id) = engine.root.find_active_pane_id() {
                    if let Some(surface) = engine.root.find_surface_for_pane(pane_id) {
                        let action_str = b"paste_from_clipboard";
                        unsafe {
                            crate::ghostty::ffi::ghostty_surface_binding_action(
                                surface,
                                action_str.as_ptr() as *const _,
                                action_str.len(),
                            );
                        }
                    }
                }
            }
        }
    });
    window.add_action(&action);

    // win.find -- stub for now (terminal find not yet implemented)
    let action = gio::SimpleAction::new("find", None);
    action.set_enabled(false);
    window.add_action(&action);

    // win.preferences — terminal appearance settings.
    let action = gio::SimpleAction::new("preferences", None);
    action.connect_activate({
        let window = window.downgrade();
        let state = state.clone();
        move |_, _| {
            if let Some(window) = window.upgrade() {
                crate::preferences::show(&window, &state);
            }
        }
    });
    window.add_action(&action);

    let action = gio::SimpleAction::new("notifications", None);
    action.connect_activate({
        let window = window.downgrade();
        let state = state.clone();
        move |_, _| {
            if let Some(window) = window.upgrade() {
                crate::inbox_view::show(&window, &state);
            }
        }
    });
    window.add_action(&action);

    // --- View section actions ---

    // win.toggle-sidebar
    let action = gio::SimpleAction::new("toggle-sidebar", None);
    action.connect_activate({
        let state = state.clone();
        let sidebar = sidebar.clone();
        move |_, _| {
            let visible = sidebar.is_visible();
            sidebar.set_visible(!visible);
            if let Some(engine) = state.borrow_mut().active_split_engine_mut() {
                engine.focus_active_surface();
            }
        }
    });
    window.add_action(&action);

    // win.split-right
    let action = gio::SimpleAction::new("split-right", None);
    action.connect_activate({
        let state = state.clone();
        move |_, _| {
            crate::shortcuts::handle_split(&state, false);
        }
    });
    window.add_action(&action);

    // win.split-down
    let action = gio::SimpleAction::new("split-down", None);
    action.connect_activate({
        let state = state.clone();
        move |_, _| {
            crate::shortcuts::handle_split(&state, true);
        }
    });
    window.add_action(&action);

    // win.rename-workspace
    let action = gio::SimpleAction::new("rename-workspace", None);
    action.connect_activate({
        let state = state.clone();
        move |_, _| {
            let (active_index, sidebar_list) = {
                let s = state.borrow();
                (s.active_index, s.sidebar_list.clone())
            };
            crate::sidebar::start_inline_rename(&sidebar_list, active_index, state.clone());
        }
    });
    window.add_action(&action);

    // --- Help section actions ---

    // win.keyboard-shortcuts (D-14)
    let action = gio::SimpleAction::new("keyboard-shortcuts", None);
    action.connect_activate({
        let window_weak = window.downgrade();
        move |_, _| {
            if let Some(win) = window_weak.upgrade() {
                let sw = build_shortcuts_window();
                sw.set_transient_for(Some(&win));
                sw.present();
            }
        }
    });
    window.add_action(&action);

    // win.about (D-15)
    let action = gio::SimpleAction::new("about", None);
    action.connect_activate({
        let window_weak = window.downgrade();
        move |_, _| {
            if let Some(win) = window_weak.upgrade() {
                let about = gtk4::AboutDialog::builder()
                    .program_name("cmux")
                    .version(env!("CARGO_PKG_VERSION"))
                    .comments("GPU-accelerated terminal multiplexer for Linux")
                    .website("https://github.com/manaflow-ai/cmux")
                    .license_type(gtk4::License::MitX11)
                    .transient_for(&win)
                    .modal(true)
                    .build();
                about.present();
            }
        }
    });
    window.add_action(&action);

    // app.quit
    let quit_action = gio::SimpleAction::new("quit", None);
    quit_action.connect_activate({
        let app = app.clone();
        move |_, _| {
            app.quit();
        }
    });
    app.add_action(&quit_action);

    // --- Browser-specific actions (D-09) ---

    // win.open-external-browser -- opens current browser pane URL in xdg-open
    // Disabled until BrowserManager exposes current_url() (wired in Plan 03)
    let action = gio::SimpleAction::new("open-external-browser", None);
    action.set_enabled(false); // TODO: enable when BrowserManager.current_url() is available
    window.add_action(&action);

    // win.copy-url -- copies current browser pane URL to clipboard
    // Disabled until BrowserManager exposes current_url() (wired in Plan 03)
    let action = gio::SimpleAction::new("copy-url", None);
    action.set_enabled(false); // TODO: enable when BrowserManager.current_url() is available
    window.add_action(&action);
}

/// Register menu accelerators from the resolved shortcut map plus fixed nonconfigurable actions.
/// GIO accelerators can activate commands, so they must agree with capture-phase dispatch.
pub fn register_accels(app: &gtk4::Application, shortcuts: &crate::config::ShortcutMap) {
    use crate::config::ShortcutAction;
    for (action, name) in [
        (ShortcutAction::NewWorkspace, "win.new-workspace"),
        (ShortcutAction::CloseWorkspace, "win.close-workspace"),
        (ShortcutAction::NewSshWorkspace, "win.new-ssh-workspace"),
        (ShortcutAction::BrowserOpen, "win.browser-open"),
        (ShortcutAction::ClosePane, "win.close-pane"),
        (ShortcutAction::ToggleSidebar, "win.toggle-sidebar"),
        (ShortcutAction::SplitRight, "win.split-right"),
        (ShortcutAction::SplitDown, "win.split-down"),
        (ShortcutAction::RenameWorkspace, "win.rename-workspace"),
    ] {
        let accelerator = shortcuts.accelerator_for(action);
        let accelerators: Vec<&str> = accelerator.as_deref().into_iter().collect();
        app.set_accels_for_action(name, &accelerators);
    }
    app.set_accels_for_action("win.new-terminal-tab", &["<Ctrl>t"]);
    app.set_accels_for_action("win.new-browser-tab", &["<Ctrl><Shift>l"]);
    app.set_accels_for_action("win.copy", &["<Ctrl><Shift>c"]);
    app.set_accels_for_action("win.paste", &["<Ctrl><Shift>v"]);
    app.set_accels_for_action("win.find", &["<Ctrl>f"]);
    app.set_accels_for_action("win.preferences", &["<Ctrl>comma"]);
    app.set_accels_for_action("win.notifications", &["<Ctrl><Shift>i"]);
    app.set_accels_for_action("app.quit", &["<Ctrl>q"]);
}

/// Build the hamburger menu model (D-11, D-12).
/// Returns a gio::Menu that can be set on a MenuButton.
/// Per D-12: sections use File/Edit/View/Help labels.
pub fn build_hamburger_menu() -> gio::Menu {
    let menu = gio::Menu::new();

    // File section (D-12)
    let file_section = gio::Menu::new();
    file_section.append(Some("New Workspace"), Some("win.new-workspace"));
    file_section.append(Some("New SSH Workspace"), Some("win.new-ssh-workspace"));
    file_section.append(Some("New Tab (Terminal)"), Some("win.new-terminal-tab"));
    file_section.append(Some("New Tab (Browser)"), Some("win.new-browser-tab"));
    file_section.append(Some("Close Pane"), Some("win.close-pane"));
    file_section.append(Some("Close Workspace"), Some("win.close-workspace"));
    file_section.append(Some("Quit"), Some("app.quit"));
    menu.append_section(Some("File"), &file_section);

    // Edit section (D-12)
    let edit_section = gio::Menu::new();
    edit_section.append(Some("Copy"), Some("win.copy"));
    edit_section.append(Some("Paste"), Some("win.paste"));
    edit_section.append(Some("Find"), Some("win.find"));
    edit_section.append(Some("Command Palette"), Some("win.project-palette"));
    edit_section.append(Some("Preferences"), Some("win.preferences"));
    menu.append_section(Some("Edit"), &edit_section);

    // View section (D-12)
    let view_section = gio::Menu::new();
    view_section.append(Some("Toggle Sidebar"), Some("win.toggle-sidebar"));
    view_section.append(Some("Notifications"), Some("win.notifications"));
    view_section.append(Some("Split Right"), Some("win.split-right"));
    view_section.append(Some("Split Down"), Some("win.split-down"));
    menu.append_section(Some("View"), &view_section);

    // Help section (D-12)
    let help_section = gio::Menu::new();
    help_section.append(Some("Keyboard Shortcuts"), Some("win.keyboard-shortcuts"));
    help_section.append(Some("About cmux"), Some("win.about"));
    menu.append_section(Some("Help"), &help_section);

    menu
}

/// Build sidebar workspace row context menu (D-03).
pub fn build_sidebar_context_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    menu.append(Some("Rename"), Some("win.rename-workspace"));
    menu.append(Some("Close"), Some("win.close-workspace"));
    menu.append(Some("Split Right"), Some("win.split-right"));
    menu.append(Some("Split Down"), Some("win.split-down"));
    menu
}

/// Build terminal pane context menu (D-08).
pub fn build_terminal_context_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    let edit_section = gio::Menu::new();
    edit_section.append(Some("Copy"), Some("win.copy"));
    edit_section.append(Some("Paste"), Some("win.paste"));
    menu.append_section(None, &edit_section);

    let pane_section = gio::Menu::new();
    pane_section.append(Some("Split Right"), Some("win.split-right"));
    pane_section.append(Some("Split Down"), Some("win.split-down"));
    pane_section.append(Some("Close Pane"), Some("win.close-pane"));
    menu.append_section(None, &pane_section);

    let browser_section = gio::Menu::new();
    browser_section.append(Some("Open Browser Here"), Some("win.browser-open"));
    menu.append_section(None, &browser_section);

    menu
}

/// Build browser preview pane context menu (D-09).
/// Includes Open in External Browser and Copy URL actions.
pub fn build_browser_context_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    menu.append(
        Some("Open in External Browser"),
        Some("win.open-external-browser"),
    );
    menu.append(Some("Copy URL"), Some("win.copy-url"));
    menu.append(Some("Close Pane"), Some("win.close-pane"));
    menu
}

/// Build GtkShortcutsWindow (D-14) with all shortcuts grouped by category.
/// Uses the GtkBox APIs inherited by shortcut containers for GTK 4.8 support.
fn build_shortcuts_window() -> gtk4::ShortcutsWindow {
    let window = gtk4::ShortcutsWindow::builder().build();
    let sections = gtk4::Box::new(gtk4::Orientation::Vertical, 12);

    // Workspaces section
    let ws_section = gtk4::ShortcutsSection::builder()
        .section_name("workspaces")
        .title("Workspaces")
        .build();

    let ws_group = gtk4::ShortcutsGroup::builder().title("Workspaces").build();
    ws_group.append(&shortcut("<Ctrl>n", "New Workspace"));
    ws_group.append(&shortcut("<Ctrl><Shift>w", "Close Workspace"));
    ws_group.append(&shortcut("<Ctrl>bracketright", "Next Workspace"));
    ws_group.append(&shortcut("<Ctrl>bracketleft", "Previous Workspace"));
    ws_group.append(&shortcut("<Ctrl><Shift>r", "Rename Workspace"));
    ws_group.append(&shortcut("<Ctrl>1..9", "Switch to Workspace 1-9"));
    ws_section.append(&ws_group);
    sections.append(&ws_section);

    // Panes section
    let pane_section = gtk4::ShortcutsSection::builder()
        .section_name("panes")
        .title("Panes")
        .build();

    let pane_group = gtk4::ShortcutsGroup::builder().title("Panes").build();
    pane_group.append(&shortcut("<Ctrl>d", "Split Right"));
    pane_group.append(&shortcut("<Ctrl><Shift>d", "Split Down"));
    pane_group.append(&shortcut("<Ctrl><Shift>x", "Close Pane"));
    pane_group.append(&shortcut("<Ctrl><Shift>Left", "Focus Left"));
    pane_group.append(&shortcut("<Ctrl><Shift>Right", "Focus Right"));
    pane_group.append(&shortcut("<Ctrl><Shift>Up", "Focus Up"));
    pane_group.append(&shortcut("<Ctrl><Shift>Down", "Focus Down"));
    pane_section.append(&pane_group);
    sections.append(&pane_section);

    // Edit section
    let edit_section = gtk4::ShortcutsSection::builder()
        .section_name("edit")
        .title("Edit")
        .build();

    let edit_group = gtk4::ShortcutsGroup::builder().title("Edit").build();
    edit_group.append(&shortcut("<Ctrl><Shift>c", "Copy"));
    edit_group.append(&shortcut("<Ctrl><Shift>v", "Paste"));
    edit_group.append(&shortcut("<Ctrl>f", "Find"));
    edit_section.append(&edit_group);
    sections.append(&edit_section);

    // View section
    let view_section = gtk4::ShortcutsSection::builder()
        .section_name("view")
        .title("View")
        .build();

    let view_group = gtk4::ShortcutsGroup::builder().title("View").build();
    view_group.append(&shortcut("<Ctrl>b", "Toggle Sidebar"));
    view_group.append(&shortcut("<Ctrl><Shift>b", "Open Browser"));
    view_group.append(&shortcut("<Ctrl><Shift>s", "New SSH Workspace"));
    view_section.append(&view_group);
    sections.append(&view_section);

    // General section
    let general_section = gtk4::ShortcutsSection::builder()
        .section_name("general")
        .title("General")
        .build();

    let general_group = gtk4::ShortcutsGroup::builder().title("General").build();
    general_group.append(&shortcut("<Ctrl>q", "Quit"));
    general_section.append(&general_group);
    sections.append(&general_section);

    window.set_child(Some(&sections));

    window
}

/// Helper to create a ShortcutsShortcut widget.
fn shortcut(accel: &str, title: &str) -> gtk4::ShortcutsShortcut {
    gtk4::ShortcutsShortcut::builder()
        .accelerator(accel)
        .title(title)
        .build()
}
