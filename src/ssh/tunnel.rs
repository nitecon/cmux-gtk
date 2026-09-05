use crate::ssh::bridge::SshBridge;
use crate::ssh::{SshEvent, SshEventTx};
use crate::workspace::ConnectionState;
use base64::Engine;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
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
type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<serde_json::Value>>>>;

/// Manage an SSH workspace connection lifecycle.
/// Runs as a tokio task. Reports state changes via ssh_tx.
pub async fn run_ssh_lifecycle(
    workspace_id: u64,
    target: String,
    ssh_tx: SshEventTx,
    bridge: Arc<SshBridge>,
) {
    let mut attempt: u32 = 0;
    let mut deployed = false;

    loop {
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
                    break;
                }

                let _ = ssh_tx
                    .send(SshEvent::StateChanged {
                        workspace_id,
                        state: ConnectionState::Disconnected,
                    })
                    .await;
                let backoff = backoff_duration(attempt);
                tokio::time::sleep(backoff).await;
                attempt += 1;
                if attempt >= MAX_RETRIES {
                    eprintln!("cmux: SSH deployment exhausted retries");
                    break;
                }
                continue;
            }
            deployed = true;
        }

        // Start SSH connection with cmuxd-remote in stdio mode
        match start_ssh(&target).await {
            Ok(mut child) => {
                let was_reconnect = attempt > 0;
                let mut connected = false;

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

                let stdin = child.stdin.take();
                let stdout = child.stdout.take();
                let stderr = child.stderr.take();

                // Log stderr from SSH/cmuxd-remote so errors aren't silently lost
                if let Some(err_reader) = stderr {
                    tokio::spawn(async move {
                        let mut buf = BufReader::new(err_reader);
                        let mut line = String::new();
                        loop {
                            line.clear();
                            match buf.read_line(&mut line).await {
                                Ok(0) => break,
                                Ok(_) => eprintln!("cmux: SSH stderr: {}", line.trim_end()),
                                Err(_) => break,
                            }
                        }
                    });
                }

                if let (Some(writer), Some(reader)) = (stdin, stdout) {
                    let mut buf_writer = BufWriter::new(writer);

                    // Send hello/handshake to verify cmuxd-remote is running
                    let hello =
                        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"hello","params":{}});
                    let hello_line = format!("{}\n", hello);
                    if let Err(e) = buf_writer.write_all(hello_line.as_bytes()).await {
                        eprintln!("cmux: SSH handshake write failed: {e}");
                    } else if let Err(e) = buf_writer.flush().await {
                        eprintln!("cmux: SSH handshake flush failed: {e}");
                    } else {
                        // Run bidirectional proxy routing
                        connected =
                            run_proxy_routing(workspace_id, buf_writer, reader, &bridge, &ssh_tx)
                                .await;
                    }
                }

                if connected {
                    attempt = 0;
                }
                // Wait for SSH process to exit
                let exit_status = child.wait().await;
                eprintln!("cmux: SSH to {target} exited: {exit_status:?}");

                // D-06: inject disconnect message into all active panes
                {
                    let ids: Vec<u64> = bridge.streams.lock().unwrap().keys().copied().collect();
                    for pane_id in ids {
                        let msg = b"\r\n\x1b[33m[SSH disconnected \xe2\x80\x94 reconnecting...]\x1b[0m\r\n";
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
            break;
        }

        // Exponential backoff before reconnect (per D-14)
        let backoff = backoff_duration(attempt);
        eprintln!(
            "cmux: SSH reconnecting to {target} in {}s (attempt {})",
            backoff.as_secs(),
            attempt + 1
        );
        tokio::time::sleep(backoff).await;
        attempt += 1;
    }
}

/// Bidirectional proxy routing between bridge channels and SSH stdin/stdout.
async fn run_proxy_routing(
    workspace_id: u64,
    buf_writer: BufWriter<tokio::process::ChildStdin>,
    reader: tokio::process::ChildStdout,
    bridge: &Arc<SshBridge>,
    ssh_tx: &SshEventTx,
) -> bool {
    let connected = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
    let writer = Arc::new(tokio::sync::Mutex::new(buf_writer));

    // Take (or recreate on reconnect) the write channel receiver from bridge
    let mut local_write_rx = bridge.take_or_recreate_write_rx();

    // Clear stale stream state from any prior connection
    bridge.clear_stream_ids();

    // Read path: parse JSON lines from SSH stdout.
    // MUST be spawned BEFORE open_remote_stream so RPC responses can be received.
    let read_bridge = bridge.clone();
    let read_ssh_tx = ssh_tx.clone();
    let read_pending = pending.clone();
    let read_connected = connected.clone();
    let read_handle = tokio::spawn(async move {
        let mut buf_reader = BufReader::new(reader);
        let mut line = String::new();
        loop {
            line.clear();
            match buf_reader.read_line(&mut line).await {
                Ok(0) => break, // EOF
                Ok(_) => {
                    if let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) {
                        if msg.get("id").and_then(|v| v.as_u64()) == Some(1)
                            && msg.get("ok").and_then(|v| v.as_bool()) == Some(true)
                        {
                            read_connected.store(true, std::sync::atomic::Ordering::Release);
                            eprintln!("cmux: SSH workspace={workspace_id} handshake complete");
                            let _ = read_ssh_tx
                                .send(SshEvent::StateChanged {
                                    workspace_id,
                                    state: ConnectionState::Connected,
                                })
                                .await;
                        }
                        handle_incoming_message(&msg, &read_bridge, &read_ssh_tx, &read_pending)
                            .await;
                    }
                }
                Err(e) => {
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
            let line = format!("{}\n", rpc);
            let mut w = write_writer.lock().await;
            if w.write_all(line.as_bytes()).await.is_err() {
                break;
            }
            if w.flush().await.is_err() {
                break;
            }
        }
    });

    let _write_guard = AbortOnDrop(write_handle.abort_handle());

    // Wait for read path to finish (SSH connection closed)
    let _ = read_handle.await;
    // Cancel write path
    write_handle.abort();
    open_handle.abort();
    connected.load(std::sync::atomic::Ordering::Acquire)
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
pub async fn open_remote_stream(
    writer: &Arc<tokio::sync::Mutex<BufWriter<tokio::process::ChildStdin>>>,
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

    // Register oneshot for response
    let (resp_tx, resp_rx) = oneshot::channel();
    if let Ok(mut map) = pending.lock() {
        map.insert(spawn_id, resp_tx);
    }

    let _spawn_request = PendingRequest(pending.clone(), spawn_id);
    // Write RPC
    {
        let line = format!("{}\n", spawn_rpc);
        let mut w = writer.lock().await;
        w.write_all(line.as_bytes())
            .await
            .map_err(|e| format!("write session.spawn failed: {e}"))?;
        w.flush()
            .await
            .map_err(|e| format!("flush session.spawn failed: {e}"))?;
    }

    // Await response
    let resp = tokio::time::timeout(std::time::Duration::from_secs(15), resp_rx)
        .await
        .map_err(|_| "session.spawn timed out".to_string())?
        .map_err(|_| "session.spawn response channel dropped".to_string())?;

    let stream_id = resp
        .get("result")
        .and_then(|r| r.get("stream_id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
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
        let _ = bridge
            .write_tx
            .lock()
            .unwrap()
            .send(crate::ssh::bridge::WriteRequest {
                stream_id,
                data_base64: String::new(),
                close: true,
                resize: None,
            });
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

    let (sub_tx, sub_rx) = oneshot::channel();
    if let Ok(mut map) = pending.lock() {
        map.insert(sub_id, sub_tx);
    }

    let _subscribe_request = PendingRequest(pending.clone(), sub_id);
    {
        let line = format!("{}\n", sub_rpc);
        let mut w = writer.lock().await;
        w.write_all(line.as_bytes())
            .await
            .map_err(|e| format!("write proxy.stream.subscribe failed: {e}"))?;
        w.flush()
            .await
            .map_err(|e| format!("flush proxy.stream.subscribe failed: {e}"))?;
    }

    // Await subscribe response
    let _sub_resp = tokio::time::timeout(std::time::Duration::from_secs(15), sub_rx)
        .await
        .map_err(|_| "proxy.stream.subscribe timed out".to_string())?
        .map_err(|_| "proxy.stream.subscribe response channel dropped".to_string())?;

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

struct AbortOnDrop(tokio::task::AbortHandle);
impl Drop for AbortOnDrop {
    /// Cancel the companion task when its owning operation leaves scope.
    fn drop(&mut self) {
        self.0.abort();
    }
}

struct PendingRequest(PendingMap, u64);
impl Drop for PendingRequest {
    /// Remove an outstanding response slot on success, error or future cancellation.
    fn drop(&mut self) {
        self.0.lock().unwrap().remove(&self.1);
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
