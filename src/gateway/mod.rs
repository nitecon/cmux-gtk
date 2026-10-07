//! Global lifecycle subscription and consent-guarded GTK terminal input.
pub mod model;
mod pipeline;
mod readiness;
mod storage;
mod ui;
mod wire;
mod worker;

use crate::app_state::AppStateRef;
use model::*;
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot, watch};

pub use ui::append_preferences;

/// GTK owns native handles; the service owns transport, readiness observations and durable delivery.
pub struct Handle {
    requests: mpsc::Sender<worker::Request>,
    snapshots: watch::Sender<Vec<Terminal>>,
    pub view: watch::Receiver<View>,
    task: tokio::task::JoinHandle<()>,
    runtime: tokio::runtime::Handle,
}

impl Drop for Handle {
    /// Cancel the service before native state retires; cancelled delivery replies cannot type later.
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Attach one owned service and a weak GTK delivery listener without launching or focusing agents.
pub fn start(state: &AppStateRef, runtime: &tokio::runtime::Handle) {
    let (requests, rx) = mpsc::channel(32);
    let (snapshots, sx) = watch::channel(Vec::new());
    let (view_tx, view) = watch::channel(View::default());
    let (deliveries, mut delivery_rx) = mpsc::channel::<worker::Delivery>(32);
    let task = runtime.spawn(worker::run(rx, sx, view_tx, deliveries));
    state.borrow_mut().gateway = Some(Handle {
        requests,
        snapshots,
        view,
        task,
        runtime: runtime.clone(),
    });
    let weak = std::rc::Rc::downgrade(state);
    glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
        let Some(state) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        snapshot(&state);
        glib::ControlFlow::Continue
    });
    let weak = std::rc::Rc::downgrade(state);
    glib::MainContext::default().spawn_local(async move {
        while let Some(delivery) = delivery_rx.recv().await {
            let Some(state) = weak.upgrade() else {
                break;
            };
            if !delivery.reply.is_closed() {
                let _ = delivery
                    .reply
                    .send(deliver(&state, &delivery.message, &delivery.session));
            }
        }
    });
    snapshot(state);
}

/// Capture local surface identity, current directory and input revision on GTK, without interpreting resume state.
fn terminals(state: &crate::app_state::AppState, capture_screen: bool) -> Vec<Terminal> {
    let mut result = Vec::new();
    for (index, engine) in state.split_engines.iter().enumerate() {
        let Some(workspace) = state.workspaces.get(index) else {
            continue;
        };
        if workspace.remote_target.is_some() {
            continue;
        }
        for (surface_id, _, _) in engine.all_panes() {
            let surface_id = surface_id.to_string();
            let Some(pointer) = engine.find_surface_by_uuid(&surface_id) else {
                continue;
            };
            let directory = crate::ghostty::registry::working_directory(pointer as usize);
            if !std::path::Path::new(&directory).is_absolute() {
                continue;
            }
            // SAFETY: the engine owns this live pointer on GTK; getters neither transfer ownership nor iterate events.
            let pid = unsafe {
                if crate::ghostty::ffi::ghostty_surface_process_exited(pointer) {
                    0
                } else {
                    crate::ghostty::ffi::ghostty_surface_foreground_pid(pointer)
                }
            };
            let capture = capture_screen
                && state.gateway.as_ref().is_some_and(|g| {
                    let view = g.view.borrow();
                    view.config.enabled && view.config.injection_approved
                });
            // SAFETY: engine keeps the native surface live; the bounded getter does not iterate GTK events.
            let screen = if capture {
                unsafe { crate::ghostty::text::read_prompt_grid(pointer) }
            } else {
                None
            };
            result.push(Terminal {
                workspace_id: workspace.uuid.to_string(),
                surface_id,
                directory: directory.into(),
                foreground_pid: pid,
                input_revision: crate::ghostty::registry::input_revision(pointer as usize),
                input_pending: crate::ghostty::registry::input_pending(pointer as usize),
                observation: None,
                screen,
                captured_at: Some(std::time::Instant::now()),
            });
        }
    }
    result
}

/// Publish bounded metadata and retire observations for closed terminals; unknown readiness stays unknown.
fn snapshot(state: &AppStateRef) {
    let current = terminals(&state.borrow(), true);
    if let Some(gateway) = &mut state.borrow_mut().gateway {
        gateway.snapshots.send_replace(current);
    }
}

/// Recheck consent, pinned target and unchanged ready input immediately before typing on GTK.
fn deliver(
    state: &AppStateRef,
    message: &Message,
    expected: &Session,
) -> Result<DeliveryOutcome, String> {
    message.validate()?;
    let pointer = {
        let state = state.borrow();
        let gateway = state.gateway.as_ref().ok_or("Gateway unavailable")?;
        let view = gateway.view.borrow();
        if !view.config.enabled || !view.config.injection_approved {
            return Err("Gateway injection is disabled".into());
        }
        let current = terminals(&state, true)
            .into_iter()
            .find(|t| t.surface_id == expected.terminal.surface_id)
            .ok_or("Agent terminal closed")?;
        if current.workspace_id != expected.terminal.workspace_id
            || current.directory != expected.terminal.directory
            || current.foreground_pid != expected.process.pid
        {
            return Err("Agent terminal changed before delivery".into());
        }
        let checked = Session {
            terminal: current,
            process: expected.process.clone(),
            repository: expected.repository.clone(),
            session_id: expected.session_id.clone(),
        };
        if checked.terminal.input_revision != expected.terminal.input_revision
            || checked.terminal.input_pending
            || !expected.ready()
            || checked.terminal.screen.as_ref().is_none_or(|f| {
                readiness::classify(&expected.process.client, f) != InputState::EmptyReady
            })
        {
            return Ok(DeliveryOutcome::Deferred);
        }
        state
            .split_engines
            .iter()
            .find_map(|e| e.find_surface_by_uuid(&expected.terminal.surface_id))
            .ok_or("Terminal is no longer attached")?
    };
    let text = message.terminal_text();
    // SAFETY: GTK owns this exact live target; model borrows are released, with no event-loop iteration or teardown.
    unsafe {
        crate::ghostty::text::send_literal(pointer, &text).map_err(str::to_owned)?;
        crate::ghostty::text::send_character(pointer, '\r').map_err(str::to_owned)?;
    }
    // The foreground agent owns terminal output and cursor state, including rendering the submitted message.
    // Writing an extra annotation into its output stream would invalidate incremental TUI redraws.
    snapshot(state);
    Ok(DeliveryOutcome::Injected)
}

/// Submit preferences or read-only session discovery; identity lookup never authorizes terminal input.
pub fn submit(
    state: &AppStateRef,
    method: &str,
    params: &Value,
) -> Result<oneshot::Receiver<Result<Value, String>>, String> {
    if matches!(method, "gateway.session" | "gateway.sessions") {
        let surface_id = if method == "gateway.session" {
            let surface = params["surface_id"].as_str().ok_or("Missing surface_id")?;
            uuid::Uuid::parse_str(surface).map_err(|_| "Invalid surface identity")?;
            Some(surface.to_owned())
        } else {
            None
        };
        let state = state.borrow();
        let gateway = state.gateway.as_ref().ok_or("Gateway unavailable")?;
        let instance_id = gateway.view.borrow().instance_id.clone();
        if instance_id.is_empty() {
            return Err("Gateway identity is still loading".into());
        }
        let terminals = terminals(&state, false);
        let (reply, result) = oneshot::channel();
        // Identity queries must not wait behind network hydration or repository discovery.
        gateway.runtime.spawn(async move {
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(1),
                worker::context(terminals, instance_id, surface_id),
            )
            .await
            .unwrap_or_else(|_| Err("Agent identity lookup timed out".into()));
            let _ = reply.send(result);
        });
        return Ok(result);
    }
    if method != "gateway.configure" {
        return Err("Unknown gateway operation".into());
    }
    let enabled = params["enabled"]
        .as_bool()
        .ok_or("enabled must be a boolean")?;
    let url = params["url"].as_str().ok_or("Missing url")?.to_owned();
    if enabled || !url.is_empty() {
        endpoint(&url)?;
    }
    let injection_approved = params["injection_approved"]
        .as_bool()
        .ok_or("injection_approved must be a boolean")?;
    let key = match params.get("api_key") {
        None | Some(Value::Null) => None,
        Some(Value::String(key)) => {
            storage::validate_key(key)?;
            Some(key.clone())
        }
        _ => return Err("api_key must be a string".into()),
    };
    let config = Config {
        enabled,
        url,
        injection_approved,
    };
    let state = state.borrow();
    let gateway = state.gateway.as_ref().ok_or("Gateway unavailable")?;
    let (reply, result) = oneshot::channel();
    gateway
        .requests
        .try_send(worker::Request {
            action: worker::Action::Configure { config, key },
            reply,
        })
        .map_err(|_| "Gateway operation queue is full or stopped")?;
    Ok(result)
}

/// Route local JSON-RPC through the same GTK validation and asynchronous worker used by preferences.
pub fn rpc(
    state: &AppStateRef,
    method: &str,
    params: Value,
    req_id: Value,
    resp_tx: crate::socket::commands::RespTx,
) {
    if method == "gateway.status" {
        let value = state
            .borrow()
            .gateway
            .as_ref()
            .map(|g| json!(&*g.view.borrow()))
            .unwrap_or(Value::Null);
        let _ = resp_tx.send(crate::socket::response::ok(req_id, value));
        return;
    }
    match submit(state, method, &params) {
        Ok(result) => {
            glib::MainContext::default().spawn_local(async move {
                let response = match result.await {
                    Ok(Ok(value)) => crate::socket::response::ok(req_id, value),
                    Ok(Err(error)) => crate::socket::response::err(req_id, "gateway_error", &error),
                    Err(_) => crate::socket::response::err(
                        req_id,
                        "gateway_error",
                        "Gateway worker stopped",
                    ),
                };
                let _ = resp_tx.send(response);
            });
        }
        Err(error) => {
            let _ = resp_tx.send(crate::socket::response::err(
                req_id,
                "invalid_params",
                &error,
            ));
        }
    }
}
