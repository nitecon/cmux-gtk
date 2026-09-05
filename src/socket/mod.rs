//! Unix socket lifecycle, peer admission and framed connection ownership.
pub(crate) mod admission;
pub mod auth;
pub mod commands;
mod dispatch;
mod framing;
mod response;
use dispatch::dispatch_line;
pub mod handlers;

use std::path::PathBuf;

/// Maximum commands waiting for GTK, independently of connection admission.
pub(crate) const COMMAND_CAPACITY: usize = 64;

/// Compute the Unix socket path per D-06.
/// $XDG_RUNTIME_DIR/cmux/cmux.sock, fallback /run/user/{uid}/cmux/cmux.sock.
pub fn socket_path() -> PathBuf {
    cmux_platform::paths::socket_path()
}

/// Returns the directory containing the socket file.
fn socket_dir() -> PathBuf {
    socket_path().parent().unwrap().to_path_buf()
}

/// Returns the last-socket-path marker file path.
fn last_socket_path_marker() -> PathBuf {
    socket_dir().join("last-socket-path")
}

/// Start the socket server:
/// 1. Creates $XDG_RUNTIME_DIR/cmux/ (mode 0700).
/// 2. Removes stale socket file from previous crash.
/// 3. Binds UnixListener, sets socket mode to 0600.
/// 4. Writes last-socket-path marker for cmux.py discovery.
/// 5. Spawns tokio accept loop.
///
/// The cmd_tx sender is used to dispatch SocketCommands to the GTK main thread
/// via the tokio::sync::mpsc bridge established in main.rs.
pub fn start_socket_server(
    runtime: &tokio::runtime::Handle,
    cmd_tx: tokio::sync::mpsc::Sender<commands::SocketCommand>,
) {
    let sock_path = socket_path();
    let dir = socket_dir();

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
    let listener = match tokio::net::UnixListener::bind(&sock_path) {
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
    if let Err(e) = std::fs::write(
        last_socket_path_marker(),
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
    stream: tokio::net::UnixStream,
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
        let response = dispatch_line(line, &cmd_tx).await;
        if let Err(error) = framing::write_response(&mut writer, &response).await {
            crate::diagnostics::record(
                "rpc.response.failed",
                serde_json::json!({
                    "error_kind": format!("{:?}", error.kind()), "response_bytes": response.len()
                }),
            );
            break;
        }
    }
}
