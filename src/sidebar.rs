use gtk4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

/// Build the sidebar widget: outer Box(V) > [ScrolledWindow(ListBox), Button(+)].
/// Returns (sidebar_box, scrolled_window, list_box).
///
/// Per Pitfall 5 from RESEARCH.md: the '+' button is OUTSIDE the ScrolledWindow
/// so it doesn't scroll away.
///
/// Per UI-SPEC:
/// - Width: 160px (set_size_request(160, -1))
/// - Background: #242424 (applied via global CssProvider in main.rs)
/// - Row height: 36px min-height (CSS)
/// - Row padding: 8px top/bottom, 16px left/right
/// - Active row: #5b8dd9 background, #ffffff text, font-weight 600
/// - Inactive row: transparent bg, #cccccc text, font-weight 400
/// - Hover (inactive): #2e2e2e
pub fn build_sidebar() -> (gtk4::Box, gtk4::ScrolledWindow, gtk4::ListBox) {
    let list_box = gtk4::ListBox::new();
    list_box.set_selection_mode(gtk4::SelectionMode::Single);
    list_box.add_css_class("workspace-list");

    let scrolled = gtk4::ScrolledWindow::new();
    scrolled.set_size_request(160, -1);
    scrolled.set_hscrollbar_policy(gtk4::PolicyType::Never);
    scrolled.set_vscrollbar_policy(gtk4::PolicyType::Automatic);
    scrolled.set_child(Some(&list_box));
    scrolled.set_vexpand(true);

    // Sidebar container: Box(V) > [ScrolledWindow(ListBox), Button(+)]
    let sidebar_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    sidebar_box.add_css_class("sidebar");
    sidebar_box.append(&scrolled);

    // '+' button at the bottom (D-01)
    let add_btn = gtk4::Button::with_label("+");
    add_btn.add_css_class("sidebar-add-btn");
    add_btn.set_tooltip_text(Some("New Workspace (Ctrl+N)"));
    add_btn.set_action_name(Some("win.new-workspace"));
    sidebar_box.append(&add_btn);

    (sidebar_box, scrolled, list_box)
}

/// Wire sidebar click-to-switch. Called from main.rs after AppState is constructed.
/// Per WS-03: clicking a row calls AppState.switch_to_index.
pub fn wire_sidebar_clicks(
    list_box: &gtk4::ListBox,
    state: Rc<RefCell<crate::app_state::AppState>>,
) {
    list_box.connect_row_activated({
        let state = Rc::downgrade(&state);
        move |_list, row| {
            let Some(state) = state.upgrade() else { return; };
            let index = row.index() as usize;
            state.borrow_mut().switch_to_index(index);
            // SPLIT-07: call ghostty_surface_set_focus on the newly active pane.
            // Workspace switches are focus changes — must call set_focus after switch.
            let surface = {
                let mut s = state.borrow_mut();
                s.active_split_engine_mut()
                    .and_then(|engine| engine.root.find_active_pane_id())
                    .and_then(|pane_id| {
                        if let Ok(reg) = crate::ghostty::callbacks::SURFACE_REGISTRY.lock() {
                            reg.iter()
                                .find(|(_, &pid)| pid == pane_id)
                                .map(|(&ptr, _)| ptr as crate::ghostty::ffi::ghostty_surface_t)
                        } else {
                            None
                        }
                    })
            };
            if let Some(surface) = surface {
                unsafe {
                    crate::ghostty::ffi::ghostty_surface_set_focus(surface, true);
                }
            }
        }
    });
}

/// Start inline rename for the active workspace row.
/// Per UI-SPEC: replaces GtkLabel with GtkEntry; Enter commits, Escape cancels.
/// Per D-03: rename triggered by Ctrl+Shift+R (keyboard only).
pub fn start_inline_rename(
    list_box: &gtk4::ListBox,
    active_index: usize,
    state: Rc<RefCell<crate::app_state::AppState>>,
) {
    let row = match list_box.row_at_index(active_index as i32) {
        Some(r) => r,
        None => return,
    };

    let (workspace_id, current_name) = {
        let s = state.borrow();
        let Some(workspace) = s.workspaces.get(active_index) else { return; };
        (workspace.id, workspace.name.clone())
    };
    let entry = gtk4::Entry::new();
    entry.set_text(&current_name);
    row.set_child(Some(&entry));
    entry.grab_focus();
    let finished = Rc::new(std::cell::Cell::new(false));
    let finish: Rc<dyn Fn(bool)> = Rc::new({
        let entry = entry.downgrade();
        let row = row.downgrade();
        let state = Rc::downgrade(&state);
        move |commit| {
            let Some(state) = state.upgrade() else { return; };
            let (Some(row), Some(entry)) = (row.upgrade(), entry.upgrade()) else { return; };
            if finished.replace(true) { return; }
            {
                let mut s = state.borrow_mut();
                if let Some(workspace) = s.workspaces.iter_mut().find(|w| w.id == workspace_id) {
                    let name = entry.text();
                    if commit && !name.trim().is_empty() { workspace.rename(name.trim().to_string()); }
                    row.set_child(Some(&workspace_row_content(workspace)));
                    style_workspace_row(&row, workspace);
                }
                s.trigger_session_save();
            }
            let app = state.borrow().gtk_app.clone();
            wire_row_close_button(&row, state.clone(), &app);
        }
    });
    entry.connect_activate({ let finish = finish.clone(); move |_| finish(true) });
    let focus = gtk4::EventControllerFocus::new();
    focus.connect_leave({ let finish = finish.clone(); move |_| finish(true) });
    entry.add_controller(focus);
    let key = gtk4::EventControllerKey::new();
    key.connect_key_pressed(move |_, key, _, _| {
        if key == gtk4::gdk::Key::Escape { finish(false); glib::Propagation::Stop }
        else { glib::Propagation::Proceed }
    });
    entry.add_controller(key);
}

/// Rebuild the Phase 4 sidebar row content:
/// GtkBox(H, 4) > [GtkBox(V, 0) > [GtkLabel(name)], GtkLabel(dot), Button(close)].
/// Dot is hidden by default (fresh state after rename).
/// Close button is hidden by default, shown on row hover via CSS (D-02).
pub fn rebuild_sidebar_row_content(name: &str) -> gtk4::Box {
    let hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    let label = gtk4::Label::new(Some(name));
    label.set_halign(gtk4::Align::Start);
    label.set_hexpand(true);
    vbox.append(&label);
    vbox.set_hexpand(true);
    hbox.append(&vbox);

    let dot = gtk4::Label::new(None);
    dot.add_css_class("attention-dot");
    dot.set_visible(false);
    hbox.append(&dot);

    // Close button (D-02) -- hidden by default, shown on row hover via CSS
    let close_btn = gtk4::Button::with_label("\u{00D7}"); // Unicode multiplication sign
    close_btn.add_css_class("sidebar-close-btn");
    close_btn.set_tooltip_text(Some("Close Workspace"));
    hbox.append(&close_btn);

    hbox
}

/// Wire the close button for a specific sidebar row.
/// Called when a row is created (in app_state::create_workspace or after rename rebuild).
pub fn wire_row_close_button(
    row: &gtk4::ListBoxRow,
    state: Rc<RefCell<crate::app_state::AppState>>,
    app: &gtk4::Application,
) {
    let close_btn = row
        .child()
        .and_downcast::<gtk4::Box>()
        .and_then(|hbox| hbox.last_child())
        .and_downcast::<gtk4::Button>();

    if let Some(btn) = close_btn {
        btn.connect_clicked({
            let state = Rc::downgrade(&state);
            let app = app.clone();
            let row = row.downgrade();
            move |_| {
                let Some(state) = state.upgrade() else { return; };
                let Some(row) = row.upgrade() else { return; };
                let index = row.index() as usize;
                let ws_count = state.borrow().workspaces.len();
                if ws_count <= 1 {
                    return; // Cannot close last workspace
                }
                // Switch to this workspace first (so close_workspace operates on the right one)
                state.borrow_mut().switch_to_index(index);
                crate::shortcuts::handle_close_workspace(&state, &app);
            }
        });
    }
}

/// Attach right-click context menu to a sidebar row (D-03).
pub fn attach_sidebar_context_menu(
    row: &gtk4::ListBoxRow,
    state: Rc<RefCell<crate::app_state::AppState>>,
) {
    wire_workspace_drag(row, state.clone());
    let menu_model = crate::menus::build_sidebar_context_menu();
    menu_model.append(Some("Move Up"), Some("workspace.move-up"));
    menu_model.append(Some("Move Down"), Some("workspace.move-down"));
    let colors = gtk4::gio::Menu::new();
    let group = gtk4::gio::SimpleActionGroup::new();
    for (name, offset) in [("move-up", -1isize), ("move-down", 1)] {
        let action = gtk4::gio::SimpleAction::new(name, None);
        action.connect_activate({
            let state = Rc::downgrade(&state); let row = row.downgrade();
            move |_, _| {
                let Some(state) = state.upgrade() else { return; };
                let Some(row) = row.upgrade() else { return; };
                let index = row.index() as usize;
                if let Some(to) = index.checked_add_signed(offset) { state.borrow_mut().reorder_workspace(index, to); }
            }
        });
        group.add_action(&action);
    }
    for (name, color) in [("Default", None), ("Blue", Some("#24466b")), ("Green", Some("#285943")), ("Purple", Some("#553b70")), ("Red", Some("#703a40")), ("Orange", Some("#74502e")), ("Gray", Some("#444444"))] {
        let action_name = format!("color-{}", name.to_lowercase());
        colors.append(Some(name), Some(&format!("workspace.{action_name}")));
        let action = gtk4::gio::SimpleAction::new(&action_name, None);
        action.connect_activate({
            let state = Rc::downgrade(&state); let row = row.downgrade();
            move |_, _| {
                let Some(state) = state.upgrade() else { return; };
                let Some(row) = row.upgrade() else { return; };
                let mut s = state.borrow_mut();
                if let Some(id) = s.workspaces.get(row.index() as usize).map(|w| w.id) {
                    s.set_workspace_color(id, color.map(str::to_string));
                }
            }
        });
        group.add_action(&action);
    }
    menu_model.append_submenu(Some("Background Color"), &colors);
    row.insert_action_group("workspace", Some(&group));
    let popover = gtk4::PopoverMenu::from_model(Some(&menu_model));
    popover.set_parent(row);
    popover.set_has_arrow(false);

    let gesture = gtk4::GestureClick::new();
    gesture.set_button(3); // Right-click
    gesture.connect_released({
        let popover = popover.downgrade();
        let state = Rc::downgrade(&state);
        let row = row.downgrade();
        move |_, _, x, y| {
            let Some(state) = state.upgrade() else { return; };
            let (Some(row), Some(popover)) = (row.upgrade(), popover.upgrade()) else { return; };
            // Switch to this workspace first so context menu actions apply to it
            let index = row.index() as usize;
            state.borrow_mut().switch_to_index(index);
            popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(
                x as i32, y as i32, 1, 1,
            )));
            popover.popup();
        }
    });
    row.add_controller(gesture);
}

/// Wire close button + context menu to the most recently added sidebar row.
pub fn wire_latest_row(
    sidebar_list: &gtk4::ListBox,
    state: Rc<RefCell<crate::app_state::AppState>>,
    app: &gtk4::Application,
) {
    let n = sidebar_list.observe_children().n_items();
    if n == 0 {
        return;
    }
    if let Some(row) = sidebar_list.row_at_index((n - 1) as i32) {
        wire_row_close_button(&row, state.clone(), app);
        attach_sidebar_context_menu(&row, state);
    }
}

/// Shared content keeps subtitles, attention and close controls intact after rename.
pub fn workspace_row_content(workspace: &crate::workspace::Workspace) -> gtk4::Box {
    let hbox = rebuild_sidebar_row_content(&workspace.name);
    let vbox = hbox.first_child().and_downcast::<gtk4::Box>().unwrap();
    if let Some(title) = vbox.first_child().and_downcast::<gtk4::Label>() {
        title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        title.set_max_width_chars(24);
    }
    let subtitle = gtk4::Label::new(Some(&workspace.subtitle()));
    subtitle.set_xalign(0.0);
    subtitle.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    subtitle.set_max_width_chars(28);
    subtitle.add_css_class("dim-label");
    subtitle.set_visible(!workspace.subtitle().is_empty());
    vbox.append(&subtitle);
    if workspace.connection_state.is_remote() {
        let status = gtk4::Label::new(Some(workspace.connection_state.display_text()));
        status.set_xalign(0.0);
        status.add_css_class("connection-state");
        status.add_css_class(workspace.connection_state.css_class());
        vbox.append(&status);
    }
    if let Some(dot) = vbox.next_sibling().and_downcast::<gtk4::Label>() {
        dot.set_visible(workspace.has_attention);
    }
    hbox
}

pub fn style_workspace_row(row: &gtk4::ListBoxRow, workspace: &crate::workspace::Workspace) {
    row.set_tooltip_text(Some(&workspace.location()));
    let context = row.style_context();
    unsafe {
        if let Some(previous) = row.steal_data::<gtk4::CssProvider>("workspace-color-provider") {
            context.remove_provider(&previous);
        }
    }
    if let Some(color) = workspace.color.as_deref().filter(|c| crate::workspace::valid_workspace_color(c)) {
        let provider = gtk4::CssProvider::new();
        provider.load_from_data(&format!(
            "row {{ background-color: {color}; color: white; }} row:selected, row.active-workspace {{ background-color: {color}; outline: 2px solid white; outline-offset: -2px; }}"
        ));
        context.add_provider(&provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);
        unsafe { row.set_data("workspace-color-provider", provider); }
    }
}

fn wire_workspace_drag(row: &gtk4::ListBoxRow, state: Rc<RefCell<crate::app_state::AppState>>) {
    let source = gtk4::DragSource::new();
    source.set_actions(gtk4::gdk::DragAction::MOVE);
    source.connect_prepare({
        let row = row.downgrade(); let state = Rc::downgrade(&state);
        move |_, _, _| {
            let state = state.upgrade()?;
            let row = row.upgrade()?;
            let s = state.borrow();
            let workspace = s.workspaces.get(row.index() as usize)?;
            Some(gtk4::gdk::ContentProvider::for_value(&format!("cmux-workspace:{}", workspace.id).to_value()))
        }
    });
    row.add_controller(source);
    let target = gtk4::DropTarget::new(String::static_type(), gtk4::gdk::DragAction::MOVE);
    target.connect_drop({
        let state = Rc::downgrade(&state);
        let row = row.downgrade();
        move |_, value, _, _| {
            let Some(state) = state.upgrade() else { return false; };
            let Some(row) = row.upgrade() else { return false; };
            let Some(id) = value.get::<String>().ok().and_then(|s| s.strip_prefix("cmux-workspace:").and_then(|s| s.parse::<u64>().ok())) else { return false; };
            let mut s = state.borrow_mut();
            let Some(from) = s.workspaces.iter().position(|w| w.id == id) else { return false; };
            s.reorder_workspace(from, row.index() as usize)
        }
    });
    row.add_controller(target);
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;

    #[test]
    #[ignore = "requires GTK display; run in the headless CI job"]
    fn workspace_controls_release_closed_widget_trees() {
        gtk4::init().unwrap();
        let app = gtk4::Application::new(Some("io.cmux.WorkspaceTest"), Default::default());
        let list = gtk4::ListBox::new();
        let state = crate::app_state::AppState::new(gtk4::Stack::new(), list.clone(), std::ptr::null_mut(), app.clone());
        for id in 1..=40 {
            let workspace = crate::workspace::Workspace::new_bound(id, id as usize, "Sample".into(), "/opt/team/repo".into());
            let row = gtk4::ListBoxRow::new();
            row.set_child(Some(&workspace_row_content(&workspace)));
            list.append(&row);
            state.borrow_mut().workspaces.push(workspace);
            wire_row_close_button(&row, state.clone(), &app);
            attach_sidebar_context_menu(&row, state.clone());
            let weak = row.downgrade();
            list.remove(&row);
            state.borrow_mut().workspaces.pop();
            drop(row);
            assert!(weak.upgrade().is_none(), "closed workspace retained by a signal callback");
        }
        let weak_state = Rc::downgrade(&state);
        drop(state);
        assert!(weak_state.upgrade().is_none());
    }
}
