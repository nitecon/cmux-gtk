//! Global gateway preferences and guarded GTK input; network transport remains pending its stream spec.
#[allow(dead_code)]
// Internal normalized ingress/readiness types await the stream and hookless observer adapters.
pub mod model;
mod pipeline;
mod storage;
mod ui;
mod worker;

use crate::app_state::AppStateRef;
use model::*;
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot, watch};

pub use ui::append_preferences;

/// GTK owns observations and native handles; the service owns storage and future transport.
pub struct Handle {
    requests: mpsc::Sender<worker::Request>,
    snapshots: watch::Sender<Vec<Terminal>>,
    pub view: watch::Receiver<View>,
    observations: std::collections::HashMap<String, Observation>,
    task: tokio::task::JoinHandle<()>,
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
        observations: Default::default(),
        task,
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
fn terminals(state: &crate::app_state::AppState) -> Vec<Terminal> {
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
            let observation = state
                .gateway
                .as_ref()
                .and_then(|g| g.observations.get(&surface_id))
                .cloned();
            result.push(Terminal {
                workspace_id: workspace.uuid.to_string(),
                surface_id,
                directory: directory.into(),
                foreground_pid: pid,
                input_revision: crate::ghostty::registry::input_revision(pointer as usize),
                input_pending: crate::ghostty::registry::input_pending(pointer as usize),
                observation,
            });
        }
    }
    result
}

/// Publish bounded metadata and retire observations for closed terminals; unknown readiness stays unknown.
fn snapshot(state: &AppStateRef) {
    let current = terminals(&state.borrow());
    if let Some(gateway) = &mut state.borrow_mut().gateway {
        gateway
            .observations
            .retain(|id, _| current.iter().any(|t| &t.surface_id == id));
        gateway.snapshots.send_replace(current);
    }
}

/// Hookless provider adapters may supply recent readiness and author identity, never restored session metadata.
/// Must run on GTK after observing the exact live foreground process and empty provider prompt.
#[allow(dead_code)] // Remains fail-closed until a reliable provider observer is wired.
pub(crate) fn observe_input(
    state: &AppStateRef,
    surface: &str,
    observation: Observation,
) -> Result<(), String> {
    let current = terminals(&state.borrow());
    let terminal = current
        .iter()
        .find(|t| t.surface_id == surface)
        .ok_or("Terminal is no longer active")?;
    if terminal.foreground_pid != observation.process.pid
        || terminal.input_revision != observation.input_revision
        || observation.observed_at.elapsed() >= std::time::Duration::from_secs(1)
    {
        return Err("Stale agent input observation".into());
    }
    if let Some(actor) = &observation.actor_id {
        identity(actor)?;
    }
    state
        .borrow_mut()
        .gateway
        .as_mut()
        .ok_or("Gateway unavailable")?
        .observations
        .insert(surface.into(), observation);
    snapshot(state);
    Ok(())
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
        let current = terminals(&state)
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
        };
        if message.kind == Kind::Commented
            && checked
                .terminal
                .observation
                .as_ref()
                .and_then(|o| o.actor_id.as_ref())
                .is_none_or(|actor| Some(actor) == message.author_id.as_ref())
        {
            return Err("Task comment attribution changed".into());
        }
        if checked.terminal.input_revision != expected.terminal.input_revision || !checked.ready() {
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
        // Display-only annotation: trusted ANSI style and validated literal text never go into agent stdin.
        let annotation = format!("\r\n\x1b[36m{}\x1b[0m\r\n", text.replace("\n", "\r\n"));
        crate::ghostty::ffi::ghostty_surface_process_output(
            pointer,
            annotation.as_ptr().cast(),
            annotation.len(),
        );
    }
    state
        .borrow_mut()
        .gateway
        .as_mut()
        .unwrap()
        .observations
        .remove(&expected.terminal.surface_id);
    snapshot(state);
    Ok(DeliveryOutcome::Injected)
}

/// Submit global preferences through a bounded worker queue; no terminal-injection or readiness RPC is exposed.
pub fn submit(
    state: &AppStateRef,
    method: &str,
    params: &Value,
) -> Result<oneshot::Receiver<Result<Value, String>>, String> {
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
