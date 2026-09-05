//! Workspace pane tree, surface ownership and interactive layout operations.

mod restore;
mod recovery;

use crate::ghostty::ffi;
use gtk4::prelude::*;
use std::sync::atomic::Ordering;
use uuid::Uuid;

/// Owned pane snapshot for protocol listings; numeric identity lasts for this application session.
pub struct PaneInfo {
    pub id: u64,
    pub surface_ids: Vec<Uuid>,
    pub selected_surface: Option<Uuid>,
}

/// Direction for pane focus navigation (Ctrl+Shift+arrows per D-10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusDirection {
    Left,
    Right,
    Up,
    Down,
}

/// A terminal or browser surface shown as a tab inside one pane.
#[derive(Clone)]
pub enum PaneSurface {
    Terminal {
        gl_area: gtk4::GLArea,
        uuid: Uuid,
    },
    Browser {
        widgets: crate::browser::PreviewPaneWidgets,
        uuid: Uuid,
    },
}

impl PaneSurface {
    /// Return the stable tab identity shared by persistence and socket commands.
    fn uuid(&self) -> Uuid {
        match self {
            Self::Terminal { uuid, .. } | Self::Browser { uuid, .. } => *uuid,
        }
    }

    /// Clone the GTK page widget without transferring native terminal ownership.
    fn widget(&self) -> gtk4::Widget {
        match self {
            Self::Terminal { gl_area, .. } => gl_area.clone().upcast(),
            Self::Browser { widgets, .. } => widgets.container.clone().upcast(),
        }
    }

    /// Select the default notebook title for a terminal or browser tab.
    fn tab_title(&self) -> &'static str {
        match self {
            Self::Terminal { .. } => "Terminal",
            Self::Browser { .. } => "Browser",
        }
    }

    /// Clone a terminal's GLArea; browser tabs deliberately have no terminal widget.
    fn terminal_area(&self) -> Option<gtk4::GLArea> {
        match self {
            Self::Terminal { gl_area, .. } => Some(gl_area.clone()),
            Self::Browser { .. } => None,
        }
    }

    /// Clone the browser's address entry for navigation or focus, excluding terminal tabs.
    fn url_entry(&self) -> Option<gtk4::Entry> {
        match self {
            Self::Browser { widgets, .. } => Some(widgets.url_entry.clone()),
            Self::Terminal { .. } => None,
        }
    }
}

/// Resolve a realized terminal handle without reading application-owned GTK object data.
fn surface_for_area(area: &gtk4::GLArea) -> Option<ffi::ghostty_surface_t> {
    crate::ghostty::callbacks::GL_TO_SURFACE
        .lock()
        .ok()
        .and_then(|registry| registry.get(&(area.as_ptr() as usize)).copied())
        .map(|surface| surface as ffi::ghostty_surface_t)
}

/// Stop a terminal and remove every global callback route before its widget is
/// detached. Ghostty synchronously stops the PTY/IO and renderer threads in
/// `ghostty_surface_free`, so the shell is gone before GTK destroys the pane.
pub(crate) fn destroy_terminal_area(area: &gtk4::GLArea) {
    unsafe {
        if let Some(retired) =
            area.data::<std::rc::Rc<std::cell::Cell<bool>>>("cmux-surface-retired")
        {
            retired.as_ref().set(true);
        }
        if let Some((bridge, ctx)) = area.steal_data::<(
            std::sync::Arc<crate::ssh::bridge::SshBridge>,
            std::sync::Arc<crate::ssh::bridge::IoWriteContext>,
        )>("cmux-remote-context")
        {
            ctx.surface_ptr.store(0, Ordering::Release);
            bridge.remove_context(ctx.pane_id);
        }
    }
    let raw_area = area.as_ptr();
    // Controllers may receive focus/resize signals during GTK teardown.
    // Clear their shared handle before freeing Ghostty, avoiding callbacks into freed memory.
    unsafe {
        if let Some(cell) = area
            .data::<std::rc::Rc<std::cell::RefCell<Option<ffi::ghostty_surface_t>>>>(
                "cmux-surface-cell",
            )
        {
            *cell.as_ref().borrow_mut() = None;
        }
    }
    // GtkGLArea does not dispose application-owned popover children for us.
    while let Some(child) = area.first_child() {
        child.unparent();
    }
    let surface = crate::ghostty::callbacks::GL_TO_SURFACE
        .lock()
        .ok()
        .and_then(|mut registry| registry.remove(&(raw_area as usize)))
        .map(|raw| raw as ffi::ghostty_surface_t);

    let Some(surface) = surface else {
        return;
    };
    unsafe { ffi::ghostty_surface_set_focus(surface, false) };
    if area.is_realized() {
        area.make_current();
        if area.error().is_none() {
            unsafe { ffi::ghostty_surface_display_unrealized(surface) };
        }
    }
    unsafe { ffi::ghostty_surface_free(surface) };
    crate::ghostty::registry::unregister(surface as usize);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseSurfaceResult {
    Closed,
    LastSurfaceInPane,
    NotFound,
}

/// Route tab-close controls through the window action that owns safe native teardown.
fn request_surface_tab_close(widget: &impl IsA<gtk4::Widget>, uuid: Uuid) {
    let _ = widget.activate_action(
        "win.close-surface-tab",
        Some(&uuid.to_string().to_variant()),
    );
}

/// Build a tab label and close affordance with weak widget captures to avoid ownership cycles.
fn surface_tab_label(surface: &PaneSurface) -> gtk4::Box {
    let uuid = surface.uuid();
    let tab = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    tab.add_css_class("surface-tab-label");
    let label = gtk4::Label::new(Some(surface.tab_title()));
    let close = gtk4::Button::from_icon_name("window-close-symbolic");
    close.add_css_class("surface-tab-close");
    close.set_tooltip_text(Some("Close Tab"));
    close.set_focusable(false);
    close.connect_clicked({
        let tab = tab.downgrade();
        move |_| {
            if let Some(tab) = tab.upgrade() {
                request_surface_tab_close(&tab, uuid);
            }
        }
    });
    tab.append(&label);
    tab.append(&close);

    let popover = gtk4::Popover::new();
    popover.set_parent(&tab);
    popover.set_has_arrow(false);
    let close_item = gtk4::Button::with_label("Close Tab");
    close_item.add_css_class("flat");
    close_item.connect_clicked({
        let tab = tab.downgrade();
        move |_| {
            if let Some(tab) = tab.upgrade() {
                request_surface_tab_close(&tab, uuid);
            }
        }
    });
    popover.set_child(Some(&close_item));
    let gesture = gtk4::GestureClick::new();
    gesture.set_button(3);
    gesture.connect_released({
        let popover = popover.downgrade();
        move |_, _, x, y| {
            let Some(popover) = popover.upgrade() else {
                return;
            };
            popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.popup();
        }
    });
    tab.add_controller(gesture);
    tab
}

/// Append a reorderable notebook page and its model entry, optionally selecting it.
fn append_pane_surface(
    notebook: &gtk4::Notebook,
    surfaces: &std::rc::Rc<std::cell::RefCell<Vec<PaneSurface>>>,
    surface: PaneSurface,
    select: bool,
) -> u32 {
    let label = surface_tab_label(&surface);
    let page = notebook.append_page(&surface.widget(), Some(&label));
    notebook.set_tab_reorderable(&surface.widget(), true);
    surfaces.borrow_mut().push(surface);
    if select {
        notebook.set_current_page(Some(page));
    }
    page
}

/// Construct a tabbed pane and synchronize native focus when its selected page changes.
fn create_pane(pane_id: u64, initial_surface: PaneSurface) -> SplitNode {
    let notebook = gtk4::Notebook::new();
    notebook.add_css_class("surface-tabs");
    notebook.set_scrollable(true);
    notebook.set_show_border(false);
    notebook.set_hexpand(true);
    notebook.set_vexpand(true);

    let actions = gtk4::Box::new(gtk4::Orientation::Horizontal, 2);
    let terminal_btn = gtk4::Button::from_icon_name("utilities-terminal-symbolic");
    terminal_btn.set_tooltip_text(Some("New Tab (Terminal) (Ctrl+T)"));
    terminal_btn.set_action_name(Some("win.new-terminal-tab"));
    terminal_btn.add_css_class("surface-tab-action");
    let browser_btn = gtk4::Button::from_icon_name("web-browser-symbolic");
    browser_btn.set_tooltip_text(Some("New Tab (Browser) (Ctrl+Shift+L)"));
    browser_btn.set_action_name(Some("win.new-browser-tab"));
    browser_btn.add_css_class("surface-tab-action");
    actions.append(&terminal_btn);
    actions.append(&browser_btn);
    notebook.set_action_widget(&actions, gtk4::PackType::End);

    let surfaces = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    append_pane_surface(&notebook, &surfaces, initial_surface, true);

    notebook.connect_switch_page({
        let surfaces = std::rc::Rc::downgrade(&surfaces);
        move |notebook, _, page| {
            let Some(surfaces) = surfaces.upgrade() else {
                return;
            };
            let is_active_pane = notebook.has_css_class("active-pane");
            for (index, surface) in surfaces.borrow().iter().enumerate() {
                if let Some(area) = surface.terminal_area() {
                    let selected = is_active_pane && index == page as usize;
                    if selected {
                        area.add_css_class("active-pane");
                        area.grab_focus();
                    } else {
                        area.remove_css_class("active-pane");
                    }
                    if let Some(handle) = surface_for_area(&area) {
                        unsafe { ffi::ghostty_surface_set_focus(handle, selected) };
                    }
                } else if is_active_pane && index == page as usize {
                    if let Some(entry) = surface.url_entry() {
                        let notebook = notebook.downgrade();
                        glib::idle_add_local_once(move || {
                            let Some(notebook) = notebook.upgrade() else {
                                return;
                            };
                            if notebook.has_css_class("active-pane")
                                && notebook.current_page() == Some(page)
                            {
                                entry.grab_focus();
                                entry.select_region(0, -1);
                            }
                        });
                    }
                }
            }
        }
    });

    SplitNode::Leaf {
        pane_id,
        notebook,
        surfaces,
        has_attention: false,
    }
}

/// Recursive pane layout tree. Each workspace has one root SplitNode.
/// - Leaf: one pane containing one or more terminal/browser surface tabs
/// - Split: two child pane subtrees separated by a GtkPaned divider
///
/// Per SPLIT-06: this is the Bonsplit Rust port — immutable-style tree where
/// split/close operations return a new root.
#[derive(Clone)]
pub enum SplitNode {
    Leaf {
        pane_id: u64,
        notebook: gtk4::Notebook,
        surfaces: std::rc::Rc<std::cell::RefCell<Vec<PaneSurface>>>,
        /// Phase 4 NOTF-01: true when this pane has unread bell activity.
        has_attention: bool,
    },
    Split {
        orientation: gtk4::Orientation,
        paned: gtk4::Paned,
        start: Box<SplitNode>,
        end: Box<SplitNode>,
    },
}

impl SplitNode {
    /// Returns the root GTK widget for this node.
    pub fn widget(&self) -> gtk4::Widget {
        match self {
            SplitNode::Leaf { notebook, .. } => notebook.clone().upcast(),
            SplitNode::Split { paned, .. } => paned.clone().upcast(),
        }
    }

    /// Find the pane_id of the active (focused) leaf by checking CSS class.
    pub fn find_active_pane_id(&self) -> Option<u64> {
        match self {
            SplitNode::Leaf {
                pane_id, notebook, ..
            } => {
                if notebook.has_css_class("active-pane") {
                    Some(*pane_id)
                } else {
                    None
                }
            }
            SplitNode::Split { start, end, .. } => start
                .find_active_pane_id()
                .or_else(|| end.find_active_pane_id()),
        }
    }

    /// Find the UUID for a pane by pane_id. Returns None if not found.
    pub fn find_uuid_for_pane(&self, target_id: u64) -> Option<String> {
        match self {
            SplitNode::Leaf {
                pane_id,
                notebook,
                surfaces,
                ..
            } => {
                if *pane_id == target_id {
                    let index = notebook.current_page().unwrap_or(0) as usize;
                    surfaces
                        .borrow()
                        .get(index)
                        .map(|surface| surface.uuid().to_string())
                } else {
                    None
                }
            }
            SplitNode::Split { start, end, .. } => start
                .find_uuid_for_pane(target_id)
                .or_else(|| end.find_uuid_for_pane(target_id)),
        }
    }

    /// Apply the active-pane CSS class to the leaf matching active_pane_id.
    /// Removes the class from all other leaves.
    pub fn update_focus_css(&self, active_pane_id: u64) {
        match self {
            SplitNode::Leaf {
                pane_id,
                notebook,
                surfaces,
                ..
            } => {
                if *pane_id == active_pane_id {
                    notebook.add_css_class("active-pane");
                } else {
                    notebook.remove_css_class("active-pane");
                }
                let active_index = notebook.current_page().unwrap_or(0) as usize;
                for (index, surface) in surfaces.borrow().iter().enumerate() {
                    if let Some(area) = surface.terminal_area() {
                        if *pane_id == active_pane_id && index == active_index {
                            area.add_css_class("active-pane");
                        } else {
                            area.remove_css_class("active-pane");
                        }
                    }
                }
            }
            SplitNode::Split { start, end, .. } => {
                start.update_focus_css(active_pane_id);
                end.update_focus_css(active_pane_id);
            }
        }
    }

    /// Find a node by pane_id.
    pub fn find_node(&self, target_id: u64) -> Option<&SplitNode> {
        match self {
            SplitNode::Leaf { pane_id, .. } => {
                if *pane_id == target_id {
                    Some(self)
                } else {
                    None
                }
            }
            SplitNode::Split { start, end, .. } => start
                .find_node(target_id)
                .or_else(|| end.find_node(target_id)),
        }
    }

    /// Collect terminal widgets so workspace teardown can stop each PTY while
    /// its GL context is still valid and unregister its callbacks.
    pub fn collect_terminal_areas(&self, out: &mut Vec<gtk4::GLArea>) {
        match self {
            SplitNode::Leaf { surfaces, .. } => {
                for surface in surfaces.borrow().iter() {
                    if let Some(area) = surface.terminal_area() {
                        out.push(area);
                    }
                }
            }
            SplitNode::Split { start, end, .. } => {
                start.collect_terminal_areas(out);
                end.collect_terminal_areas(out);
            }
        }
    }

    /// Find the Ghostty surface handle for a specific pane by pane_id.
    /// Used by debug.type to send text to a specific pane's surface.
    pub fn find_surface_for_pane(&self, target_id: u64) -> Option<ffi::ghostty_surface_t> {
        find_gl_area_in_tree(self, target_id).and_then(|area| surface_for_area(&area))
    }

    /// Collect (uuid, pane_id, active) for all leaves in this subtree.
    pub fn collect_pane_info(&self, out: &mut Vec<(Uuid, u64, bool)>, active_id: u64) {
        match self {
            SplitNode::Leaf {
                pane_id,
                notebook,
                surfaces,
                ..
            } => {
                let selected = notebook.current_page().unwrap_or(0) as usize;
                for (index, surface) in surfaces.borrow().iter().enumerate() {
                    out.push((
                        surface.uuid(),
                        *pane_id,
                        *pane_id == active_id && index == selected,
                    ));
                }
            }
            SplitNode::Split { start, end, .. } => {
                start.collect_pane_info(out, active_id);
                end.collect_pane_info(out, active_id);
            }
        }
    }

    /// Clone the terminal tab widget matching a UUID anywhere in this subtree, including hidden tabs.
    fn find_terminal_by_uuid(&self, target_uuid: &str) -> Option<gtk4::GLArea> {
        match self {
            SplitNode::Leaf { surfaces, .. } => {
                surfaces.borrow().iter().find_map(|surface| match surface {
                    PaneSurface::Terminal { gl_area, uuid } if uuid.to_string() == target_uuid => {
                        Some(gl_area.clone())
                    }
                    _ => None,
                })
            }
            SplitNode::Split { start, end, .. } => start
                .find_terminal_by_uuid(target_uuid)
                .or_else(|| end.find_terminal_by_uuid(target_uuid)),
        }
    }

    /// Find the pane_id for the leaf matching target_uuid (UUID string).
    pub fn find_pane_id_by_uuid(&self, target_uuid: &str) -> Option<u64> {
        match self {
            SplitNode::Leaf {
                surfaces, pane_id, ..
            } => surfaces
                .borrow()
                .iter()
                .any(|surface| surface.uuid().to_string() == target_uuid)
                .then_some(*pane_id),
            SplitNode::Split { start, end, .. } => start
                .find_pane_id_by_uuid(target_uuid)
                .or_else(|| end.find_pane_id_by_uuid(target_uuid)),
        }
    }

    /// Set has_attention on the leaf matching pane_id. Returns true if found.
    pub fn set_attention(&mut self, target_pane_id: u64, value: bool) -> bool {
        match self {
            SplitNode::Leaf {
                pane_id,
                has_attention,
                ..
            } => {
                if *pane_id == target_pane_id {
                    *has_attention = value;
                    true
                } else {
                    false
                }
            }
            SplitNode::Split { start, end, .. } => {
                start.set_attention(target_pane_id, value)
                    || end.set_attention(target_pane_id, value)
            }
        }
    }

    /// Returns true if any leaf in this subtree has attention.
    pub fn any_attention(&self) -> bool {
        match self {
            SplitNode::Leaf { has_attention, .. } => *has_attention,
            SplitNode::Split { start, end, .. } => start.any_attention() || end.any_attention(),
        }
    }

    /// Check if a specific pane has attention.
    pub fn pane_has_attention(&self, target_pane_id: u64) -> bool {
        match self {
            SplitNode::Leaf {
                pane_id,
                has_attention,
                ..
            } => *pane_id == target_pane_id && *has_attention,
            SplitNode::Split { start, end, .. } => {
                start.pane_has_attention(target_pane_id) || end.pane_has_attention(target_pane_id)
            }
        }
    }

    /// Clear attention on all leaves in this subtree.
    pub fn clear_all_attention(&mut self) {
        match self {
            SplitNode::Leaf { has_attention, .. } => *has_attention = false,
            SplitNode::Split { start, end, .. } => {
                start.clear_all_attention();
                end.clear_all_attention();
            }
        }
    }
}

/// Attach right-click context menu to a terminal GLArea (D-08).
/// Uses button 3 (right-click only) to avoid interfering with Ghostty's mouse handling.
fn attach_terminal_context_menu(gl_area: &gtk4::GLArea) {
    let menu_model = crate::menus::build_terminal_context_menu();
    let popover = gtk4::PopoverMenu::from_model(Some(&menu_model));
    popover.set_parent(gl_area);
    popover.set_has_arrow(false);

    let gesture = gtk4::GestureClick::new();
    gesture.set_button(3); // Right-click only
    gesture.connect_released({
        let popover = popover.downgrade();
        move |_, _, x, y| {
            let Some(popover) = popover.upgrade() else {
                return;
            };
            popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.popup();
        }
    });
    gl_area.add_controller(gesture);
}

/// SplitEngine manages one workspace's pane layout tree.
pub struct SplitEngine {
    pub root: SplitNode,
    pub active_pane_id: u64,
    /// Monotonically increasing pane ID counter.
    next_pane_id: u64,
    /// Ghostty app handle needed to create new surfaces.
    ghostty_app: ffi::ghostty_app_t,
    /// Workspace binding used for every new local terminal pane.
    working_directory: Option<std::path::PathBuf>,
    pub launch_command: Option<String>,
    pub remote_launch: Option<crate::ghostty::surface::SurfaceIoMode>,
}

impl SplitEngine {
    /// Take ownership of an initial terminal widget and create one selected pane on GTK.
    /// The widget owns its deferred native-surface initialization and cleanup callbacks.
    pub fn new(
        ghostty_app: ffi::ghostty_app_t,
        initial_gl_area: gtk4::GLArea,
        pane_id: u64,
        working_directory: Option<std::path::PathBuf>,
    ) -> Self {
        attach_terminal_context_menu(&initial_gl_area);
        let root = create_pane(
            pane_id,
            PaneSurface::Terminal {
                gl_area: initial_gl_area,
                uuid: Uuid::new_v4(),
            },
        );
        root.update_focus_css(pane_id);
        SplitEngine {
            root,
            active_pane_id: pane_id,
            next_pane_id: pane_id + 1,
            ghostty_app,
            working_directory,
            launch_command: None,
            remote_launch: None,
        }
    }

    /// Returns the root widget of this workspace's split tree.
    pub fn root_widget(&self) -> gtk4::Widget {
        self.root.widget()
    }

    /// Grab GTK keyboard focus for the active pane's GLArea.
    /// Called after workspace switch so key events route to Ghostty, not the sidebar.
    pub fn grab_active_focus(&self) {
        if let Some(gl_area) = self.find_gl_area(self.active_pane_id) {
            gl_area.grab_focus();
        } else if let Some(entry) = find_url_entry_in_tree(&self.root, self.active_pane_id) {
            entry.grab_focus();
        }
    }

    /// Returns the UUID of the currently active pane, if found.
    pub fn active_pane_uuid(&self) -> Option<String> {
        self.root.find_uuid_for_pane(self.active_pane_id)
    }

    /// Restore GTK and native focus to this workspace's selected surface on the GTK thread.
    /// Resolve ownership through the pane tree; CSS classes in other workspaces are not identity.
    pub fn focus_active_surface(&self) {
        self.grab_active_focus();
        if let Some(area) = self.find_gl_area(self.active_pane_id) {
            if let Some(surface) = surface_for_area(&area) {
                // SAFETY: the selected widget owns this live native handle; lookup releases
                // the registry lock before calling Ghostty, which may invoke GTK callbacks.
                unsafe { ffi::ghostty_surface_set_focus(surface, true) };
            }
            if area.is_realized() {
                area.queue_render();
            }
        }
    }

    /// Mark a pane active without changing GTK focus. Pointer handlers use this
    /// before focusing the exact child that was clicked.
    pub fn activate_pane(&mut self, pane_id: u64) -> bool {
        if self.root.find_node(pane_id).is_none() {
            return false;
        }
        self.active_pane_id = pane_id;
        self.root.update_focus_css(pane_id);
        true
    }

    /// Select a surface's notebook page and owning pane, then move GTK keyboard focus there.
    /// Return false without changing selection when the surface is absent from this workspace.
    pub fn focus_surface(&mut self, uuid: &str) -> bool {
        let Some(pane_id) = self.root.find_pane_id_by_uuid(uuid) else {
            return false;
        };
        let Some((notebook, surfaces)) = find_pane_tabs(&self.root, pane_id) else {
            return false;
        };
        let page = surfaces.borrow().iter()
            .find(|surface| surface.uuid().to_string() == uuid)
            .and_then(|surface| notebook.page_num(&surface.widget()));
        let Some(page) = page else {
            return false;
        };
        // Release the surface-list borrow before GTK emits switch-page callbacks.
        self.active_pane_id = pane_id;
        notebook.set_current_page(Some(page));
        self.root.update_focus_css(pane_id);
        self.grab_active_focus();
        true
    }

    /// Split the active pane to the right (Ctrl+D per D-10).
    /// Replaces the active Leaf with a Split(Horizontal) containing the old leaf + new leaf.
    /// Per D-08: new surface inherits CWD via ghostty_surface_inherited_config.
    /// Per D-09: initial split ratio is 50/50 (set in paned.connect_realize).
    /// Per SPLIT-07: new pane receives focus immediately.
    pub fn split_right(&mut self) -> Option<u64> {
        self.split_active(gtk4::Orientation::Horizontal)
    }

    /// Split the active pane downward (Ctrl+Shift+D per D-10).
    pub fn split_down(&mut self) -> Option<u64> {
        self.split_active(gtk4::Orientation::Vertical)
    }

    /// Split the active pane and focus a new terminal, inheriting native context when available.
    /// Browser-only panes use workspace launch settings. Return None for a missing active pane.
    pub fn split_active(&mut self, orientation: gtk4::Orientation) -> Option<u64> {
        let active_id = self.active_pane_id;
        self.root.find_node(active_id)?;
        let new_pane_id = self.next_pane_id;
        self.next_pane_id += 1;

        // When the root is a Leaf (first split), the GLArea is a direct child of the GtkStack
        // page. The replacer will remove it from the Stack (via remove_widget_from_parent) and
        // place it inside the new Paned. We then need to add the Paned to the Stack page.
        // Only capture this for Leaf roots — for nested splits the outer Paned stays in the Stack.
        let old_root_widget = self.root.widget();
        let stack_slot: Option<(gtk4::Stack, String)> =
            if matches!(self.root, SplitNode::Leaf { .. }) {
                old_root_widget
                    .parent()
                    .and_then(|p| p.downcast::<gtk4::Stack>().ok())
                    .and_then(|stack| {
                        let name = stack.page(&old_root_widget).name()?.to_string();
                        Some((stack, name))
                    })
            } else {
                None
            };

        let inherited_config = find_any_terminal_surface(&self.root, active_id).map(|surface| unsafe {
            // Stop the previous terminal receiving input before the new pane takes focus.
            ffi::ghostty_surface_set_focus(surface, false);
            ffi::ghostty_surface_inherited_config(
                surface,
                ffi::ghostty_surface_context_e_GHOSTTY_SURFACE_CONTEXT_SPLIT,
            )
        });
        let new_gl_area = self.create_terminal_widget(new_pane_id, inherited_config);

        // Replace the active leaf in the tree with a Split node.
        let new_leaf = create_pane(
            new_pane_id,
            PaneSurface::Terminal {
                gl_area: new_gl_area.clone(),
                uuid: Uuid::new_v4(),
            },
        );

        self.replace_leaf_with_split(active_id, new_leaf, orientation)?;

        // If the root was a Leaf, it's now a Split whose Paned has no parent.
        // Re-parent the new Paned root into the GtkStack page we saved above.
        if let Some((stack, name)) = stack_slot {
            let new_root = self.root.widget();
            stack.add_named(&new_root, Some(&name));
            stack.set_visible_child_name(&name);
        }

        // After realize, update active focus to the new pane.
        self.active_pane_id = new_pane_id;
        self.root.update_focus_css(new_pane_id);

        // Focus the new GLArea widget so it receives keyboard events.
        new_gl_area.grab_focus();

        Some(new_pane_id)
    }

    /// Construct a terminal widget on GTK with shared workspace launch and context-menu policy.
    /// Native initialization is deferred to realization; the widget owns surface cleanup.
    fn create_terminal_widget(
        &self,
        pane_id: u64,
        inherited: Option<ffi::ghostty_surface_config_s>,
    ) -> gtk4::GLArea {
        let (gl_area, _surface_cell) = crate::ghostty::surface::create_surface(
            self.ghostty_app,
            inherited,
            self.working_directory.clone(),
            pane_id,
            self.remote_launch.clone().unwrap_or_else(|| {
                self.launch_command
                    .clone()
                    .map(crate::ghostty::surface::SurfaceIoMode::Command)
                    .unwrap_or(crate::ghostty::surface::SurfaceIoMode::Exec)
            }),
        );
        attach_terminal_context_menu(&gl_area);
        gl_area
    }

    /// Create and select a terminal surface tab in the focused pane.
    pub fn new_terminal_tab(&mut self) -> Option<Uuid> {
        let pane_id = self.active_pane_id;
        let inherited = find_any_terminal_surface(&self.root, pane_id).map(|surface| unsafe {
            ffi::ghostty_surface_inherited_config(
                surface,
                ffi::ghostty_surface_context_e_GHOSTTY_SURFACE_CONTEXT_TAB,
            )
        });
        if let Some(surface) = self.find_surface(pane_id) {
            unsafe { ffi::ghostty_surface_set_focus(surface, false) };
        }
        let gl_area = self.create_terminal_widget(pane_id, inherited);
        let uuid = Uuid::new_v4();
        let (notebook, surfaces) = find_pane_tabs(&self.root, pane_id)?;
        append_pane_surface(
            &notebook,
            &surfaces,
            PaneSurface::Terminal {
                gl_area: gl_area.clone(),
                uuid,
            },
            true,
        );
        self.root.update_focus_css(pane_id);
        gl_area.grab_focus();
        Some(uuid)
    }

    /// Select a pane and create a terminal tab there (used by Ghostty actions).
    pub fn new_terminal_tab_for_pane(&mut self, pane_id: u64) -> Option<Uuid> {
        self.root.find_node(pane_id)?;
        self.active_pane_id = pane_id;
        self.root.update_focus_css(pane_id);
        self.new_terminal_tab()
    }

    /// Create and select a browser surface tab in the focused pane.
    pub fn split_active_with_preview(&mut self) -> Option<crate::browser::PreviewPaneWidgets> {
        let active_id = self.active_pane_id;
        let widgets = crate::browser::create_preview_pane(active_id);

        // Phase 9: Attach right-click context menu to browser preview (D-09)
        {
            let menu_model = crate::menus::build_browser_context_menu();
            let popover = gtk4::PopoverMenu::from_model(Some(&menu_model));
            popover.set_parent(&widgets.container);
            popover.set_has_arrow(false);

            let gesture = gtk4::GestureClick::new();
            gesture.set_button(3); // Right-click only
            gesture.connect_released({
                let popover = popover.clone();
                move |_, _, x, y| {
                    popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(
                        x as i32, y as i32, 1, 1,
                    )));
                    popover.popup();
                }
            });
            widgets.container.add_controller(gesture);
        }

        let (notebook, surfaces) = find_pane_tabs(&self.root, active_id)?;
        append_pane_surface(
            &notebook,
            &surfaces,
            PaneSurface::Browser {
                widgets: widgets.clone(),
                uuid: widgets.uuid,
            },
            true,
        );
        self.root.update_focus_css(active_id);
        widgets.url_entry.grab_focus();
        widgets.url_entry.select_region(0, -1);

        Some(widgets)
    }

    /// Replace the leaf with `target_pane_id` with a Split(orientation) node.
    /// Returns Some(()) on success, None if the leaf was not found.
    fn replace_leaf_with_split(
        &mut self,
        target_pane_id: u64,
        new_leaf: SplitNode,
        orientation: gtk4::Orientation,
    ) -> Option<()> {
        let orientation_cap = orientation;
        let mut replacer = Some(|old_leaf: SplitNode| {
            let old_widget = old_leaf.widget();
            let new_widget = new_leaf.widget();

            // GTK4 requires a widget to have no parent before set_start/end_child.
            // old_widget may be parented to the Stack (first split) or an outer Paned (nested).
            remove_widget_from_parent(&old_widget);

            let paned = gtk4::Paned::new(orientation_cap);
            // Both children must be allowed to resize — GTK4 default for resize_end_child
            // is TRUE but be explicit to ensure drag works in both directions.
            paned.set_resize_start_child(true);
            paned.set_resize_end_child(true);
            // Prevent children from collapsing to 0px when dragging to an extreme.
            paned.set_shrink_start_child(false);
            paned.set_shrink_end_child(false);
            // Wide handle makes the divider grabable (default is ~5px, hard to click).
            paned.set_wide_handle(true);

            paned.set_start_child(Some(&old_widget));
            paned.set_end_child(Some(&new_widget));

            // Set 50/50 position after the first layout pass (per D-09 and RESEARCH Pitfall 2).
            // connect_realize fires before GTK allocates sizes, so p.width() is 0 there.
            // idle_add_local_once defers to the next main-loop idle, after layout completes.
            {
                let paned_ref = paned.clone();
                gtk4::glib::idle_add_local_once(move || {
                    let size = if orientation_cap == gtk4::Orientation::Horizontal {
                        paned_ref.width()
                    } else {
                        paned_ref.height()
                    };
                    if size > 0 {
                        paned_ref.set_position(size / 2);
                    }
                });
            }

            recovery::install(&paned);

            SplitNode::Split {
                orientation: orientation_cap,
                paned: paned.clone(),
                start: Box::new(old_leaf),
                end: Box::new(new_leaf),
            }
        });
        replace_in_tree(&mut self.root, target_pane_id, &mut replacer)
    }

    /// Close the active pane (Ctrl+Shift+X per UI-SPEC).
    /// Removes the active leaf, replaces its parent Split with the surviving sibling.
    /// Returns the new active pane_id, or None if this was the last pane.
    pub fn close_active(&mut self) -> Option<u64> {
        let active_id = self.active_pane_id;

        // Cannot close the last pane — workspace close is handled at AppState level.
        let is_single_pane =
            matches!(&self.root, SplitNode::Leaf { pane_id, .. } if *pane_id == active_id);
        if is_single_pane {
            return None; // Signal to AppState: close the workspace instead
        }

        let terminal_areas = find_pane_tabs(&self.root, active_id)
            .map(|(_, surfaces)| {
                surfaces
                    .borrow()
                    .iter()
                    .filter_map(PaneSurface::terminal_area)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        // End the PTYs and renderers while their GL contexts are still valid.
        for area in &terminal_areas {
            destroy_terminal_area(area);
        }

        // Remove the leaf from the tree and get the surviving sibling's pane_id.
        let surviving_id = remove_leaf_from_tree(&mut self.root, active_id)?;

        // Update focus to the surviving pane.
        self.active_pane_id = surviving_id;
        self.root.update_focus_css(surviving_id);

        // Call ghostty_surface_set_focus on the surviving surface (SPLIT-07).
        if let Some(surface) = self.find_surface(surviving_id) {
            unsafe {
                ffi::ghostty_surface_set_focus(surface, true);
            }
        }

        // Grab GTK focus on the surviving pane's widget (GLArea or URL entry).
        if let Some(gl_area) = self.find_gl_area(surviving_id) {
            gl_area.grab_focus();
        } else if let Some(entry) = find_url_entry_in_tree(&self.root, surviving_id) {
            entry.grab_focus();
        }

        Some(surviving_id)
    }

    /// Close a tab and its pane when empty, leaving final-workspace policy to the caller.
    /// LastSurfaceInPane means only the workspace's final pane remains; no workspace is removed here.
    pub fn close_surface_and_empty_pane(&mut self, uuid: Uuid) -> CloseSurfaceResult {
        match self.close_surface_tab(uuid) {
            CloseSurfaceResult::LastSurfaceInPane if self.close_active().is_some() => {
                CloseSurfaceResult::Closed
            }
            result => result,
        }
    }

    /// Close one sibling surface tab without removing its containing pane.
    /// Closing the final surface delegates to the existing pane-close flow.
    pub fn close_surface_tab(&mut self, uuid: Uuid) -> CloseSurfaceResult {
        let Some(pane_id) = self.find_pane_id_by_uuid(&uuid.to_string()) else {
            crate::diagnostics::event(format_args!("surface-tab close not found uuid={uuid}"));
            return CloseSurfaceResult::NotFound;
        };
        let Some((notebook, surfaces)) = find_pane_tabs(&self.root, pane_id) else {
            crate::diagnostics::event(format_args!(
                "surface-tab close missing pane tabs uuid={uuid} pane={pane_id}"
            ));
            return CloseSurfaceResult::NotFound;
        };
        let index = surfaces
            .borrow()
            .iter()
            .position(|surface| surface.uuid() == uuid);
        let Some(index) = index else {
            crate::diagnostics::event(format_args!(
                "surface-tab close missing surface uuid={uuid} pane={pane_id}"
            ));
            return CloseSurfaceResult::NotFound;
        };
        if surfaces.borrow().len() == 1 {
            crate::diagnostics::event(format_args!(
                "surface-tab close delegates to pane uuid={uuid} pane={pane_id}"
            ));
            self.active_pane_id = pane_id;
            self.root.update_focus_css(pane_id);
            return CloseSurfaceResult::LastSurfaceInPane;
        }

        let surface = surfaces.borrow()[index].clone();
        crate::diagnostics::event(format_args!(
            "surface-tab closing uuid={uuid} pane={pane_id} kind={} index={index}",
            surface.tab_title().to_ascii_lowercase(),
        ));
        let widget = surface.widget();
        if let Some(area) = surface.terminal_area() {
            destroy_terminal_area(&area);
        }
        if let Some(page) = notebook.page_num(&widget) {
            notebook.remove_page(Some(page));
        }
        surfaces.borrow_mut().remove(index);
        self.active_pane_id = pane_id;
        self.root.update_focus_css(pane_id);
        self.focus_active_surface();
        crate::diagnostics::event(format_args!(
            "surface-tab closed uuid={uuid} pane={pane_id}"
        ));
        CloseSurfaceResult::Closed
    }

    /// Navigate focus to the pane adjacent in `direction` (Ctrl+Alt+arrows per D-10).
    pub fn focus_next_in_direction(&mut self, direction: FocusDirection) -> bool {
        let active_id = self.active_pane_id;
        if let Some(new_id) = find_adjacent(&self.root, active_id, direction) {
            // Unfocus old surface.
            if let Some(old_surface) = self.find_surface(active_id) {
                unsafe {
                    ffi::ghostty_surface_set_focus(old_surface, false);
                }
            }
            self.active_pane_id = new_id;
            self.root.update_focus_css(new_id);
            // Focus new surface or Preview URL entry.
            if let Some(new_surface) = self.find_surface(new_id) {
                unsafe {
                    ffi::ghostty_surface_set_focus(new_surface, true);
                }
            }
            if let Some(gl_area) = self.find_gl_area(new_id) {
                gl_area.grab_focus();
            } else if let Some(entry) = find_url_entry_in_tree(&self.root, new_id) {
                entry.grab_focus();
            }
            true
        } else {
            false
        }
    }

    /// Resolve only the selected terminal; a selected browser never falls back to a hidden terminal.
    fn find_surface(&self, pane_id: u64) -> Option<ffi::ghostty_surface_t> {
        self.root.find_surface_for_pane(pane_id)
    }

    /// Resolve the pane's selected terminal widget for focus and rendering operations.
    fn find_gl_area(&self, pane_id: u64) -> Option<gtk4::GLArea> {
        find_gl_area_in_tree(&self.root, pane_id)
    }

    /// Returns all leaf panes in this engine as (uuid, pane_id, active) tuples.
    pub fn all_panes(&self) -> Vec<(Uuid, u64, bool)> {
        let mut panes = Vec::new();
        self.root.collect_pane_info(&mut panes, self.active_pane_id);
        panes
    }

    /// Snapshot split panes in traversal order, keeping sibling tabs grouped under their owner.
    pub fn pane_info(&self) -> Vec<PaneInfo> {
        let mut panes = Vec::new();
        collect_pane_snapshots(&self.root, &mut panes);
        panes
    }

    /// Focus a session-local pane reference or a legacy surface UUID without switching its tab.
    pub fn focus_pane_ref(&mut self, reference: &str) -> bool {
        let pane_id = if let Some(number) = reference.strip_prefix("pane:") {
            number.parse::<u64>().ok()
        } else {
            self.find_pane_id_by_uuid(reference)
        };
        let Some(pane_id) = pane_id else { return false; };
        if !self.activate_pane(pane_id) {
            return false;
        }
        self.grab_active_focus();
        true
    }

    /// Clone a terminal widget by stable tab identity without changing notebook selection or focus.
    pub fn gl_area_for_surface(&self, uuid: &str) -> Option<gtk4::GLArea> {
        self.root.find_terminal_by_uuid(uuid)
    }

    /// Look up a surface by its UUID string. Returns the ghostty surface handle if found.
    pub fn find_surface_by_uuid(&self, target_uuid: &str) -> Option<ffi::ghostty_surface_t> {
        self.gl_area_for_surface(target_uuid).and_then(|area| surface_for_area(&area))
    }

    /// Look up a pane_id by its UUID string.
    pub fn find_pane_id_by_uuid(&self, target_uuid: &str) -> Option<u64> {
        self.root.find_pane_id_by_uuid(target_uuid)
    }

    /// Look up a GLArea by pane_id (public wrapper for socket handlers).
    pub fn gl_area_for_pane(&self, pane_id: u64) -> Option<gtk4::GLArea> {
        find_gl_area_in_tree(&self.root, pane_id)
    }

    /// Browser tabs reconstructed from a saved session and awaiting signal wiring.
    pub fn browser_tabs(&self) -> Vec<crate::browser::PreviewPaneWidgets> {
        let mut tabs = Vec::new();
        collect_browser_tabs(&self.root, &mut tabs);
        tabs
    }
}

/// Return the first browser picture in a subtree for socket-driven frames.
pub fn first_browser_picture(node: &SplitNode) -> Option<gtk4::Picture> {
    match node {
        SplitNode::Leaf { surfaces, .. } => surfaces.borrow().iter().find_map(|surface| {
            if let PaneSurface::Browser { widgets, .. } = surface {
                Some(widgets.picture.clone())
            } else {
                None
            }
        }),
        SplitNode::Split { start, end, .. } => {
            first_browser_picture(start).or_else(|| first_browser_picture(end))
        }
    }
}

/// Collect browser widgets with their stable surface IDs across the pane tree.
fn collect_browser_tabs(node: &SplitNode, out: &mut Vec<crate::browser::PreviewPaneWidgets>) {
    match node {
        SplitNode::Leaf { surfaces, .. } => {
            out.extend(surfaces.borrow().iter().filter_map(|surface| {
                if let PaneSurface::Browser { widgets, .. } = surface {
                    Some(widgets.clone())
                } else {
                    None
                }
            }));
        }
        SplitNode::Split { start, end, .. } => {
            collect_browser_tabs(start, out);
            collect_browser_tabs(end, out);
        }
    }
}

// ── Tree traversal helpers ───────────────────────────────────────────────────

/// Replace the leaf with `target_id` using `replacer` function. Returns Some(()) if found.
fn replace_in_tree<F>(node: &mut SplitNode, target_id: u64, replacer: &mut Option<F>) -> Option<()>
where
    F: FnOnce(SplitNode) -> SplitNode,
{
    match node {
        SplitNode::Leaf { pane_id, .. } if *pane_id == target_id => {
            if let Some(r) = replacer.take() {
                // Take ownership of the old node to pass to replacer.
                let old = std::mem::replace(
                    node,
                    create_pane(
                        0,
                        PaneSurface::Terminal {
                            gl_area: gtk4::GLArea::new(),
                            uuid: Uuid::new_v4(),
                        },
                    ),
                );
                *node = r(old);
                Some(())
            } else {
                None
            }
        }
        SplitNode::Leaf { .. } => None,
        SplitNode::Split {
            start, end, paned, ..
        } => {
            if let Some(()) = replace_in_tree(start, target_id, replacer) {
                // Update paned start child to new widget.
                paned.set_start_child(Some(&start.widget()));
                Some(())
            } else if let Some(()) = replace_in_tree(end, target_id, replacer) {
                paned.set_end_child(Some(&end.widget()));
                Some(())
            } else {
                None
            }
        }
    }
}

/// Remove leaf `target_id` from the tree. Returns the surviving sibling's pane_id.
/// Replaces the parent Split with the surviving sibling in the GTK widget tree.
fn remove_leaf_from_tree(node: &mut SplitNode, target_id: u64) -> Option<u64> {
    match node {
        SplitNode::Leaf { .. } => None, // Caller ensures we never remove the root leaf
        SplitNode::Split {
            start, end, paned, ..
        } => {
            // Check if start is the target leaf.
            let start_is_target = match start.as_ref() {
                SplitNode::Leaf { pane_id, .. } => *pane_id == target_id,
                _ => false,
            };
            if start_is_target {
                // Surviving sibling is end. Replace this Split with end in the GTK tree.
                let surviving = *end.clone();
                let surviving_widget = surviving.widget();
                // Detach from the split being removed before inserting into
                // its parent; GTK rejects widgets that already have a parent.
                remove_widget_from_parent(&surviving_widget);
                // Find the paned's parent and replace it with the surviving widget.
                if let Some(parent) = paned.parent() {
                    replace_child_in_parent(&parent, &paned.clone().upcast(), &surviving_widget);
                }
                let surviving_id = first_pane_id(&surviving);
                *node = surviving;
                return Some(surviving_id);
            }
            // Check if end is the target leaf.
            let end_is_target = match end.as_ref() {
                SplitNode::Leaf { pane_id, .. } => *pane_id == target_id,
                _ => false,
            };
            if end_is_target {
                let surviving = *start.clone();
                let surviving_widget = surviving.widget();
                remove_widget_from_parent(&surviving_widget);
                if let Some(parent) = paned.parent() {
                    replace_child_in_parent(&parent, &paned.clone().upcast(), &surviving_widget);
                }
                let surviving_id = first_pane_id(&surviving);
                *node = surviving;
                return Some(surviving_id);
            }
            // Recurse into start subtree.
            if let Some(id) = remove_leaf_from_tree(start, target_id) {
                paned.set_start_child(Some(&start.widget()));
                return Some(id);
            }
            // Recurse into end subtree.
            if let Some(id) = remove_leaf_from_tree(end, target_id) {
                paned.set_end_child(Some(&end.widget()));
                return Some(id);
            }
            None
        }
    }
}

/// Replace `old_widget` with `new_widget` in `parent`. Handles GtkPaned children and GtkStack pages.
fn replace_child_in_parent(
    parent: &gtk4::Widget,
    old_widget: &gtk4::Widget,
    new_widget: &gtk4::Widget,
) {
    if let Some(paned) = parent.downcast_ref::<gtk4::Paned>() {
        if paned
            .start_child()
            .as_ref()
            .map(|w| w == old_widget)
            .unwrap_or(false)
        {
            paned.set_start_child(Some(new_widget));
        } else {
            paned.set_end_child(Some(new_widget));
        }
    } else if let Some(stack) = parent.downcast_ref::<gtk4::Stack>() {
        let page = stack.page(old_widget);
        if let Some(name) = page.name() {
            let name_str = name.to_string();
            stack.remove(old_widget);
            // new_widget may still be parented to the Paned we're replacing; unparent first.
            remove_widget_from_parent(new_widget);
            stack.add_named(new_widget, Some(&name_str));
            stack.set_visible_child_name(&name_str);
        } else {
            stack.remove(old_widget);
        }
    }
    // If parent is something else, the widget swap is a no-op (should not happen in Phase 2).
}

/// Return the first (leftmost/topmost) pane_id in a subtree.
fn first_pane_id(node: &SplitNode) -> u64 {
    match node {
        SplitNode::Leaf { pane_id, .. } => *pane_id,
        SplitNode::Split { start, .. } => first_pane_id(start),
    }
}

/// Find a pane once and clone its notebook/model handles for selected-tab operations.
fn find_pane_tabs(
    node: &SplitNode,
    pane_id: u64,
) -> Option<(
    gtk4::Notebook,
    std::rc::Rc<std::cell::RefCell<Vec<PaneSurface>>>,
)> {
    match node {
        SplitNode::Leaf {
            pane_id: id,
            notebook,
            surfaces,
            ..
        } if *id == pane_id => Some((notebook.clone(), surfaces.clone())),
        SplitNode::Leaf { .. } => None,
        SplitNode::Split { start, end, .. } => {
            find_pane_tabs(start, pane_id).or_else(|| find_pane_tabs(end, pane_id))
        }
    }
}

/// Find a realized terminal for inheritance even when the pane currently shows a browser tab.
fn find_any_terminal_surface(node: &SplitNode, pane_id: u64) -> Option<ffi::ghostty_surface_t> {
    let (_, surfaces) = find_pane_tabs(node, pane_id)?;
    let found = surfaces
        .borrow()
        .iter()
        .filter_map(PaneSurface::terminal_area)
        .find_map(|area| surface_for_area(&area));
    found
}

/// Return the selected terminal widget in a pane located through the shared tree lookup.
fn find_gl_area_in_tree(node: &SplitNode, pane_id: u64) -> Option<gtk4::GLArea> {
    let (notebook, surfaces) = find_pane_tabs(node, pane_id)?;
    let page = notebook.current_page()?;
    let area = surfaces
        .borrow()
        .get(page as usize)
        .and_then(PaneSurface::terminal_area);
    area
}

/// Return the selected browser's address entry, excluding terminal tabs.
fn find_url_entry_in_tree(node: &SplitNode, pane_id: u64) -> Option<gtk4::Entry> {
    let (notebook, surfaces) = find_pane_tabs(node, pane_id)?;
    let page = notebook.current_page()?;
    let entry = surfaces
        .borrow()
        .get(page as usize)
        .and_then(PaneSurface::url_entry);
    entry
}

/// Find the pane adjacent to `active_id` in `direction`.
/// Strategy: collect ordered leaf positions and find the neighbor.
/// This is a directional approximation: Left/Up = previous leaf, Right/Down = next leaf.
/// A full spatial algorithm (comparing widget coordinates) can be added in a future phase.
fn find_adjacent(root: &SplitNode, active_id: u64, direction: FocusDirection) -> Option<u64> {
    let mut leaves = Vec::new();
    collect_leaves_in_order(root, &mut leaves);
    let pos = leaves.iter().position(|&id| id == active_id)?;
    match direction {
        FocusDirection::Left | FocusDirection::Up => {
            if pos > 0 {
                Some(leaves[pos - 1])
            } else {
                None
            }
        }
        FocusDirection::Right | FocusDirection::Down => {
            if pos + 1 < leaves.len() {
                Some(leaves[pos + 1])
            } else {
                None
            }
        }
    }
}

/// Remove `widget` from its current GTK parent so it can be reparented.
/// GTK4 requires `gtk_widget_get_parent(child) == NULL` before set_start/end_child.
fn remove_widget_from_parent(widget: &gtk4::Widget) {
    let Some(parent) = widget.parent() else {
        return;
    };
    if let Some(paned) = parent.downcast_ref::<gtk4::Paned>() {
        if paned
            .start_child()
            .as_ref()
            .map(|w| w == widget)
            .unwrap_or(false)
        {
            paned.set_start_child(None::<&gtk4::Widget>);
        } else {
            paned.set_end_child(None::<&gtk4::Widget>);
        }
    } else if let Some(stack) = parent.downcast_ref::<gtk4::Stack>() {
        stack.remove(widget);
    }
}

/// Copy pane/tab identities and notebook selection without retaining widgets or moving focus.
fn collect_pane_snapshots(node: &SplitNode, panes: &mut Vec<PaneInfo>) {
    match node {
        SplitNode::Leaf { pane_id, notebook, surfaces, .. } => {
            let surface_ids: Vec<Uuid> = surfaces.borrow().iter().map(PaneSurface::uuid).collect();
            let selected_surface = notebook.current_page()
                .and_then(|index| surface_ids.get(index as usize)).copied();
            panes.push(PaneInfo { id: *pane_id, surface_ids, selected_surface });
        }
        SplitNode::Split { start, end, .. } => {
            collect_pane_snapshots(start, panes);
            collect_pane_snapshots(end, panes);
        }
    }
}

/// Collect pane IDs in split traversal order for directional focus and restore fallback.
fn collect_leaves_in_order(node: &SplitNode, out: &mut Vec<u64>) {
    match node {
        SplitNode::Leaf { pane_id, .. } => out.push(*pane_id),
        SplitNode::Split { start, end, .. } => {
            collect_leaves_in_order(start, out);
            collect_leaves_in_order(end, out);
        }
    }
}

/// Serde-friendly mirror of SplitNode for session persistence.
/// GTK widget references (GLArea, Paned) cannot be serialized — this parallel type holds
/// only the data needed to reconstruct the tree on restore.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum SplitNodeData {
    Leaf {
        pane_id: u64,
        surface_uuid: Uuid,
        /// Shell executable path, e.g. "/bin/zsh" or "/bin/bash"
        shell: String,
        /// Last known terminal directory; empty until a launch path or native report is known.
        cwd: String,
    },
    Pane {
        #[serde(default)]
        active_surface_uuid: Option<Uuid>,
        #[serde(default)]
        surfaces: Vec<PaneSurfaceData>,
    },
    Split {
        /// "horizontal" or "vertical"
        orientation: String,
        /// Divider position as fraction 0.0-1.0 relative to parent size (D-03).
        #[serde(default = "default_ratio")]
        ratio: f64,
        start: Box<SplitNodeData>,
        end: Box<SplitNodeData>,
    },
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum PaneSurfaceData {
    Terminal {
        surface_uuid: Uuid,
        shell: String,
        cwd: String,
    },
    Browser {
        surface_uuid: Uuid,
        #[serde(default = "default_browser_url")]
        url: String,
    },
}

/// Supply a blank page when an older saved browser tab lacks its URL.
fn default_browser_url() -> String {
    "about:blank".to_string()
}

/// Restore equal pane sizes when a saved split omits its divider ratio.
fn default_ratio() -> f64 {
    0.5
}

impl SplitNode {
    /// Produce a serializable snapshot of this node's tree structure.
    /// Directories come from each terminal's native reports or explicit launch path.
    /// Unknown directories stay empty; the shell retains the configured environment default.
    pub fn to_data(&self) -> SplitNodeData {
        match self {
            SplitNode::Leaf {
                notebook, surfaces, ..
            } => {
                let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
                let surface_data = surfaces
                    .borrow()
                    .iter()
                    .map(|surface| match surface {
                        PaneSurface::Terminal { gl_area, uuid } => PaneSurfaceData::Terminal {
                            surface_uuid: *uuid,
                            shell: shell.clone(),
                            cwd: surface_for_area(gl_area)
                                .map(|pointer| {
                                    crate::ghostty::registry::working_directory(pointer as usize)
                                })
                                .unwrap_or_default(),
                        },
                        PaneSurface::Browser { widgets, uuid } => PaneSurfaceData::Browser {
                            surface_uuid: *uuid,
                            url: widgets.url_entry.text().to_string(),
                        },
                    })
                    .collect();
                let active_surface_uuid = notebook
                    .current_page()
                    .and_then(|page| surfaces.borrow().get(page as usize).map(PaneSurface::uuid));
                SplitNodeData::Pane {
                    active_surface_uuid,
                    surfaces: surface_data,
                }
            }
            SplitNode::Split {
                orientation,
                paned,
                start,
                end,
                ..
            } => {
                let total_size = if *orientation == gtk4::Orientation::Horizontal {
                    paned.width()
                } else {
                    paned.height()
                };
                let ratio = if total_size > 0 {
                    (paned.position() as f64) / (total_size as f64)
                } else {
                    0.5 // default if not yet laid out
                };
                SplitNodeData::Split {
                    orientation: match orientation {
                        gtk4::Orientation::Horizontal => "horizontal".to_string(),
                        gtk4::Orientation::Vertical => "vertical".to_string(),
                        _ => "horizontal".to_string(),
                    },
                    ratio,
                    start: Box::new(start.to_data()),
                    end: Box::new(end.to_data()),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify legacy leaf JSON retains the stable surface identity.
    #[test]
    fn split_node_data_leaf_has_surface_uuid() {
        // Build a minimal SplitNodeData::Leaf directly and verify surface_uuid field exists.
        let id = Uuid::new_v4();
        let data = SplitNodeData::Leaf {
            pane_id: 42,
            surface_uuid: id,
            shell: "/bin/bash".to_string(),
            cwd: "/home/user".to_string(),
        };
        if let SplitNodeData::Leaf {
            surface_uuid,
            pane_id,
            ..
        } = data
        {
            assert_eq!(surface_uuid, id);
            assert_eq!(pane_id, 42);
        } else {
            panic!("Expected SplitNodeData::Leaf");
        }
    }

    /// Preserve a legacy terminal leaf through JSON serialization.
    #[test]
    fn split_node_data_roundtrip_json() {
        // Verify SplitNodeData serializes and deserializes via serde_json.
        let leaf = SplitNodeData::Leaf {
            pane_id: 1,
            surface_uuid: Uuid::new_v4(),
            shell: "/bin/zsh".to_string(),
            cwd: "/tmp".to_string(),
        };
        let json = serde_json::to_string(&leaf).expect("serialize failed");
        let restored: SplitNodeData = serde_json::from_str(&json).expect("deserialize failed");
        if let (
            SplitNodeData::Leaf {
                pane_id: p1,
                surface_uuid: u1,
                ..
            },
            SplitNodeData::Leaf {
                pane_id: p2,
                surface_uuid: u2,
                ..
            },
        ) = (&leaf, &restored)
        {
            assert_eq!(p1, p2);
            assert_eq!(u1, u2);
        } else {
            panic!("Roundtrip changed variant");
        }
    }

    /// Preserve mixed terminal/browser tab order, URLs and selection through serialization.
    #[test]
    fn pane_tabs_roundtrip_preserves_browser_url_and_active_surface() {
        let terminal_uuid = Uuid::new_v4();
        let browser_uuid = Uuid::new_v4();
        let pane = SplitNodeData::Pane {
            active_surface_uuid: Some(browser_uuid),
            surfaces: vec![
                PaneSurfaceData::Terminal {
                    surface_uuid: terminal_uuid,
                    shell: "/bin/sh".to_string(),
                    cwd: "/tmp".to_string(),
                },
                PaneSurfaceData::Browser {
                    surface_uuid: browser_uuid,
                    url: "https://example.com/path".to_string(),
                },
            ],
        };

        let json = serde_json::to_string(&pane).expect("serialize pane tabs");
        let restored: SplitNodeData = serde_json::from_str(&json).expect("restore pane tabs");
        let SplitNodeData::Pane {
            active_surface_uuid,
            surfaces,
        } = restored
        else {
            panic!("expected Pane session node");
        };
        assert_eq!(active_surface_uuid, Some(browser_uuid));
        assert!(matches!(
            &surfaces[1],
            PaneSurfaceData::Browser { surface_uuid, url }
                if *surface_uuid == browser_uuid && url == "https://example.com/path"
        ));
    }

    /// Preserve split orientation, divider ratio and child identities in saved layouts.
    #[test]
    fn split_node_data_split_roundtrip_json() {
        // Verify nested SplitNodeData serializes correctly with ratio field.
        let split = SplitNodeData::Split {
            orientation: "horizontal".to_string(),
            ratio: 0.35,
            start: Box::new(SplitNodeData::Leaf {
                pane_id: 1,
                surface_uuid: Uuid::new_v4(),
                shell: String::new(),
                cwd: String::new(),
            }),
            end: Box::new(SplitNodeData::Leaf {
                pane_id: 2,
                surface_uuid: Uuid::new_v4(),
                shell: String::new(),
                cwd: String::new(),
            }),
        };
        let json = serde_json::to_string(&split).expect("serialize failed");
        let restored: SplitNodeData = serde_json::from_str(&json).expect("deserialize failed");
        if let SplitNodeData::Split {
            orientation, ratio, ..
        } = restored
        {
            assert_eq!(orientation, "horizontal");
            assert!(
                (ratio - 0.35).abs() < f64::EPSILON,
                "ratio not preserved in roundtrip"
            );
        } else {
            panic!("Roundtrip changed variant to non-Split");
        }

        // Verify v1-compat: Split without ratio field deserializes with default 0.5
        let v1_json = r#"{"type":"Split","orientation":"vertical","start":{"type":"Leaf","pane_id":1,"surface_uuid":"00000000-0000-0000-0000-000000000000","shell":"","cwd":""},"end":{"type":"Leaf","pane_id":2,"surface_uuid":"00000000-0000-0000-0000-000000000000","shell":"","cwd":""}}"#;
        let v1_restored: SplitNodeData =
            serde_json::from_str(v1_json).expect("v1 deserialize failed");
        if let SplitNodeData::Split { ratio, .. } = v1_restored {
            assert!(
                (ratio - 0.5).abs() < f64::EPSILON,
                "v1 missing ratio should default to 0.5"
            );
        } else {
            panic!("v1 deserialize changed variant");
        }
    }
}
