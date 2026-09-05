//! Executable request validation, correlation and queue-admission scenarios.
use super::*;
use crate::socket::COMMAND_CAPACITY;

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
        let response: serde_json::Value = serde_json::from_str(&response).unwrap();
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
            let response: serde_json::Value = serde_json::from_str(&response).unwrap();
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
        let response: serde_json::Value = serde_json::from_str(&response).unwrap();
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
        let response: serde_json::Value = serde_json::from_str(&response).unwrap();
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
    let rejected: serde_json::Value = serde_json::from_str(&rejected).unwrap();
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
    let response: serde_json::Value = serde_json::from_str(&response).unwrap();
    assert_eq!(response["id"], 10);
    assert_eq!(response["result"]["pong"], true);
}

/// A vanished GTK receiver reports closure rather than overload or waiting forever.
#[tokio::test]
async fn closed_queue_reports_handler_loss() {
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    drop(rx);
    let response = dispatch_line(r#"{"id":11,"method":"system.ping"}"#.into(), &tx).await;
    let response: serde_json::Value = serde_json::from_str(&response).unwrap();
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
    let response: serde_json::Value = serde_json::from_str(&response).unwrap();
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"]["code"], "invalid_directory");
    assert!(
        rx.try_recv().is_err(),
        "invalid request reached GTK dispatch"
    );
}
