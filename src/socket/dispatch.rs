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
/// Returns encoded JSON and its validated operation identity for transport diagnostics.
pub(super) async fn dispatch_line(
    line: String,
    cmd_tx: &tokio::sync::mpsc::Sender<commands::SocketCommand>,
) -> DispatchedResponse {
    let mut operation = None;
    let response = dispatch_request(line, cmd_tx, &mut operation).await;
    let body = super::response::encode(response, operation.as_mut());
    DispatchedResponse {
        body,
        trace_id: operation.as_ref().map(|operation| operation.id),
    }
}

/// Carry correlation across encoding without retaining request contents or operation accounting.
pub(super) struct DispatchedResponse {
    pub body: String,
    pub trace_id: Option<uuid::Uuid>,
}

/// Validate and execute one request; the caller retains its operation through response encoding.
async fn dispatch_request(
    line: String,
    cmd_tx: &tokio::sync::mpsc::Sender<commands::SocketCommand>,
    operation: &mut Option<crate::diagnostics::Operation>,
) -> serde_json::Value {
    let mut req: serde_json::Value = match serde_json::from_str(&line) {
        Ok(v) => v,
        Err(_) => {
            return err(serde_json::Value::Null, "parse_error", "invalid JSON");
        }
    };

    drop(line);
    if !req.is_object() {
        return err(
            serde_json::Value::Null,
            "invalid_request",
            "request must be an object",
        );
    }
    let req_id = req
        .get_mut("id")
        .map(serde_json::Value::take)
        .unwrap_or(serde_json::Value::Null);
    let method = match req.get_mut("method").map(serde_json::Value::take) {
        Some(serde_json::Value::String(method)) => method,
        _ => return err(req_id, "invalid_request", "method must be a string"),
    };
    let mut params = req
        .get_mut("params")
        .map(serde_json::Value::take)
        .unwrap_or(serde_json::Value::Object(Default::default()));

    let operation = operation.insert(crate::diagnostics::Operation::begin(
        &method,
        req.get("trace_id").and_then(|id| id.as_str()),
    ));
    drop(req);
    // Early validation failures below are errors; cancellation while awaiting
    // execution is reset explicitly before yielding to the response channel.
    operation.finish(false);

    if params.is_null() {
        params = serde_json::json!({});
    } else if !params.is_object() {
        return err(req_id, "invalid_params", "params must be an object or null");
    }

    if method == "system.diagnostics" {
        drop(params);
        operation.pending();
        return match tokio::task::spawn_blocking(crate::diagnostics::snapshot).await {
            Ok(snapshot) => {
                operation.finish(true);
                ok(req_id, snapshot)
            }
            Err(_) => {
                operation.finish(false);
                err(req_id, "internal_error", "diagnostic sampling failed")
            }
        };
    }

    let target = if matches!(
        method.as_str(),
        "surface.split"
            | "surface.send_text"
            | "surface.send_key"
            | "surface.read_text"
            | "surface.read_scrollback"
            | "surface.health"
            | "surface.refresh"
            | "pane.focus"
    ) {
        match optional_target(&params) {
            Ok(target) => target,
            Err(message) => return err(req_id, "invalid_params", message),
        }
    } else {
        None
    };

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();

    let cmd = match method.as_str() {
        "surface.resume.set" | "surface.resume.show" | "surface.resume.clear" => {
            let id = match params.get("surface_id").or_else(|| params.get("id")) {
                None | Some(serde_json::Value::Null) => None,
                Some(serde_json::Value::String(id)) if uuid::Uuid::parse_str(id).is_ok() => {
                    Some(id.clone())
                }
                _ => return err(req_id, "invalid_params", "surface_id must be a UUID"),
            };
            let action = match method.as_str() {
                "surface.resume.set" => {
                    if params
                        .get("auto_resume")
                        .is_some_and(|value| value != &serde_json::Value::Bool(false))
                    {
                        return err(
                            req_id,
                            "not_supported",
                            "automatic resume requires a configured hook policy",
                        );
                    }
                    let mut binding =
                        match serde_json::from_value::<crate::resume::ResumeBinding>(params.take())
                        {
                            Ok(binding) => binding,
                            Err(_) => {
                                return err(
                                    req_id,
                                    "invalid_params",
                                    "invalid resume binding fields",
                                )
                            }
                        };
                    if let Err(message) = binding.validate() {
                        return err(req_id, "invalid_params", message);
                    }
                    binding.sanitize_environment();
                    crate::resume::ResumeAction::Set(binding)
                }
                "surface.resume.clear" => {
                    let checkpoint_id = match params.get("checkpoint_id") {
                        None | Some(serde_json::Value::Null) => None,
                        Some(serde_json::Value::String(value))
                            if value.len() <= 16384 && !value.contains('\0') =>
                        {
                            Some(value.clone())
                        }
                        _ => return err(req_id, "invalid_params", "invalid checkpoint_id"),
                    };
                    crate::resume::ResumeAction::Clear { checkpoint_id }
                }
                _ => crate::resume::ResumeAction::Show,
            };
            commands::SocketCommand::SurfaceResume {
                req_id: req_id.clone(),
                id,
                action,
                resp_tx,
            }
        }
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
                            return err(req_id, "invalid_directory", &message);
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
        "workspace.reorder_many" => {
            let Some(values) = params
                .get("workspace_ids")
                .or_else(|| params.get("order"))
                .and_then(|value| value.as_array())
                .filter(|values| !values.is_empty() && values.len() <= 4096)
            else {
                return err(
                    req_id,
                    "invalid_params",
                    "workspace_ids must contain 1..4096 UUIDs",
                );
            };
            let Some(order) = values
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .and_then(|value| uuid::Uuid::parse_str(value).ok())
                })
                .collect::<Option<Vec<_>>>()
            else {
                return err(req_id, "invalid_params", "invalid workspace UUID");
            };
            let dry_run = match params.get("dry_run") {
                None => false,
                Some(value) => match value.as_bool() {
                    Some(value) => value,
                    None => return err(req_id, "invalid_params", "dry_run must be boolean"),
                },
            };
            commands::SocketCommand::WorkspaceReorderMany {
                req_id: req_id.clone(),
                order,
                dry_run,
                resp_tx,
            }
        }
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
                );
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
        "surface.send_text" | "surface.send_key" | "debug.type" => {
            let field = if method == "surface.send_key" {
                "key"
            } else {
                "text"
            };
            let Some(input) = params.get(field).and_then(serde_json::Value::as_str) else {
                return err(
                    req_id,
                    "invalid_params",
                    &format!("{field} must be a string"),
                );
            };
            let input = input.to_owned();
            match method.as_str() {
                "surface.send_text" => commands::SocketCommand::SurfaceSendText {
                    req_id: req_id.clone(),
                    id: target,
                    text: input,
                    resp_tx,
                },
                "surface.send_key" => commands::SocketCommand::SurfaceSendKey {
                    req_id: req_id.clone(),
                    id: target,
                    key: input,
                    resp_tx,
                },
                _ => commands::SocketCommand::DebugType {
                    req_id: req_id.clone(),
                    text: input,
                    resp_tx,
                },
            }
        }
        "surface.read_text" | "surface.read_scrollback" => {
            commands::SocketCommand::SurfaceReadText {
                scrollback: method == "surface.read_scrollback",
                req_id: req_id.clone(),
                id: target,
                resp_tx,
            }
        }
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

        "project.actions.run" => {
            let workspace = match params.get("workspace_id").filter(|value| !value.is_null()) {
                None => None,
                Some(value) => match value
                    .as_str()
                    .and_then(|value| uuid::Uuid::parse_str(value).ok())
                {
                    Some(id) => Some(id),
                    None => return err(req_id, "invalid_params", "invalid workspace UUID"),
                },
            };
            let Some(action_id) = params
                .get("action_id")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty() && value.len() <= 128)
            else {
                return err(req_id, "invalid_params", "invalid action ID");
            };
            let Some(fingerprint) = params
                .get("fingerprint")
                .and_then(serde_json::Value::as_str)
                .filter(|value| value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()))
            else {
                return err(req_id, "invalid_params", "review fingerprint required");
            };
            let confirmed = match params.get("confirmed").filter(|value| !value.is_null()) {
                None => false,
                Some(serde_json::Value::Bool(value)) => *value,
                Some(_) => return err(req_id, "invalid_params", "confirmed must be boolean"),
            };
            commands::SocketCommand::ProjectActionRun {
                req_id: req_id.clone(),
                workspace,
                action_id: action_id.into(),
                fingerprint: fingerprint.into(),
                confirmed,
                resp_tx,
            }
        }
        "project.actions.list" => {
            let workspace = match params.get("workspace_id").filter(|value| !value.is_null()) {
                None => None,
                Some(value) => match value
                    .as_str()
                    .and_then(|value| uuid::Uuid::parse_str(value).ok())
                {
                    Some(id) => Some(id),
                    None => return err(req_id, "invalid_params", "invalid workspace UUID"),
                },
            };
            commands::SocketCommand::ProjectActionsList {
                req_id: req_id.clone(),
                workspace,
                resp_tx,
            }
        }
        "ports.list" => {
            let parse = |name: &str| -> Result<Option<uuid::Uuid>, &'static str> {
                params
                    .get(name)
                    .filter(|value| !value.is_null())
                    .map(|value| {
                        value
                            .as_str()
                            .and_then(|id| uuid::Uuid::parse_str(id).ok())
                            .ok_or("invalid scope UUID")
                    })
                    .transpose()
            };
            let workspace = match parse("workspace_id") {
                Ok(id) => id,
                Err(message) => return err(req_id, "invalid_params", message),
            };
            let surface = match parse("surface_id") {
                Ok(id) => id,
                Err(message) => return err(req_id, "invalid_params", message),
            };
            commands::SocketCommand::PortsList {
                req_id: req_id.clone(),
                workspace,
                surface,
                resp_tx,
            }
        }
        "sidebar.metadata"
        | "sidebar.set_status"
        | "sidebar.clear_status"
        | "sidebar.set_progress"
        | "sidebar.clear_progress"
        | "sidebar.report_meta_block"
        | "sidebar.clear_meta_block" => {
            let workspace = match params.get("workspace_id").filter(|value| !value.is_null()) {
                Some(value) => match value.as_str().and_then(|id| uuid::Uuid::parse_str(id).ok()) {
                    Some(id) => Some(id),
                    None => return err(req_id, "invalid_params", "invalid workspace UUID"),
                },
                None => None,
            };
            let action = match crate::workspace_metadata::parse(&method, &params) {
                Ok(action) => action,
                Err(message) => return err(req_id, "invalid_params", message),
            };
            commands::SocketCommand::WorkspaceMetadata {
                req_id: req_id.clone(),
                workspace,
                action,
                resp_tx,
            }
        }
        "notification.list" => commands::SocketCommand::NotificationList {
            req_id: req_id.clone(),
            resp_tx,
        },
        "notification.create"
        | "notification.create_for_surface"
        | "notification.create_for_caller"
        | "notification.create_for_target"
        | "notification.clear"
        | "notification.mark_read"
        | "notification.dismiss"
        | "notification.open"
        | "notification.jump_to_unread" => {
            let action = match crate::inbox::parse(&method, &params) {
                Ok(action) => action,
                Err(message) => return err(req_id, "invalid_params", message),
            };
            commands::SocketCommand::Inbox {
                req_id: req_id.clone(),
                action,
                resp_tx,
            }
        }

        "browser.open" => {
            let workspace = match params.get("workspace").filter(|value| !value.is_null()) {
                Some(value) => match value
                    .as_str()
                    .filter(|value| uuid::Uuid::parse_str(value).is_ok())
                {
                    Some(value) => Some(value.to_owned()),
                    None => return err(req_id, "invalid_params", "invalid workspace UUID"),
                },
                None => None,
            };
            commands::SocketCommand::BrowserOpen {
                req_id: req_id.clone(),
                url: params
                    .get("url")
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .to_owned(),
                workspace,
                resp_tx,
            }
        }
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

        _ => {
            return err(
                req_id,
                "not_implemented",
                &format!("{method} is not implemented"),
            )
        }
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
        return err(req_id, code, message);
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
    response
}

#[cfg(test)]
#[path = "dispatch_tests.rs"]
mod tests;
