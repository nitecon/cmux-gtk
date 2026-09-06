//! GTK browser-tab orchestration: widget wiring, navigation and input forwarding.
//!
//! This adapter owns AppState interaction on GTK. The sibling CLI, transport and
//! stream modules own worker I/O; remaining synchronous callbacks are tracked in
//! the architecture refactor audit.

use super::input::{keyboard_event, picture_point, viewport_size};
use crate::app_state::AppState;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

/// Own provisional startup cleanup across completion, cancellation and dropped GTK futures.
pub(crate) struct StartupOwner {
    state: std::rc::Weak<RefCell<AppState>>,
    session: String,
}

impl StartupOwner {
    /// Capture only weak application ownership; successful installation removes the provisional slot.
    pub(crate) fn new(state: &Rc<RefCell<AppState>>, session: String) -> Self {
        Self {
            state: Rc::downgrade(state),
            session,
        }
    }
}

impl Drop for StartupOwner {
    /// Cleanup is deferred if GTK emits destruction inside an existing model borrow.
    fn drop(&mut self) {
        /// Retire the matching provisional owner without reentrant GTK borrowing.
        fn retire(state: std::rc::Weak<RefCell<AppState>>, session: String) {
            let Some(owner) = state.upgrade() else {
                return;
            };
            if let Ok(mut owner) = owner.try_borrow_mut() {
                owner.cancel_browser_startup(&session);
            } else {
                glib::idle_add_local_once(move || retire(state, session));
            };
        }
        retire(self.state.clone(), self.session.clone());
    }
}

/// Start preview initialization on Tokio and install its pane in the originally requested workspace.
/// No GTK borrow spans I/O; shutdown cancels startup and stale manager/workspace results are ignored.
pub fn handle_browser_open(state: &Rc<RefCell<AppState>>) {
    let mut activity = super::metrics::Activity::begin("preview_startup", None);
    let (session, workspace, task) = {
        let mut s = state.borrow_mut();
        let Some(runtime) = s.runtime_handle.clone() else {
            activity.finish("unavailable");
            return;
        };
        let Some(workspace) = s
            .workspaces
            .get(s.active_index)
            .map(|workspace| workspace.uuid)
        else {
            activity.finish("missing_workspace");
            return;
        };
        if s.browser_manager.is_some() {
            activity.finish("overlap_rejected");
            return;
        }
        let manager = match super::BrowserManager::for_workspace(&s, workspace) {
            Ok(manager) => manager,
            Err(error) => {
                activity.finish("unavailable");
                eprintln!("cmux: browser startup failed: {error}");
                return;
            }
        };
        let browser = s.browser_manager.insert(manager);
        (
            browser.session_name.clone(),
            workspace,
            runtime.spawn(browser.prepare_preview_async("about:blank".into(), activity.id)),
        )
    };
    let owner = StartupOwner::new(state, session.clone());
    let cancel = crate::task::AbortOnDrop(task.abort_handle());
    let state = Rc::downgrade(state);
    glib::MainContext::default().spawn_local(async move {
        let _owner = owner;
        let _cancel = cancel;
        let binary = match task.await {
            Ok(Ok(binary)) => binary,
            Ok(Err(error)) => {
                activity.finish("error");
                eprintln!("cmux: browser preview startup failed: {error}");
                return;
            }
            Err(_) => {
                activity.finish("task_error");
                return;
            }
        };
        let Some(state) = state.upgrade() else {
            return;
        };
        let widgets = {
            let mut s = state.borrow_mut();
            let Some(browser) = s
                .browser_manager
                .as_mut()
                .filter(|browser| browser.session_name == session)
            else {
                activity.finish("stale_manager");
                return;
            };
            browser.binary_path = Some(binary);
            browser.preview_state = super::PreviewState::Connected;
            let Some(index) = s
                .workspaces
                .iter()
                .position(|candidate| candidate.uuid == workspace)
            else {
                activity.finish("missing_workspace");
                return;
            };
            s.split_engines
                .get_mut(index)
                .and_then(|engine| engine.split_active_with_preview())
        };
        if let Some(widgets) = widgets {
            wire_browser_tab(&state, widgets, activity.id);
            activity.finish("success");
        } else {
            activity.finish("missing_pane");
        }
    });
}

/// Register lazy initialization for saved panes; hidden pages retain only their saved URL and widgets.
pub fn restore_browser_tabs(state: &Rc<RefCell<AppState>>) {
    let tabs: Vec<_> = state
        .borrow()
        .split_engines
        .iter()
        .flat_map(|engine| engine.browser_tabs())
        .collect();
    wire_restored_browser_tabs(state, tabs);
}

/// Register one newly installed layout's browser tabs without rescanning or rewiring older surfaces.
pub(crate) fn wire_restored_browser_tabs(
    state: &Rc<RefCell<AppState>>,
    tabs: Vec<super::PreviewPaneWidgets>,
) {
    for widgets in tabs {
        let uuid = widgets.uuid;
        {
            let mut state = state.borrow_mut();
            if !state
                .browser_surface_refs
                .values()
                .any(|value| value == &uuid.to_string())
            {
                state.browser_surface_counter += 1;
                let reference = state.browser_surface_counter;
                state
                    .browser_surface_refs
                    .insert(reference, uuid.to_string());
            }
        }
        let weak = Rc::downgrade(state);
        widgets
            .container
            .connect_map(move |_| schedule_browser_restore(weak.clone(), uuid));
        let weak = Rc::downgrade(state);
        widgets.container.connect_destroy(move |_| {
            if let Some(state) = weak.upgrade() {
                retire_surface(state, uuid);
            }
        });
        if widgets.container.is_mapped() {
            schedule_browser_restore(Rc::downgrade(state), uuid);
        }
    }
}

/// Defer map signals past GTK model mutations; one manager identity prevents duplicate starts.
fn schedule_browser_restore(state: std::rc::Weak<RefCell<AppState>>, uuid: uuid::Uuid) {
    glib::idle_add_local_once(move || restore_browser_surface(state, uuid));
}

/// Start a mapped saved page without taking focus; map callbacks share the agent startup path.
fn restore_browser_surface(state: std::rc::Weak<RefCell<AppState>>, uuid: uuid::Uuid) {
    glib::MainContext::default().spawn_local(async move {
        let visible = state.upgrade().is_some_and(|state| {
            find_browser_tab(&state.borrow(), uuid)
                .is_some_and(|widgets| widgets.container.is_mapped())
        });
        if visible {
            let _ = initialize_browser_surface(state, uuid, None).await;
        }
    });
}

/// Own an unfinished restored session; cancellation retires the daemon but preserves its live pane reference.
struct RestoreOwner {
    state: std::rc::Weak<RefCell<AppState>>,
    uuid: uuid::Uuid,
    session: String,
}

impl Drop for RestoreOwner {
    /// Defer cleanup past reentrant model borrows; a connected or replaced manager is not ours to retire.
    fn drop(&mut self) {
        fn retire(state: std::rc::Weak<RefCell<AppState>>, uuid: uuid::Uuid, session: String) {
            let Some(owner) = state.upgrade() else {
                return;
            };
            if let Ok(mut owner) = owner.try_borrow_mut() {
                owner.cancel_browser_restore(uuid, &session);
            } else {
                glib::idle_add_local_once(move || retire(state, uuid, session));
            };
        }
        retire(self.state.clone(), self.uuid, self.session.clone());
    }
}

/// Initialize a saved page on demand without selecting its workspace or surface.
/// Serialized admission and widget-owned cancellation bound startup; duplicate in-flight requests fail explicitly.
pub(crate) async fn initialize_browser_surface(
    state: std::rc::Weak<RefCell<AppState>>,
    uuid: uuid::Uuid,
    trace: Option<uuid::Uuid>,
) -> Result<(), String> {
    let mut activity = super::metrics::Activity::begin("preview_restore", trace);
    let (session, completion) = {
        let owner = state.upgrade().ok_or("application closed")?;
        let mut s = owner.borrow_mut();
        let widgets = find_browser_tab(&s, uuid).ok_or("browser surface closed")?;
        if let Some(browser) = s.browser_sessions.get(&uuid) {
            return if matches!(
                browser.preview_state,
                super::PreviewState::Connected | super::PreviewState::Streaming
            ) {
                Ok(())
            } else {
                Err("browser surface is already starting".into())
            };
        }
        let gate = s.browser_restore_gate.clone();
        let runtime = s
            .runtime_handle
            .clone()
            .ok_or("async runtime unavailable")?;
        let index = s
            .split_engines
            .iter()
            .position(|engine| engine.browser_tabs().iter().any(|tab| tab.uuid == uuid))
            .ok_or("browser workspace missing")?;
        let workspace = s
            .workspaces
            .get(index)
            .ok_or("browser workspace missing")?
            .uuid;
        let manager = super::BrowserManager::for_workspace(&s, workspace)?;
        let browser = s.browser_sessions.entry(uuid).or_insert(manager);
        let session = browser.session_identity();
        let url = widgets.url_entry.text().to_string();
        let prepare = browser.prepare_preview_async(
            if url.is_empty() {
                "about:blank".into()
            } else {
                url
            },
            activity.id,
        );
        let task = runtime.spawn(async move {
            let _permit =
                tokio::time::timeout(std::time::Duration::from_secs(15), gate.acquire_owned())
                    .await
                    .map_err(|_| "browser restore admission timed out".to_owned())?
                    .map_err(|_| "browser restore stopped".to_owned())?;
            prepare.await
        });
        (session, widget_task_result(&widgets.container, task))
    };
    let _owner = RestoreOwner {
        state: state.clone(),
        uuid,
        session: session.clone(),
    };
    let binary = completion
        .await
        .ok_or("browser surface closed")?
        .map_err(|_| "browser startup worker failed")??;
    let owner = state.upgrade().ok_or("application closed")?;
    let widgets = {
        let mut s = owner.borrow_mut();
        let widgets = find_browser_tab(&s, uuid).ok_or("browser surface closed")?;
        let browser = s
            .browser_sessions
            .get_mut(&uuid)
            .filter(|browser| browser.session_identity() == session)
            .ok_or("browser session replaced")?;
        browser.binary_path = Some(binary);
        browser.preview_state = super::PreviewState::Connected;
        widgets
    };
    wire_browser_tab(&owner, widgets, activity.id);
    activity.finish("success");
    Ok(())
}

/// Defer retirement when GTK destruction is emitted during a model mutation.
fn retire_surface(state: Rc<RefCell<AppState>>, id: uuid::Uuid) {
    if let Ok(mut state) = state.try_borrow_mut() {
        state.shutdown_browser_surface(id);
    } else {
        let state = Rc::downgrade(&state);
        glib::idle_add_local_once(move || {
            if let Some(state) = state.upgrade() {
                retire_surface(state, id);
            }
        });
    }
}

/// Find a surviving browser surface by its identity without persisting references across worker I/O.
fn find_browser_tab(state: &AppState, uuid: uuid::Uuid) -> Option<super::PreviewPaneWidgets> {
    state
        .split_engines
        .iter()
        .flat_map(|engine| engine.browser_tabs())
        .find(|widgets| widgets.uuid == uuid)
}

/// Attach streaming, navigation and input handlers to a browser tab with an initialized manager.
/// GTK handlers capture stable surface identity; mapping an existing tab never navigates it.
pub(crate) fn wire_browser_tab(
    state: &Rc<RefCell<AppState>>,
    widgets: crate::browser::PreviewPaneWidgets,
    trace: uuid::Uuid,
) {
    let surface_uuid = widgets.uuid;
    let pane_id = widgets.pane_id;
    let picture = widgets.picture.clone();
    let url_entry = widgets.url_entry.clone();
    let picture_ref = picture.clone();

    // Step 3: Start WebSocket stream to pipe frames to Picture widget
    {
        let mut s = state.borrow_mut();
        let runtime = s.runtime_handle.clone();
        if !s.browser_sessions.contains_key(&surface_uuid) {
            if let Some(browser) = s.browser_manager.take() {
                s.browser_sessions.insert(surface_uuid, browser);
            }
        }
        let Some(bm) = s.browser_sessions.get_mut(&surface_uuid) else {
            return;
        };
        if let Some(ref rt) = runtime {
            bm.start_stream(rt, picture, Some(trace));
        }
        if !s
            .browser_surface_refs
            .values()
            .any(|id| id == &surface_uuid.to_string())
        {
            s.browser_surface_counter += 1;
            let reference = s.browser_surface_counter;
            s.browser_surface_refs
                .insert(reference, surface_uuid.to_string());
        }
    } // drop borrow

    let closing_state = Rc::downgrade(state);
    widgets.container.connect_destroy(move |_| {
        let Some(state) = closing_state.upgrade() else {
            return;
        };
        retire_surface(state, surface_uuid);
    });

    // Step 3b: Wire nav button signals (D-06, D-07)
    {
        let focus_controller = gtk4::EventControllerFocus::new();
        let entry_for_focus = url_entry.downgrade();
        focus_controller.connect_enter(move |_| {
            let Some(entry_for_focus) = entry_for_focus.upgrade() else {
                return;
            };
            let _ = entry_for_focus.activate_action("win.focus-pane", Some(&pane_id.to_variant()));
        });
        url_entry.add_controller(focus_controller);

        // Back button
        let state_for_back = Rc::downgrade(state);
        let entry_for_back = url_entry.clone();
        widgets.back_btn.connect_clicked(move |_| {
            let Some(state_for_back) = state_for_back.upgrade() else {
                return;
            };
            run_browser_navigation(&state_for_back, surface_uuid, &entry_for_back, "back");
        });

        // Forward button
        let state_for_fwd = Rc::downgrade(state);
        let entry_for_fwd = url_entry.clone();
        widgets.forward_btn.connect_clicked(move |_| {
            let Some(state_for_fwd) = state_for_fwd.upgrade() else {
                return;
            };
            run_browser_navigation(&state_for_fwd, surface_uuid, &entry_for_fwd, "forward");
        });

        // Reload button
        let state_for_reload = Rc::downgrade(state);
        let entry_for_reload = url_entry.clone();
        widgets.reload_btn.connect_clicked(move |_| {
            let Some(state_for_reload) = state_for_reload.upgrade() else {
                return;
            };
            run_browser_navigation(&state_for_reload, surface_uuid, &entry_for_reload, "reload");
        });

        // Go button: reads URL entry, auto-prepends https://, navigates
        let state_for_go = Rc::downgrade(state);
        let url_entry_for_go = url_entry.clone();
        let picture_for_go = picture_ref.clone();
        widgets.go_btn.connect_clicked(move |_| {
            let Some(state_for_go) = state_for_go.upgrade() else {
                return;
            };
            navigate_browser_entry(
                &state_for_go,
                surface_uuid,
                &url_entry_for_go,
                &picture_for_go,
            );
        });
    }

    // Step 3.5: Create async motion forwarder channel (D-08)
    let motion_tx = {
        let s = state.borrow();
        let runtime = s.runtime_handle.clone();
        let bm = s.browser_sessions.get(&surface_uuid);
        match (runtime, bm) {
            (Some(rt), Some(bm)) => Some(crate::browser::spawn_motion_forwarder(
                &rt,
                bm.daemon_socket_path(),
            )),
            _ => None,
        }
    };

    // Apply the initial viewport after allocation without retaining a closed preview.
    {
        let state_for_viewport = Rc::downgrade(state);
        let picture_for_viewport = picture_ref.downgrade();
        glib::idle_add_local_once(move || {
            if let (Some(state), Some(picture)) =
                (state_for_viewport.upgrade(), picture_for_viewport.upgrade())
            {
                resize_browser_preview(&state, surface_uuid, &picture);
            }
        });
    }

    // Attach mouse click controller to the Picture for browser interaction
    {
        let click_ctrl = gtk4::GestureClick::new();
        let state_for_click = Rc::downgrade(state);
        let picture_for_click = picture_ref.downgrade();
        click_ctrl.connect_released(move |_gesture, _n_press, x, y| {
            let Some(picture_for_click) = picture_for_click.upgrade() else {
                return;
            };
            let Some(state_for_click) = state_for_click.upgrade() else {
                return;
            };
            let _ =
                picture_for_click.activate_action("win.focus-pane", Some(&pane_id.to_variant()));
            // Keep browser-page keystrokes scoped to the picture. In particular,
            // this must not steal typing from the sibling URL GtkEntry.
            picture_for_click.grab_focus();
            let Some((cx, cy)) = picture_point(&picture_for_click, x, y) else {
                return;
            };

            forward_mouse_input(&state_for_click, surface_uuid, vec![
                serde_json::json!({"type": "mousePressed", "x": cx, "y": cy, "button": "left", "clickCount": 1}),
                serde_json::json!({"type": "mouseReleased", "x": cx, "y": cy, "button": "left", "clickCount": 1}),
            ]);
        });
        picture_ref.add_controller(click_ctrl);

        // Attach mouse motion controller for hover effects (async channel, D-08)
        let motion_ctrl = gtk4::EventControllerMotion::new();
        if let Some(mtx) = motion_tx {
            let picture_for_motion = picture_ref.downgrade();
            motion_ctrl.connect_motion(move |_ctrl, x, y| {
                let Some(picture_for_motion) = picture_for_motion.upgrade() else {
                    return;
                };
                let Some((mx, my)) = picture_point(&picture_for_motion, x, y) else {
                    return;
                };
                let _ = mtx.send((mx, my));
            });
        }
        picture_ref.add_controller(motion_ctrl);

        // Attach scroll controller for scroll wheel forwarding
        let scroll_ctrl = gtk4::EventControllerScroll::new(
            gtk4::EventControllerScrollFlags::VERTICAL | gtk4::EventControllerScrollFlags::DISCRETE,
        );
        let state_for_scroll = Rc::downgrade(state);
        let picture_for_scroll = picture_ref.downgrade();
        scroll_ctrl.connect_scroll(move |_ctrl, _dx, dy| {
            let Some(picture_for_scroll) = picture_for_scroll.upgrade() else {
                return gtk4::glib::Propagation::Proceed;
            };
            let Some(state_for_scroll) = state_for_scroll.upgrade() else {
                return gtk4::glib::Propagation::Proceed;
            };
            let Some((vp_w, vp_h)) = viewport_size(&picture_for_scroll) else {
                return gtk4::glib::Propagation::Proceed;
            };
            // Scroll at the center of the rendered viewport.
            let cx = (vp_w / 2.0) as i64;
            let cy = (vp_h / 2.0) as i64;
            // CDP mouseWheel uses pixel delta; ~120px per scroll tick
            let delta_y = (dy * 120.0) as i64;

            if forward_mouse_input(
                &state_for_scroll,
                surface_uuid,
                vec![serde_json::json!({
                    "type": "mouseWheel", "x": cx, "y": cy, "deltaX": 0, "deltaY": delta_y,
                })],
            ) {
                gtk4::glib::Propagation::Stop
            } else {
                gtk4::glib::Propagation::Proceed
            }
        });
        picture_ref.add_controller(scroll_ctrl);

        // Attach keyboard controller for key forwarding to Chrome
        let key_ctrl = gtk4::EventControllerKey::new();
        // Bubble phase so cmux capture-phase shortcuts (Ctrl+Shift+B etc) take priority
        key_ctrl.set_propagation_phase(gtk4::PropagationPhase::Bubble);
        let state_for_key = Rc::downgrade(state);
        key_ctrl.connect_key_pressed(move |_ctrl, keyval, keycode, mods| {
            let Some(state) = state_for_key.upgrade() else {
                return gtk4::glib::Propagation::Proceed;
            };
            let Some(params) = keyboard_event(keyval, mods, true) else {
                return gtk4::glib::Propagation::Proceed;
            };
            if forward_key_input(&state, surface_uuid, keycode, true, params) {
                gtk4::glib::Propagation::Stop
            } else {
                gtk4::glib::Propagation::Proceed
            }
        });
        let state_for_keyup = Rc::downgrade(state);
        key_ctrl.connect_key_released(move |_ctrl, keyval, keycode, mods| {
            let Some(state) = state_for_keyup.upgrade() else {
                return;
            };
            if let Some(params) = keyboard_event(keyval, mods, false) {
                forward_key_input(&state, surface_uuid, keycode, false, params);
            }
        });
        let focus = gtk4::EventControllerFocus::new();
        let state_for_blur = Rc::downgrade(state);
        let session_for_blur = state
            .borrow()
            .browser_sessions
            .get(&surface_uuid)
            .map(|browser| browser.session_name.clone());
        focus.connect_leave(move |_| {
            if let (Some(state), Some(session)) =
                (state_for_blur.upgrade(), session_for_blur.as_ref())
            {
                release_browser_keys(state, surface_uuid, session.clone());
            }
        });
        picture_ref.add_controller(focus);
        picture_ref.set_focusable(true);
        picture_ref.add_controller(key_ctrl);
    }

    // Step 5: Connect URL entry — Enter navigates the browser
    let state_for_entry = Rc::downgrade(state);
    let picture_for_nav = picture_ref.clone();
    url_entry.connect_activate(move |entry| {
        let Some(state_for_entry) = state_for_entry.upgrade() else {
            return;
        };
        navigate_browser_entry(&state_for_entry, surface_uuid, entry, &picture_for_nav);
    });

    // Step 6: DevTools toggle (D-10)
    let state_for_devtools = Rc::downgrade(state);
    let picture_for_devtools = picture_ref.clone();
    widgets.devtools_btn.connect_toggled(move |btn| {
        let Some(state_for_devtools) = state_for_devtools.upgrade() else {
            return;
        };
        if btn.is_active() {
            // Create scrollable text overlay on the Picture's parent Overlay
            if let Some(overlay) = picture_for_devtools
                .parent()
                .and_then(|p| p.downcast::<gtk4::Overlay>().ok())
            {
                let label = gtk4::Label::new(Some("Loading snapshot…"));
                label.set_selectable(true);
                label.set_wrap(true);
                label.set_xalign(0.0);
                label.set_yalign(0.0);
                label.add_css_class("devtools-snapshot");
                let scrolled = gtk4::ScrolledWindow::new();
                scrolled.set_child(Some(&label));
                scrolled.set_hexpand(true);
                scrolled.set_vexpand(true);
                scrolled.add_css_class("devtools-overlay");
                overlay.add_overlay(&scrolled);
                load_devtools_snapshot(&state_for_devtools, surface_uuid, &label);
            }
        } else {
            // Remove the DevTools overlay
            if let Some(overlay) = picture_for_devtools
                .parent()
                .and_then(|p| p.downcast::<gtk4::Overlay>().ok())
            {
                if let Some(child) = overlay.first_child() {
                    let mut current = Some(child);
                    while let Some(widget) = current {
                        let next = widget.next_sibling();
                        if widget.has_css_class("devtools-overlay") {
                            overlay.remove_overlay(&widget);
                        }
                        current = next;
                    }
                }
            }
        }
    });

    crate::diagnostics::event(format_args!(
        "browser tab wiring complete uuid={surface_uuid}"
    ));
    state.borrow().trigger_session_save();
}

/// Own worker cancellation and its weak GTK notification for every result-future exit path.
struct WidgetTaskGuard {
    _task: crate::task::AbortOnDrop,
    notification: Option<glib::object::WeakRefNotify<glib::Object>>,
}

impl Drop for WidgetTaskGuard {
    /// Disconnect even when the result future is abandoned while its widget remains alive.
    fn drop(&mut self) {
        if let Some(notification) = self.notification.take() {
            notification.disconnect();
        }
    }
}

/// Await worker completion without retaining its widget; destruction aborts and reaps the task.
/// Dropping the returned future also cancels the worker, even if it has never been polled.
/// All browser result-delivery callbacks share this cancellation boundary on the GTK context.
fn widget_task_result<T: 'static>(
    widget: &impl IsA<glib::Object>,
    mut task: tokio::task::JoinHandle<T>,
) -> impl std::future::Future<Output = Option<Result<T, tokio::task::JoinError>>> + 'static {
    let (destroyed_tx, mut destroyed_rx) = tokio::sync::oneshot::channel();
    let destruction = widget
        .upcast_ref::<glib::Object>()
        .add_weak_ref_notify_local(move || {
            let _ = destroyed_tx.send(());
        });
    let guard = WidgetTaskGuard {
        _task: crate::task::AbortOnDrop(task.abort_handle()),
        notification: Some(destruction),
    };
    async move {
        let _guard = guard;
        let result = tokio::select! {
            biased;
            _ = &mut destroyed_rx => {
                task.abort();
                let _ = task.await;
                None
            }
            result = &mut task => Some(result),
        };
        result
    }
}

/// Start bounded snapshot I/O on Tokio without holding application state across execution.
/// The overlay label owns result delivery and cancels the exchange when destroyed.
fn load_devtools_snapshot(
    state: &Rc<RefCell<AppState>>,
    surface_uuid: uuid::Uuid,
    label: &gtk4::Label,
) {
    let mut activity = crate::browser::metrics::Activity::begin("devtools_snapshot", None);
    let task = {
        let state = state.borrow();
        let (Some(browser), Some(runtime)) = (
            state.browser_sessions.get(&surface_uuid),
            state.runtime_handle.as_ref(),
        ) else {
            activity.finish("unavailable");
            label.set_text("No browser session active");
            return;
        };
        runtime.spawn(browser.snapshot_async(activity.id))
    };
    finish_devtools_snapshot(label, task, activity);
}

/// Update a surviving snapshot label on GTK, cancelling worker I/O when its overlay is removed.
/// Weak ownership prevents the pending request from retaining a closed browser tab.
fn finish_devtools_snapshot(
    label: &gtk4::Label,
    task: tokio::task::JoinHandle<Result<String, String>>,
    mut activity: crate::browser::metrics::Activity,
) {
    let completion = widget_task_result(label, task);
    let label = label.downgrade();
    glib::MainContext::default().spawn_local(async move {
        let Some(result) = completion.await else {
            activity.finish("cancelled");
            return;
        };
        let Some(label) = label.upgrade() else {
            activity.finish("stale_widget");
            return;
        };
        let text = match result {
            Ok(Ok(text)) => {
                activity.finish("success");
                text
            }
            Ok(Err(error)) => {
                activity.finish("error");
                format!("Snapshot error: {error}")
            }
            Err(error) => {
                activity.finish("task_error");
                format!("Snapshot task error: {error}")
            }
        };
        label.set_text(&text);
    });
}
/// Run initial viewport sizing on Tokio; destruction cancels I/O and GTK never awaits under a model borrow.
fn resize_browser_preview(
    state: &Rc<RefCell<AppState>>,
    surface_uuid: uuid::Uuid,
    picture: &gtk4::Picture,
) {
    let (width, height) = (picture.width(), picture.height());
    if width <= 0 || height <= 0 {
        return;
    }
    let mut activity = super::metrics::Activity::begin("viewport", None);
    let task = {
        let s = state.borrow();
        let (Some(browser), Some(runtime)) = (
            s.browser_sessions.get(&surface_uuid),
            s.runtime_handle.as_ref(),
        ) else {
            activity.finish("unavailable");
            return;
        };
        runtime.spawn(browser.resize_async(width, height, activity.id))
    };
    let completion = widget_task_result(picture, task);
    glib::MainContext::default().spawn_local(async move {
        let Some(result) = completion.await else {
            activity.finish("cancelled");
            return;
        };
        activity.finish(match result {
            Ok(Ok(())) => "success",
            Ok(Err(_)) => "error",
            Err(_) => "task_error",
        });
    });
}

/// Release held keys for the originating manager; defer focus signals emitted during model mutation.
fn release_browser_keys(state: Rc<RefCell<AppState>>, surface_uuid: uuid::Uuid, session: String) {
    if let Ok(mut s) = state.try_borrow_mut() {
        if let Some(browser) = s
            .browser_sessions
            .get_mut(&surface_uuid)
            .filter(|browser| browser.session_name == session)
        {
            browser.release_input_keys();
        }
    } else {
        let state = Rc::downgrade(&state);
        glib::idle_add_local_once(move || {
            if let Some(state) = state.upgrade() {
                release_browser_keys(state, surface_uuid, session);
            }
        });
    }
}

/// Admit a mouse gesture under a short GTK borrow; the manager owns ordered socket delivery.
fn forward_mouse_input(
    state: &Rc<RefCell<AppState>>,
    surface_uuid: uuid::Uuid,
    events: Vec<serde_json::Value>,
) -> bool {
    let mut s = state.borrow_mut();
    let Some(runtime) = s.runtime_handle.clone() else {
        return false;
    };
    s.browser_sessions
        .get_mut(&surface_uuid)
        .is_some_and(|browser| browser.queue_mouse(&runtime, events))
}

/// Admit a physical key transition without blocking GTK, reserving future release capacity.
fn forward_key_input(
    state: &Rc<RefCell<AppState>>,
    surface_uuid: uuid::Uuid,
    physical: u32,
    pressed: bool,
    params: serde_json::Value,
) -> bool {
    let mut s = state.borrow_mut();
    let Some(runtime) = s.runtime_handle.clone() else {
        return false;
    };
    s.browser_sessions
        .get_mut(&surface_uuid)
        .is_some_and(|browser| browser.queue_key(&runtime, physical, pressed, params))
}

/// Run history navigation on Tokio and update its surviving address widget on GTK.
/// Widget destruction cancels the child operation; no AppState borrow crosses an await.
fn run_browser_navigation(
    state: &Rc<RefCell<AppState>>,
    surface_uuid: uuid::Uuid,
    entry: &gtk4::Entry,
    command: &str,
) {
    let mut activity = crate::browser::metrics::Activity::begin("history_navigation", None);
    let task = {
        let s = state.borrow();
        let (Some(browser), Some(runtime)) = (
            s.browser_sessions.get(&surface_uuid),
            s.runtime_handle.as_ref(),
        ) else {
            activity.finish("unavailable");
            return;
        };
        runtime.spawn(browser.navigate_async(command.to_owned(), activity.id))
    };
    finish_browser_navigation(state, entry, task, activity);
}

/// Normalize a typed address and submit viewport/open commands through the shared navigation gate.
fn navigate_browser_entry(
    state: &Rc<RefCell<AppState>>,
    surface_uuid: uuid::Uuid,
    entry: &gtk4::Entry,
    picture: &gtk4::Picture,
) {
    let raw_url = entry.text().to_string();
    if raw_url.is_empty() {
        return;
    }
    let url = crate::browser_address::normalize(&raw_url);
    entry.set_text(&url);
    let mut activity = crate::browser::metrics::Activity::begin("url_navigation", None);
    let task = {
        let s = state.borrow();
        let (Some(browser), Some(runtime)) = (
            s.browser_sessions.get(&surface_uuid),
            s.runtime_handle.as_ref(),
        ) else {
            activity.finish("unavailable");
            return;
        };
        runtime.spawn(browser.open_async(
            url,
            Some((picture.width(), picture.height())),
            activity.id,
        ))
    };
    finish_browser_navigation(state, entry, task, activity);
}

/// Apply surviving navigation results on GTK and abort worker execution when its entry is destroyed.
/// Share cancellation, stale-address checks and persistence across history and explicit URL entry.
fn finish_browser_navigation(
    state: &Rc<RefCell<AppState>>,
    entry: &gtk4::Entry,
    task: tokio::task::JoinHandle<Result<Option<String>, String>>,
    mut activity: crate::browser::metrics::Activity,
) {
    let completion = widget_task_result(entry, task);
    let original_url = entry.text();
    let entry = entry.downgrade();
    let state = Rc::downgrade(state);
    glib::MainContext::default().spawn_local(async move {
        let Some(result) = completion.await else {
            activity.finish("cancelled");
            return;
        };
        match result {
            Ok(Ok(Some(url))) => {
                if let (Some(entry), Some(state)) = (entry.upgrade(), state.upgrade()) {
                    if entry.text() != original_url {
                        activity.finish("stale_widget");
                        return;
                    }
                    entry.set_text(&url);
                    state.borrow().trigger_session_save();
                    activity.finish("success");
                }
            }
            Ok(Ok(None)) => activity.finish("missing_url"),
            Ok(Err(error)) => {
                activity.finish("error");
                eprintln!("cmux: browser navigation failed: {error}");
            }
            Err(error) => {
                activity.finish(if error.is_cancelled() {
                    "cancelled"
                } else {
                    "task_error"
                });
                eprintln!("cmux: browser navigation task failed: {error}");
            }
        }
    });
}

/// Close the browser preview and shut down the daemon (Ctrl+Shift+Q).
pub fn handle_browser_close(state: &Rc<RefCell<AppState>>) {
    let mut state = state.borrow_mut();
    let targets: Vec<_> = state
        .split_engines
        .iter()
        .flat_map(|engine| engine.browser_tabs())
        .map(|widgets| widgets.uuid)
        .collect();
    for id in targets {
        state.close_browser_surface(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Delayed snapshot delivery leaves GTK responsive; destroying the label aborts owned work.
    #[test]
    #[ignore = "requires GTK display; run in headless Linux CI"]
    fn devtools_snapshot_delivery_and_cancellation() {
        gtk4::init().unwrap();
        let context = glib::MainContext::default();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        for (response, expected) in [
            (
                Ok(serde_json::json!({"data": "DOM snapshot"})),
                "DOM snapshot",
            ),
            (Ok(serde_json::json!({"result": "alternate"})), "alternate"),
            (
                Ok(serde_json::json!({"data": {"nodes": []}})),
                r#"{"data":{"nodes":[]}}"#,
            ),
            (
                Err("peer unavailable".to_string()),
                "Snapshot error: peer unavailable",
            ),
        ] {
            let label = gtk4::Label::new(Some("Loading snapshot…"));
            let (reply, receiver) =
                tokio::sync::oneshot::channel::<Result<serde_json::Value, String>>();
            let task = runtime.spawn(async move {
                receiver
                    .await
                    .unwrap()
                    .and_then(crate::browser::snapshot_text)
            });
            finish_devtools_snapshot(
                &label,
                task,
                crate::browser::metrics::Activity::begin("devtools_test", None),
            );
            context.block_on(async {
                // A GTK timer must run while the worker still awaits its reply.
                glib::timeout_future(std::time::Duration::from_millis(20)).await;
                assert_eq!(label.text(), "Loading snapshot…");
                reply.send(response).unwrap();
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                while label.text() != expected {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "snapshot result was not delivered"
                    );
                    glib::timeout_future(std::time::Duration::from_millis(10)).await;
                }
            });
        }

        // Abandoning result delivery must cancel the worker even while its widget survives.
        let retained_label = gtk4::Label::new(None);
        let (abandoned_reply, receiver) = tokio::sync::oneshot::channel::<()>();
        let task = runtime.spawn(async move {
            let _ = receiver.await;
        });
        drop(widget_task_result(&retained_label, task));
        runtime.block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(3), async {
                while !abandoned_reply.is_closed() {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
        });
        assert!(retained_label.downgrade().upgrade().is_some());

        let label = gtk4::Label::new(Some("Loading snapshot…"));
        let weak = label.downgrade();
        let (reply, receiver) =
            tokio::sync::oneshot::channel::<Result<serde_json::Value, String>>();
        let task = runtime.spawn(async move {
            receiver
                .await
                .unwrap()
                .and_then(crate::browser::snapshot_text)
        });
        finish_devtools_snapshot(
            &label,
            task,
            crate::browser::metrics::Activity::begin("devtools_test", None),
        );
        drop(label);
        context.block_on(async {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while !reply.is_closed() {
                assert!(
                    std::time::Instant::now() < deadline,
                    "closed overlay retained snapshot task"
                );
                glib::timeout_future(std::time::Duration::from_millis(10)).await;
            }
        });
        assert!(
            weak.upgrade().is_none(),
            "worker retained the destroyed label"
        );
    }
}
