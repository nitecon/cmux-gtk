//! Global lifecycle subscription and consent-guarded GTK terminal input.
mod actor;
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

/// GTK owns composition/execution; the service owns transport, process metadata and durable delivery.
pub struct Handle {
    requests: mpsc::Sender<worker::Request>,
    snapshots: watch::Sender<Vec<Terminal>>,
    pub view: watch::Receiver<View>,
    task: tokio::task::JoinHandle<()>,
    runtime: tokio::runtime::Handle,
}

impl Drop for Handle {
    /// Cancel the transport before native state retires; execution separately rechecks its live target.
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
    // Prompt observation must yield to native keyboard events and rendering.
    glib::timeout_add_local_full(
        std::time::Duration::from_millis(500),
        glib::Priority::DEFAULT_IDLE,
        move || {
            let Some(state) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            snapshot(&state);
            glib::ControlFlow::Continue
        },
    );
    let weak = std::rc::Rc::downgrade(state);
    glib::MainContext::default().spawn_local(async move {
        while let Some(mut delivery) = delivery_rx.recv().await {
            let Some(state) = weak.upgrade() else {
                break;
            };
            if !delivery.reply.is_closed() {
                let result = tokio::select! {
                    biased;
                    _ = delivery.reply.closed() => None,
                    result = deliver(&state, &delivery.message, &delivery.session) => Some(result),
                };
                if let Some(result) = result {
                    let _ = delivery.reply.send(result);
                }
            }
        }
    });
    snapshot(state);
}

/// Capture native surface identity, input generation and optional bounded prompt grids on GTK.
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
            let pid = unsafe { crate::ghostty::tty::root_pid(pointer) };
            let capture = capture_screen
                && state.gateway.as_ref().is_some_and(|gateway| {
                    let view = gateway.view.borrow();
                    view.config.enabled
                        && view.config.injection_approved
                        && view.prompt_surfaces.contains(&surface_id)
                });
            // SAFETY: the engine owns this live surface on GTK; bounded capture does not yield.
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
                composer_active: false,
                input_revision: crate::ghostty::registry::input_revision(pointer as usize),
                screen,
            });
        }
    }
    result
}

/// Publish native process metadata and bounded prompt observations without intercepting human input.
fn snapshot(state: &AppStateRef) {
    let current = terminals(&state.borrow(), true);
    if let Some(gateway) = &mut state.borrow_mut().gateway {
        gateway.snapshots.send_replace(current);
    }
}

/// Reverify the process, human input generation and empty native prompt before one atomic paste/Enter.
/// GTK owns native handles; a closed worker reply cancels submission after asynchronous process inspection.
async fn deliver(
    state: &AppStateRef,
    message: &Message,
    expected: &Session,
) -> Result<DeliveryOutcome, String> {
    message.validate()?;
    let runtime = state
        .borrow()
        .gateway
        .as_ref()
        .ok_or("Gateway unavailable")?
        .runtime
        .clone();
    let process = expected.process.clone();
    let root = expected.terminal.foreground_pid;
    let verified = runtime
        .spawn_blocking(move || {
            cmux_platform::process::input_identity(root).as_ref() == Some(&process)
        })
        .await
        .unwrap_or(false);
    if !verified {
        return Err("Agent process changed".into());
    }
    let pointer = {
        let state = state.borrow();
        let gateway = state.gateway.as_ref().ok_or("Gateway unavailable")?;
        let view = gateway.view.borrow();
        if !view.config.enabled || !view.config.injection_approved {
            return Err("Gateway injection is disabled".into());
        }
        if !fresh_at(message.created_at, now_millis()) {
            return Err("Gateway event expired before delivery".into());
        }
        let current = terminals(&state, true)
            .into_iter()
            .find(|terminal| terminal.surface_id == expected.terminal.surface_id)
            .ok_or("Agent terminal closed")?;
        if current.workspace_id != expected.terminal.workspace_id
            || current.directory != expected.terminal.directory
            || current.foreground_pid != expected.terminal.foreground_pid
        {
            return Err("Agent terminal changed before delivery".into());
        }
        if current.input_revision != expected.terminal.input_revision
            || current
                .screen
                .as_ref()
                .is_none_or(|frame| readiness::classify(frame) != InputState::EmptyReady)
        {
            return Ok(DeliveryOutcome::Deferred);
        }
        state
            .split_engines
            .iter()
            .find_map(|engine| engine.find_surface_by_uuid(&expected.terminal.surface_id))
            .ok_or("Terminal is no longer attached")?
    };
    // SAFETY: GTK keeps this exact surface live; no yield occurs between literal paste and native Enter.
    unsafe {
        crate::ghostty::text::send_literal(pointer, &message.terminal_text())
            .map_err(str::to_owned)?;
        if !crate::ghostty::text::submit(pointer, true) {
            return Ok(DeliveryOutcome::Uncertain);
        }
    }
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
    peer_pid: Option<u32>,
    req_id: Value,
    resp_tx: crate::socket::commands::RespTx,
) {
    if matches!(
        method,
        "gateway.session.announce" | "gateway.session.resolve"
    ) {
        let prepared = (|| {
            if !matches!(params["version"].as_u64(), Some(1 | 2)) {
                return Err("Unsupported actor identity version".to_owned());
            }
            let actor: actor::Actor = serde_json::from_value(params.clone())
                .map_err(|_| "Invalid actor announcement".to_owned())?;
            let peer_pid = peer_pid.ok_or("Caller process is not kernel authenticated")?;
            if params.get("enrollment_token").is_some() {
                return Err("Identity enrollment is not supported".into());
            }
            let repository = match params.get("repository") {
                None | Some(Value::Null) => None,
                Some(Value::String(repo)) => {
                    Some(model::repository(repo).ok_or("Invalid repository context")?)
                }
                _ => return Err("Invalid repository context".into()),
            };
            let state = state.borrow();
            let gateway = state.gateway.as_ref().ok_or("Gateway unavailable")?;
            Ok((
                actor,
                peer_pid,
                repository,
                gateway.runtime.clone(),
                gateway.requests.clone(),
            ))
        })();
        let Ok((actor, peer_pid, repository, runtime, requests)) = prepared else {
            let _ = resp_tx.send(crate::socket::response::err(
                req_id,
                "invalid_params",
                &prepared.err().unwrap(),
            ));
            return;
        };
        runtime.spawn(async move {
            let checked_actor = actor.clone();
            let result = async {
                tokio::task::spawn_blocking(move || checked_actor.verify_peer(peer_pid))
                    .await
                    .map_err(|_| "Caller identity worker stopped")??;
                let (reply, result) = oneshot::channel();
                requests
                    .try_send(worker::Request {
                        action: worker::Action::Actor { actor, repository },
                        reply,
                    })
                    .map_err(|_| "Gateway operation queue is full or stopped")?;
                tokio::time::timeout(std::time::Duration::from_secs(2), result)
                    .await
                    .map_err(|_| "Actor registration timed out")?
                    .map_err(|_| "Gateway worker stopped")?
            }
            .await;
            let response = match result {
                Ok(value) => crate::socket::response::ok(req_id, value),
                Err(error) => crate::socket::response::err(req_id, "identity_unverified", &error),
            };
            let _ = resp_tx.send(response);
        });
        return;
    }
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
