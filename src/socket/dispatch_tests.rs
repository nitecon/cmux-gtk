//! Executable request validation, correlation and queue-admission scenarios.
use super::*;
use crate::socket::COMMAND_CAPACITY;

/// Invalid input cannot consume GTK queue slots or turn into a successful empty paste.
#[tokio::test]
async fn malformed_terminal_input_never_reaches_gtk() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(COMMAND_CAPACITY);
    for (method, field) in [
        ("surface.send_text", "text"),
        ("surface.send_key", "key"),
        ("debug.type", "text"),
    ] {
        for input in [
            None,
            Some(serde_json::json!(null)),
            Some(serde_json::json!(false)),
            Some(serde_json::json!(7)),
            Some(serde_json::json!([])),
            Some(serde_json::json!({})),
        ] {
            let mut params = serde_json::json!({});
            if let Some(input) = input {
                params[field] = input;
            }
            let request = serde_json::json!({"id": 16, "method": method, "params": params});
            let response = tokio::time::timeout(
                std::time::Duration::from_secs(1),
                dispatch_line(request.to_string(), &tx),
            )
            .await
            .expect("invalid input must not await GTK");
            let response: serde_json::Value = serde_json::from_str(&response.body).unwrap();
            assert_eq!(response["id"], 16);
            assert_eq!(response["error"]["code"], "invalid_params", "{method}");
            assert!(rx.try_recv().is_err());
        }
    }
}

/// Input dispatch preserves empty text, Unicode, control characters and explicit targets.
#[tokio::test]
async fn terminal_input_reaches_gtk_unchanged() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(COMMAND_CAPACITY);
    for (method, field, input) in [
        ("surface.send_text", "text", ""),
        ("surface.send_text", "text", "echo λ\n"),
        ("surface.send_key", "key", "λ"),
        ("surface.send_key", "key", "\r"),
        ("debug.type", "text", ""),
        ("debug.type", "text", "echo λ\n"),
    ] {
        let mut params = serde_json::json!({"id": "target-uuid"});
        params[field] = serde_json::json!(input);
        let request = serde_json::json!({"id": 17, "method": method, "params": params});
        let dispatch = dispatch_line(request.to_string(), &tx);
        let consume = async {
            let commands::SocketCommand::Observed { command, .. } = rx.recv().await.unwrap() else {
                panic!("missing observed request");
            };
            let (req_id, resp_tx) = match *command {
                commands::SocketCommand::SurfaceSendText {
                    req_id,
                    id,
                    text,
                    resp_tx,
                } => {
                    assert_eq!(method, "surface.send_text");
                    assert_eq!(id.as_deref(), Some("target-uuid"));
                    assert_eq!(text, input);
                    (req_id, resp_tx)
                }
                commands::SocketCommand::SurfaceSendKey {
                    req_id,
                    id,
                    key,
                    resp_tx,
                } => {
                    assert_eq!(method, "surface.send_key");
                    assert_eq!(id.as_deref(), Some("target-uuid"));
                    assert_eq!(key, input);
                    (req_id, resp_tx)
                }
                commands::SocketCommand::DebugType {
                    req_id,
                    text,
                    resp_tx,
                } => {
                    assert_eq!(method, "debug.type");
                    assert_eq!(text, input);
                    (req_id, resp_tx)
                }
                _ => panic!("wrong input command"),
            };
            resp_tx.send(ok(req_id, serde_json::json!({}))).unwrap();
        };
        let (response, ()) = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            tokio::join!(dispatch, consume)
        })
        .await
        .unwrap();
        let response: serde_json::Value = serde_json::from_str(&response.body).unwrap();
        assert_eq!(response["id"], 17);
        assert_eq!(response["ok"], true);
        assert!(rx.try_recv().is_err());
    }
}

/// Non-object parameters cannot erase explicit targets and activate command defaults.
#[tokio::test]
async fn malformed_envelopes_never_reach_gtk() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(COMMAND_CAPACITY);
    let mut cases = vec![
        (serde_json::json!([]), "invalid_request"),
        (serde_json::json!(null), "invalid_request"),
        (serde_json::json!({"id": 15}), "invalid_request"),
        (
            serde_json::json!({"id": 15, "method": false}),
            "invalid_request",
        ),
    ];
    for method in [
        "surface.split",
        "surface.send_text",
        "workspace.create",
        "system.diagnostics",
    ] {
        for params in [
            serde_json::json!([]),
            serde_json::json!(false),
            serde_json::json!(3),
            serde_json::json!("target"),
        ] {
            cases.push((
                serde_json::json!({"id": 15, "method": method, "params": params}),
                "invalid_params",
            ));
        }
    }
    for (request, code) in cases {
        let response = dispatch_line(request.to_string(), &tx).await;
        let response: serde_json::Value = serde_json::from_str(&response.body).unwrap();
        assert_eq!(response["error"]["code"], code);
        assert_eq!(
            response["id"],
            request.get("id").cloned().unwrap_or_default()
        );
        assert!(rx.try_recv().is_err());
    }
}

/// Malformed explicit targets never become implicit active-terminal operations.
#[tokio::test]
async fn invalid_optional_targets_never_reach_gtk() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(COMMAND_CAPACITY);
    for method in [
        "surface.split",
        "surface.send_text",
        "surface.send_key",
        "surface.read_text",
        "surface.health",
        "surface.refresh",
        "pane.focus",
    ] {
        for id in [
            serde_json::json!(0),
            serde_json::json!(false),
            serde_json::json!([]),
            serde_json::json!({}),
        ] {
            let request = serde_json::json!({"id": 14, "method": method,
                "params": {"id": id, "text": "echo wrong-target", "key": "x"}});
            let response = dispatch_line(request.to_string(), &tx).await;
            let response: serde_json::Value = serde_json::from_str(&response.body).unwrap();
            assert_eq!(response["id"], 14);
            assert_eq!(response["error"]["code"], "invalid_params", "{method}");
            assert!(rx.try_recv().is_err());
        }
    }
}

/// Missing or malformed reorder positions cannot silently move a workspace to index zero.
#[tokio::test]
async fn invalid_reorder_positions_never_reach_gtk() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(COMMAND_CAPACITY);
    for position in [
        None,
        Some(serde_json::json!(-1)),
        Some(serde_json::json!(true)),
        Some(serde_json::json!("0")),
        Some(serde_json::json!(1.5)),
        Some(serde_json::json!(null)),
    ] {
        let mut params = serde_json::json!({"id": "test-workspace"});
        if let Some(position) = position {
            params["position"] = position;
        }
        let request =
            serde_json::json!({"id": 13, "method": "workspace.reorder", "params": params});
        let response = dispatch_line(request.to_string(), &tx).await;
        let response: serde_json::Value = serde_json::from_str(&response.body).unwrap();
        assert_eq!(response["id"], 13);
        assert_eq!(response["error"]["code"], "invalid_params");
        assert!(rx.try_recv().is_err());
    }
}

/// Malformed directions fail before GTK admission, preserving request correlation.
#[tokio::test]
async fn invalid_split_directions_never_reach_gtk() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(COMMAND_CAPACITY);
    for direction in [
        serde_json::json!("diagonal"),
        serde_json::json!(false),
        serde_json::json!(null),
        serde_json::json!(3),
    ] {
        let request = serde_json::json!({"id": 12, "method": "surface.split",
                                       "params": {"direction": direction}});
        let response = dispatch_line(request.to_string(), &tx).await;
        let response: serde_json::Value = serde_json::from_str(&response.body).unwrap();
        assert_eq!(response["id"], 12);
        assert_eq!(response["error"]["code"], "invalid_params");
        assert!(rx.try_recv().is_err());
    }
}

/// Saturation fails promptly with the original ID and allows dispatch after draining.
#[tokio::test]
async fn queue_overload_recovers_after_drain() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let (resp_tx, _resp_rx) = tokio::sync::oneshot::channel();
    assert!(tx
        .try_send(commands::SocketCommand::Ping {
            req_id: serde_json::json!(0),
            resp_tx,
        })
        .is_ok());
    let rejected = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        dispatch_line(r#"{"id":9,"method":"system.ping"}"#.into(), &tx),
    )
    .await
    .expect("full queue must not await GTK");
    let rejected: serde_json::Value = serde_json::from_str(&rejected.body).unwrap();
    assert_eq!(rejected["id"], 9);
    assert_eq!(rejected["error"]["code"], "overloaded");
    drop(rx.try_recv().unwrap());
    assert!(rx.try_recv().is_err(), "rejected request entered queue");

    let dispatch = dispatch_line(r#"{"id":10,"method":"system.ping"}"#.into(), &tx);
    let consume = async {
        let commands::SocketCommand::Observed { command, .. } = rx.recv().await.unwrap() else {
            panic!("missing observed request");
        };
        let commands::SocketCommand::Ping { req_id, resp_tx } = *command else {
            panic!("wrong command");
        };
        resp_tx
            .send(serde_json::json!({"id": req_id, "ok": true, "result": {"pong": true}}))
            .unwrap();
    };
    let (response, ()) = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        tokio::join!(dispatch, consume)
    })
    .await
    .unwrap();
    let response: serde_json::Value = serde_json::from_str(&response.body).unwrap();
    assert_eq!(response["id"], 10);
    assert_eq!(response["result"]["pong"], true);
}

/// A vanished GTK receiver reports closure rather than overload or waiting forever.
#[tokio::test]
async fn closed_queue_reports_handler_loss() {
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    drop(rx);
    let response = dispatch_line(r#"{"id":11,"method":"system.ping"}"#.into(), &tx).await;
    let response: serde_json::Value = serde_json::from_str(&response.body).unwrap();
    assert_eq!(response["id"], 11);
    assert_eq!(response["error"]["code"], "internal_error");
}

/// Reject an invalid directory through the real dispatcher before any command reaches GTK.
#[tokio::test]
async fn workspace_create_rejects_invalid_directory_before_ui_dispatch() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(COMMAND_CAPACITY);
    let response = dispatch_line(
        r#"{"id":7,"method":"workspace.create","params":{"working_directory":"/definitely/not/a/cmux/directory"}}"#.into(),
        &tx,
    )
    .await;
    let response: serde_json::Value = serde_json::from_str(&response.body).unwrap();
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"]["code"], "invalid_directory");
    assert!(
        rx.try_recv().is_err(),
        "invalid request reached GTK dispatch"
    );
}

/// Run an isolated dispatcher to verify overflow events and final outcome share the caller trace.
#[tokio::test]
async fn oversized_response_records_correlated_failure() {
    const CHILD: &str = "CMUX_TEST_RESPONSE_TRACE_CHILD";
    const TRACE: &str = "c7069e3b-fafd-466d-a4f0-85815be68d86";
    if std::env::var_os(CHILD).is_none() {
        // Isolation keeps the global logger and concurrent request counters out of this assertion.
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "socket::dispatch::tests::oversized_response_records_correlated_failure",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let records: Vec<serde_json::Value> = String::from_utf8(output.stderr)
            .unwrap()
            .lines()
            .filter_map(|line| line.strip_prefix("cmux: "))
            .filter_map(|line| serde_json::from_str(line).ok())
            .filter(|record: &serde_json::Value| record["fields"]["trace_id"] == TRACE)
            .collect();
        let overflow = records
            .iter()
            .position(|record| record["event"] == "rpc.response.oversized")
            .unwrap();
        let completions: Vec<_> = records
            .iter()
            .enumerate()
            .filter(|(_, record)| record["event"] == "rpc.complete")
            .collect();
        assert_eq!(completions.len(), 1);
        assert!(overflow < completions[0].0);
        assert_eq!(completions[0].1["fields"]["outcome"], "error");
        return;
    }
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let request = serde_json::json!({"id": 12, "method": "system.ping", "trace_id": TRACE});
    let dispatch = dispatch_line(request.to_string(), &tx);
    let consume = async {
        let commands::SocketCommand::Observed { command, .. } = rx.recv().await.unwrap() else {
            panic!("missing observed request");
        };
        let commands::SocketCommand::Ping { req_id, resp_tx } = *command else {
            panic!("wrong command");
        };
        resp_tx
            .send(super::ok(
                req_id,
                serde_json::json!("x".repeat(super::super::response::MAX_RESPONSE_BYTES)),
            ))
            .unwrap();
    };
    let (response, ()) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(dispatch, consume)
    })
    .await
    .unwrap();
    let response: serde_json::Value = serde_json::from_str(&response.body).unwrap();
    assert_eq!(response["id"], 12);
    assert_eq!(response["error"]["code"], "response_too_large");
}

/// Preserve validated or generated correlation through encoding, including protocol errors.
#[tokio::test]
async fn encoded_response_retains_operation_identity() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let trace = uuid::Uuid::new_v4();
    for supplied in [trace.to_string(), "invalid-trace".to_owned()] {
        let request = serde_json::json!({
            "id": 41, "method": "surface.send_text", "params": {"text": false}, "trace_id": supplied
        });
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            dispatch_line(request.to_string(), &tx),
        )
        .await
        .expect("invalid input must finish without GTK");
        let retained = response
            .trace_id
            .expect("validated method creates an operation");
        if supplied == trace.to_string() {
            assert_eq!(retained, trace);
        } else {
            assert_ne!(retained, uuid::Uuid::nil());
        }
        let body: serde_json::Value = serde_json::from_str(&response.body).unwrap();
        assert_eq!(body["id"], 41);
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"]["code"], "invalid_params");
        assert!(rx.try_recv().is_err());
    }
    let malformed = dispatch_line("{broken".to_owned(), &tx).await;
    assert!(malformed.trace_id.is_none());
    let body: serde_json::Value = serde_json::from_str(&malformed.body).unwrap();
    assert_eq!(body["error"]["code"], "parse_error");
}
