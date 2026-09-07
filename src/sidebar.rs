use gtk4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

const WORKSPACE_ID_KEY: &str = "workspace-id";

/// Bind a sidebar row to stable model identity; visual row positions may include group headers.
pub fn bind_workspace_row(row: &gtk4::ListBoxRow, workspace_id: u64) {
    // SAFETY: the row owns this copied scalar for its full GTK lifetime.
    unsafe { row.set_data(WORKSPACE_ID_KEY, workspace_id) };
}

/// Read stable workspace identity; group/header rows deliberately return None.
pub fn workspace_row_id(row: &gtk4::ListBoxRow) -> Option<u64> {
    // SAFETY: bind_workspace_row stores a u64 under this private key.
    unsafe { row.data::<u64>(WORKSPACE_ID_KEY).map(|id| *id.as_ref()) }
}

/// Resolve a visual row against the current model without trusting its GTK index.
pub fn workspace_index_for_row(
    state: &crate::app_state::AppState,
    row: &gtk4::ListBoxRow,
) -> Option<usize> {
    let id = workspace_row_id(row)?;
    state
        .workspaces
        .iter()
        .position(|workspace| workspace.id == id)
}

/// Locate one workspace row while ignoring future group/header rows.
pub fn row_for_workspace(list: &gtk4::ListBox, workspace_id: u64) -> Option<gtk4::ListBoxRow> {
    workspace_rows(list)
        .into_iter()
        .find(|row| workspace_row_id(row) == Some(workspace_id))
}

/// Snapshot workspace rows in visual order without retaining the list or model.
pub fn workspace_rows(list: &gtk4::ListBox) -> Vec<gtk4::ListBoxRow> {
    let mut rows = Vec::new();
    let mut child = list.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        if let Ok(row) = widget.downcast::<gtk4::ListBoxRow>() {
            if workspace_row_id(&row).is_some() {
                rows.push(row);
            }
        }
    }
    rows
}

fn group_header_row(
    group: &crate::workspace_group::WorkspaceGroup,
    member_count: usize,
    unread_count: usize,
    state: &crate::app_state::AppStateRef,
) -> gtk4::ListBoxRow {
    let row = gtk4::ListBoxRow::new();
    row.set_selectable(false);
    row.set_activatable(false);
    row.set_widget_name(&format!("workspace-group-{}", group.id));
    row.add_css_class("workspace-group");
    let button = gtk4::Button::new();
    button.add_css_class("flat");
    button.set_hexpand(true);
    let content = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    let arrow = gtk4::Label::new(Some(if group.collapsed { "▸" } else { "▾" }));
    let title = gtk4::Label::new(Some(&group.name));
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    let count = gtk4::Label::new(Some(&member_count.to_string()));
    count.add_css_class("dim-label");
    content.append(&arrow);
    if let Some(color) = &group.color {
        let swatch = gtk4::DrawingArea::new();
        swatch.set_size_request(8, 8);
        swatch.add_css_class("group-color-swatch");
        let provider = gtk4::CssProvider::new();
        provider.load_from_data(&format!(
            ".group-color-swatch {{ background-color: {color}; border-radius: 50%; }}"
        ));
        swatch
            .style_context()
            .add_provider(&provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
        content.append(&swatch);
    }
    content.append(&title);
    let unread = gtk4::Label::new(Some(&unread_count.to_string()));
    unread.add_css_class("group-unread");
    unread.set_visible(unread_count > 0);
    content.append(&unread);
    content.append(&count);
    button.set_child(Some(&content));
    row.set_child(Some(&button));
    let group_id = group.id;
    button.connect_clicked({
        let state = Rc::downgrade(state);
        move |_| {
            let Some(state) = state.upgrade() else {
                return;
            };
            let collapsed = {
                let state = state.borrow();
                state
                    .workspace_groups
                    .iter()
                    .find(|group| group.id == group_id)
                    .map(|group| !group.collapsed)
            };
            if let Some(collapsed) = collapsed {
                let _ = state.borrow_mut().update_workspace_group(
                    group_id,
                    None,
                    None,
                    Some(collapsed),
                    None,
                );
                rebuild_grouped_sidebar(&state);
            }
        }
    });
    row
}

/// Refresh group unread badges after pane attention changes without rebuilding workspace rows.
pub fn update_group_attention(state: &crate::app_state::AppState) {
    let mut child = state.sidebar_list.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        let Ok(row) = widget.downcast::<gtk4::ListBoxRow>() else {
            continue;
        };
        let Some(id) = row
            .widget_name()
            .strip_prefix("workspace-group-")
            .and_then(|value| uuid::Uuid::parse_str(value).ok())
        else {
            continue;
        };
        let unread_count = state
            .workspaces
            .iter()
            .filter(|workspace| workspace.group_id == Some(id) && workspace.has_attention)
            .count();
        let content = row
            .child()
            .and_downcast::<gtk4::Button>()
            .and_then(|button| button.child())
            .and_downcast::<gtk4::Box>();
        let Some(content) = content else {
            continue;
        };
        let mut item = content.first_child();
        while let Some(widget) = item {
            item = widget.next_sibling();
            if widget.has_css_class("group-unread") {
                if let Ok(label) = widget.downcast::<gtk4::Label>() {
                    label.set_text(&unread_count.to_string());
                    label.set_visible(unread_count > 0);
                }
                break;
            }
        }
    }
}

/// Rebuild sidebar presentation from stable model identity, including persistent group headers.
pub fn rebuild_grouped_sidebar(state: &crate::app_state::AppStateRef) {
    let (list, app, groups, active_id) = {
        let state = state.borrow();
        (
            state.sidebar_list.clone(),
            state.gtk_app.clone(),
            state.workspace_groups.clone(),
            state.active_workspace().map(|workspace| workspace.id),
        )
    };
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }

    for group in &groups {
        let (member_ids, unread) = {
            let state = state.borrow();
            let members: Vec<_> = state
                .workspaces
                .iter()
                .filter(|workspace| workspace.group_id == Some(group.id))
                .map(|workspace| workspace.id)
                .collect();
            let unread = state
                .workspaces
                .iter()
                .filter(|workspace| workspace.group_id == Some(group.id) && workspace.has_attention)
                .count();
            (members, unread)
        };
        list.append(&group_header_row(group, member_ids.len(), unread, state));
        for id in member_ids {
            let row = {
                let state = state.borrow();
                state
                    .workspaces
                    .iter()
                    .find(|workspace| workspace.id == id)
                    .map(|workspace| state.build_sidebar_row(workspace))
            };
            if let Some(row) = row {
                row.set_visible(!group.collapsed);
                list.append(&row);
                wire_row_close_button(&row, state.clone(), &app);
                attach_sidebar_context_menu(&row, state.clone());
            }
        }
    }
    let ungrouped: Vec<_> = state
        .borrow()
        .workspaces
        .iter()
        .filter(|workspace| workspace.group_id.is_none())
        .map(|workspace| workspace.id)
        .collect();
    for id in ungrouped {
        let row = {
            let state = state.borrow();
            state
                .workspaces
                .iter()
                .find(|workspace| workspace.id == id)
                .map(|workspace| state.build_sidebar_row(workspace))
        };
        if let Some(row) = row {
            list.append(&row);
            wire_row_close_button(&row, state.clone(), &app);
            attach_sidebar_context_menu(&row, state.clone());
        }
    }
    if let Some(active_id) = active_id {
        let row = row_for_workspace(&list, active_id);
        if let Some(row) = &row {
            row.add_css_class("active-workspace");
            if let Some(label) = row
                .child()
                .and_then(|child| child.first_child())
                .and_then(|child| child.first_child())
                .and_downcast::<gtk4::Label>()
            {
                label.add_css_class("active-workspace-label");
            }
        }
        list.select_row(row.as_ref());
    }
}

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
            let Some(state) = state.upgrade() else {
                return;
            };
            let index = workspace_index_for_row(&state.borrow(), row);
            if let Some(index) = index {
                state.borrow_mut().switch_to_index(index);
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
    let workspace_id = match state.borrow().workspaces.get(active_index) {
        Some(workspace) => workspace.id,
        None => return,
    };
    let row = match row_for_workspace(list_box, workspace_id) {
        Some(r) => r,
        None => return,
    };

    let current_name = {
        let s = state.borrow();
        let Some(workspace) = s.workspaces.get(active_index) else {
            return;
        };
        workspace.name.clone()
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
            let Some(state) = state.upgrade() else {
                return;
            };
            let (Some(row), Some(entry)) = (row.upgrade(), entry.upgrade()) else {
                return;
            };
            if finished.replace(true) {
                return;
            }
            {
                let mut s = state.borrow_mut();
                if let Some(workspace) = s.workspaces.iter_mut().find(|w| w.id == workspace_id) {
                    let name = entry.text();
                    if commit && !name.trim().is_empty() {
                        workspace.rename(name.trim().to_string());
                    }
                    row.set_child(Some(&workspace_row_content(workspace)));
                    style_workspace_row(&row, workspace);
                }
                s.trigger_session_save();
            }
            let app = state.borrow().gtk_app.clone();
            wire_row_close_button(&row, state.clone(), &app);
        }
    });
    entry.connect_activate({
        let finish = finish.clone();
        move |_| finish(true)
    });
    let focus = gtk4::EventControllerFocus::new();
    focus.connect_leave({
        let finish = finish.clone();
        move |_| finish(true)
    });
    entry.add_controller(focus);
    let key = gtk4::EventControllerKey::new();
    key.connect_key_pressed(move |_, key, _, _| {
        if key == gtk4::gdk::Key::Escape {
            finish(false);
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
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
                let Some(state) = state.upgrade() else {
                    return;
                };
                let Some(row) = row.upgrade() else {
                    return;
                };
                let Some(index) = workspace_index_for_row(&state.borrow(), &row) else {
                    return;
                };
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
            let state = Rc::downgrade(&state);
            let row = row.downgrade();
            move |_, _| {
                let Some(state) = state.upgrade() else {
                    return;
                };
                let Some(row) = row.upgrade() else {
                    return;
                };
                let Some(index) = workspace_index_for_row(&state.borrow(), &row) else {
                    return;
                };
                let to = state.borrow().adjacent_workspace_in_group(index, offset);
                if let Some(to) = to {
                    let changed = { state.borrow_mut().reorder_workspace(index, to) };
                    if changed {
                        rebuild_grouped_sidebar(&state);
                    }
                }
            }
        });
        group.add_action(&action);
    }
    for (name, color) in [
        ("Default", None),
        ("Blue", Some("#24466b")),
        ("Green", Some("#285943")),
        ("Purple", Some("#553b70")),
        ("Red", Some("#703a40")),
        ("Orange", Some("#74502e")),
        ("Gray", Some("#444444")),
    ] {
        let action_name = format!("color-{}", name.to_lowercase());
        colors.append(Some(name), Some(&format!("workspace.{action_name}")));
        let action = gtk4::gio::SimpleAction::new(&action_name, None);
        action.connect_activate({
            let state = Rc::downgrade(&state);
            let row = row.downgrade();
            move |_, _| {
                let Some(state) = state.upgrade() else {
                    return;
                };
                let Some(row) = row.upgrade() else {
                    return;
                };
                let mut s = state.borrow_mut();
                if let Some(id) = workspace_index_for_row(&s, &row)
                    .and_then(|index| s.workspaces.get(index))
                    .map(|workspace| workspace.id)
                {
                    s.set_workspace_color(id, color.map(str::to_string));
                }
            }
        });
        group.add_action(&action);
    }
    menu_model.append_submenu(Some("Background Color"), &colors);
    let groups_menu = gtk4::gio::Menu::new();
    groups_menu.append(
        Some("New Group from Workspace"),
        Some("workspace.group-new"),
    );
    groups_menu.append(Some("Ungrouped"), Some("workspace.group-none"));
    let available_groups: Vec<_> = state
        .borrow()
        .workspace_groups
        .iter()
        .map(|group| (group.id, group.name.clone()))
        .collect();
    for (position, (group_id, name)) in available_groups.into_iter().enumerate() {
        let action_name = format!("group-{position}");
        groups_menu.append(Some(&name), Some(&format!("workspace.{action_name}")));
        let action = gtk4::gio::SimpleAction::new(&action_name, None);
        action.connect_activate({
            let state = Rc::downgrade(&state);
            let row = row.downgrade();
            move |_, _| {
                let (Some(state), Some(row)) = (state.upgrade(), row.upgrade()) else {
                    return;
                };
                let workspace_id = {
                    let state = state.borrow();
                    workspace_index_for_row(&state, &row)
                        .and_then(|index| state.workspaces.get(index))
                        .map(|workspace| workspace.uuid)
                };
                if let Some(workspace_id) = workspace_id {
                    let _ = state
                        .borrow_mut()
                        .assign_workspace_group(Some(group_id), &[workspace_id]);
                    rebuild_grouped_sidebar(&state);
                }
            }
        });
        group.add_action(&action);
    }
    let ungroup = gtk4::gio::SimpleAction::new("group-none", None);
    ungroup.connect_activate({
        let state = Rc::downgrade(&state);
        let row = row.downgrade();
        move |_, _| {
            let (Some(state), Some(row)) = (state.upgrade(), row.upgrade()) else {
                return;
            };
            let workspace_id = {
                let state = state.borrow();
                workspace_index_for_row(&state, &row)
                    .and_then(|index| state.workspaces.get(index))
                    .map(|workspace| workspace.uuid)
            };
            if let Some(workspace_id) = workspace_id {
                let _ = state
                    .borrow_mut()
                    .assign_workspace_group(None, &[workspace_id]);
                rebuild_grouped_sidebar(&state);
            }
        }
    });
    group.add_action(&ungroup);
    let create_group = gtk4::gio::SimpleAction::new("group-new", None);
    create_group.connect_activate({
        let state = Rc::downgrade(&state);
        let row = row.downgrade();
        move |_, _| {
            let (Some(state), Some(row)) = (state.upgrade(), row.upgrade()) else {
                return;
            };
            let workspace = {
                let state = state.borrow();
                workspace_index_for_row(&state, &row)
                    .and_then(|index| state.workspaces.get(index))
                    .map(|workspace| (workspace.uuid, workspace.name.clone()))
            };
            if let Some((workspace_id, name)) = workspace {
                let group_id = { state.borrow_mut().create_workspace_group(name, None) };
                if let Ok(group_id) = group_id {
                    let _ = state
                        .borrow_mut()
                        .assign_workspace_group(Some(group_id), &[workspace_id]);
                    rebuild_grouped_sidebar(&state);
                }
            }
        }
    });
    group.add_action(&create_group);
    menu_model.append_submenu(Some("Workspace Group"), &groups_menu);
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
            let Some(state) = state.upgrade() else {
                return;
            };
            let (Some(row), Some(popover)) = (row.upgrade(), popover.upgrade()) else {
                return;
            };
            // Switch to this workspace first so context menu actions apply to it
            let Some(index) = workspace_index_for_row(&state.borrow(), &row) else {
                return;
            };
            state.borrow_mut().switch_to_index(index);
            popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
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
    if let Some(row) = workspace_rows(sidebar_list).last() {
        wire_row_close_button(row, state.clone(), app);
        attach_sidebar_context_menu(row, state);
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
    let git = gtk4::Label::new(None);
    git.set_xalign(0.0);
    git.set_single_line_mode(true);
    git.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    git.set_max_width_chars(28);
    git.add_css_class("workspace-git");
    git.add_css_class("dim-label");
    crate::git_metadata::render(&git, workspace.git.as_ref());
    vbox.append(&git);
    let ports = gtk4::Label::new(None);
    ports.set_xalign(0.0);
    ports.set_single_line_mode(true);
    ports.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    ports.add_css_class("workspace-ports");
    ports.add_css_class("dim-label");
    crate::ports::render(&ports, workspace.ports.as_deref());
    vbox.append(&ports);
    let metadata = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    metadata.add_css_class("workspace-metadata");
    crate::workspace_metadata::render(&metadata, &workspace.metadata);
    vbox.append(&metadata);
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

/// Replace the row color provider and full-location tooltip without accumulating providers.
pub fn style_workspace_row(row: &gtk4::ListBoxRow, workspace: &crate::workspace::Workspace) {
    row.set_tooltip_text(Some(&workspace.location()));
    let context = row.style_context();
    unsafe {
        if let Some(previous) = row.steal_data::<gtk4::CssProvider>("workspace-color-provider") {
            context.remove_provider(&previous);
        }
    }
    if let Some(color) = workspace
        .color
        .as_deref()
        .filter(|c| crate::workspace::valid_workspace_color(c))
    {
        let provider = gtk4::CssProvider::new();
        provider.load_from_data(&format!(
            "row {{ background-color: {color}; color: white; }} row:selected, row.active-workspace {{ background-color: {color}; outline: 2px solid white; outline-offset: -2px; }}"
        ));
        context.add_provider(&provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);
        unsafe {
            row.set_data("workspace-color-provider", provider);
        }
    }
}

/// Bind workspace-ID drag/drop to model reordering with weak widget and state captures.
fn wire_workspace_drag(row: &gtk4::ListBoxRow, state: Rc<RefCell<crate::app_state::AppState>>) {
    let source = gtk4::DragSource::new();
    source.set_actions(gtk4::gdk::DragAction::MOVE);
    source.connect_prepare({
        let row = row.downgrade();
        let state = Rc::downgrade(&state);
        move |_, _, _| {
            let state = state.upgrade()?;
            let row = row.upgrade()?;
            let s = state.borrow();
            let workspace = s.workspaces.get(workspace_index_for_row(&s, &row)?)?;
            Some(gtk4::gdk::ContentProvider::for_value(
                &format!("cmux-workspace:{}", workspace.id).to_value(),
            ))
        }
    });
    row.add_controller(source);
    let target = gtk4::DropTarget::new(String::static_type(), gtk4::gdk::DragAction::MOVE);
    target.connect_drop({
        let state = Rc::downgrade(&state);
        let row = row.downgrade();
        move |_, value, _, _| {
            let Some(state) = state.upgrade() else {
                return false;
            };
            let Some(row) = row.upgrade() else {
                return false;
            };
            let Some(id) = value.get::<String>().ok().and_then(|s| {
                s.strip_prefix("cmux-workspace:")
                    .and_then(|s| s.parse::<u64>().ok())
            }) else {
                return false;
            };
            let mut s = state.borrow_mut();
            let Some(from) = s.workspaces.iter().position(|w| w.id == id) else {
                return false;
            };
            let Some(to) = workspace_index_for_row(&s, &row) else {
                return false;
            };
            let target_group = s.workspaces[to].group_id;
            s.workspaces[from].group_id = target_group;
            let changed = s.reorder_workspace(from, to);
            drop(s);
            if changed {
                rebuild_grouped_sidebar(&state);
            }
            changed
        }
    });
    row.add_controller(target);
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;

    /// Exercise sidebar controls and verify removed workspace widget trees release their references.
    #[test]
    #[ignore = "requires GTK display; run in the headless CI job"]
    fn workspace_controls_release_closed_widget_trees() {
        gtk4::init().unwrap();
        let app = gtk4::Application::new(Some("io.cmux.WorkspaceTest"), Default::default());
        let list = gtk4::ListBox::new();
        let state = crate::app_state::AppState::new(
            gtk4::Stack::new(),
            list.clone(),
            std::ptr::null_mut(),
            app.clone(),
        );
        let (snapshots, latest) = tokio::sync::watch::channel(None);
        state.borrow_mut().session_tx = Some(snapshots);
        for id in 1..=40 {
            let workspace = crate::workspace::Workspace::new_bound(
                id,
                id as usize,
                "Sample".into(),
                "/opt/team/repo".into(),
            );
            let row = gtk4::ListBoxRow::new();
            bind_workspace_row(&row, workspace.id);
            row.set_child(Some(&workspace_row_content(&workspace)));
            list.append(&row);
            state.borrow_mut().workspaces.push(workspace);
            wire_row_close_button(&row, state.clone(), &app);
            attach_sidebar_context_menu(&row, state.clone());
            row.activate_action("workspace.color-red", None).unwrap();
            assert_eq!(
                state.borrow().workspaces[0].color.as_deref(),
                Some("#703a40")
            );
            assert_eq!(
                latest.borrow().as_ref().unwrap().workspaces[0]
                    .color
                    .as_deref(),
                Some("#703a40")
            );
            row.activate_action("workspace.color-default", None)
                .unwrap();
            assert_eq!(state.borrow().workspaces[0].color, None);
            let weak = row.downgrade();
            list.remove(&row);
            state.borrow_mut().workspaces.pop();
            drop(row);
            assert!(
                weak.upgrade().is_none(),
                "closed workspace retained by a signal callback"
            );
        }
        for _ in 1..=20 {
            state.borrow_mut().browser_manager = Some(crate::browser::BrowserManager::new());
            let widgets = crate::browser::create_preview_pane();
            let weak = widgets.container.downgrade();
            crate::browser::ui::wire_browser_tab(&state, widgets, uuid::Uuid::new_v4());
            // Deferred viewport work is finite, but must be allowed to release its clone.
            while glib::MainContext::default().pending() {
                glib::MainContext::default().iteration(false);
            }
            assert!(
                weak.upgrade().is_none(),
                "closed browser tree retained by its callbacks"
            );
        }
        for _ in 0..3 {
            state.borrow_mut().create_workspace();
        }
        let active = state.borrow().active_workspace().unwrap().uuid;
        let first_id = state.borrow().workspaces[0].id;
        assert!(state.borrow_mut().reorder_workspace(0, 2));
        assert_eq!(state.borrow().active_workspace().unwrap().uuid, active);
        let moved = list.row_at_index(2).unwrap();
        assert_eq!(
            unsafe { *moved.data::<u64>("workspace-id").unwrap().as_ref() },
            first_id
        );
        assert_eq!(
            latest.borrow().as_ref().unwrap().workspaces[2].uuid,
            state.borrow().workspaces[2].uuid.to_string()
        );
        assert!(!state.borrow_mut().reorder_workspace(0, 99));
        drop(moved);
        let weak_state = Rc::downgrade(&state);
        drop(state);
        assert!(weak_state.upgrade().is_none());
    }
}
