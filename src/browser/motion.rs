//! Coalesced pointer input without GTK or blocking-worker ownership.
use super::transport;
use std::path::PathBuf;
use std::time::Duration;

/// Forward the latest pointer position at most once per sixty milliseconds.
/// Dropping all senders ends the worker; each in-flight exchange has a one-second deadline.
pub fn spawn_motion_forwarder(
    runtime: &tokio::runtime::Handle,
    daemon_socket_path: PathBuf,
) -> tokio::sync::watch::Sender<(i64, i64)> {
    let (tx, mut rx) = tokio::sync::watch::channel((0i64, 0i64));
    runtime.spawn(async move {
        let interval = Duration::from_millis(60);
        let mut next_send = tokio::time::Instant::now();
        while rx.changed().await.is_ok() {
            tokio::time::sleep_until(next_send).await;
            if rx.has_changed().is_err() {
                break;
            }
            let (x, y) = *rx.borrow_and_update();
            let request = serde_json::json!({
                "id": "motion", "action": "input_mouse", "type": "mouseMoved", "x": x, "y": y,
            });
            let _ = tokio::time::timeout(
                Duration::from_secs(1),
                transport::request_async(&daemon_socket_path, &request, None),
            )
            .await;
            next_send = tokio::time::Instant::now() + interval;
        }
    });
    tx
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

    /// A single initial movement is delivered even when no later event arrives to wake the worker.
    #[tokio::test]
    async fn first_motion_is_delivered() {
        let path = std::env::temp_dir().join(format!("cmux-motion-{}.sock", uuid::Uuid::new_v4()));
        let listener = cmux_platform::local_socket::Listener::bind(&path).unwrap();
        let sender = spawn_motion_forwarder(&tokio::runtime::Handle::current(), path.clone());
        sender.send((42, 24)).unwrap();
        let accepted = tokio::time::timeout(Duration::from_secs(5), listener.accept()).await;
        let _ = std::fs::remove_file(path);
        let (peer, _) = accepted.unwrap().unwrap();
        let mut peer = tokio::io::BufReader::new(peer);
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(5), peer.read_line(&mut line))
            .await
            .unwrap()
            .unwrap();
        let request: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(request["x"], 42);
        assert_eq!(request["y"], 24);
        peer.get_mut().write_all(b"{}\n").await.unwrap();
        drop(sender);
    }
}
