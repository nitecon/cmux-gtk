//! Synchronous browser command framing, isolated from GTK widget ownership.

use serde_json::Value;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

const MAX_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;

/// Connect and exchange one command with bounded response memory and socket I/O timeouts.
/// This function blocks its caller; connection establishment has no explicit deadline.
pub(super) fn request(path: &Path, request: &Value) -> Result<Value, String> {
    let stream = UnixStream::connect(path)
        .map_err(|error| format!("Failed to connect to daemon socket: {error}"))?;
    exchange(stream, request, Duration::from_secs(5))
        .map_err(|error| format!("Browser daemon exchange failed: {error}"))
}

/// Exchange asynchronously with an overall five-second deadline and at most sixteen active requests.
/// Reject excess work immediately rather than retaining an unbounded queue of browser operations.
pub(super) async fn request_async(path: &Path, request: &Value) -> Result<Value, String> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
    static EXCHANGES: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(16);
    let _permit = EXCHANGES
        .try_acquire()
        .map_err(|_| "Browser command capacity reached".to_string())?;
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
        if response.len() as u64 > MAX_RESPONSE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "browser response exceeds 4 MiB",
            ));
        }
        serde_json::from_slice(&response).map_err(io::Error::from)
    };
    tokio::time::timeout(Duration::from_secs(5), exchange)
        .await
        .map_err(|_| "Browser command exceeded five-second deadline".to_string())?
        .map_err(|error| format!("Browser daemon exchange failed: {error}"))
}

/// Write one JSON line and read one bounded response; timeout applies to individual socket I/O calls.
fn exchange(mut stream: UnixStream, request: &Value, timeout: Duration) -> io::Result<Value> {
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    serde_json::to_writer(&mut stream, request)?;
    stream.write_all(b"\n")?;
    let mut response = Vec::new();
    BufReader::new(stream)
        .take(MAX_RESPONSE_BYTES + 1)
        .read_until(b'\n', &mut response)?;
    if response.len() as u64 > MAX_RESPONSE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "browser response exceeds 4 MiB",
        ));
    }
    serde_json::from_slice(&response).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    #[test]
    fn silent_peer_times_out() {
        let (client, _peer) = UnixStream::pair().unwrap();
        let error = exchange(
            client,
            &serde_json::json!({"action": "ping"}),
            Duration::from_millis(30),
        )
        .unwrap_err();
        assert!(matches!(
            error.kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
        ));
    }

    /// Exercise actual socket framing and reject a peer response larger than the memory budget.
    #[test]
    fn response_bounds() {
        for oversized in [false, true] {
            let (client, peer) = UnixStream::pair().unwrap();
            let server = std::thread::spawn(move || {
                peer.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                peer.set_write_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut reader = BufReader::new(peer);
                let mut command = String::new();
                reader.read_line(&mut command).unwrap();
                assert_eq!(
                    serde_json::from_str::<Value>(&command).unwrap()["action"],
                    "ping"
                );
                let mut peer = reader.into_inner();
                if oversized {
                    let _ = peer.write_all(&vec![b'x'; MAX_RESPONSE_BYTES as usize + 1]);
                } else {
                    peer.write_all(b"{\"success\":true}\n").unwrap();
                }
            });
            let result = exchange(
                client,
                &serde_json::json!({"action": "ping"}),
                Duration::from_secs(5),
            );
            if oversized {
                assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
            } else {
                assert_eq!(result.unwrap()["success"], true);
            }
            server.join().unwrap();
        }
    }
}
