use crate::ghostty::ffi;
use crate::split_engine::SplitEngine;
use crate::workspace::{ConnectionState, Workspace};
use gtk4::prelude::*;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub type AppStateRef = Rc<RefCell<AppState>>;

pub struct AppState {
    pub split_engines: Vec<SplitEngine>,
    pub gtk_app: gtk4::Application,
    /// All open workspaces. Never empty after initialization — create_workspace is called in new().
    pub workspaces: Vec<Workspace>,
    /// Index into workspaces of the currently visible workspace.
    pub active_index: usize,
    /// GtkStack holding one page per workspace (the workspace's root GTK widget).
    pub stack: gtk4::Stack,
    /// GtkListBox in the sidebar showing workspace names.
    pub sidebar_list: gtk4::ListBox,
    /// Ghostty app handle — used by create_surface() for new panes.
    pub ghostty_app: ffi::ghostty_app_t,
    /// Next workspace ID (monotonically increasing).
    next_id: u64,
    /// Next display number for default names ("Workspace N").
    next_display_number: usize,
    /// Bounded retained messages, separate from transient terminal BEL attention.
    pub inbox: crate::inbox::Inbox,
    /// Validated application-owned authority for automatic local terminal resume.
    pub resume_policy: crate::resume_policy::ResumePolicy,
    /// Sender for session snapshots to the debounce task.
    /// Each mutation snapshots SessionData on the main thread and sends it here.
    pub session_tx: Option<tokio::sync::watch::Sender<Option<crate::session::Snapshot>>>,
    /// Sender for SSH events (cloned into SSH lifecycle tokio tasks).
    pub ssh_event_tx: Option<crate::ssh::SshEventTx>,
    /// Tokio runtime handle for spawning SSH lifecycle tasks.
    pub runtime_handle: Option<tokio::runtime::Handle>,
    /// Handles to SSH lifecycle tasks, keyed by workspace id. Used for cleanup on close.
    pub ssh_task_handles: std::collections::HashMap<u64, tokio::task::JoinHandle<()>>,
    /// Maps workspace_id -> SshBridge for remote workspaces.
    pub workspace_bridges:
        std::collections::HashMap<u64, std::sync::Arc<crate::ssh::bridge::SshBridge>>,
    /// Browser preview daemon manager (Phase 8).
    pub browser_manager: Option<crate::browser::BrowserManager>,
    /// Retain asynchronous daemon-close tasks for the post-GTK shutdown drain.
    pub browser_shutdown_tasks: crate::browser::ShutdownTasks,
    /// Next browser surface short-ref counter (monotonically increasing, per D-06).
    pub browser_surface_counter: u32,
    /// Maps short-ref ID -> surface UUID (lost on restart, per D-06).
    pub browser_surface_refs: std::collections::HashMap<u32, String>,
}

impl AppState {
    /// Create a new AppState. Does NOT create the first workspace — caller must call
    /// create_workspace() after constructing the GTK widget tree (Plan 04 wires this).
    pub fn new(
        stack: gtk4::Stack,
        sidebar_list: gtk4::ListBox,
        ghostty_app: ffi::ghostty_app_t,
        gtk_app: gtk4::Application,
    ) -> AppStateRef {
        let state = AppState {
            workspaces: Vec::new(),
            split_engines: Vec::new(),
            active_index: 0,
            stack,
            sidebar_list,
            ghostty_app,
            gtk_app,
            next_id: 1,
            next_display_number: 1,
            session_tx: None,
            resume_policy: Default::default(),
            inbox: Default::default(),
            ssh_event_tx: None,
            runtime_handle: None,
            ssh_task_handles: std::collections::HashMap::new(),
            workspace_bridges: std::collections::HashMap::new(),
            browser_manager: None,
            browser_shutdown_tasks: Default::default(),
            browser_surface_counter: 0,
            browser_surface_refs: std::collections::HashMap::new(),
        };
        Rc::new(RefCell::new(state))
    }

    /// Create a new workspace. Allocates an ID, creates a sidebar row, and adds a placeholder
    /// page to the GtkStack. The actual GLArea/split root is added by the caller (Plan 04).
    /// Returns the new workspace id.
    pub fn create_workspace(&mut self) -> u64 {
        self.create_local_workspace(None, None, None)
    }

    /// Create a local workspace bound to an existing directory.
    pub fn create_workspace_in(
        &mut self,
        name: String,
        working_directory: &Path,
    ) -> Result<u64, String> {
        let (name, working_directory) =
            crate::workspace::prepare_local_workspace(&name, working_directory)?;
        Ok(self.create_workspace_bound(name, working_directory))
    }

    /// Create a workspace from inputs already validated off the GTK main thread.
    pub fn create_workspace_bound(&mut self, name: String, working_directory: PathBuf) -> u64 {
        self.create_local_workspace(Some(name), Some(working_directory), None)
    }

    /// Validate the launch directory and readable script before creating a local GTK workspace.
    pub fn create_script_workspace(
        &mut self,
        name: String,
        directory: &Path,
        script: &Path,
    ) -> Result<u64, String> {
        let (name, directory) = crate::workspace::prepare_local_workspace(&name, directory)?;
        let script = crate::workspace::prepare_startup_script(script)?;
        Ok(self.create_local_workspace(Some(name), Some(directory), Some(script)))
    }

    /// Allocate identity, construct the pane tree and sidebar row, select and schedule persistence.
    fn create_local_workspace(
        &mut self,
        name: Option<String>,
        working_directory: Option<PathBuf>,
        startup_script: Option<PathBuf>,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let display_number = self.next_display_number;
        self.next_display_number += 1;

        let mut workspace = match (name, working_directory) {
            (Some(name), Some(directory)) => {
                Workspace::new_bound(id, display_number, name, directory)
            }
            _ => Workspace::new(id, display_number),
        };

        workspace.startup_script = startup_script;
        let launch_command = workspace
            .startup_script
            .as_deref()
            .map(crate::workspace::startup_command);
        let row = self.build_sidebar_row(&workspace);
        self.sidebar_list.append(&row);

        // Create surface and split engine
        let pane_id = id * 1000;
        eprintln!(
            "cmux: create_workspace calling create_surface for workspace_id={}, pane_id={}",
            id, pane_id
        );
        let (gl_area, _) = crate::ghostty::surface::create_surface(
            self.ghostty_app,
            None,
            workspace.working_directory.clone(),
            pane_id,
            launch_command
                .clone()
                .map(crate::ghostty::surface::SurfaceIoMode::Command)
                .unwrap_or(crate::ghostty::surface::SurfaceIoMode::Exec),
        );
        let mut engine = SplitEngine::new(
            self.ghostty_app,
            gl_area,
            pane_id,
            workspace.working_directory.clone(),
        );

        engine.launch_command = launch_command;

        // Add to stack
        let page_name = format!("workspace-{}", id);
        self.stack
            .add_named(&engine.root_widget(), Some(&page_name));
        workspace.stack_page_name = page_name;

        self.workspaces.push(workspace);
        self.split_engines.push(engine);

        let new_index = self.workspaces.len() - 1;
        self.switch_to_index(new_index);

        self.trigger_session_save();
        id
    }

    /// Restore a workspace from a session snapshot (SESS-02).
    /// Creates sidebar row, reconstructs the saved split tree with its launch context.
    /// Returns the workspace id, or None if tree is invalid (D-14 depth limit).
    pub fn restore_workspace(&mut self, ws: &crate::session::WorkspaceSession) -> Option<u64> {
        let id = self.next_id;
        self.next_id += 1;
        let display_number = self.next_display_number;
        self.next_display_number += 1;

        let mut workspace = Workspace::new(id, display_number);
        workspace.name = ws.name.clone();
        workspace.uuid = uuid::Uuid::parse_str(&ws.uuid).unwrap_or_else(|_| uuid::Uuid::new_v4());
        workspace.color = ws
            .color
            .clone()
            .filter(|c| crate::workspace::valid_workspace_color(c));
        workspace.startup_script = ws.startup_script.clone();
        workspace.remote_directory = ws.remote_directory.clone();
        workspace.working_directory = ws.working_directory.clone();

        workspace.remote_target = ws.remote_target.clone();
        let remote_bridge = ws.remote_target.as_ref().map(|_| {
            let bridge = std::sync::Arc::new(crate::ssh::bridge::SshBridge::new());
            *bridge.directory.lock().unwrap() = ws.remote_directory.clone();
            workspace.connection_state = ConnectionState::Reconnecting(0);
            bridge
        });
        let remote_launch =
            remote_bridge
                .as_ref()
                .map(|bridge| crate::ghostty::surface::SurfaceIoMode::Remote {
                    bridge: bridge.clone(),
                    ssh_tx: self.ssh_event_tx.clone().unwrap(),
                });

        let row = self.build_sidebar_row(&workspace);

        // Build split tree from session data (D-05)
        let engine = crate::split_engine::SplitEngine::from_data_with_command(
            self.ghostty_app,
            &ws.layout,
            ws.active_pane_uuid.as_deref(),
            ws.working_directory.clone(),
            ws.startup_script
                .as_deref()
                .map(crate::workspace::startup_command),
            remote_launch,
            &self.resume_policy,
        )?;

        self.sidebar_list.append(&row);
        // Add to stack
        let page_name = format!("workspace-{}", id);
        self.stack
            .add_named(&engine.root_widget(), Some(&page_name));
        workspace.stack_page_name = page_name;

        self.workspaces.push(workspace);
        self.split_engines.push(engine);

        if let (Some(bridge), Some(target)) = (remote_bridge, ws.remote_target.clone()) {
            self.start_ssh(id, target, bridge, None, "restore");
        }
        Some(id)
    }

    /// Retain the workspace bridge and own its SSH task, linking retries to the initiating operation.
    /// Call on GTK after workspace creation; absent runtime/channel records unavailable without spawning.
    pub(crate) fn start_ssh(
        &mut self,
        id: u64,
        target: String,
        bridge: std::sync::Arc<crate::ssh::bridge::SshBridge>,
        parent: Option<uuid::Uuid>,
        origin: &'static str,
    ) {
        let trace_id = parent.unwrap_or_else(uuid::Uuid::new_v4);
        self.workspace_bridges.insert(id, bridge.clone());
        let ready = self.runtime_handle.is_some() && self.ssh_event_tx.is_some();
        crate::diagnostics::record(
            "workspace.ssh.launch",
            serde_json::json!({"trace_id": trace_id, "workspace_id": id, "origin": origin,
                "outcome": if ready { "scheduled" } else { "unavailable" }}),
        );
        if let (Some(rt), Some(tx)) = (self.runtime_handle.as_ref(), self.ssh_event_tx.clone()) {
            let handle = rt.spawn(crate::ssh::tunnel::run_ssh_lifecycle(
                id, target, tx, bridge, trace_id,
            ));
            if let Some(previous) = self.ssh_task_handles.insert(id, handle) {
                previous.abort();
            }
        }
    }

    /// Build an unattached GTK sidebar row with workspace identity, styling and controls.
    /// Local, remote and restored workspaces share this construction path.
    fn build_sidebar_row(&self, workspace: &Workspace) -> gtk4::ListBoxRow {
        let row = gtk4::ListBoxRow::new();
        row.set_child(Some(&crate::sidebar::workspace_row_content(workspace)));
        crate::sidebar::style_workspace_row(&row, workspace);
        unsafe {
            row.set_data("workspace-id", workspace.id);
        }
        row
    }

    /// Create a remote SSH workspace. Returns workspace id.
    /// The bridge is used to create an IoWriteContext for the initial pane's manual I/O mode surface.
    pub fn create_remote_workspace(
        &mut self,
        target: String,
        bridge: &std::sync::Arc<crate::ssh::bridge::SshBridge>,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let display_number = self.next_display_number;
        self.next_display_number += 1;

        let workspace = Workspace::new_remote(id, display_number, target);
        let row = self.build_sidebar_row(&workspace);
        self.sidebar_list.append(&row);

        // Create remote surface with manual I/O mode
        let pane_id = id * 1000;
        let remote_launch = crate::ghostty::surface::SurfaceIoMode::Remote {
            bridge: bridge.clone(),
            ssh_tx: self
                .ssh_event_tx
                .clone()
                .expect("SSH event channel initialized"),
        };
        let (gl_area, _) = crate::ghostty::surface::create_surface(
            self.ghostty_app,
            None,
            None,
            pane_id,
            remote_launch.clone(),
        );
        let mut engine = SplitEngine::new(self.ghostty_app, gl_area, pane_id, None);

        engine.remote_launch = Some(remote_launch);
        let page_name = workspace.stack_page_name.clone();
        self.stack
            .add_named(&engine.root_widget(), Some(&page_name));

        self.workspaces.push(workspace);
        self.split_engines.push(engine);

        let new_index = self.workspaces.len() - 1;
        self.switch_to_index(new_index);
        self.trigger_session_save();
        id
    }

    /// Update the connection state of a workspace and refresh its sidebar row.
    pub fn update_connection_state(&mut self, workspace_id: u64, state: ConnectionState) {
        if let Some(idx) = self.workspaces.iter().position(|ws| ws.id == workspace_id) {
            self.workspaces[idx].connection_state = state.clone();
            // Update sidebar subtitle
            if let Some(row) = self.sidebar_list.row_at_index(idx as i32) {
                if let Some(hbox) = row.child().and_downcast::<gtk4::Box>() {
                    if let Some(vbox) = hbox.first_child().and_downcast::<gtk4::Box>() {
                        // Last child in vbox is the status label (if it has connection-state class)
                        if let Some(status) = vbox.last_child().and_downcast::<gtk4::Label>() {
                            if status.has_css_class("connection-state") {
                                status.set_text(state.display_text());
                                status.remove_css_class("connected");
                                status.remove_css_class("disconnected");
                                status.remove_css_class("reconnecting");
                                status.add_css_class(state.css_class());
                            }
                        }
                    }
                }
            }
        }
    }

    /// Close the workspace at `index`. Removes the sidebar row and GtkStack page.
    /// Returns false if there is only one workspace (cannot close the last one).
    pub fn close_workspace(&mut self, index: usize) -> bool {
        if self.workspaces.len() <= 1 {
            return false; // Cannot close the last workspace
        }

        // Abort SSH lifecycle task if this is a remote workspace.
        if let Some(ws) = self.workspaces.get(index) {
            if let Some(handle) = self.ssh_task_handles.remove(&ws.id) {
                handle.abort();
            }
        }

        // Stop PTYs and unregister their callbacks before removing GTK widgets.
        if let Some(engine) = self.split_engines.get(index) {
            let mut terminal_areas = Vec::new();
            engine.root.collect_terminal_areas(&mut terminal_areas);
            for area in terminal_areas {
                crate::split_engine::destroy_terminal_area(&area);
            }
        }
        self.split_engines.remove(index);

        let workspace = self.workspaces.remove(index);
        self.workspace_bridges.remove(&workspace.id);

        // Remove sidebar row.
        if let Some(row) = self.sidebar_list.row_at_index(index as i32) {
            self.sidebar_list.remove(&row);
        }

        // Remove GtkStack page.
        if let Some(child) = self.stack.child_by_name(&workspace.stack_page_name) {
            self.stack.remove(&child);
        }

        // At least one workspace survives the guard above.
        self.active_index =
            crate::selection::after_removal(self.active_index, index, self.workspaces.len())
                .expect("workspace close preserves a survivor");

        self.switch_to_index(self.active_index);
        self.trigger_session_save();
        true
    }

    /// Switch to the workspace at `index` (0-based). Updates GtkStack visible child and
    /// sidebar selection. Does nothing if index is out of bounds.
    pub fn switch_to_index(&mut self, index: usize) {
        if index >= self.workspaces.len() {
            return;
        }
        // Phase 4: clear attention when user switches to a workspace (D-05).
        self.clear_workspace_attention(index);
        self.active_index = index;
        let page_name = self.workspaces[index].stack_page_name.clone();
        self.stack.set_visible_child_name(&page_name);
        if let Some(row) = self.sidebar_list.row_at_index(index as i32) {
            self.sidebar_list.select_row(Some(&row));
            // Update CSS classes: active row gets "active-workspace" for styling.
            // All rows: remove first, then add to active.
            let count = self.workspaces.len() as i32;
            for i in 0..count {
                if let Some(r) = self.sidebar_list.row_at_index(i) {
                    r.remove_css_class("active-workspace");
                    // Phase 4: navigate nested layout: row > hbox > vbox > label
                    if let Some(hbox) = r.child().and_downcast::<gtk4::Box>() {
                        if let Some(vbox) = hbox.first_child().and_downcast::<gtk4::Box>() {
                            if let Some(label) = vbox.first_child().and_downcast::<gtk4::Label>() {
                                label.set_css_classes(&[]);
                            }
                        }
                    }
                }
            }
            row.add_css_class("active-workspace");
            // Phase 4: navigate nested layout: row > hbox > vbox > label
            if let Some(hbox) = row.child().and_downcast::<gtk4::Box>() {
                if let Some(vbox) = hbox.first_child().and_downcast::<gtk4::Box>() {
                    if let Some(label) = vbox.first_child().and_downcast::<gtk4::Label>() {
                        label.add_css_class("active-workspace-label");
                    }
                }
            }
        }
        // Restore focus through the selected surface for every workspace-switch caller.
        if let Some(engine) = self.split_engines.get(index) {
            engine.focus_active_surface();
        }
    }

    /// Move a workspace and its engine together while retaining active identity and focus.
    pub fn reorder_workspace(&mut self, from: usize, to: usize) -> bool {
        if from >= self.workspaces.len() || to >= self.workspaces.len() || from == to {
            return false;
        }
        let active_id = self.workspaces[self.active_index].id;
        let workspace = self.workspaces.remove(from);
        self.workspaces.insert(to, workspace);
        let engine = self.split_engines.remove(from);
        self.split_engines.insert(to, engine);
        if let Some(row) = self.sidebar_list.row_at_index(from as i32) {
            self.sidebar_list.remove(&row);
            self.sidebar_list.insert(&row, to as i32);
        }
        self.active_index = self
            .workspaces
            .iter()
            .position(|w| w.id == active_id)
            .unwrap();
        self.sidebar_list.select_row(
            self.sidebar_list
                .row_at_index(self.active_index as i32)
                .as_ref(),
        );
        self.trigger_session_save();
        true
    }

    /// Apply a validated RGB color to model and sidebar, then schedule a session save.
    /// Invalid colors and unknown workspace IDs leave state unchanged.
    pub fn set_workspace_color(&mut self, id: u64, color: Option<String>) {
        if color
            .as_deref()
            .is_some_and(|c| !crate::workspace::valid_workspace_color(c))
        {
            return;
        }
        if let Some(index) = self.workspaces.iter().position(|w| w.id == id) {
            self.workspaces[index].color = color;
            if let Some(row) = self.sidebar_list.row_at_index(index as i32) {
                crate::sidebar::style_workspace_row(&row, &self.workspaces[index]);
            }
            self.trigger_session_save();
        }
    }

    /// Switch to next workspace (wrap-around). Per D-10: Ctrl+].
    pub fn switch_next(&mut self) {
        if self.workspaces.is_empty() {
            return;
        }
        let next = (self.active_index + 1) % self.workspaces.len();
        self.switch_to_index(next);
    }

    /// Switch to previous workspace (wrap-around). Per D-10: Ctrl+[.
    pub fn switch_prev(&mut self) {
        if self.workspaces.is_empty() {
            return;
        }
        let prev = if self.active_index == 0 {
            self.workspaces.len() - 1
        } else {
            self.active_index - 1
        };
        self.switch_to_index(prev);
    }

    /// Borrow the selected workspace's pane tree, or None when no workspace exists.
    pub fn active_split_engine(&self) -> Option<&SplitEngine> {
        self.split_engines.get(self.active_index)
    }

    /// Mutably borrow the selected pane tree for GTK-thread workspace operations.
    pub fn active_split_engine_mut(&mut self) -> Option<&mut SplitEngine> {
        self.split_engines.get_mut(self.active_index)
    }

    /// Rename the active workspace. Per D-03/D-10: Ctrl+Shift+R (UI wired in Plan 04/05).
    pub fn rename_active(&mut self, new_name: String) {
        self.rename_workspace_at(self.active_index, new_name);
    }

    /// Update the workspace name and sidebar label, then schedule persistence; ignore invalid indices.
    pub fn rename_workspace_at(&mut self, index: usize, new_name: String) {
        if let Some(ws) = self.workspaces.get_mut(index) {
            ws.rename(new_name.clone());
            // Update the sidebar label (Phase 4 nested layout: row > hbox > vbox > label).
            if let Some(row) = self.sidebar_list.row_at_index(index as i32) {
                if let Some(hbox) = row.child().and_downcast::<gtk4::Box>() {
                    if let Some(vbox) = hbox.first_child().and_downcast::<gtk4::Box>() {
                        if let Some(label) = vbox.first_child().and_downcast::<gtk4::Label>() {
                            label.set_text(&new_name);
                        }
                    }
                }
            }
            self.trigger_session_save();
        }
    }

    /// Returns the active workspace, if any.
    pub fn active_workspace(&self) -> Option<&Workspace> {
        self.workspaces.get(self.active_index)
    }

    /// Set attention on a specific pane. Called from bell handler.
    /// Updates workspace has_attention and sidebar dot.
    pub fn set_pane_attention(&mut self, pane_id: u64) {
        for (idx, engine) in self.split_engines.iter_mut().enumerate() {
            if engine.root.set_attention(pane_id, true) {
                self.workspaces[idx].has_attention = engine.root.any_attention();
                self.update_sidebar_attention(idx);

                // Desktop notification when window is unfocused (NOTF-03)
                let window_focused = self
                    .gtk_app
                    .active_window()
                    .map(|w| w.is_active())
                    .unwrap_or(false);
                if !window_focused && self.workspaces[idx].has_attention {
                    let should_notify = self.workspaces[idx]
                        .last_notification
                        .map(|t| t.elapsed() >= std::time::Duration::from_secs(5))
                        .unwrap_or(true);
                    if should_notify {
                        self.workspaces[idx].last_notification = Some(std::time::Instant::now());
                        if let Some(runtime) = &self.runtime_handle {
                            crate::notification::send(
                                runtime,
                                cmux_platform::notification::terminal_bell(
                                    &self.workspaces[idx].name,
                                ),
                                self.workspaces[idx].uuid,
                            );
                        }
                    }
                }
                break;
            }
        }
    }

    /// Clear all attention in the workspace at `index`.
    pub fn clear_workspace_attention(&mut self, index: usize) {
        if let Some(engine) = self.split_engines.get_mut(index) {
            engine.root.clear_all_attention();
        }
        if let Some(ws) = self.workspaces.get_mut(index) {
            ws.has_attention = false;
        }
        self.update_sidebar_attention(index);
    }

    /// Update the sidebar dot visibility for workspace at `index`.
    pub(crate) fn update_sidebar_attention(&self, index: usize) {
        if let Some(row) = self.sidebar_list.row_at_index(index as i32) {
            let has_attention = self
                .workspaces
                .get(index)
                .map(|ws| {
                    ws.has_attention
                        || self
                            .inbox
                            .records
                            .iter()
                            .any(|record| record.workspace_id == ws.uuid && !record.is_read)
                })
                .unwrap_or(false);
            // Row layout: GtkBox(H) > [GtkBox(V) > [GtkLabel(name)], GtkLabel(dot)]
            if let Some(hbox) = row.child().and_downcast::<gtk4::Box>() {
                if let Some(dot) = hbox.last_child() {
                    dot.set_visible(has_attention);
                }
            }
        }
    }

    /// Remove the manager and cancel its local work now; close its daemon on Tokio without GTK I/O.
    pub fn shutdown_browser(&mut self) {
        let Some(browser) = self.browser_manager.take() else {
            return;
        };
        let close = browser.shutdown();
        if let Some(runtime) = self.runtime_handle.as_ref() {
            let mut tasks = self.browser_shutdown_tasks.borrow_mut();
            // Reap completed closes during normal use so retained handles do not accumulate.
            while tasks.try_join_next().is_some() {}
            tasks.spawn_on(close, runtime);
        } else {
            crate::diagnostics::record(
                "browser.shutdown.runtime_unavailable",
                serde_json::json!({}),
            );
        }
    }

    /// Publish the final live layout before native teardown and prevent later callbacks overwriting it.
    /// Idempotent on GTK; the composition root separately waits for durable worker completion.
    pub fn finish_session(&mut self) {
        self.trigger_session_save();
        self.session_tx.take();
    }

    /// Trigger a debounced session save. Call after any workspace/pane mutation.
    /// Snapshots SessionData on the main thread (safe for Rc) and sends to the
    /// tokio debounce task which handles the file I/O. Records GTK construction
    /// and publication cost separately from the worker's serialization/write timing.
    pub fn trigger_session_save(&self) {
        // Snapshot on GTK; the worker shares ownership rather than cloning the tree.
        if let Some(ref tx) = self.session_tx {
            let started = std::time::Instant::now();
            let session = crate::session::SessionData {
                version: 3, // Per-pane terminal/browser tabs and persisted URLs
                active_index: self.active_index,
                resume_policy: self.resume_policy.clone(),
                inbox: self.inbox.clone(),
                workspaces: self
                    .workspaces
                    .iter()
                    .enumerate()
                    .map(|(i, ws)| {
                        // D-02: save full split tree for ALL workspaces
                        let layout = if i < self.split_engines.len() {
                            self.split_engines[i].root.to_data()
                        } else {
                            // Fallback: shouldn't happen, but be safe
                            crate::split_engine::SplitNodeData::Leaf {
                                pane_id: 0,
                                surface_uuid: uuid::Uuid::nil(),
                                shell: String::new(),
                                cwd: String::new(),
                            }
                        };
                        // D-04: save active_pane_uuid per workspace
                        let active_pane_uuid = if i < self.split_engines.len() {
                            self.split_engines[i].active_pane_uuid()
                        } else {
                            None
                        };
                        crate::session::WorkspaceSession {
                            uuid: ws.uuid.to_string(),
                            name: ws.name.clone(),
                            color: ws.color.clone(),
                            startup_script: ws.startup_script.clone(),
                            remote_target: ws.remote_target.clone(),
                            remote_directory: ws.remote_directory.clone(),
                            working_directory: ws.working_directory.clone(),
                            active_pane_uuid,
                            layout,
                        }
                    })
                    .collect(),
            };
            let construction_us = started.elapsed().as_micros() as u64;
            let published = tx.send(Some(std::sync::Arc::new(session))).is_ok();
            crate::diagnostics::record(
                "session.snapshot",
                serde_json::json!({
                    "outcome": if published { "published" } else { "worker_closed" },
                    "workspaces": self.workspaces.len(),
                    "construction_us": construction_us,
                    "duration_us": started.elapsed().as_micros() as u64,
                }),
            );
        }
    }
}
