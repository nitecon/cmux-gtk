use super::writer::RpcWriter;
use crate::ssh::bridge::SshBridge;
use crate::ssh::{SshEvent, SshEventTx};
use crate::task::AbortOnDrop;
use crate::workspace::ConnectionState;
use base64::Engine;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, BufReader, BufWriter};
use tokio::process::{Child, Command};
use tokio::sync::oneshot;

/// Maximum reconnection backoff delay.
const MAX_BACKOFF_SECS: u64 = 30;

/// Maximum number of reconnection attempts before giving up.
const MAX_RETRIES: u32 = 10;

/// Whether a failure is permanent (no point retrying) or transient (retry with backoff).
enum FailureKind {
    Permanent,
    Transient,
}

/// Pending RPC responses awaiting completion.
pub(super) type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<serde_json::Value>>>>;

/// Manage an SSH workspace connection lifecycle.
/// Runs as a tokio task. Reports state changes via ssh_tx.
pub async fn run_ssh_lifecycle(
    workspace_id: u64,
    target: String,
    ssh_tx: SshEventTx,
    bridge: Arc<SshBridge>,
    launch_trace: uuid::Uuid,
) {
    let mut attempt: u32 = 0;
    let mut deployed = false;

    loop {
        let mut connection = super::metrics::Attempt::begin(workspace_id, attempt, launch_trace);
        // Update state to reconnecting
        let _ = ssh_tx
            .send(SshEvent::StateChanged {
                workspace_id,
                state: ConnectionState::Reconnecting(attempt),
            })
            .await;

        // A failed upload must be retried; never fall through to an older or
        // missing daemon simply because the connection attempt count advanced.
        if !deployed {
            connection.phase("deployment");
            if let Err(e) = crate::ssh::deploy::deploy_remote(&target).await {
                eprintln!("cmux: SSH deploy failed: {e}");

                // Classify: binary-not-found is permanent, everything else is transient
                let kind = if e.contains("not found at") {
                    FailureKind::Permanent
                } else {
                    FailureKind::Transient
                };

                if matches!(kind, FailureKind::Permanent) {
                    let _ = ssh_tx
                        .send(SshEvent::StateChanged {
                            workspace_id,
                            state: ConnectionState::Disconnected,
                        })
                        .await;
                    eprintln!("cmux: SSH permanent failure, giving up: {e}");
                    connection.finish("permanent_failure");
                    break;
                }

                let _ = ssh_tx
                    .send(SshEvent::StateChanged {
                        workspace_id,
                        state: ConnectionState::Disconnected,
                    })
                    .await;
                let backoff = backoff_duration(attempt);
                connection.phase("deployment_backoff");
                tokio::time::sleep(backoff).await;
                attempt += 1;
                if attempt >= MAX_RETRIES {
                    eprintln!("cmux: SSH deployment exhausted retries");
                    connection.finish("retries_exhausted");
                    break;
                }
                connection.finish("retry");
                continue;
            }
            deployed = true;
        }

        // Start SSH connection with cmuxd-remote in stdio mode
        connection.phase("process_spawn");
        match start_ssh(&target).await {
            Ok(mut child) => {
                let was_reconnect = attempt > 0;
                let mut connected = false;
                let mut input_rejected = false;

                let stdin = child.stdin.take();
                let stdout = child.stdout.take();
                let stderr = child.stderr.take();

                // The reader belongs to this connection, including lifecycle cancellation.
                let _stderr_guard = stderr.map(|reader| {
                    let task = tokio::spawn(drain_stderr(reader, workspace_id));
                    AbortOnDrop(task.abort_handle())
                });

                if let (Some(writer), Some(reader)) = (stdin, stdout) {
                    let writer = Arc::new(RpcWriter::new(
                        BufWriter::new(writer),
                        workspace_id,
                        connection.id,
                    ));
                    let mut reader = BufReader::new(reader);
                    let started = std::time::Instant::now();
                    connection.phase("handshake");
                    let result =
                        super::handshake::establish(&writer, &mut reader, Duration::from_secs(15))
                            .await;
                    crate::diagnostics::record(
                        "ssh.handshake.complete",
                        serde_json::json!({
                            "workspace_id": workspace_id, "duration_us": started.elapsed().as_micros() as u64,
                            "trace_id": connection.id,
                            "ports_supported": result.as_ref().ok().map(|value| value.ports),
                            "forwarding_supported": result.as_ref().ok().map(|value| value.forwarding),
                            "remote_handler_duration_us": result.as_ref().ok().and_then(|value| value.handler_duration_us),
                            "outcome": if result.is_ok() { "success" } else { "error" },
                            "error_kind": result.as_ref().err().map(|error| format!("{:?}", error.kind())),
                        }),
                    );
                    if let Err(error) = result {
                        eprintln!("cmux: SSH handshake failed: {error}");
                    } else {
                        connected = true;
                        connection.phase("connected_gtk_admission");
                        let _ = ssh_tx
                            .send(SshEvent::StateChanged {
                                workspace_id,
                                state: ConnectionState::Connected,
                            })
                            .await;
                        // D-07: inject reconnect message if this was a reconnection
                        if was_reconnect {
                            {
                                let ids: Vec<u64> =
                                    bridge.streams.lock().unwrap().keys().copied().collect();
                                for pane_id in ids {
                                    let msg =
                                b"\r\n\x1b[32m[Reconnected \xe2\x80\x94 new session]\x1b[0m\r\n";
                                    let _ = ssh_tx
                                        .send(SshEvent::RemoteOutput {
                                            pane_id,
                                            data: msg.to_vec(),
                                        })
                                        .await;
                                }
                            }
                        }

                        connection.phase("routing");
                        input_rejected = run_proxy_routing(
                            workspace_id,
                            writer,
                            reader,
                            &bridge,
                            &ssh_tx,
                            result.unwrap(),
                        )
                        .await;
                    }
                }

                if connected {
                    attempt = 0;
                }
                // Routing has ended; an uncooperative child cannot indefinitely stall reconnect.
                let exit_started = std::time::Instant::now();
                connection.phase("process_reap");
                let exit_status = crate::task::reap_child(child, Duration::from_secs(2)).await;
                crate::diagnostics::record(
                    "ssh.process.exit",
                    serde_json::json!({
                        "workspace_id": workspace_id,
                        "trace_id": connection.id,
                        "duration_us": exit_started.elapsed().as_micros() as u64,
                        "forced": exit_status.as_ref().ok().map(|(_, forced)| *forced),
                        "input_rejected": input_rejected,
                        "exit_code": exit_status.as_ref().ok().and_then(|(status, _)| status.code()),
                        "error_kind": exit_status.as_ref().err().map(|error| format!("{:?}", error.kind())),
                    }),
                );
                eprintln!("cmux: SSH to {target} exited: {exit_status:?}");
                connection.phase("disconnected_gtk_admission");

                // D-06: inject disconnect message into all active panes
                {
                    let ids: Vec<u64> = bridge.streams.lock().unwrap().keys().copied().collect();
                    for pane_id in ids {
                        let msg: &[u8] = if input_rejected {
                            b"\r\n[SSH input queue unavailable; some input was not sent. Reconnecting with a new session.]\r\n"
                        } else {
                            b"\r\n\x1b[33m[SSH disconnected \xe2\x80\x94 reconnecting...]\x1b[0m\r\n"
                        };
                        let _ = ssh_tx
                            .send(SshEvent::RemoteOutput {
                                pane_id,
                                data: msg.to_vec(),
                            })
                            .await;
                    }
                }

                let _ = ssh_tx
                    .send(SshEvent::StateChanged {
                        workspace_id,
                        state: ConnectionState::Disconnected,
                    })
                    .await;
            }
            Err(e) => {
                connection.phase("spawn_failed_gtk_admission");
                eprintln!("cmux: SSH connection to {target} failed: {e}");
                let _ = ssh_tx
                    .send(SshEvent::StateChanged {
                        workspace_id,
                        state: ConnectionState::Disconnected,
                    })
                    .await;
            }
        }

        if attempt >= MAX_RETRIES {
            eprintln!("cmux: SSH max retries ({MAX_RETRIES}) exceeded for {target}, giving up");
            let _ = ssh_tx
                .send(SshEvent::StateChanged {
                    workspace_id,
                    state: ConnectionState::Disconnected,
                })
                .await;
            connection.finish("retries_exhausted");
            break;
        }

        // Exponential backoff before reconnect (per D-14)
        let backoff = backoff_duration(attempt);
        eprintln!(
            "cmux: SSH reconnecting to {target} in {}s (attempt {})",
            backoff.as_secs(),
            attempt + 1
        );
        connection.phase("reconnect_backoff");
        tokio::time::sleep(backoff).await;
        attempt += 1;
        connection.finish("retry");
    }
}

/// Bidirectional proxy routing between bridge channels and SSH stdin/stdout.
async fn run_proxy_routing(
    workspace_id: u64,
    writer: Arc<RpcWriter<BufWriter<tokio::process::ChildStdin>>>,
    reader: BufReader<tokio::process::ChildStdout>,
    bridge: &Arc<SshBridge>,
    ssh_tx: &SshEventTx,
    negotiated: super::handshake::Negotiated,
) -> bool {
    let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));

    // Clear old targets before publishing a new sender generation to GTK callbacks.
    bridge.clear_stream_ids();
    let (mut local_write_rx, mut input_failure) = bridge.take_or_recreate_write_rx();

    // Read path: parse JSON lines from SSH stdout.
    // MUST be spawned BEFORE open_remote_stream so RPC responses can be received.
    let read_bridge = bridge.clone();
    let read_ssh_tx = ssh_tx.clone();
    let read_pending = pending.clone();
    let read_handle = tokio::spawn(async move {
        let mut buf_reader = reader;
        loop {
            match crate::line_reader::next_line(
                &mut buf_reader,
                4 * 1024 * 1024,
                Duration::from_secs(10),
            )
            .await
            {
                Ok(None) => break,
                Ok(Some(line)) => {
                    if let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) {
                        handle_incoming_message(&msg, &read_bridge, &read_ssh_tx, &read_pending)
                            .await;
                    }
                }
                Err(e) => {
                    crate::diagnostics::record(
                        "ssh.framing.rejected",
                        serde_json::json!({
                            "workspace_id": workspace_id, "error_kind": format!("{:?}", e.kind()),
                        }),
                    );
                    eprintln!("cmux: SSH stdout read error: {e}");
                    break;
                }
            }
        }
    });

    let _read_guard = AbortOnDrop(read_handle.abort_handle());

    // New terminals can appear after the tunnel connects. Surface initialization
    // registers them only once GTK can receive their startup output.
    let open_writer = writer.clone();
    let open_bridge = bridge.clone();
    let open_tx = ssh_tx.clone();
    let open_pending = pending.clone();
    let open_handle = tokio::spawn(async move {
        loop {
            let ids: Vec<u64> = open_bridge
                .streams
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, stream)| stream.stream_id.is_empty())
                .map(|(&id, _)| id)
                .collect();
            for id in ids {
                if let Err(error) = open_remote_stream(
                    &open_writer,
                    &open_bridge,
                    id,
                    &open_pending,
                    &open_tx,
                    80,
                    24,
                )
                .await
                {
                    eprintln!("cmux: remote terminal launch failed: {error}");
                    let message = format!("\r\n[Remote terminal failed: {error}]\r\n");
                    let _ = open_tx
                        .send(SshEvent::RemoteOutput {
                            pane_id: id,
                            data: message.into_bytes(),
                        })
                        .await;
                    let _ = open_tx.send(SshEvent::RemoteEof { pane_id: id }).await;
                    open_bridge.remove_pane(id);
                }
            }
            open_bridge.changed.notified().await;
        }
    });

    let _open_guard = AbortOnDrop(open_handle.abort_handle());

    // Poll one current PTY at a time; dropping the connection cancels request ownership and the poller.
    let ports_writer = writer.clone();
    let ports_bridge = bridge.clone();
    let ports_pending = pending.clone();
    let ports_handle = tokio::spawn(async move {
        if !negotiated.ports {
            return;
        }
        let mut cursor = 0usize;
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let target = {
                let streams = ports_bridge.streams.lock().unwrap();
                let mut live: Vec<_> = streams
                    .iter()
                    .filter(|(_, stream)| !stream.stream_id.is_empty())
                    .map(|(&id, stream)| (id, stream.stream_id.clone()))
                    .collect();
                live.sort_by_key(|(id, _)| *id);
                if live.is_empty() {
                    continue;
                }
                let target = live[cursor % live.len()].clone();
                cursor = cursor.wrapping_add(1);
                target
            };
            let id = ports_bridge.next_id();
            let request = serde_json::json!({"jsonrpc":"2.0","id":id,"method":"ports.list","params":{"stream_id":target.1}});
            let response =
                request_remote(&ports_writer, &ports_pending, id, "ports.list", request).await;
            let rows = response.ok().and_then(|value| {
                let result = value.get("result")?;
                if result.get("stream_id")?.as_str()? != target.1 {
                    return None;
                }
                let rows = result.get("ports")?.as_array()?;
                if rows.len() > 256 {
                    return None;
                }
                let rows: Vec<crate::ports::RemotePort> =
                    serde_json::from_value(serde_json::Value::Array(rows.clone())).ok()?;
                rows.iter()
                    .all(|row| row.port > 0 && row.pid > 0 && row.provenance == "remote")
                    .then_some(rows)
            });
            let streams = ports_bridge.streams.lock().unwrap();
            if streams
                .get(&target.0)
                .is_some_and(|stream| stream.stream_id == target.1)
            {
                ports_bridge
                    .listeners
                    .lock()
                    .unwrap()
                    .insert(target.0, (target.1, rows));
            }
        }
    });
    let _ports_guard = AbortOnDrop(ports_handle.abort_handle());

    let _forward_guard = negotiated.forwarding.then(|| {
        let task = tokio::spawn(super::forward::run(
            writer.clone(),
            pending.clone(),
            bridge.clone(),
        ));
        AbortOnDrop(task.abort_handle())
    });

    // Write path: consume WriteRequests and send as JSON-RPC to SSH stdin
    let write_writer = writer.clone();
    let write_bridge = bridge.clone();
    let write_handle = tokio::spawn(async move {
        while let Some(req) = local_write_rx.recv().await {
            let rpc_id = write_bridge.next_id();
            let rpc = serde_json::json!({
                "jsonrpc": "2.0",
                "id": rpc_id,
                "method": if req.close { "proxy.close" } else if req.resize.is_some() { "stream.resize" } else { "proxy.write" },
                "params": {
                    "stream_id": req.stream_id,
                    "data_base64": req.data_base64,
                    "cols": req.resize.map(|v| v.0),
                    "rows": req.resize.map(|v| v.1),
                }
            });
            if write_writer.send(&rpc).await.is_err() {
                break;
            }
        }
    });

    let _write_guard = AbortOnDrop(write_handle.abort_handle());

    // Any failed writer or completed companion retires the connection.
    tokio::select! {
        _ = read_handle => {},
        _ = write_handle => {},
        _ = open_handle => {},
        _ = writer.failed() => {},
        _ = input_failure.wait_for(|failed| *failed) => {},
    }
    // Scope-owned abort guards cancel every remaining companion.
    let rejected = *input_failure.borrow();
    rejected
}

/// Handle an incoming JSON message from cmuxd-remote.
async fn handle_incoming_message(
    msg: &serde_json::Value,
    bridge: &SshBridge,
    ssh_tx: &SshEventTx,
    pending: &PendingMap,
) {
    if let Some(event) = msg.get("event").and_then(|v| v.as_str()) {
        let stream = msg.get("stream_id").and_then(|v| v.as_str()).unwrap_or("");
        if bridge.proxy_routes.event(stream, event, msg) {
            return;
        }
        let id = bridge.stream_to_pane.lock().unwrap().get(stream).copied();
        let Some(pane_id) = id else {
            return;
        };
        match event {
            "proxy.stream.data" => {
                if let Some(encoded) = msg.get("data_base64").and_then(|v| v.as_str()) {
                    if let Ok(data) = base64::engine::general_purpose::STANDARD.decode(encoded) {
                        // Backpressure the SSH reader when GTK is busy. Never drop terminal output.
                        let _ = ssh_tx.send(SshEvent::RemoteOutput { pane_id, data }).await;
                    }
                }
            }
            "proxy.stream.eof" | "proxy.stream.error" => {
                bridge.remove_pane(pane_id);
                let _ = ssh_tx.send(SshEvent::RemoteEof { pane_id }).await;
            }
            _ => {}
        }
        return;
    }
    if let Some(id) = msg.get("id").and_then(|v| v.as_u64()) {
        if let Some(tx) = pending.lock().unwrap().remove(&id) {
            let _ = tx.send(msg.clone());
        }
    }
}

/// Open a remote PTY stream for a pane via session.spawn + proxy.stream.subscribe.
async fn open_remote_stream(
    writer: &Arc<RpcWriter<BufWriter<tokio::process::ChildStdin>>>,
    bridge: &SshBridge,
    pane_id: u64,
    pending: &PendingMap,
    ssh_tx: &SshEventTx,
    cols: u16,
    rows: u16,
) -> Result<String, String> {
    let (cols, rows) = bridge
        .contexts
        .lock()
        .unwrap()
        .get(&pane_id)
        .map(|ctx| *ctx.size.lock().unwrap())
        .unwrap_or((cols, rows));
    // Send session.spawn RPC
    let spawn_id = bridge.next_id();
    let spawn_rpc = serde_json::json!({
        "jsonrpc": "2.0",
        "id": spawn_id,
        "method": "session.spawn",
        "params": {
            "cols": cols,
            "rows": rows,
            "cwd": bridge.directory.lock().unwrap().clone(),
        }
    });

    let resp = request_remote(writer, pending, spawn_id, "session.spawn", spawn_rpc).await?;

    let stream_id = resp
        .get("result")
        .and_then(|r| r.get("stream_id"))
        .and_then(|v| v.as_str())
        .filter(|id| !id.is_empty() && id.len() <= super::outbound::MAX_STREAM_ID)
        .ok_or_else(|| {
            writer.retire_unanswered_request();
            let err_msg = resp
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("unknown error");
            format!("session.spawn failed: {err_msg}")
        })?
        .to_string();

    // Install routing before subscribing: the daemon may emit output immediately.
    let registered = {
        let contexts = bridge.contexts.lock().unwrap();
        if let Some(ctx) = contexts.get(&pane_id) {
            bridge.register_pane(pane_id, stream_id.clone());
            *ctx.stream_id.lock().unwrap() = Some(stream_id.clone());
            true
        } else {
            false
        }
    };
    if !registered {
        bridge.request_close(stream_id);
        return Err("terminal closed while its remote PTY was starting".into());
    }

    // Subscribe to the stream
    let sub_id = bridge.next_id();
    let sub_rpc = serde_json::json!({
        "jsonrpc": "2.0",
        "id": sub_id,
        "method": "proxy.stream.subscribe",
        "params": {
            "stream_id": &stream_id,
        }
    });

    if let Err(error) =
        request_remote(writer, pending, sub_id, "proxy.stream.subscribe", sub_rpc).await
    {
        bridge.request_close(stream_id.clone());
        return Err(error);
    }

    bridge.mark_subscribed(pane_id);
    eprintln!("cmux: remote terminal={pane_id} stream={stream_id} subscribed");

    // Notify via SSH event
    let _ = ssh_tx
        .send(SshEvent::StreamOpened {
            pane_id,
            stream_id: stream_id.clone(),
        })
        .await;

    Ok(stream_id)
}

/// Register before writing, await a correlated successful response and remove the slot on every exit.
/// The existing fifteen-second reply budget begins after the separately bounded request write.
pub(super) async fn request_remote<W: tokio::io::AsyncWrite + Unpin>(
    writer: &RpcWriter<W>,
    pending: &PendingMap,
    id: u64,
    method: &'static str,
    mut request: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let (sender, receiver) = oneshot::channel();
    pending
        .lock()
        .map_err(|_| "remote response registry unavailable".to_string())?
        .insert(id, sender);
    let mut awaiting = PendingRequest {
        pending: pending.clone(),
        id,
        writer,
        settled: false,
        trace_id: uuid::Uuid::new_v4(),
        started: std::time::Instant::now(),
        method,
        outcome: "cancelled",
        remote_duration_us: None,
    };
    request["trace_id"] = serde_json::json!(awaiting.trace_id);
    crate::diagnostics::record(
        "ssh.rpc.begin",
        serde_json::json!({
            "workspace_id": writer.workspace_id(), "trace_id": awaiting.trace_id,
            "parent_trace_id": writer.connection_id(),
            "request_id": id, "method": method,
        }),
    );
    writer.send(&request).await.map_err(|error| {
        awaiting.outcome = "write_error";
        format!("write {method} failed: {error}")
    })?;
    let response = tokio::time::timeout(Duration::from_secs(15), receiver)
        .await
        .map_err(|_| {
            awaiting.outcome = "timeout";
            format!("{method} timed out")
        })?
        .map_err(|_| {
            awaiting.outcome = "response_channel_closed";
            format!("{method} response channel dropped")
        })?;
    awaiting.outcome = "invalid_response";
    if response.get("id").and_then(|value| value.as_u64()) != Some(id) {
        return Err(format!("{method} response identity mismatch"));
    }
    awaiting.remote_duration_us = super::metrics::remote_timing(&response, awaiting.trace_id)
        .map_err(|message| format!("{method} {message}"))?;
    let accepted = response
        .get("ok")
        .and_then(|value| value.as_bool())
        .ok_or_else(|| format!("{method} response status invalid"))?;
    awaiting.settled = true;
    if !accepted {
        awaiting.outcome = "remote_error";
        let message = response
            .pointer("/error/message")
            .and_then(|value| value.as_str())
            .unwrap_or("remote request rejected");
        return Err(format!("{method} failed: {message}"));
    }
    awaiting.outcome = "success";
    Ok(response)
}

/// Start an SSH process with cmuxd-remote in stdio mode.
async fn start_ssh(target: &str) -> Result<Child, String> {
    crate::workspace::validate_ssh_target(target)?;
    let child = Command::new("ssh")
        .args([
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ServerAliveCountMax=3",
            "-o",
            "ConnectTimeout=10",
            "-o",
            "BatchMode=yes",
            target,
            ".local/bin/cmuxd-remote",
            "serve",
            "--stdio",
        ])
        .kill_on_drop(true)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn ssh: {e}"))?;

    Ok(child)
}

/// Calculate exponential backoff: 1s, 2s, 4s, 8s, 16s, 30s cap (per D-14).
fn backoff_duration(attempt: u32) -> Duration {
    let secs = (1u64 << attempt.min(5)).min(MAX_BACKOFF_SECS);
    Duration::from_secs(secs)
}

/// Own response registration and retire uncertain remote side effects on cancellation or lost replies.
struct PendingRequest<'a, W: tokio::io::AsyncWrite + Unpin> {
    pending: PendingMap,
    id: u64,
    writer: &'a RpcWriter<W>,
    settled: bool,
    trace_id: uuid::Uuid,
    started: std::time::Instant,
    method: &'static str,
    outcome: &'static str,
    remote_duration_us: Option<u64>,
}
impl<W: tokio::io::AsyncWrite + Unpin> Drop for PendingRequest<'_, W> {
    /// Release the slot on every exit; only correlated boolean-status replies settle the request.
    fn drop(&mut self) {
        crate::diagnostics::record(
            "ssh.rpc.complete",
            serde_json::json!({
                "workspace_id": self.writer.workspace_id(), "trace_id": self.trace_id,
                "parent_trace_id": self.writer.connection_id(),
                "request_id": self.id, "method": self.method, "outcome": self.outcome,
                "duration_us": self.started.elapsed().as_micros(),
                "remote_handler_duration_us": self.remote_duration_us,
            }),
        );
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(&self.id);
        }
        if !self.settled {
            self.writer.retire_unanswered_request();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Grow reconnect delays exponentially while capping long retry sequences.
    #[test]
    fn test_backoff_duration() {
        assert_eq!(backoff_duration(0), Duration::from_secs(1));
        assert_eq!(backoff_duration(1), Duration::from_secs(2));
        assert_eq!(backoff_duration(2), Duration::from_secs(4));
        assert_eq!(backoff_duration(3), Duration::from_secs(8));
        assert_eq!(backoff_duration(4), Duration::from_secs(16));
        assert_eq!(backoff_duration(5), Duration::from_secs(30)); // capped
        assert_eq!(backoff_duration(10), Duration::from_secs(30)); // still capped
    }
}

/// Drain a connection's stderr with fixed storage; log only its first 64 KiB, then discard excess.
/// Chunk boundaries may split UTF-8 diagnostics; lossy formatting stays bounded and content is never structured telemetry.
async fn drain_stderr<R: AsyncRead + Unpin>(mut reader: R, workspace_id: u64) {
    const LOG_LIMIT: usize = 64 * 1024;
    let mut buffer = [0; 4096];
    let mut logged = 0;
    let mut limited = false;
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) => break,
            Ok(count) => {
                let emit = count.min(LOG_LIMIT - logged);
                if emit > 0 {
                    eprintln!(
                        "cmux: SSH stderr: {}",
                        String::from_utf8_lossy(&buffer[..emit])
                    );
                    logged += emit;
                }
                if emit < count && !limited {
                    limited = true;
                    crate::diagnostics::record(
                        "ssh.stderr.limited",
                        serde_json::json!({
                            "workspace_id": workspace_id, "limit_bytes": LOG_LIMIT,
                        }),
                    );
                }
            }
            Err(error) => {
                crate::diagnostics::record(
                    "ssh.stderr.failed",
                    serde_json::json!({
                        "workspace_id": workspace_id, "error_kind": format!("{:?}", error.kind()),
                    }),
                );
                break;
            }
        }
    }
}

#[cfg(test)]
mod stderr_tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    /// Keep draining beyond the logging cap so a noisy peer cannot block on its stderr pipe.
    #[tokio::test]
    async fn drains_unterminated_flood() {
        let (reader, mut writer) = tokio::io::duplex(4096);
        let task = tokio::spawn(drain_stderr(reader, 0));
        let guard = AbortOnDrop(task.abort_handle());
        tokio::time::timeout(Duration::from_secs(5), async {
            for _ in 0..32 {
                writer.write_all(&[b'x'; 4096]).await.unwrap();
            }
            drop(writer);
            task.await.unwrap();
        })
        .await
        .unwrap();
        drop(guard);
    }

    /// Retiring the connection cancels a blocked stderr reader and releases its pipe.
    #[tokio::test]
    async fn cancellation_releases_reader() {
        let (reader, mut writer) = tokio::io::duplex(16);
        let task = tokio::spawn(drain_stderr(reader, 0));
        let guard = AbortOnDrop(task.abort_handle());
        drop(guard);
        assert!(tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap_err()
            .is_cancelled());
        assert!(writer.write_all(b"closed").await.is_err());
    }
}

#[cfg(test)]
mod request_tests {
    use super::*;
    use tokio::io::AsyncBufReadExt;

    /// Real request delivery requires correlated ok=true replies and cleans slots after every response outcome.
    #[tokio::test]
    async fn validates_remote_response_and_releases_slot() {
        for reply in [
            Some(serde_json::json!({"id": 42, "ok": true, "result": {}})),
            Some(
                serde_json::json!({"id": 42, "ok": false, "error": {"message": "subscription refused"}}),
            ),
            Some(serde_json::json!({"id": 43, "ok": true, "result": {}})),
            Some(serde_json::json!({"id": 42, "ok": null})),
            None,
        ] {
            let expected = reply
                .as_ref()
                .is_some_and(|value| value["id"] == 42 && value["ok"] == true);
            let should_retire = !reply
                .as_ref()
                .is_some_and(|value| value["id"] == 42 && value["ok"].is_boolean());
            let (pipe, reader) = tokio::io::duplex(4096);
            let writer = RpcWriter::new(pipe, 0, uuid::Uuid::new_v4());
            let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
            let request =
                serde_json::json!({"id": 42, "method": "proxy.stream.subscribe", "params": {}});
            let respond = async {
                let mut reader = BufReader::new(reader);
                let mut line = String::new();
                reader.read_line(&mut line).await.unwrap();
                let mut received: serde_json::Value = serde_json::from_str(&line).unwrap();
                let trace = received
                    .as_object_mut()
                    .unwrap()
                    .remove("trace_id")
                    .unwrap();
                assert!(uuid::Uuid::parse_str(trace.as_str().unwrap()).is_ok());
                assert_eq!(received, request);
                let sender = pending
                    .lock()
                    .unwrap()
                    .remove(&42)
                    .expect("registered before write");
                if let Some(mut reply) = reply {
                    reply["trace_id"] = trace;
                    reply["handler_duration_us"] = serde_json::json!(7);
                    sender.send(reply).unwrap();
                }
            };
            let (result, ()) = tokio::time::timeout(Duration::from_secs(1), async {
                tokio::join!(
                    request_remote(
                        &writer,
                        &pending,
                        42,
                        "proxy.stream.subscribe",
                        request.clone()
                    ),
                    respond
                )
            })
            .await
            .unwrap();
            assert_eq!(result.is_ok(), expected);
            assert_eq!(
                tokio::time::timeout(Duration::from_millis(30), writer.failed())
                    .await
                    .is_ok(),
                should_retire
            );
            assert!(pending.lock().unwrap().is_empty());
        }
    }

    /// Optional legacy replies remain valid; malformed or mismatched remote trace metadata retires the writer.
    #[tokio::test]
    async fn validates_optional_remote_trace_metadata() {
        for mode in ["legacy", "matched", "mismatch", "invalid_duration"] {
            let (pipe, reader) = tokio::io::duplex(4096);
            let writer = RpcWriter::new(pipe, 0, uuid::Uuid::new_v4());
            let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
            let respond = async {
                let mut line = String::new();
                BufReader::new(reader).read_line(&mut line).await.unwrap();
                let request: serde_json::Value = serde_json::from_str(&line).unwrap();
                let mut response = serde_json::json!({"id": 42, "ok": true});
                if mode != "legacy" {
                    response["trace_id"] = if mode == "mismatch" {
                        serde_json::json!(uuid::Uuid::new_v4())
                    } else {
                        request["trace_id"].clone()
                    };
                    response["handler_duration_us"] = if mode == "invalid_duration" {
                        serde_json::json!(-1)
                    } else {
                        serde_json::json!(0)
                    };
                }
                pending
                    .lock()
                    .unwrap()
                    .remove(&42)
                    .unwrap()
                    .send(response)
                    .unwrap();
            };
            let (result, ()) = tokio::time::timeout(Duration::from_secs(1), async {
                tokio::join!(
                    request_remote(
                        &writer,
                        &pending,
                        42,
                        "session.spawn",
                        serde_json::json!({"id": 42, "method": "session.spawn"})
                    ),
                    respond
                )
            })
            .await
            .unwrap();
            let valid = matches!(mode, "legacy" | "matched");
            assert_eq!(result.is_ok(), valid);
            assert_eq!(
                tokio::time::timeout(Duration::from_millis(30), writer.failed())
                    .await
                    .is_err(),
                valid
            );
        }
    }

    /// Cancellation while awaiting a peer response removes the slot without leaving a response waiter.
    #[tokio::test]
    async fn cancellation_releases_pending_request() {
        let writer = Arc::new(RpcWriter::new(tokio::io::sink(), 0, uuid::Uuid::new_v4()));
        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let task_pending = pending.clone();
        let observe_writer = writer.clone();
        let task = tokio::spawn(async move {
            request_remote(
                &writer,
                &task_pending,
                43,
                "session.spawn",
                serde_json::json!({"id": 43}),
            )
            .await
        });
        let guard = AbortOnDrop(task.abort_handle());
        tokio::time::timeout(Duration::from_secs(1), async {
            while !pending.lock().unwrap().contains_key(&43) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        drop(guard);
        assert!(task.await.unwrap_err().is_cancelled());
        tokio::time::timeout(Duration::from_secs(1), observe_writer.failed())
            .await
            .unwrap();
        assert!(pending.lock().unwrap().is_empty());
    }
}
