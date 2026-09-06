//! Unix socket lifecycle, peer admission and framed connection ownership.
pub(crate) mod admission;
pub mod auth;
pub mod commands;
mod dispatch;
mod framing;
mod response;
use dispatch::dispatch_line;
pub mod handlers;

pub use cmux_platform::paths::socket_path;

/// Maximum commands waiting for GTK, independently of connection admission.
pub(crate) const COMMAND_CAPACITY: usize = 64;

/// Start the socket server:
/// 1. Creates $XDG_RUNTIME_DIR/cmux/ (mode 0700).
/// 2. Removes stale socket file from previous crash.
/// 3. Binds UnixListener, sets socket mode to 0600.
/// 4. Atomically replaces the private last-socket-path discovery marker.
/// 5. Spawns tokio accept loop.
///
/// The cmd_tx sender is used to dispatch SocketCommands to the GTK main thread
/// via the tokio::sync::mpsc bridge established in main.rs.
pub fn start_socket_server(
    runtime: &tokio::runtime::Handle,
    cmd_tx: tokio::sync::mpsc::Sender<commands::SocketCommand>,
) {
    let sock_path = socket_path();
    let dir = cmux_platform::paths::runtime_dir();

    // Create directory with restrictive permissions.
    if let Err(e) = cmux_platform::filesystem::create_private_directory(&dir) {
        eprintln!("cmux: socket dir create failed: {e}");
        return;
    }

    // Remove stale socket from previous run (ignore ENOENT).
    let _ = std::fs::remove_file(&sock_path);

    // Enter the tokio runtime context so UnixListener::bind can register with the reactor.
    // bind() is synchronous but requires an active reactor context.
    let _guard = runtime.enter();
    let listener = match cmux_platform::local_socket::Listener::bind(&sock_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("cmux: socket bind failed at {}: {e}", sock_path.display());
            return;
        }
    };

    // Set socket file mode to 0600 (owner read/write only).
    if let Err(e) = cmux_platform::filesystem::restrict_file_to_owner(&sock_path) {
        eprintln!("cmux: socket chmod failed: {e}");
        let _ = std::fs::remove_file(&sock_path);
        return;
    }

    // Write last-socket-path marker so cmux.py can discover the socket.
    if let Err(e) = cmux_platform::filesystem::atomic_write(
        &cmux_platform::paths::socket_marker_path(),
        sock_path.to_string_lossy().as_bytes(),
    ) {
        eprintln!("cmux: last-socket-path write failed: {e}");
    }

    eprintln!("cmux: socket server listening at {}", sock_path.display());

    // Spawn the accept loop in tokio.
    runtime.spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    // Validate peer UID before reading any data.
                    match auth::validate_peer_uid(&stream) {
                        Ok(true) => {
                            let Some(permit) = admission::admit() else {
                                continue;
                            };
                            let tx = cmd_tx.clone();
                            tokio::spawn(async move {
                                let _permit = permit;
                                handle_connection(stream, tx).await;
                            });
                        }
                        Ok(false) => {
                            eprintln!("cmux: socket connection rejected (UID mismatch)");
                        }
                        Err(e) => {
                            eprintln!("cmux: SO_PEERCRED check failed: {e}");
                        }
                    }
                }
                Err(e) => {
                    eprintln!("cmux: socket accept error: {e}");
                    break;
                }
            }
        }
    });
}

/// Per-connection handler running in a tokio task.
/// Reads newline-delimited JSON requests, dispatches via mpsc channel, writes responses.
async fn handle_connection(
    stream: cmux_platform::local_socket::Stream,
    cmd_tx: tokio::sync::mpsc::Sender<commands::SocketCommand>,
) {
    use tokio::io::BufReader;

    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    loop {
        let line = match framing::next_request(&mut reader).await {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(error) => {
                crate::diagnostics::record(
                    "rpc.framing.rejected",
                    serde_json::json!({"error_kind": format!("{:?}", error.kind())}),
                );
                break;
            }
        };
        let response = tokio::select! {
            biased;
            // Preserve admission of a complete request even if its sender has already closed.
            response = dispatch_line(line, &cmd_tx) => response,
            closed = wait_for_disconnect(writer.as_ref()) => {
                crate::diagnostics::record("rpc.connection.abandoned", serde_json::json!({
                    "monitor_error": closed.err().map(|error| format!("{:?}", error.kind())),
                }));
                break;
            }
        };
        if let Err(error) = framing::write_response(&mut writer, &response.body).await {
            crate::diagnostics::record(
                "rpc.response.failed",
                serde_json::json!({
                    "error_kind": format!("{:?}", error.kind()),
                    "response_bytes": response.body.len(), "trace_id": response.trace_id
                }),
            );
            break;
        }
    }
}

/// Monitor only an outstanding dispatch; preserve half-closed and pipelined clients without reading ahead.
/// A coarse timer avoids spinning on cached EOF/read readiness while keeping per-connection work bounded.
async fn wait_for_disconnect(stream: &cmux_platform::local_socket::Stream) -> std::io::Result<()> {
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        interval.tick().await;
        match cmux_platform::peer::disconnected(stream) {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod connection_tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

    /// Full disconnect drops the dispatcher reply receiver even when GTK has not answered.
    #[tokio::test]
    async fn disconnected_client_cancels_dispatch() {
        let (mut client, server) = cmux_platform::local_socket::Stream::pair().unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let connection = tokio::spawn(handle_connection(server, tx));
        client
            .write_all(b"{\"id\":1,\"method\":\"system.ping\"}\n")
            .await
            .unwrap();
        let commands::SocketCommand::Observed { command, .. } = rx.recv().await.unwrap() else {
            panic!("unobserved request");
        };
        let commands::SocketCommand::Ping { mut resp_tx, .. } = *command else {
            panic!("unexpected command");
        };
        drop(client);
        tokio::time::timeout(std::time::Duration::from_secs(2), resp_tx.closed())
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), connection)
            .await
            .unwrap()
            .unwrap();
    }

    /// A complete request from a sender that immediately closes still reaches command admission.
    #[tokio::test]
    async fn complete_request_is_admitted_before_disconnect() {
        let (mut client, server) = cmux_platform::local_socket::Stream::pair().unwrap();
        client
            .write_all(b"{\"id\":1,\"method\":\"system.ping\"}\n")
            .await
            .unwrap();
        drop(client);
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let connection = tokio::spawn(handle_connection(server, tx));
        assert!(matches!(
            tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
                .await
                .unwrap(),
            Some(commands::SocketCommand::Observed { .. })
        ));
        connection.await.unwrap();
    }

    /// A pipelined client may finish writing and still receive both ordered replies after monitor ticks.
    #[tokio::test]
    async fn half_closed_client_receives_pipelined_responses() {
        let (mut client, server) = cmux_platform::local_socket::Stream::pair().unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::channel(2);
        let connection = tokio::spawn(handle_connection(server, tx));
        client
            .write_all(
                b"{\"id\":1,\"method\":\"system.ping\"}\n{\"id\":2,\"method\":\"system.ping\"}\n",
            )
            .await
            .unwrap();
        client.shutdown().await.unwrap();
        let mut client = tokio::io::BufReader::new(client);
        for id in [1, 2] {
            let commands::SocketCommand::Observed { command, .. } = rx.recv().await.unwrap() else {
                panic!("unobserved request");
            };
            let commands::SocketCommand::Ping { req_id, resp_tx } = *command else {
                panic!("unexpected command");
            };
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            resp_tx
                .send(response::ok(req_id, serde_json::json!({"pong":true})))
                .unwrap();
            let mut line = String::new();
            tokio::time::timeout(
                std::time::Duration::from_secs(2),
                client.read_line(&mut line),
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&line).unwrap()["id"],
                id
            );
        }
        connection.await.unwrap();
    }
}
