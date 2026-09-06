// src/socket/handlers.rs — GTK main thread command dispatch

use crate::socket::commands::SocketCommand;
use gtk4::prelude::*;
use serde_json::{json, Value};

use super::response::{err, ok};

/// Snapshot one workspace's identity and layout counts without changing focus or retaining widgets.
/// Return None for an absent index; missing engines produce unknown counts rather than fabricated zeroes.
fn workspace_record(state: &crate::app_state::AppState, index: usize) -> Option<Value> {
    let workspace = state.workspaces.get(index)?;
    let counts = state.split_engines.get(index).map(|engine| {
        let panes = engine.pane_info();
        let surfaces: usize = panes.iter().map(|pane| pane.surface_ids.len()).sum();
        (panes.len(), surfaces)
    });
    Some(json!({
        "index": index,
        "id": workspace.uuid,
        "uuid": workspace.uuid,
        "title": workspace.name,
        "name": workspace.name,
        "working_directory": workspace.working_directory.as_ref().map(|path| path.to_string_lossy()),
        "selected": index == state.active_index,
        "pane_count": counts.map(|(panes, _)| panes),
        "surface_count": counts.map(|(_, surfaces)| surfaces),
    }))
}

/// Resolve a live terminal in the current workspace without focus changes; GTK-thread callers only.
fn terminal_target(
    state: &crate::app_state::AppStateRef,
    id: Option<&str>,
) -> Result<crate::ghostty::ffi::ghostty_surface_t, (&'static str, &'static str)> {
    let surface = {
        let state = state.borrow();
        state
            .split_engines
            .get(state.active_index)
            .and_then(|engine| match id {
                Some(id) => engine.find_surface_by_uuid(id),
                None => engine.root.find_surface_for_pane(engine.active_pane_id),
            })
    }
    .filter(|surface| !surface.is_null())
    .ok_or(("not_found", "live terminal surface not found"))?;
    Ok(surface)
}

/// Send literal UTF-8 text to a live terminal in the active workspace without changing focus.
/// Resolve the optional UUID before native calls; reject missing targets and embedded NUL bytes.
fn send_terminal_text(
    state: &crate::app_state::AppStateRef,
    id: Option<&str>,
    text: &str,
) -> Result<(), (&'static str, &'static str)> {
    let surface = terminal_target(state, id)?;
    // SAFETY: target resolution found a live native terminal on GTK and released
    // the model borrow. No event-loop iteration or teardown occurs before delivery.
    unsafe { crate::ghostty::text::send_literal(surface, text) }
        .map_err(|message| ("invalid_params", message))
}

/// Resolve a surface_ref string ("surface:N" or UUID) to a UUID string.
/// Returns Ok(uuid_string) or Err((error_message, available_refs)).
fn resolve_surface_ref(
    surface_ref: &str,
    refs: &std::collections::HashMap<u32, String>,
) -> Result<String, (String, Vec<String>)> {
    if let Some(n_str) = surface_ref.strip_prefix("surface:") {
        if let Ok(n) = n_str.parse::<u32>() {
            if let Some(uuid) = refs.get(&n) {
                return Ok(uuid.clone());
            }
            let available: Vec<String> = refs.keys().map(|k| format!("surface:{}", k)).collect();
            return Err((format!("surface:{} not found", n), available));
        }
    }
    // Treat as UUID directly
    Ok(surface_ref.to_string())
}

/// Dispatch a SocketCommand on the GTK main thread.
/// SOCK-05: Only focus-intent commands (workspace.select, workspace.next/previous/last,
/// pane.focus, pane.last, surface.focus) may call grab_active_focus() or focus_active_surface().
#[allow(unused_variables)]
pub fn handle_socket_command(cmd: SocketCommand, state: &crate::app_state::AppStateRef) {
    handle_socket_command_traced(cmd, state, None);
}

/// Carry an observed request identity through dispatch into asynchronous service completion.
#[allow(unused_variables)]
fn handle_socket_command_traced(
    cmd: SocketCommand,
    state: &crate::app_state::AppStateRef,
    trace_id: Option<String>,
) {
    match cmd {
        SocketCommand::Observed {
            command,
            trace_id,
            queued_at,
        } => {
            let started = std::time::Instant::now();
            crate::diagnostics::record(
                "rpc.gtk.start",
                json!({
                    "trace_id": trace_id, "queue_wait_us": queued_at.elapsed().as_micros(),
                }),
            );
            handle_socket_command_traced(*command, state, Some(trace_id.to_string()));
            crate::diagnostics::record(
                "rpc.gtk.dispatched",
                json!({
                    "trace_id": trace_id, "duration_us": started.elapsed().as_micros(),
                }),
            );
        }

        // -- system.* --
        SocketCommand::Ping { req_id, resp_tx } => {
            let _ = resp_tx.send(ok(req_id, json!({"pong": true})));
        }

        SocketCommand::Identify { req_id, resp_tx } => {
            let socket_path = crate::socket::socket_path().to_string_lossy().to_string();
            let _ = resp_tx.send(ok(
                req_id,
                json!({
                    "version": env!("CARGO_PKG_VERSION"),
                    "platform": "linux",
                    "socket_path": socket_path,
                }),
            ));
        }

        SocketCommand::Capabilities { req_id, resp_tx } => {
            let methods: Vec<&str> = vec![
                "system.ping",
                "system.identify",
                "system.capabilities",
                "system.diagnostics",
                "workspace.list",
                "workspace.current",
                "workspace.create",
                "workspace.select",
                "workspace.close",
                "workspace.rename",
                "workspace.next",
                "workspace.previous",
                "workspace.last",
                "workspace.reorder",
                "surface.list",
                "surface.split",
                "surface.focus",
                "surface.close",
                "surface.send_text",
                "surface.send_key",
                "surface.read_text",
                "surface.resume.set",
                "surface.resume.show",
                "surface.resume.clear",
                "surface.health",
                "surface.refresh",
                "pane.list",
                "pane.focus",
                "pane.last",
                "window.list",
                "window.current",
                "notification.list",
                "notification.clear",
                // Browser lifecycle + streaming
                "browser.open",
                "browser.close",
                "browser.list",
                "browser.stream.enable",
                "browser.stream.disable",
                "browser.snapshot",
                "browser.screenshot",
                // P0: navigation
                "browser.navigate",
                "browser.goto",
                "browser.back",
                "browser.forward",
                "browser.reload",
                // P0: interaction
                "browser.click",
                "browser.dblclick",
                "browser.type",
                "browser.fill",
                "browser.press",
                "browser.keydown",
                "browser.keyup",
                "browser.hover",
                "browser.focus",
                "browser.check",
                "browser.uncheck",
                "browser.select",
                "browser.scroll",
                "browser.scroll_into_view",
                "browser.drag",
                "browser.upload",
                "browser.download",
                "browser.pdf",
                // P0: evaluation + waiting
                "browser.eval",
                "browser.wait",
                // P0: getters
                "browser.get.url",
                "browser.get.title",
                "browser.get.text",
                "browser.get.html",
                "browser.get.value",
                "browser.get.attr",
                "browser.get.count",
                "browser.get.box",
                "browser.get.styles",
                // P0: state checks
                "browser.is.visible",
                "browser.is.enabled",
                "browser.is.checked",
                // P1: locators
                "browser.find.role",
                "browser.find.text",
                "browser.find.label",
                "browser.find.placeholder",
                "browser.find.alt",
                "browser.find.title",
                "browser.find.testid",
                "browser.find.nth",
                "browser.find.first",
                "browser.find.last",
                // P1: frames, dialogs, console, errors
                "browser.frame.select",
                "browser.frame.main",
                "browser.dialog.accept",
                "browser.dialog.dismiss",
                "browser.console.list",
                "browser.errors.list",
                "browser.highlight",
                "browser.state.save",
                "browser.state.load",
                // Debug
                "debug.layout",
                "debug.type",
            ];
            let _ = resp_tx.send(ok(req_id, json!({"methods": methods})));
        }

        // -- workspace.* --
        SocketCommand::WorkspaceList { req_id, resp_tx } => {
            // SOCK-05: No focus side effects.
            let s = state.borrow();
            let list: Vec<Value> = (0..s.workspaces.len())
                .filter_map(|index| workspace_record(&s, index))
                .collect();
            let _ = resp_tx.send(ok(req_id, json!({"workspaces": list})));
        }

        SocketCommand::WorkspaceCurrent { req_id, resp_tx } => {
            // SOCK-05: No focus side effects.
            let s = state.borrow();
            let response = match workspace_record(&s, s.active_index) {
                Some(workspace) => ok(req_id, workspace),
                None => err(req_id, "no_workspace", "no active workspace"),
            };
            let _ = resp_tx.send(response);
        }

        SocketCommand::WorkspaceCreate {
            req_id,
            remote_target,
            name,
            working_directory,
            resp_tx,
        } => {
            if let Some(target) = remote_target {
                // SSH workspace creation per D-13, D-15
                // Create per-workspace bridge for SSH I/O routing
                let bridge = std::sync::Arc::new(crate::ssh::bridge::SshBridge::new());
                let id = state
                    .borrow_mut()
                    .create_remote_workspace(target.clone(), &bridge);
                let uuid_str = {
                    let s = state.borrow();
                    s.workspaces
                        .iter()
                        .find(|ws| ws.id == id)
                        .map(|ws| ws.uuid.to_string())
                        .unwrap_or_default()
                };
                state.borrow_mut().start_ssh(
                    id,
                    target,
                    bridge,
                    trace_id
                        .as_deref()
                        .and_then(|value| uuid::Uuid::parse_str(value).ok()),
                    "rpc",
                );
                let _ = resp_tx.send(ok(req_id, json!({"uuid": uuid_str, "remote": true})));
            } else {
                let id = if let Some(path) = working_directory {
                    state
                        .borrow_mut()
                        .create_workspace_bound(name.unwrap_or_default(), path)
                } else {
                    let id = state.borrow_mut().create_workspace();
                    if let Some(name) = name.filter(|value| !value.trim().is_empty()) {
                        state.borrow_mut().rename_active(name);
                    }
                    id
                };
                let s = state.borrow();
                let workspace = s.workspaces.iter().find(|ws| ws.id == id);
                let uuid_str = workspace.map(|ws| ws.uuid.to_string()).unwrap_or_default();
                let directory = workspace
                    .and_then(|ws| ws.working_directory.as_ref())
                    .map(|path| path.to_string_lossy());
                let _ = resp_tx.send(ok(
                    req_id,
                    json!({
                        "uuid": uuid_str,
                        "working_directory": directory,
                    }),
                ));
            }
            let (list, app) = {
                let s = state.borrow();
                (s.sidebar_list.clone(), s.gtk_app.clone())
            };
            crate::sidebar::wire_latest_row(&list, state.clone(), &app);
        }

        SocketCommand::WorkspaceSelect {
            req_id,
            id,
            resp_tx,
        } => {
            // SOCK-05: workspace.select IS a focus-intent command.
            let idx = {
                let s = state.borrow();
                s.workspaces.iter().position(|ws| ws.uuid.to_string() == id)
            };
            match idx {
                Some(i) => {
                    state.borrow_mut().switch_to_index(i);
                    let _ = resp_tx.send(ok(req_id, json!({})));
                }
                None => {
                    let _ = resp_tx.send(err(req_id, "not_found", "workspace not found"));
                }
            }
        }

        SocketCommand::WorkspaceClose {
            req_id,
            id,
            resp_tx,
        } => {
            // SOCK-05: No focus side effects (close_workspace adjusts index internally).
            let idx = {
                let s = state.borrow();
                s.workspaces.iter().position(|ws| ws.uuid.to_string() == id)
            };
            match idx {
                Some(i) => {
                    let closed = state.borrow_mut().close_workspace(i);
                    if closed {
                        let _ = resp_tx.send(ok(req_id, json!({})));
                    } else {
                        let _ = resp_tx.send(err(
                            req_id,
                            "last_workspace",
                            "cannot close the last workspace",
                        ));
                    }
                }
                None => {
                    let _ = resp_tx.send(err(req_id, "not_found", "workspace not found"));
                }
            }
        }

        SocketCommand::WorkspaceRename {
            req_id,
            id,
            name,
            resp_tx,
        } => {
            // SOCK-05: No focus side effects. Find workspace by uuid, switch to it
            // (rename_active requires the target to be active), then rename.
            let idx = {
                let s = state.borrow();
                s.workspaces.iter().position(|ws| ws.uuid.to_string() == id)
            };
            match idx {
                Some(i) => {
                    let mut s = state.borrow_mut();
                    s.rename_workspace_at(i, name);
                    drop(s);
                    let _ = resp_tx.send(ok(req_id, json!({})));
                }
                None => {
                    let _ = resp_tx.send(err(req_id, "not_found", "workspace not found"));
                }
            }
        }

        SocketCommand::WorkspaceNext { req_id, resp_tx } => {
            // SOCK-05: focus-intent command.
            state.borrow_mut().switch_next();
            let _ = resp_tx.send(ok(req_id, json!({})));
        }

        SocketCommand::WorkspacePrev { req_id, resp_tx } => {
            // SOCK-05: focus-intent command.
            state.borrow_mut().switch_prev();
            let _ = resp_tx.send(ok(req_id, json!({})));
        }

        SocketCommand::WorkspaceLast { req_id, resp_tx } => {
            // SOCK-05: focus-intent command.
            // "Last" = most recently visited; for now same as prev (Phase 4 can track history).
            state.borrow_mut().switch_prev();
            let _ = resp_tx.send(ok(req_id, json!({})));
        }

        SocketCommand::WorkspaceReorder {
            req_id,
            id,
            position,
            resp_tx,
        } => {
            // SOCK-05: No focus side effects.
            let mut s = state.borrow_mut();
            let idx = s.workspaces.iter().position(|ws| ws.uuid.to_string() == id);
            match idx {
                Some(from) => {
                    let to = position.min(s.workspaces.len().saturating_sub(1));
                    s.reorder_workspace(from, to);
                    drop(s);
                    let _ = resp_tx.send(ok(req_id, json!({})));
                }
                None => {
                    drop(s);
                    let _ = resp_tx.send(err(req_id, "not_found", "workspace not found"));
                }
            }
        }

        // -- window.* --
        SocketCommand::WindowList { req_id, resp_tx } => {
            // SOCK-05: No focus side effects.
            let workspace_count = state.borrow().workspaces.len();
            let _ = resp_tx.send(ok(
                req_id,
                json!({
                    "windows": [{"id": "main", "workspaces": workspace_count}]
                }),
            ));
        }

        SocketCommand::WindowCurrent { req_id, resp_tx } => {
            // SOCK-05: No focus side effects.
            let _ = resp_tx.send(ok(req_id, json!({"id": "main"})));
        }

        // -- debug.* --
        SocketCommand::DebugLayout { req_id, resp_tx } => {
            // SOCK-05: No focus side effects.
            let s = state.borrow();
            match s.split_engines.get(s.active_index) {
                Some(engine) => {
                    let data = engine.root.to_data();
                    let json_tree = serde_json::to_value(&data).unwrap_or(Value::Null);
                    let _ = resp_tx.send(ok(req_id, json!({"layout": json_tree})));
                }
                None => {
                    let _ = resp_tx.send(err(req_id, "no_workspace", "no active workspace"));
                }
            }
        }

        SocketCommand::DebugType {
            req_id,
            text,
            resp_tx,
        } => {
            // SOCK-05: No focus side effects (sends text to active surface without changing focus).
            let s = state.borrow();
            if let Some(engine) = s.split_engines.get(s.active_index) {
                if let Some(pane_id) = engine.root.find_active_pane_id() {
                    if let Some(surface) = engine.root.find_surface_for_pane(pane_id) {
                        if !surface.is_null() {
                            let c_text = std::ffi::CString::new(text.clone()).unwrap_or_default();
                            unsafe {
                                crate::ghostty::ffi::ghostty_surface_text(
                                    surface,
                                    c_text.as_ptr(),
                                    c_text.to_bytes().len(),
                                );
                            }
                        }
                    }
                }
            }
            let _ = resp_tx.send(ok(req_id, json!({})));
        }

        // ── surface.* ────────────────────────────────────────────────────
        SocketCommand::SurfaceResume {
            req_id,
            id,
            action,
            resp_tx,
        } => {
            let s = state.borrow();
            let id = id.or_else(|| {
                s.split_engines
                    .get(s.active_index)
                    .and_then(|engine| engine.active_pane_uuid())
            });
            let target = id.as_ref().and_then(|id| {
                s.split_engines
                    .iter()
                    .enumerate()
                    .find(|(_, engine)| engine.find_pane_id_by_uuid(id).is_some())
            });
            let response = match (id.as_ref(), target) {
                (Some(id), Some((index, engine))) => match engine.resume_action(id, &action) {
                    Ok(binding) => {
                        if !matches!(action, crate::resume::ResumeAction::Show) {
                            s.trigger_session_save();
                        }
                        ok(
                            req_id,
                            json!({"surface_id": id, "workspace_id": s.workspaces[index].uuid,
                            "resume_binding": binding, "auto_resume": false}),
                        )
                    }
                    Err(message) => err(
                        req_id,
                        if message == "checkpoint mismatch" {
                            "conflict"
                        } else {
                            "invalid_params"
                        },
                        message,
                    ),
                },
                _ => err(req_id, "not_found", "terminal surface not found"),
            };
            let _ = resp_tx.send(response);
        }
        SocketCommand::SurfaceList { req_id, resp_tx } => {
            // SOCK-05: No focus side effects.
            let s = state.borrow();
            let mut panes: Vec<Value> = Vec::new();
            for (ws_idx, (ws, engine)) in
                s.workspaces.iter().zip(s.split_engines.iter()).enumerate()
            {
                for (pane_uuid, _pane_id, active) in engine.all_panes() {
                    panes.push(json!({
                        "uuid": pane_uuid.to_string(),
                        "workspace_uuid": ws.uuid.to_string(),
                        "active": active && ws_idx == s.active_index,
                    }));
                }
            }
            let _ = resp_tx.send(ok(req_id, json!({"surfaces": panes})));
        }

        SocketCommand::SurfaceSplit {
            req_id,
            id,
            direction,
            resp_tx,
        } => {
            // Split the requested surface in the active workspace, or its active pane.
            // Selecting/splitting is focus intent; invalid targets must not create a pane.
            let orientation = match direction {
                super::commands::SplitDirection::Vertical => gtk4::Orientation::Vertical,
                super::commands::SplitDirection::Horizontal => gtk4::Orientation::Horizontal,
            };
            let result = {
                let mut s = state.borrow_mut();
                let idx = s.active_index;
                if let Some(engine) = s.split_engines.get_mut(idx) {
                    if id
                        .as_deref()
                        .is_some_and(|target| !engine.focus_surface(target))
                    {
                        let _ = resp_tx.send(err(req_id, "not_found", "surface not found"));
                        return;
                    }
                    engine.split_active(orientation).and_then(|new_pane_id| {
                        // Find the uuid of the newly created pane.
                        engine
                            .all_panes()
                            .into_iter()
                            .find(|(_, pid, _)| *pid == new_pane_id)
                            .map(|(uuid, _, _)| uuid.to_string())
                    })
                } else {
                    None
                }
            };
            match result {
                Some(uuid_str) => {
                    let _ = resp_tx.send(ok(req_id, json!({"uuid": uuid_str})));
                }
                None => {
                    let _ = resp_tx.send(err(req_id, "split_failed", "could not split pane"));
                }
            }
        }

        SocketCommand::SurfaceFocus {
            req_id,
            id,
            resp_tx,
        } => {
            // SOCK-05: surface.focus IS a focus-intent command — allowed to change focus.
            let focused = {
                let mut s = state.borrow_mut();
                let idx = s.active_index;
                s.split_engines
                    .get_mut(idx)
                    .is_some_and(|engine| engine.focus_surface(&id))
            };
            let response = if focused {
                ok(req_id, json!({}))
            } else {
                err(req_id, "not_found", "surface not found")
            };
            let _ = resp_tx.send(response);
        }

        SocketCommand::SurfaceClose {
            req_id,
            id,
            resp_tx,
        } => {
            let Some(uuid) = uuid::Uuid::parse_str(&id).ok() else {
                let _ = resp_tx.send(err(req_id, "invalid_request", "invalid surface UUID"));
                return;
            };
            let closed = {
                let mut s = state.borrow_mut();
                let idx = s.active_index;
                s.split_engines
                    .get_mut(idx)
                    .map(|engine| engine.close_surface_and_empty_pane(uuid))
            };
            match closed {
                Some(crate::split_engine::CloseSurfaceResult::Closed) => {
                    state.borrow().trigger_session_save();
                    let _ = resp_tx.send(ok(req_id, json!({})));
                }
                Some(crate::split_engine::CloseSurfaceResult::LastSurfaceInPane) => {
                    let _ = resp_tx.send(err(req_id, "close_failed", "cannot close last surface"));
                }
                Some(crate::split_engine::CloseSurfaceResult::NotFound) | None => {
                    let _ = resp_tx.send(err(req_id, "not_found", "surface not found"));
                }
            }
        }

        SocketCommand::SurfaceSendText {
            req_id,
            id,
            text,
            resp_tx,
        } => {
            let response = match send_terminal_text(state, id.as_deref(), &text) {
                Ok(()) => ok(req_id, json!({})),
                Err((code, message)) => err(req_id, code, message),
            };
            let _ = resp_tx.send(response);
        }

        SocketCommand::SurfaceSendKey {
            req_id,
            id,
            key,
            resp_tx,
        } => {
            // Literal characters use typed input. Named key combinations require
            // native key translation; never report them as successfully delivered.
            let mut characters = key.chars();
            let result = if let (Some(character), None) = (characters.next(), characters.next()) {
                terminal_target(state, id.as_deref()).and_then(|surface| {
                    // SAFETY: resolution releases the model borrow and returns a
                    // live GTK-owned terminal; no teardown occurs before input.
                    unsafe { crate::ghostty::text::send_character(surface, character) }
                        .map_err(|message| ("invalid_params", message))
                })
            } else {
                Err((
                    "not_supported",
                    "send-key currently accepts one literal character",
                ))
            };
            let response = match result {
                Ok(()) => ok(req_id, json!({})),
                Err((code, message)) => err(req_id, code, message),
            };
            let _ = resp_tx.send(response);
        }

        SocketCommand::SurfaceReadText {
            req_id,
            id,
            resp_tx,
        } => {
            let result = terminal_target(state, id.as_deref()).and_then(|surface| {
                // SAFETY: resolution found a live GTK-owned terminal. No model
                // borrow or event-loop iteration spans this bounded native read.
                unsafe { crate::ghostty::text::read_visible(surface) }
                    .map_err(|message| ("read_failed", message))
            });
            let response = match result {
                Ok(text) => ok(req_id, json!({"text": text})),
                Err((code, message)) => err(req_id, code, message),
            };
            let _ = resp_tx.send(response);
        }

        SocketCommand::SurfaceHealth {
            req_id,
            id,
            resp_tx,
        } => {
            // SOCK-05: health is NOT focus-intent — NO focus change.
            let (found, has_attention) = {
                let s = state.borrow();
                if let Some(engine) = s.split_engines.get(s.active_index) {
                    if let Some(ref uuid_str) = id {
                        let alive = engine.find_surface_by_uuid(uuid_str).is_some();
                        let attn = engine
                            .find_pane_id_by_uuid(uuid_str)
                            .map(|pid| engine.root.pane_has_attention(pid))
                            .unwrap_or(false);
                        (alive, attn)
                    } else {
                        let alive = engine
                            .root
                            .find_surface_for_pane(engine.active_pane_id)
                            .is_some();
                        let attn = engine.root.pane_has_attention(engine.active_pane_id);
                        (alive, attn)
                    }
                } else {
                    (false, false)
                }
            };
            let _ = resp_tx.send(ok(
                req_id,
                json!({"alive": found, "has_attention": has_attention}),
            ));
        }

        SocketCommand::SurfaceRefresh {
            req_id,
            id,
            resp_tx,
        } => {
            // SOCK-05: refresh is NOT focus-intent — NO focus change.
            // Queue a render on the target surface's GLArea.
            let gl_area = {
                let s = state.borrow();
                if let Some(engine) = s.split_engines.get(s.active_index) {
                    match id.as_deref() {
                        Some(uuid) => engine.gl_area_for_surface(uuid),
                        None => engine.gl_area_for_pane(engine.active_pane_id),
                    }
                } else {
                    None
                }
            };
            let response = if let Some(area) = gl_area {
                area.queue_render();
                ok(req_id, json!({}))
            } else {
                err(req_id, "not_found", "terminal surface not found")
            };
            let _ = resp_tx.send(response);
        }

        // ── pane.* ───────────────────────────────────────────────────────────
        SocketCommand::PaneList { req_id, resp_tx } => {
            let s = state.borrow();
            let mut panes = Vec::new();
            for (ws_idx, (ws, engine)) in s.workspaces.iter().zip(&s.split_engines).enumerate() {
                for pane in engine.pane_info() {
                    panes.push(json!({
                        "id": format!("pane:{}", pane.id),
                        "uuid": pane.selected_surface,
                        "workspace_uuid": ws.uuid,
                        "surface_ids": pane.surface_ids,
                        "active_surface_uuid": pane.selected_surface,
                        "focused": ws_idx == s.active_index && pane.id == engine.active_pane_id,
                        "active": ws_idx == s.active_index && pane.id == engine.active_pane_id,
                    }));
                }
            }
            let _ = resp_tx.send(ok(req_id, json!({"panes": panes})));
        }

        SocketCommand::PaneFocus {
            req_id,
            id,
            resp_tx,
        } => {
            let focused = {
                let mut s = state.borrow_mut();
                let idx = s.active_index;
                s.split_engines.get_mut(idx).is_some_and(|engine| {
                    id.as_deref()
                        .is_some_and(|reference| engine.focus_pane_ref(reference))
                })
            };
            let response = if focused {
                ok(req_id, json!({}))
            } else {
                err(req_id, "not_found", "pane not found")
            };
            let _ = resp_tx.send(response);
        }

        SocketCommand::PaneLast { req_id, resp_tx } => {
            // SOCK-05: pane.last IS focus-intent — allowed to change focus.
            // Phase 3 stub: re-grab focus on current active pane. Phase 4 tracks focus history.
            {
                let s = state.borrow();
                if let Some(engine) = s.split_engines.get(s.active_index) {
                    engine.grab_active_focus();
                }
            }
            let _ = resp_tx.send(ok(req_id, json!({})));
        }

        // -- notification.* (Phase 4) --
        SocketCommand::NotificationList { req_id, resp_tx } => {
            // SOCK-05: No focus side effects. Read-only attention state query.
            let s = state.borrow();
            let notifications: Vec<Value> = s
                .workspaces
                .iter()
                .map(|ws| {
                    json!({
                        "workspace_uuid": ws.uuid.to_string(),
                        "workspace_name": ws.name,
                        "has_attention": ws.has_attention,
                    })
                })
                .collect();
            let _ = resp_tx.send(ok(req_id, json!({"notifications": notifications})));
        }

        SocketCommand::NotificationClear {
            req_id,
            id,
            resp_tx,
        } => {
            // SOCK-05: No focus side effects. Clears attention without switching workspace.
            let idx = {
                let s = state.borrow();
                s.workspaces.iter().position(|ws| ws.uuid.to_string() == id)
            };
            match idx {
                Some(i) => {
                    state.borrow_mut().clear_workspace_attention(i);
                    let _ = resp_tx.send(ok(req_id, json!({})));
                }
                None => {
                    let _ = resp_tx.send(err(req_id, "not_found", "workspace not found"));
                }
            }
        }

        // -- browser.* (Phase 8: D-04 lifecycle + streaming) --
        // SOCK-05: None of these commands steal focus.
        SocketCommand::BrowserOpen {
            req_id,
            url,
            workspace,
            resp_tx,
        } => {
            let mut params = json!({"url": url});
            if let Some(workspace) = workspace {
                params["workspace"] = json!(workspace);
            }
            start_browser_lifecycle(
                state,
                crate::browser::StartupRequest::Open(params),
                req_id,
                resp_tx,
                trace_id,
            );
        }

        SocketCommand::BrowserStreamEnable { req_id, resp_tx } => {
            start_browser_lifecycle(
                state,
                crate::browser::StartupRequest::Stream,
                req_id,
                resp_tx,
                trace_id,
            );
        }

        SocketCommand::BrowserStreamDisable { req_id, resp_tx } => {
            let s = state.borrow();
            let Some(browser) = s.browser_manager.as_ref() else {
                let _ = resp_tx.send(err(req_id, "not_running", "No browser session active"));
                return;
            };
            let Some(runtime) = s.runtime_handle.clone() else {
                let _ = resp_tx.send(err(req_id, "not_running", "Async runtime unavailable"));
                return;
            };
            let exchange = browser.send_command_async(
                "stream_disable",
                json!({}),
                trace_id
                    .as_deref()
                    .and_then(|id| uuid::Uuid::parse_str(id).ok()),
            );
            drop(s);
            spawn_browser_exchange(
                &runtime,
                exchange,
                req_id,
                resp_tx,
                "stream_error",
                trace_id,
            );
        }

        SocketCommand::BrowserList { req_id, resp_tx } => {
            let s = state.borrow();
            let surfaces: Vec<serde_json::Value> = s
                .browser_surface_refs
                .iter()
                .map(|(ref_id, uuid)| {
                    serde_json::json!({
                        "ref": format!("surface:{}", ref_id),
                        "uuid": uuid,
                        "status": "registered",
                    })
                })
                .collect();
            let _ = resp_tx.send(ok(req_id, serde_json::json!({"surfaces": surfaces})));
        }

        // -- browser.* generic proxy (P0/P1 parity) --
        SocketCommand::BrowserAction {
            req_id,
            action,
            mut params,
            surface_ref,
            resp_tx,
        } => {
            let s = state.borrow();
            if let Some(ref bm) = s.browser_manager {
                // Resolve surface ref if provided
                if let Some(ref sref) = surface_ref {
                    match resolve_surface_ref(sref, &s.browser_surface_refs) {
                        Ok(uuid) => {
                            if let Some(obj) = params.as_object_mut() {
                                obj.remove("surface_ref");
                                obj.insert("surface_id".to_string(), serde_json::json!(uuid));
                            }
                        }
                        Err((msg, available)) => {
                            let mut response = err(req_id, "surface_not_found", &msg);
                            response["available"] = json!(available);
                            let _ = resp_tx.send(response);
                            return;
                        }
                    }
                }
                // Translate cmux CLI action names to agent-browser action names
                let daemon_action = match action.as_str() {
                    "open" => "launch",
                    "goto" => "navigate",
                    "eval" => "evaluate",
                    "gethtml" => "innerhtml",
                    "stream.enable" => "stream_enable",
                    "stream.disable" => "stream_disable",
                    _ => &action,
                };
                let Some(runtime) = s.runtime_handle.clone() else {
                    let _ = resp_tx.send(err(req_id, "not_running", "Async runtime unavailable"));
                    return;
                };
                let exchange = bm.send_command_async(
                    daemon_action,
                    params,
                    trace_id
                        .as_deref()
                        .and_then(|id| uuid::Uuid::parse_str(id).ok()),
                );
                drop(s);
                spawn_browser_exchange(
                    &runtime,
                    exchange,
                    req_id,
                    resp_tx,
                    "browser_error",
                    trace_id,
                );
            } else {
                let _ = resp_tx.send(err(req_id, "not_running", "No browser session active"));
            }
        }
    }
}

/// Initialize and command the daemon on Tokio, then apply surviving results on GTK without stealing focus.
fn start_browser_lifecycle(
    state: &std::rc::Rc<std::cell::RefCell<crate::app_state::AppState>>,
    request: crate::browser::StartupRequest,
    req_id: Value,
    mut resp_tx: super::commands::RespTx,
    trace_id: Option<String>,
) {
    let is_open = matches!(request, crate::browser::StartupRequest::Open(_));
    let trace = trace_id
        .as_deref()
        .and_then(|id| uuid::Uuid::parse_str(id).ok())
        .unwrap_or_else(uuid::Uuid::new_v4);
    let (session, workspace, mut task) = {
        let mut s = state.borrow_mut();
        let Some(runtime) = s.runtime_handle.clone() else {
            let _ = resp_tx.send(err(req_id, "not_running", "Async runtime unavailable"));
            return;
        };
        let workspace = s
            .workspaces
            .get(s.active_index)
            .map(|workspace| workspace.uuid);
        let browser = s
            .browser_manager
            .get_or_insert_with(crate::browser::BrowserManager::new);
        (
            browser.session_identity(),
            workspace,
            runtime.spawn(browser.startup_async(request, trace)),
        )
    };
    let guard = crate::task::AbortOnDrop(task.abort_handle());
    let state = std::rc::Rc::downgrade(state);
    glib::MainContext::default().spawn_local(async move {
        let _guard = guard;
        let mut activity = crate::browser::metrics::Activity::begin("rpc_startup", Some(trace));
        let completed = tokio::select! {
            biased;
            _ = resp_tx.closed() => { task.abort(); let _ = task.await; return; }
            result = &mut task => result,
        };
        let (binary, mut result) = match completed {
            Ok(Ok(result)) => result,
            Ok(Err(error)) => {
                activity.finish("error");
                let _ = resp_tx.send(err(req_id, error.code, &error.message));
                return;
            }
            Err(_) => {
                activity.finish("task_error");
                let _ = resp_tx.send(err(req_id, "daemon_error", "Browser startup worker failed"));
                return;
            }
        };
        let Some(state) = state.upgrade() else {
            return;
        };
        let (new_widgets, picture, runtime) = {
            let mut s = state.borrow_mut();
            if !s
                .browser_manager
                .as_mut()
                .is_some_and(|browser| browser.install_startup(&session, binary))
            {
                activity.finish("stale_manager");
                let _ = resp_tx.send(err(req_id, "not_running", "Browser session was replaced"));
                return;
            }
            let index = workspace.and_then(|id| {
                s.workspaces
                    .iter()
                    .position(|workspace| workspace.uuid == id)
            });
            let Some(index) = index else {
                activity.finish("missing_workspace");
                let _ = resp_tx.send(err(
                    req_id,
                    "not_found",
                    "Target workspace closed during browser startup",
                ));
                return;
            };
            if is_open {
                s.browser_surface_counter += 1;
                let ref_id = s.browser_surface_counter;
                let uuid = result
                    .get("id")
                    .or_else(|| result.get("surface_id"))
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_owned();
                s.browser_surface_refs.insert(ref_id, uuid.clone());
                if let Some(fields) = result.as_object_mut() {
                    fields.insert("surface_ref".into(), json!(format!("surface:{ref_id}")));
                    fields.insert("uuid".into(), json!(uuid));
                }
            }
            let mut widgets = None;
            let picture = s.split_engines.get_mut(index).and_then(|engine| {
                find_preview_picture(&engine.root).or_else(|| {
                    widgets = engine.add_preview(false);
                    widgets.as_ref().map(|widgets| widgets.picture.clone())
                })
            });
            (widgets, picture, s.runtime_handle.clone())
        };
        if let Some(widgets) = new_widgets {
            crate::browser::ui::wire_browser_tab(&state, widgets, activity.id);
        } else if let (Some(picture), Some(runtime)) = (picture, runtime) {
            if let Some(browser) = state.borrow_mut().browser_manager.as_mut() {
                browser.start_stream(&runtime, picture, Some(activity.id));
            }
        }
        activity.finish("success");
        let _ = resp_tx.send(ok(req_id, result));
    });
}

/// Deliver a browser exchange off GTK, preserving endpoint errors and cancelling when its caller leaves.
fn spawn_browser_exchange(
    runtime: &tokio::runtime::Handle,
    exchange: impl std::future::Future<Output = Result<Value, String>> + Send + 'static,
    req_id: Value,
    mut resp_tx: super::commands::RespTx,
    error_code: &'static str,
    trace_id: Option<String>,
) {
    runtime.spawn(async move {
        let started = std::time::Instant::now();
        let outcome = tokio::select! {
            biased;
            _ = resp_tx.closed() => "cancelled",
            result = exchange => {
                match result {
                    Ok(result) => { let _ = resp_tx.send(ok(req_id, result)); "success" }
                    Err(error) => { let _ = resp_tx.send(err(req_id, error_code, &error)); "error" }
                }
            }
        };
        crate::diagnostics::record(
            "browser.rpc.complete",
            json!({
                "trace_id": trace_id, "outcome": outcome,
                "duration_us": started.elapsed().as_micros(),
            }),
        );
    });
}

/// Walk the split tree to find the first Preview node's Picture widget.
fn find_preview_picture(node: &crate::split_engine::SplitNode) -> Option<gtk4::Picture> {
    crate::split_engine::first_browser_picture(node)
}

#[cfg(test)]
mod browser_exchange_tests {
    use super::*;

    /// Shared browser delivery preserves identities, successful data and endpoint-specific failures.
    #[tokio::test]
    async fn responses_preserve_endpoint_contract() {
        for result in [
            Ok(json!({"streaming":false})),
            Err("unavailable".to_string()),
        ] {
            let success = result.is_ok();
            let (tx, rx) = tokio::sync::oneshot::channel();
            spawn_browser_exchange(
                &tokio::runtime::Handle::current(),
                async move { result },
                json!(7),
                tx,
                "stream_error",
                None,
            );
            let response = tokio::time::timeout(std::time::Duration::from_secs(1), rx)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(response["id"], 7);
            assert_eq!(response["ok"], success);
            if success {
                assert_eq!(response["result"]["streaming"], false);
            } else {
                assert_eq!(response["error"]["code"], "stream_error");
            }
        }
    }

    /// A caller that stops awaiting its response releases the exchange's owned resources.
    #[tokio::test]
    async fn abandoned_response_cancels_exchange() {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let (resource, dropped) = tokio::sync::oneshot::channel::<()>();
        let exchange = async move {
            let _resource = resource;
            std::future::pending::<Result<Value, String>>().await
        };
        spawn_browser_exchange(
            &tokio::runtime::Handle::current(),
            exchange,
            json!(8),
            tx,
            "browser_error",
            None,
        );
        drop(rx);
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(1), dropped)
                .await
                .unwrap()
                .is_err()
        );
    }
}
