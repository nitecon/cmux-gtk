//! Optional interactive gateway bridge. GTK owns routing; the worker owns transport and durable state.
pub mod model;
mod storage;
mod ui;
mod worker;

use crate::app_state::AppStateRef;
use model::*;
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot, watch};

pub use ui::append_preferences;

/// GTK-owned handle and native session snapshot; no credential or network object lives in AppState.
pub struct Handle {
    requests: mpsc::Sender<worker::Request>,
    snapshots: watch::Sender<Vec<Session>>,
    pub view: watch::Receiver<View>,
    sessions: Vec<Session>,
    /// Ephemeral foreground IDs confirmed by live native hooks, never restored as readiness.
    foregrounds: std::collections::HashMap<String, (String, u64)>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Handle {
    /// Cancel the owned connection when GTK state is retired; pre-delivery fences are already durable.
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Attach one worker and a weak GTK listener; periodic snapshots detect closed/moved native surfaces.
pub fn start(state: &AppStateRef, runtime: &tokio::runtime::Handle) {
    let (requests, rx) = mpsc::channel(32);
    let (snapshots, sx) = watch::channel(Vec::new());
    let (view_tx, view) = watch::channel(View::default());
    let (events, mut event_rx) = mpsc::channel(32);
    let task = runtime.spawn(worker::run(rx, sx, view_tx, events));
    state.borrow_mut().gateway = Some(Handle {
        requests,
        snapshots,
        view,
        sessions: Vec::new(),
        foregrounds: Default::default(),
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
        while let Some(event) = event_rx.recv().await {
            let Some(state) = weak.upgrade() else { break; };
            match event {
                worker::Event::Offered(assignment) => {
                    let mut state = state.borrow_mut();
                    let _ = crate::inbox_actions::handle(&mut state, crate::inbox::Action::Create {
                        scope: crate::inbox::Scope { workspace_id:assignment.workspace_id.parse().ok(),
                            surface_id:assignment.surface_id.parse().ok() },
                        content: crate::inbox::Content { title:"Delegated task ready".into(),
                            subtitle:assignment.task_id.clone(),
                            body:"Open Preferences → Gateway tasks. Confirm the agent is at an empty prompt, then choose Send to agent.".into() },
                    });
                }
                worker::Event::Deliver { assignment, session, reply } => {
                    // A cancelled worker must not leave delayed native input queued on GTK.
                    if !reply.is_closed() { let _ = reply.send(deliver(&state, &assignment, &session)); }
                }
            }
        }
    });
    snapshot(state);
}

/// Resolve a local terminal and its current native binding without moving focus.
fn native_session(
    state: &crate::app_state::AppState,
    surface: &str,
) -> Option<(Session, crate::ghostty::ffi::ghostty_surface_t)> {
    let (index, engine) = state
        .split_engines
        .iter()
        .enumerate()
        .find(|(_, engine)| engine.find_surface_by_uuid(surface).is_some())?;
    let workspace = state.workspaces.get(index)?;
    // Local hook transport establishes native identity; remote shell input is not a supported executor.
    if workspace.remote_target.is_some() {
        return None;
    }
    let binding = engine
        .resume_action(surface, &crate::resume::ResumeAction::Show)
        .ok()??;
    let kind = binding.kind?;
    if !matches!(kind.as_str(), "claude" | "codex") {
        return None;
    }
    let cwd = binding
        .cwd
        .filter(|cwd| std::path::Path::new(cwd).is_absolute())?;
    let session_id = binding.checkpoint_id?;
    identity(&cwd).ok()?;
    identity(&session_id).ok()?;
    Some((
        Session {
            workspace_id: workspace.uuid.to_string(),
            surface_id: surface.into(),
            session_id,
            project_ident: String::new(),
            client: kind,
            model: None,
            cwd,
            state: "busy".into(),
        },
        engine.find_surface_by_uuid(surface)?,
    ))
}

/// Read native foreground identity on GTK while keeping the resolved handle alive.
fn foreground(pointer: crate::ghostty::ffi::ghostty_surface_t) -> u64 {
    // SAFETY: caller resolves a live native terminal on GTK immediately before these
    // bounded non-callback metadata getters; no ownership is transferred.
    unsafe {
        if crate::ghostty::ffi::ghostty_surface_process_exited(pointer) {
            0
        } else {
            crate::ghostty::ffi::ghostty_surface_foreground_pid(pointer)
        }
    }
}

/// Advertise existing bindings conservatively as busy until a live provider hook establishes readiness.
fn snapshot(state: &AppStateRef) {
    let mut state = state.borrow_mut();
    let mut current = Vec::new();
    for engine in &state.split_engines {
        for (id, _, _) in engine.all_panes() {
            if let Some((mut session, pointer)) = native_session(&state, &id.to_string()) {
                if let Some(previous) = state.gateway.as_ref().and_then(|g| {
                    g.sessions.iter().find(|s| {
                        s.surface_id == session.surface_id && s.session_id == session.session_id
                    })
                }) {
                    let current_pid = foreground(pointer);
                    let attached = state
                        .gateway
                        .as_ref()
                        .and_then(|g| g.foregrounds.get(&session.surface_id))
                        .is_some_and(|(native, pid)| {
                            native == &session.session_id && *pid > 0 && *pid == current_pid
                        });
                    if attached
                        && previous.workspace_id == session.workspace_id
                        && previous.cwd == session.cwd
                    {
                        session.state = previous.state.clone();
                    }
                }
                current.push(session);
            }
        }
    }
    if let Some(gateway) = &mut state.gateway {
        gateway.sessions = current;
        gateway.foregrounds.retain(|surface, (native, _)| {
            gateway
                .sessions
                .iter()
                .any(|s| &s.surface_id == surface && &s.session_id == native)
        });
        let snapshot = gateway.sessions.clone();
        gateway.snapshots.send_if_modified(|existing| {
            if *existing == snapshot {
                false
            } else {
                *existing = snapshot;
                true
            }
        });
    }
}

/// Recheck surface, workspace, native session and idle hook immediately before literal input on GTK.
fn deliver(state: &AppStateRef, assignment: &Assignment, expected: &Session) -> Result<(), String> {
    let pointer = {
        let state = state.borrow();
        let gateway = state.gateway.as_ref().ok_or("Gateway bridge unavailable")?;
        if !gateway.view.borrow().config.enabled {
            return Err("Gateway integration was disabled".into());
        }
        let mapping = gateway.view.borrow().config.mappings.iter().any(|m| {
            m.workspace_id == assignment.workspace_id && m.project_ident == assignment.project_ident
        });
        if !mapping {
            return Err("Workspace mapping changed".into());
        }
        let (session, pointer) = native_session(&state, &assignment.surface_id)
            .ok_or("Native session is no longer attached")?;
        let pid = foreground(pointer);
        let attached =
            gateway
                .foregrounds
                .get(&session.surface_id)
                .is_some_and(|(native, saved)| {
                    native == &session.session_id && *saved > 0 && *saved == pid
                });
        if !attached
            || session.cwd != expected.cwd
            || session.client != expected.client
            || session.workspace_id != assignment.workspace_id
            || session.session_id != assignment.session_id
            || !gateway.sessions.iter().any(|s| {
                s.surface_id == session.surface_id
                    && s.session_id == session.session_id
                    && s.state == "idle"
            })
        {
            return Err("Native session became busy or changed; prompt not sent".into());
        }
        pointer
    };
    let prompt = assignment.terminal_prompt();
    // SAFETY: exact live native target resolved on GTK; all RefCell borrows released,
    // no callback/event-loop iteration or teardown between the paste and separate Enter.
    unsafe {
        crate::ghostty::text::send_literal(pointer, &prompt).map_err(str::to_owned)?;
        crate::ghostty::text::send_character(pointer, '\r').map_err(str::to_owned)?;
    }
    if let Some(gateway) = &mut state.borrow_mut().gateway {
        if let Some(session) = gateway.sessions.iter_mut().find(|s| {
            s.surface_id == assignment.surface_id && s.session_id == assignment.session_id
        }) {
            session.state = "busy".into();
        }
    }
    snapshot(state);
    Ok(())
}

/// Parse a bounded local operation and validate its current GTK context before enqueueing a worker.
fn action(
    state: &mut crate::app_state::AppState,
    method: &str,
    params: &Value,
) -> Result<worker::Action, String> {
    let string = |key: &str| {
        params[key]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| format!("Missing {key}"))
    };
    match method {
        "gateway.configure" => {
            let enabled = params["enabled"]
                .as_bool()
                .ok_or("enabled must be a boolean")?;
            let url = string("url")?;
            if enabled || !url.is_empty() {
                endpoint(&url)?;
            }
            let key = match params.get("api_key") {
                None | Some(Value::Null) => None,
                Some(Value::String(key)) => {
                    storage::validate_key(key)?;
                    Some(key.clone())
                }
                _ => return Err("api_key must be a string".into()),
            };
            Ok(worker::Action::Configure { enabled, url, key })
        }
        "gateway.bind" => {
            let workspace = string("workspace_id")?;
            if !state
                .workspaces
                .iter()
                .any(|w| w.uuid.to_string() == workspace && w.remote_target.is_none())
            {
                return Err("Choose an existing local workspace".into());
            }
            Ok(worker::Action::Bind {
                workspace,
                project: string("project_ident")?,
            })
        }
        "gateway.accept" => {
            if params["confirm_ready"] != true {
                return Err(
                    "Confirm that the native agent is at an empty prompt before sending".into(),
                );
            }
            Ok(worker::Action::Accept {
                run: string("run_id")?,
            })
        }
        "gateway.report" => {
            let report_state = string("state")?.replace('-', "_");
            let summary = params
                .get("summary")
                .and_then(Value::as_str)
                .map(str::to_owned);
            if matches!(report_state.as_str(), "finished" | "failed")
                && summary.as_ref().is_none_or(|s| s.trim().is_empty())
            {
                return Err("A final report requires a concise summary".into());
            }
            Ok(worker::Action::Report {
                run: string("run_id")?,
                state: report_state,
                message: string("message")?,
                summary,
            })
        }
        "gateway.agent_event" => {
            let surface = string("surface_id")?;
            let session_id = string("session_id")?;
            let client = string("client")?;
            let event = string("event")?;
            identity(&session_id)?;
            let (mut native, pointer) = native_session(state, &surface)
                .ok_or("Agent hook target is not a supported local native session")?;
            if native.session_id != session_id || native.client != client {
                return Err("Agent hook identity mismatch".into());
            }
            native.state = match event.as_str() {
                "start" | "stop" => "idle",
                "prompt" => "busy",
                "attention" => "waiting_input",
                "exit" => "exited",
                _ => return Err("Unknown agent lifecycle event".into()),
            }
            .into();
            let gateway = state.gateway.as_mut().ok_or("Gateway bridge unavailable")?;
            gateway
                .foregrounds
                .insert(surface.clone(), (session_id.clone(), foreground(pointer)));
            gateway.sessions.retain(|s| s.surface_id != surface);
            gateway.sessions.push(native);
            gateway.snapshots.send_replace(gateway.sessions.clone());
            Ok(worker::Action::Lifecycle {
                surface,
                session: session_id,
                event,
                message: bounded(params["message"].as_str().unwrap_or(""), 4096),
            })
        }
        _ => Err("Unknown gateway operation".into()),
    }
}

/// Submit a validated user operation with visible bounded-queue failure instead of silent dropping.
pub fn submit(
    state: &AppStateRef,
    method: &str,
    params: &Value,
) -> Result<oneshot::Receiver<Result<Value, String>>, String> {
    snapshot(state);
    let mut state = state.borrow_mut();
    let action = action(&mut state, method, params)?;
    let gateway = state.gateway.as_ref().ok_or("Gateway bridge unavailable")?;
    let (reply, result) = oneshot::channel();
    gateway
        .requests
        .try_send(worker::Request { action, reply })
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
        snapshot(state);
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
