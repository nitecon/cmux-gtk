//! Bounded asynchronous browser command framing and admission.

use serde_json::Value;
use std::io;
use std::path::Path;
use std::time::Duration;

const MAX_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;
const EXCHANGE_CAPACITY: usize = 16;
static EXCHANGES: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(EXCHANGE_CAPACITY);
static REJECTED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Report admitted async exchanges and overload rejections without acquiring capacity or waiting.
pub(crate) fn snapshot() -> Value {
    serde_json::json!({
        "capacity": EXCHANGE_CAPACITY,
        "in_flight": EXCHANGE_CAPACITY - EXCHANGES.available_permits(),
        "rejected": REJECTED.load(std::sync::atomic::Ordering::Relaxed),
    })
}

/// Exchange asynchronously with an overall five-second deadline and at most sixteen active requests.
/// Reject excess work immediately rather than retaining an unbounded queue of browser operations.
pub(super) async fn request_async(path: &Path, request: &Value) -> Result<Value, String> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
    let _permit = EXCHANGES.try_acquire().map_err(|_| {
        REJECTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        "Browser command capacity reached".to_string()
    })?;
    let exchange = async {
        let mut stream = tokio::net::UnixStream::connect(path).await?;
        let mut payload = serde_json::to_vec(request)?;
        payload.push(b'\n');
        stream.write_all(&payload).await?;
        let mut response = Vec::new();
        tokio::io::BufReader::new(stream)
            .take(MAX_RESPONSE_BYTES + 1)
            .read_until(b'\n', &mut response)
            .await?;
        parse_response(&response)
    };
    tokio::time::timeout(Duration::from_secs(5), exchange)
        .await
        .map_err(|_| "Browser command exceeded five-second deadline".to_string())?
        .map_err(|error| format!("Browser daemon exchange failed: {error}"))
}

/// Validate response size and JSON before exposing data to the caller.
fn parse_response(response: &[u8]) -> io::Result<Value> {
    if response.len() as u64 > MAX_RESPONSE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "browser response exceeds 4 MiB",
        ));
    }
    serde_json::from_slice(response).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Aborting an in-flight exchange closes the real socket instead of retaining a waiting peer.
    #[tokio::test]
    async fn cancelled_exchange_closes_socket() {
        use tokio::io::{AsyncBufReadExt, AsyncReadExt};
        let path = std::env::temp_dir().join(format!("cmux-cancel-{}.sock", uuid::Uuid::new_v4()));
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let client_path = path.clone();
        let client = tokio::spawn(async move {
            request_async(&client_path, &serde_json::json!({"action": "ping"})).await
        });
        let (peer, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
            .await
            .unwrap()
            .unwrap();
        let _ = std::fs::remove_file(path);
        let mut peer = tokio::io::BufReader::new(peer);
        let mut command = String::new();
        tokio::time::timeout(Duration::from_secs(5), peer.read_line(&mut command))
            .await
            .unwrap()
            .unwrap();
        client.abort();
        assert!(client.await.unwrap_err().is_cancelled());
        let mut byte = [0];
        let read = tokio::time::timeout(Duration::from_secs(5), peer.read(&mut byte))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(read, 0, "cancelled exchange retained its socket");
    }

    /// Exercise the async transport against a real Unix listener with a fragmented response.
    #[tokio::test]
    async fn async_response_roundtrip() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
        let path = std::env::temp_dir().join(format!("cmux-browser-{}.sock", uuid::Uuid::new_v4()));
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let server = tokio::spawn(async move {
            let (peer, _) = listener.accept().await.unwrap();
            let mut reader = tokio::io::BufReader::new(peer);
            let mut command = String::new();
            reader.read_line(&mut command).await.unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&command).unwrap()["action"],
                "ping"
            );
            let mut peer = reader.into_inner();
            peer.write_all(b"{\"success\":").await.unwrap();
            tokio::task::yield_now().await;
            peer.write_all(b"true}\n").await.unwrap();
        });
        let result = request_async(&path, &serde_json::json!({"action": "ping"})).await;
        let _ = std::fs::remove_file(path);
        assert_eq!(result.unwrap()["success"], true);
        server.await.unwrap();
    }

    /// A connected peer that never answers must return a socket timeout instead of blocking indefinitely.
    #[tokio::test]
    async fn silent_peer_times_out() {
        let path = std::env::temp_dir().join(format!("cmux-silent-{}.sock", uuid::Uuid::new_v4()));
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let payload = serde_json::json!({"action":"ping"});
        let request = request_async(&path, &payload);
        let server = async {
            let (peer, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(6)).await;
            drop(peer);
        };
        let (result, ()) = tokio::join!(request, server);
        assert!(result.unwrap_err().contains("five-second deadline"));
        std::fs::remove_file(path).unwrap();
    }

    /// Exercise actual socket framing and reject a peer response larger than the memory budget.
    #[tokio::test]
    async fn response_bounds() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
        for oversized in [false, true] {
            let path = std::env::temp_dir().join(format!(
                "cmux-response-bounds-{}.sock",
                uuid::Uuid::new_v4()
            ));
            let listener = tokio::net::UnixListener::bind(&path).unwrap();
            let server = tokio::spawn(async move {
                let (peer, _) = listener.accept().await.unwrap();
                let mut reader = tokio::io::BufReader::new(peer);
                let mut command = String::new();
                reader.read_line(&mut command).await.unwrap();
                assert_eq!(
                    serde_json::from_str::<Value>(&command).unwrap()["action"],
                    "ping"
                );
                if oversized {
                    let _ = reader
                        .get_mut()
                        .write_all(&vec![b'x'; MAX_RESPONSE_BYTES as usize + 1])
                        .await;
                } else {
                    reader
                        .get_mut()
                        .write_all(b"{\"success\":true}\n")
                        .await
                        .unwrap();
                }
            });
            let result = request_async(&path, &serde_json::json!({"action":"ping"})).await;
            if oversized {
                assert!(result.unwrap_err().contains("4 MiB"));
            } else {
                assert_eq!(result.unwrap()["success"], true);
            }
            tokio::time::timeout(Duration::from_secs(5), server)
                .await
                .unwrap()
                .unwrap();
            std::fs::remove_file(path).unwrap();
        }
    }
}
