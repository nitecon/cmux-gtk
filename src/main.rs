use gtk4::prelude::*;
use gtk4::{gio, Application, ApplicationWindow, CssProvider};
use std::ffi::CString;

mod app_state;
mod bounded_json;
mod browser;
mod config;
mod diagnostics;
mod ghostty;
mod header_bar;
mod menus;
mod preferences;
mod selection;
mod session;
mod shortcuts;
mod sidebar;
mod socket;
mod split_engine;
mod ssh;
mod ssh_dialog;
mod ssh_hosts;
#[allow(dead_code)]
mod updater;
mod window_state;
mod workspace;
mod workspace_dialog;

const APP_ID: &str = "io.cmux.App";

const APP_CSS: &str = "
/* cmux Phase 2 styles — per UI-SPEC.md */
window { background-color: #1a1a1a; }
.sidebar { background-color: #242424; }
.workspace-list { background-color: #242424; }
.workspace-list row { min-height: 36px; padding: 8px 16px; }
.workspace-list row label { color: #cccccc; font-size: 14px; font-weight: 400; }
.workspace-list row:hover:not(.active-workspace) { background-color: #2e2e2e; }
.workspace-list row.active-workspace { background-color: #5b8dd9; }
.workspace-list row.active-workspace label { color: #ffffff; font-weight: 600; }
.active-pane { border: 1px solid #5b8dd9; }
.rename-entry { font-size: 14px; padding: 2px 4px; }
/* GtkPaned separator styling — makes divider visible on dark backgrounds.
   wide-handle is set programmatically; separator gets min-width/height for draggability. */
paned > separator { background-color: #3a3a3a; min-width: 4px; min-height: 4px; }
paned > separator:hover { background-color: #5b8dd9; }
/* Phase 4: Attention dot for bell notifications (NOTF-02) */
.attention-dot {
    background-color: #e8a444;
    border-radius: 50%;
    min-width: 8px;
    min-height: 8px;
    margin: 0 4px;
}
/* Phase 4: SSH connection state subtitle (SSH-01, SSH-04) */
.connection-state {
    font-size: 11px;
    font-weight: 400;
    color: #888888;
}
.connection-state.connected { color: #5b8dd9; }
.connection-state.disconnected { color: #888888; }
.connection-state.reconnecting { color: #e8a444; }
/* Phase 7.1: SSH connect dialog (SSH-01) */
.ssh-dialog { background-color: #242424; }
.ssh-dialog entry { font-size: 14px; padding: 8px 16px; }
/* Phase 8: Browser preview pane */
.browser-preview { background-color: #1a1a1a; }
.browser-url-bar { font-size: 13px; padding: 4px 8px; background-color: #2a2a2a; color: #e0e0e0; border-bottom: 1px solid #444; }
.preview-container { background-color: #1a1a1a; }
.preview-empty, .preview-error { color: #888888; font-size: 14px; font-weight: 400; padding: 32px; }
.preview-error { color: #cc4444; }
.stream-indicator { font-size: 9px; font-weight: 600; color: #5b8dd9; margin: 0 4px; padding: 1px 4px; border-radius: 2px; background-color: rgba(91, 141, 217, 0.15); }
/* Phase 8 Plan 05: Navigation bar */
.browser-nav-bar { background-color: #242424; padding: 4px 8px; border-bottom: 1px solid #3a3a3a; }
.browser-nav-btn { min-width: 28px; min-height: 28px; padding: 4px; margin: 0 2px; border-radius: 4px; background-color: transparent; color: #cccccc; border: none; font-size: 14px; }
.browser-nav-btn:hover { background-color: rgba(255, 255, 255, 0.08); }
.browser-nav-btn:active { background-color: rgba(255, 255, 255, 0.12); }
.browser-nav-btn:disabled { color: #555555; }
.browser-nav-go { color: #5b8dd9; font-weight: 600; }
.browser-nav-devtools:checked { background-color: rgba(91, 141, 217, 0.2); color: #5b8dd9; }
/* Phase 8 Plan 06: DevTools overlay */
.devtools-overlay { background-color: rgba(26, 26, 26, 0.92); }
.devtools-snapshot { color: #cccccc; font-family: monospace; font-size: 12px; padding: 16px; }
/* Phase 9: Header bar (D-04) */
.cmux-headerbar { background-color: #242424; }
.headerbar-btn { min-width: 28px; min-height: 28px; padding: 4px; margin: 0 2px; border-radius: 4px; background-color: transparent; color: #cccccc; border: none; }
.headerbar-btn:hover { background-color: rgba(255, 255, 255, 0.08); }
.headerbar-btn:active { background-color: rgba(255, 255, 255, 0.12); }
/* Phase 9: Sidebar add button (D-01) */
.sidebar-add-btn { min-height: 36px; padding: 8px 16px; background-color: transparent; color: #cccccc; border: none; border-top: 1px solid #3a3a3a; font-size: 16px; }
.sidebar-add-btn:hover { background-color: #2e2e2e; }
/* Phase 9: Sidebar close button (D-02) */
.sidebar-close-btn { min-width: 20px; min-height: 20px; padding: 0; margin: 0; background-color: transparent; color: #888888; border: none; font-size: 14px; opacity: 0; }
.workspace-list row:hover .sidebar-close-btn { opacity: 1; }
.sidebar-close-btn:hover { color: #cccccc; }
/* Surface tabs: reveal a compact close affordance on hover/focus. */
.surface-tab-close { min-width: 18px; min-height: 18px; padding: 0; margin: 0; background: transparent; border: none; opacity: 0; }
.surface-tab-label:hover .surface-tab-close, .surface-tab-close:focus { opacity: 1; }
.surface-tab-close:hover { color: #cccccc; background-color: rgba(255, 255, 255, 0.08); }
/* Phase 9: Context menu popover */
popover.menu { background-color: #2a2a2a; border: 1px solid #3a3a3a; border-radius: 8px; }
popover.menu modelbutton { padding: 8px 16px; color: #cccccc; font-size: 14px; }
popover.menu modelbutton:hover { background-color: #3a3a3a; }
popover.menu accelerator { color: #5b8dd9; font-size: 12px; }
";

/// Own logging, background workers and the GTK application through orderly exit.
fn main() {
    if matches!(std::env::args().nth(1).as_deref(), Some("--version" | "-V")) {
        println!("cmux-app {}", env!("CMUX_VERSION"));
        return;
    }

    let _diagnostic_guard = diagnostics::initialize();
    diagnostics::install_panic_hook();
    diagnostics::event(format_args!(
        "starting version={} pid={} diagnostics={}",
        env!("CMUX_VERSION"),
        std::process::id(),
        diagnostics::log_path().display(),
    ));

    updater::spawn_auto_update();

    crate::ghostty::gtk_environment::configure();

    // Tokio runtime for socket I/O (kept alive for app lifetime).
    let runtime = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    let runtime_handle = runtime.handle().clone();
    diagnostics::start_sampler(&runtime_handle);

    // glib::MainContext::channel pattern: event-driven bridge from tokio to GTK main thread.
    // NOTE: glib::MainContext::channel was removed in glib 0.18+. We replicate its semantics
    // using tokio::sync::mpsc::channel + glib::MainContext::default().spawn_local()
    // in build_ui. The Sender is Send+Clone — tokio tasks hold it. The Receiver is consumed by
    // a spawn_local future that processes commands on the GTK main thread.
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel::<crate::socket::commands::SocketCommand>(
        crate::socket::COMMAND_CAPACITY,
    );

    let app = Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();

    eprintln!("cmux: GtkApplication created, connecting activate signal");

    // Try to restore session from previous run (SESS-02, SESS-04).
    // load_session() returns None if file is missing or invalid -- that's fine.
    let saved_session = crate::session::load_session();
    if let Some(ref s) = saved_session {
        eprintln!(
            "cmux: restoring session ({} workspace(s))",
            s.workspaces.len()
        );
    }

    // Session save infrastructure: Notify for debounce, channel for session snapshots.
    let save_notify = std::sync::Arc::new(tokio::sync::Notify::new());
    let (session_tx, session_rx) = tokio::sync::watch::channel(None::<crate::session::SessionData>);

    // Spawn debounce task in tokio. Waits for notify, debounces 500ms, then writes
    // the latest session snapshot to disk atomically (SESS-01, SESS-03).
    {
        let notify = save_notify.clone();
        let mut session_rx = session_rx;
        runtime_handle.spawn(async move {
            loop {
                notify.notified().await;
                // Debounce: 500ms window -- drain extra notifications that arrive.
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                let latest = session_rx.borrow_and_update().clone();
                if let Some(session) = latest {
                    if let Err(e) = crate::session::save_session_atomic(&session) {
                        eprintln!("cmux: session save failed: {e}");
                    }
                }
            }
        });
    }

    // Load config once at startup (D-06). ShortcutMap must be built inside
    // activate (after GTK init) because accelerator_parse requires GTK.
    let config = crate::config::load_config();

    // Move runtime_handle, cmd_tx, cmd_rx into the activate closure.
    // cmd_rx is wrapped in Mutex<Option<...>> so it can be taken once from a Fn closure.
    let cmd_rx = std::sync::Mutex::new(Some(cmd_rx));
    let saved_session = std::sync::Mutex::new(Some(saved_session));
    app.connect_activate({
        let runtime_handle = runtime_handle.clone();
        let save_notify = save_notify.clone();
        let session_tx = session_tx.clone();
        move |app| {
            let rx = cmd_rx
                .lock()
                .unwrap()
                .take()
                .expect("activate called more than once");
            let session = saved_session.lock().unwrap().take().flatten();
            let smap = crate::config::ShortcutMap::from_config(&config.shortcuts);
            build_ui(
                app,
                runtime_handle.clone(),
                cmd_tx.clone(),
                rx,
                save_notify.clone(),
                session_tx.clone(),
                session,
                smap,
                &config,
            );
        }
    });

    eprintln!("cmux: calling app.run()");
    let gtk_probe = diagnostics::start_gtk_probe();
    let _exit_code = app.run();
    gtk_probe.remove();
    eprintln!("cmux: app.run() returned");

    // Runtime drops here — tokio tasks are cancelled.
    drop(runtime);
}

/// Assemble the window, restore its model and connect GTK-side command/lifecycle handlers.
fn build_ui(
    app: &Application,
    runtime_handle: tokio::runtime::Handle,
    cmd_tx: tokio::sync::mpsc::Sender<crate::socket::commands::SocketCommand>,
    mut cmd_rx: tokio::sync::mpsc::Receiver<crate::socket::commands::SocketCommand>,
    save_notify: std::sync::Arc<tokio::sync::Notify>,
    session_tx: tokio::sync::watch::Sender<Option<crate::session::SessionData>>,
    saved_session: Option<crate::session::SessionData>,
    shortcut_map: crate::config::ShortcutMap,
    config: &crate::config::Config,
) {
    // 1. Initialize Ghostty once
    let ghostty_app = unsafe {
        use crate::ghostty::callbacks::APP_PTR;
        use crate::ghostty::ffi;
        use std::sync::atomic::Ordering;

        let argv: Vec<CString> = std::env::args().map(|a| CString::new(a).unwrap()).collect();
        let mut ptrs: Vec<*mut i8> = argv.iter().map(|a| a.as_ptr() as *mut i8).collect();
        ffi::ghostty_init(ptrs.len(), ptrs.as_mut_ptr());

        let config = ffi::ghostty_config_new();
        // CFG-03: Ghostty loads its own config from ~/.config/ghostty/config
        ffi::ghostty_config_load_default_files(config);
        ffi::ghostty_config_finalize(config);

        let runtime_config = ffi::ghostty_runtime_config_s {
            userdata: std::ptr::null_mut(),
            supports_selection_clipboard: true,
            wakeup_cb: Some(crate::ghostty::callbacks::wakeup_cb),
            action_cb: Some(crate::ghostty::callbacks::action_cb),
            read_clipboard_cb: Some(crate::ghostty::surface::read_clipboard_cb),
            confirm_read_clipboard_cb: Some(crate::ghostty::surface::confirm_read_clipboard_cb),
            write_clipboard_cb: Some(crate::ghostty::surface::write_clipboard_cb),
            close_surface_cb: Some(crate::ghostty::callbacks::close_surface_cb),
            tmux_control_cb: None,
        };

        let ghostty_app = ffi::ghostty_app_new(&runtime_config, config);
        ffi::ghostty_config_free(config);
        if ghostty_app.is_null() {
            eprintln!("cmux: FATAL — ghostty_app_new returned null");
            std::process::exit(1);
        }
        APP_PTR.store(ghostty_app as usize, Ordering::SeqCst);
        ghostty_app
    };

    // 2. Load CSS
    let provider = CssProvider::new();
    provider.load_from_data(APP_CSS);
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().expect("no display"),
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    // 3. Build the window layout
    let window = ApplicationWindow::builder()
        .application(app)
        .title("cmux")
        .default_width(800)
        .default_height(600)
        .build();

    let (sidebar_box, _sidebar_scroll, sidebar_list) = crate::sidebar::build_sidebar();
    let stack = gtk4::Stack::new();
    stack.set_transition_type(gtk4::StackTransitionType::None);

    let hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    hbox.append(&sidebar_box);
    hbox.append(&stack);
    // Make the stack expand to fill remaining width.
    stack.set_hexpand(true);
    stack.set_vexpand(true);

    // Phase 9: Set HeaderBar as titlebar (D-04)
    if let Some(header) = crate::header_bar::build_header_bar(config) {
        window.set_titlebar(Some(&header));
    }

    window.set_child(Some(&hbox));

    // 4. Create AppState and initial workspace
    let state = crate::app_state::AppState::new(
        stack.clone(),
        sidebar_list.clone(),
        ghostty_app,
        app.clone(),
    );

    // Wire sidebar click-to-switch.
    crate::sidebar::wire_sidebar_clicks(&sidebar_list, state.clone());

    // Set save_notify, session_tx, and SSH event channel on AppState.
    let (ssh_event_tx, mut ssh_event_rx) = tokio::sync::mpsc::channel::<crate::ssh::SshEvent>(256);
    {
        let mut s = state.borrow_mut();
        s.save_notify = Some(save_notify);
        s.session_tx = Some(session_tx);
        s.ssh_event_tx = Some(ssh_event_tx);
        s.runtime_handle = Some(runtime_handle.clone());
    }

    // Restore session if available (SESS-02), otherwise create default workspace.
    {
        let has_session = saved_session
            .as_ref()
            .map(|s| !s.workspaces.is_empty())
            .unwrap_or(false);
        if has_session {
            let session = saved_session.unwrap();
            if session.version >= 2 {
                // Version 2: full tree restore (D-05)
                let mut restored_count = 0;
                for ws_session in &session.workspaces {
                    if state.borrow_mut().restore_workspace(ws_session).is_some() {
                        restored_count += 1;
                    } else {
                        // D-15: tree invalid or too deep, fall back to single pane
                        eprintln!(
                            "cmux: workspace '{}' tree invalid, creating default",
                            ws_session.name
                        );
                        state.borrow_mut().create_workspace();
                        state.borrow_mut().rename_active(ws_session.name.clone());
                    }
                }
                eprintln!(
                    "cmux: restored {} workspaces from session v{}",
                    restored_count, session.version
                );
            } else {
                // Version 1: name-only restore (auto-upgrade on next save per D-01)
                for ws_session in &session.workspaces {
                    state.borrow_mut().create_workspace();
                    state.borrow_mut().rename_active(ws_session.name.clone());
                }
            }
            // Restore active workspace index.
            let ws_count = state.borrow().workspaces.len();
            let active = session.active_index.min(ws_count.saturating_sub(1));
            state.borrow_mut().switch_to_index(active);
        } else {
            // No session -- create the default first workspace.
            state.borrow_mut().create_workspace();
        }
    }

    // Phase 9: Wire close buttons + context menus on all sidebar rows created above.
    {
        let n = sidebar_list.observe_children().n_items();
        for i in 0..n {
            if let Some(row) = sidebar_list.row_at_index(i as i32) {
                crate::sidebar::wire_row_close_button(&row, state.clone(), app);
                crate::sidebar::attach_sidebar_context_menu(&row, state.clone());
            }
        }
    }

    // Attach command receiver to GTK main loop via glib::MainContext::default().spawn_local.
    // This replaces the old glib::MainContext::channel pattern (removed in glib 0.18+).
    // The spawn_local future runs on the GTK main thread, receiving SocketCommands sent from
    // tokio tasks via the Sender (cmd_tx). All AppState mutations happen here.
    // Full handler dispatch is wired in Plan 03. For now, just attach with stub dispatch.
    {
        let state = state.clone();
        glib::MainContext::default().spawn_local(async move {
            while let Some(cmd) = cmd_rx.recv().await {
                crate::socket::handlers::handle_socket_command(cmd, &state);
            }
        });
    }

    // Drain native actions outside ghostty_app_tick to avoid reentrant GTK mutation.
    // SSH events arrive via ssh_event_rx from tokio tasks.
    {
        let state = state.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
            let (native_events, dropped) = crate::ghostty::events::take();
            if dropped > 0 {
                crate::diagnostics::record(
                    "native.events.overflow",
                    serde_json::json!({"dropped": dropped}),
                );
            }
            let mut created = false;
            for event in native_events {
                match event {
                    crate::ghostty::events::Event::NewTerminalTab(pane_id) => {
                        created |= state
                            .borrow_mut()
                            .split_engines
                            .iter_mut()
                            .find_map(|engine| engine.new_terminal_tab_for_pane(pane_id))
                            .is_some();
                    }
                    crate::ghostty::events::Event::Bell(pane_id) => {
                        state.borrow_mut().set_pane_attention(pane_id);
                    }
                }
            }
            if created {
                state.borrow().trigger_session_save();
            }
            // Process SSH events
            // Bound work per GTK turn as well as queue capacity, so sustained
            // remote output cannot monopolize the main loop.
            for _ in 0..128 {
                let Ok(event) = ssh_event_rx.try_recv() else {
                    break;
                };
                match event {
                    crate::ssh::SshEvent::StateChanged {
                        workspace_id,
                        state: conn_state,
                    } => {
                        // Auto-save host on successful connection (D-04)
                        if conn_state == crate::workspace::ConnectionState::Connected {
                            let remote_target = state
                                .borrow()
                                .workspaces
                                .iter()
                                .find(|ws| ws.id == workspace_id)
                                .and_then(|ws| ws.remote_target.clone());
                            if let Some(target) = remote_target {
                                crate::ssh_hosts::save_host(&target);
                            }
                        }
                        state
                            .borrow_mut()
                            .update_connection_state(workspace_id, conn_state);
                    }
                    crate::ssh::SshEvent::RemoteOutput { pane_id, data } => {
                        // Dispatch remote output to the Ghostty surface via process_output.
                        {
                            let surface_ptr = remote_context(&state, pane_id).and_then(|ctx| {
                                let ptr =
                                    ctx.surface_ptr.load(std::sync::atomic::Ordering::Acquire);
                                (ptr != 0).then_some(ptr as crate::ghostty::ffi::ghostty_surface_t)
                            });
                            if let Some(surface) = surface_ptr {
                                unsafe {
                                    crate::ghostty::ffi::ghostty_surface_process_output(
                                        surface,
                                        data.as_ptr() as *const _,
                                        data.len(),
                                    );
                                }
                                // Queue render for the GLArea associated with this surface
                                if let Ok(gl_areas) =
                                    crate::ghostty::callbacks::GL_TO_SURFACE.lock()
                                {
                                    for (&gl_ptr, &s_ptr) in gl_areas.iter() {
                                        if s_ptr == surface as usize {
                                            let area: glib::translate::Borrowed<gtk4::GLArea> = unsafe {
                                                glib::translate::from_glib_borrow(
                                                    gl_ptr as *mut gtk4::ffi::GtkGLArea,
                                                )
                                            };
                                            area.queue_render();
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    crate::ssh::SshEvent::RemoteEof { pane_id } => {
                        // D-08: write exit message to surface, keep pane open for user to close
                        {
                            let surface_ptr = remote_context(&state, pane_id).and_then(|ctx| {
                                let ptr =
                                    ctx.surface_ptr.load(std::sync::atomic::Ordering::Acquire);
                                (ptr != 0).then_some(ptr as crate::ghostty::ffi::ghostty_surface_t)
                            });
                            if let Some(surface) = surface_ptr {
                                let msg = b"\r\n\x1b[90m[Remote shell exited. Press any key to close]\x1b[0m\r\n";
                                unsafe {
                                    crate::ghostty::ffi::ghostty_surface_process_output(
                                        surface,
                                        msg.as_ptr() as *const _,
                                        msg.len(),
                                    );
                                }
                            }
                        }
                        // Clear stream_id and set eof flag so next keypress triggers close
                        if let Some(ctx) = remote_context(&state, pane_id) {
                            if let Ok(mut sid) = ctx.stream_id.lock() {
                                *sid = None;
                            }
                            ctx.eof_received
                                .store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                    crate::ssh::SshEvent::ClosePaneRequest { pane_id } => {
                        let surface = remote_context(&state, pane_id)
                            .map(|ctx| ctx.surface_ptr.load(std::sync::atomic::Ordering::Acquire));
                        let target = surface.and_then(|ptr| {
                            let s = state.borrow();
                            s.split_engines
                                .iter()
                                .enumerate()
                                .find_map(|(index, engine)| {
                                    engine.all_panes().into_iter().find_map(|(uuid, _, _)| {
                                        (engine
                                            .find_surface_by_uuid(&uuid.to_string())
                                            .map(|s| s as usize)
                                            == Some(ptr))
                                        .then_some((index, uuid))
                                    })
                                })
                        });
                        if let Some((index, uuid)) = target {
                            let mut s = state.borrow_mut();
                            if s.split_engines[index].close_surface_and_empty_pane(uuid)
                                == crate::split_engine::CloseSurfaceResult::LastSurfaceInPane
                            {
                                s.close_workspace(index);
                            }
                            s.trigger_session_save();
                        }
                    }
                    crate::ssh::SshEvent::StreamOpened { pane_id, stream_id } => {
                        // Set the stream_id on the IoWriteContext so keystrokes start flowing
                        if let Some(ctx) = remote_context(&state, pane_id) {
                            if let Ok(mut sid) = ctx.stream_id.lock() {
                                *sid = Some(stream_id);
                            }
                        }
                    }
                }
            }
            glib::ControlFlow::Continue
        });
    }

    // Phase 8: Clean up browser daemon on app shutdown.
    // connect_shutdown fires while GTK is still alive and runtime handle is valid,
    // ensuring the daemon gets a clean shutdown command before tokio tasks are cancelled.
    {
        let state_for_shutdown = state.clone();
        app.connect_shutdown(move |_| {
            state_for_shutdown.borrow_mut().shutdown_browser();
        });
    }

    // Start socket server (tokio accept loop + XDG path setup).
    // cmd_tx is passed in so the socket server dispatches commands through the
    // existing tokio mpsc bridge to the GTK main thread (spawn_local above).
    crate::socket::start_socket_server(&runtime_handle, state.clone(), cmd_tx);

    // 5. Handle delete-event for close confirmation
    window.connect_close_request({
        let state = state.clone();
        move |_win| {
            let count = state.borrow().workspaces.len();
            if count == 0 {
                return gtk4::glib::Propagation::Proceed;
            }
            // Show close confirmation dialog.
            // let dialog = gtk4::AlertDialog::builder()
            //     .message("Close Workspace?")
            //     .detail("All panes in this workspace will be closed. This cannot be undone.")
            //     .modal(true)
            //     .build();
            // dialog.set_buttons(&["Keep Workspace", "Close Workspace"]);
            // dialog.set_default_button(0);
            // dialog.set_cancel_button(0);

            // For window close, just allow it — full per-workspace dialog wired in shortcuts.rs.
            // This dialog is for the window X button — proceed to close.
            gtk4::glib::Propagation::Proceed
        }
    });

    // 6. Sidebar toggle state (D-04, Ctrl+B — full shortcut wired in Plan 05):
    // Storing sidebar_scroll on the stack is enough for now. Plan 05 will pass it to shortcuts.

    // Phase 9: Register GIO actions for menu/button dispatch
    crate::menus::register_actions(&window, state.clone(), &sidebar_box, app);
    crate::menus::register_accels(app, &shortcut_map);

    // 7. Install keyboard shortcuts (config-driven, D-06)
    crate::shortcuts::install_shortcuts(&window, state.clone(), &sidebar_box, app, shortcut_map);

    // 8. Present the window
    crate::window_state::install(&window);
    window.present();

    // Browser widgets are part of the saved pane tree; reconnect them only after
    // the window is presented so viewport dimensions and focus are meaningful.
    {
        let state_for_browser_restore = state.clone();
        glib::idle_add_local_once(move || {
            crate::shortcuts::restore_browser_tabs(&state_for_browser_restore);
        });
    }
}

/// Resolve a remote surface identity across workspace bridges without retaining GTK state.
fn remote_context(
    state: &crate::app_state::AppStateRef,
    id: u64,
) -> Option<std::sync::Arc<crate::ssh::bridge::IoWriteContext>> {
    state
        .borrow()
        .workspace_bridges
        .values()
        .find_map(|bridge| bridge.contexts.lock().ok()?.get(&id).cloned())
}
