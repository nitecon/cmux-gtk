//! Worker-side request validation, typed command construction and response correlation.
//! Owns no GTK state; admitted commands cross the bounded bridge for UI execution.
use super::{
    commands,
    response::{err, ok},
};

/// Decode a nullable optional target without silently turning malformed IDs into active-pane fallback.
fn optional_target(params: &serde_json::Value) -> Result<Option<String>, &'static str> {
    match params.get("id") {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(id)) => Ok(Some(id.clone())),
        _ => Err("id must be a string or null"),
    }
}

/// Parse a JSON-RPC line and dispatch to the appropriate SocketCommand.
/// Consumes raw input and releases unused JSON fields before awaiting execution.
/// Returns the JSON response string (without trailing newline).
pub(super) async fn dispatch_line(
    line: String,
    cmd_tx: &tokio::sync::mpsc::Sender<commands::SocketCommand>,
) -> String {
    let mut req: serde_json::Value = match serde_json::from_str(&line) {
        Ok(v) => v,
        Err(_) => {
            return err(serde_json::Value::Null, "parse_error", "invalid JSON").to_string();
        }
    };

    drop(line);
    if !req.is_object() {
        return err(
            serde_json::Value::Null,
            "invalid_request",
            "request must be an object",
        )
        .to_string();
    }
    let req_id = req
        .get_mut("id")
        .map(serde_json::Value::take)
        .unwrap_or(serde_json::Value::Null);
    let method = match req.get_mut("method").map(serde_json::Value::take) {
        Some(serde_json::Value::String(method)) => method,
        _ => return err(req_id, "invalid_request", "method must be a string").to_string(),
    };
    let mut params = req
        .get_mut("params")
        .map(serde_json::Value::take)
        .unwrap_or(serde_json::Value::Object(Default::default()));

    let mut operation = crate::diagnostics::Operation::begin(
        &method,
        req.get("trace_id").and_then(|id| id.as_str()),
    );
    drop(req);
    // Early validation failures below are errors; cancellation while awaiting
    // execution is reset explicitly before yielding to the response channel.
    operation.finish(false);

    if params.is_null() {
        params = serde_json::json!({});
    } else if !params.is_object() {
        return err(req_id, "invalid_params", "params must be an object or null").to_string();
    }

    if method == "system.diagnostics" {
        drop(params);
        operation.pending();
        return match tokio::task::spawn_blocking(crate::diagnostics::snapshot).await {
            Ok(snapshot) => {
                operation.finish(true);
                ok(req_id, snapshot).to_string()
            }
            Err(_) => {
                operation.finish(false);
                err(req_id, "internal_error", "diagnostic sampling failed").to_string()
            }
        };
    }

    let target = if matches!(
        method.as_str(),
        "surface.split"
            | "surface.send_text"
            | "surface.send_key"
            | "surface.read_text"
            | "surface.health"
            | "surface.refresh"
            | "pane.focus"
    ) {
        match optional_target(&params) {
            Ok(target) => target,
            Err(message) => return err(req_id, "invalid_params", message).to_string(),
        }
    } else {
        None
    };

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();

    let cmd = match method.as_str() {
        "system.ping" => commands::SocketCommand::Ping {
            req_id: req_id.clone(),
            resp_tx,
        },
        "system.identify" => commands::SocketCommand::Identify {
            req_id: req_id.clone(),
            resp_tx,
        },
        "system.capabilities" => commands::SocketCommand::Capabilities {
            req_id: req_id.clone(),
            resp_tx,
        },

        "workspace.list" => commands::SocketCommand::WorkspaceList {
            req_id: req_id.clone(),
            resp_tx,
        },
        "workspace.current" => commands::SocketCommand::WorkspaceCurrent {
            req_id: req_id.clone(),
            resp_tx,
        },
        "workspace.create" => {
            let remote_target = params
                .get("remote_target")
                .and_then(|v| v.as_str())
                .map(String::from);
            let mut name = params
                .get("name")
                .and_then(|v| v.as_str())
                .map(String::from);
            let working_directory = params
                .get("working_directory")
                .or_else(|| params.get("cwd"))
                .and_then(|v| v.as_str());
            let working_directory = if remote_target.is_none() {
                match working_directory {
                    Some(path) => match crate::workspace::prepare_local_workspace(
                        name.as_deref().unwrap_or(""),
                        std::path::Path::new(path),
                    ) {
                        Ok((prepared_name, path)) => {
                            name = Some(prepared_name);
                            Some(path)
                        }
                        Err(message) => {
                            return err(req_id, "invalid_directory", &message).to_string();
                        }
                    },
                    None => None,
                }
            } else {
                None
            };
            commands::SocketCommand::WorkspaceCreate {
                req_id: req_id.clone(),
                remote_target,
                name,
                working_directory,
                resp_tx,
            }
        }
        "workspace.select" => commands::SocketCommand::WorkspaceSelect {
            req_id: req_id.clone(),
            id: params
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            resp_tx,
        },
        "workspace.close" => commands::SocketCommand::WorkspaceClose {
            req_id: req_id.clone(),
            id: params
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            resp_tx,
        },
        "workspace.rename" => commands::SocketCommand::WorkspaceRename {
            req_id: req_id.clone(),
            id: params
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            name: params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            resp_tx,
        },
        "workspace.next" => commands::SocketCommand::WorkspaceNext {
            req_id: req_id.clone(),
            resp_tx,
        },
        "workspace.previous" => commands::SocketCommand::WorkspacePrev {
            req_id: req_id.clone(),
            resp_tx,
        },
        "workspace.last" => commands::SocketCommand::WorkspaceLast {
            req_id: req_id.clone(),
            resp_tx,
        },
        "workspace.reorder" => {
            let Some(position) = params
                .get("position")
                .and_then(|value| value.as_u64())
                .and_then(|value| usize::try_from(value).ok())
            else {
                return err(
                    req_id,
                    "invalid_params",
                    "position must be a nonnegative integer within the native index range",
                )
                .to_string();
            };
            commands::SocketCommand::WorkspaceReorder {
                req_id: req_id.clone(),
                id: params
                    .get("id")
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .to_string(),
                position,
                resp_tx,
            }
        }

        "surface.list" => commands::SocketCommand::SurfaceList {
            req_id: req_id.clone(),
            resp_tx,
        },
        "surface.split" => {
            let direction = match params.get("direction") {
                None => commands::SplitDirection::Horizontal,
                Some(serde_json::Value::String(value)) if value == "horizontal" => {
                    commands::SplitDirection::Horizontal
                }
                Some(serde_json::Value::String(value)) if value == "vertical" => {
                    commands::SplitDirection::Vertical
                }
                _ => {
                    return err(
                        req_id,
                        "invalid_params",
                        "direction must be horizontal or vertical",
                    )
                    .to_string()
                }
            };
            commands::SocketCommand::SurfaceSplit {
                req_id: req_id.clone(),
                id: target,
                direction,
                resp_tx,
            }
        }
        "surface.focus" => commands::SocketCommand::SurfaceFocus {
            req_id: req_id.clone(),
            id: params
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            resp_tx,
        },
        "surface.close" => commands::SocketCommand::SurfaceClose {
            req_id: req_id.clone(),
            id: params
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            resp_tx,
        },
        "surface.send_text" => commands::SocketCommand::SurfaceSendText {
            req_id: req_id.clone(),
            id: target,
            text: params
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            resp_tx,
        },
        "surface.send_key" => commands::SocketCommand::SurfaceSendKey {
            req_id: req_id.clone(),
            id: target,
            key: params
                .get("key")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            resp_tx,
        },
        "surface.read_text" => commands::SocketCommand::SurfaceReadText {
            req_id: req_id.clone(),
            id: target,
            resp_tx,
        },
        "surface.health" => commands::SocketCommand::SurfaceHealth {
            req_id: req_id.clone(),
            id: target,
            resp_tx,
        },
        "surface.refresh" => commands::SocketCommand::SurfaceRefresh {
            req_id: req_id.clone(),
            id: target,
            resp_tx,
        },

        "pane.list" => commands::SocketCommand::PaneList {
            req_id: req_id.clone(),
            resp_tx,
        },
        "pane.focus" => commands::SocketCommand::PaneFocus {
            req_id: req_id.clone(),
            id: target,
            resp_tx,
        },
        "pane.last" => commands::SocketCommand::PaneLast {
            req_id: req_id.clone(),
            resp_tx,
        },

        "window.list" => commands::SocketCommand::WindowList {
            req_id: req_id.clone(),
            resp_tx,
        },
        "window.current" => commands::SocketCommand::WindowCurrent {
            req_id: req_id.clone(),
            resp_tx,
        },

        "debug.layout" => commands::SocketCommand::DebugLayout {
            req_id: req_id.clone(),
            resp_tx,
        },
        "debug.type" => commands::SocketCommand::DebugType {
            req_id: req_id.clone(),
            text: params
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            resp_tx,
        },

        "notification.list" => commands::SocketCommand::NotificationList {
            req_id: req_id.clone(),
            resp_tx,
        },
        "notification.clear" => commands::SocketCommand::NotificationClear {
            req_id: req_id.clone(),
            id: params
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            resp_tx,
        },

        "browser.open" => commands::SocketCommand::BrowserOpen {
            req_id: req_id.clone(),
            url: params
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            workspace: params
                .get("workspace")
                .and_then(|v| v.as_str())
                .map(String::from),
            resp_tx,
        },
        "browser.stream.enable" => commands::SocketCommand::BrowserStreamEnable {
            req_id: req_id.clone(),
            resp_tx,
        },
        "browser.stream.disable" => commands::SocketCommand::BrowserStreamDisable {
            req_id: req_id.clone(),
            resp_tx,
        },
        "browser.list" => commands::SocketCommand::BrowserList {
            req_id: req_id.clone(),
            resp_tx,
        },

        // Route all other browser.* methods to the generic proxy (P0/P1 parity)
        _ if method.starts_with("browser.") => {
            let action = method.strip_prefix("browser.").unwrap().to_string();
            let surface_ref = params
                .get("surface_ref")
                .and_then(|v| v.as_str())
                .map(String::from);
            commands::SocketCommand::BrowserAction {
                req_id: req_id.clone(),
                action,
                params: params.take(),
                surface_ref,
                resp_tx,
            }
        }

        _ => commands::SocketCommand::NotImplemented {
            req_id: req_id.clone(),
            method: method.clone(),
            resp_tx,
        },
    };

    drop(params);
    drop(method);
    let observed = commands::SocketCommand::Observed {
        command: Box::new(cmd),
        trace_id: operation.id,
        queued_at: std::time::Instant::now(),
    };
    if let Err(error) = cmd_tx.try_send(observed) {
        let (code, message) = match error {
            tokio::sync::mpsc::error::TrySendError::Full(_) => {
                ("overloaded", "GTK command queue is full")
            }
            tokio::sync::mpsc::error::TrySendError::Closed(_) => {
                ("internal_error", "handler channel closed")
            }
        };
        crate::diagnostics::record(
            "rpc.queue.rejected",
            serde_json::json!({
                "trace_id": operation.id, "code": code, "capacity": cmd_tx.max_capacity(),
            }),
        );
        return err(req_id, code, message).to_string();
    }

    operation.pending();
    let response = resp_rx
        .await
        .unwrap_or_else(|_| err(req_id, "internal_error", "handler dropped response"));
    operation.finish(
        response
            .get("ok")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
    );
    response.to_string()
}

#[cfg(test)]
#[path = "dispatch_tests.rs"]
mod tests;
