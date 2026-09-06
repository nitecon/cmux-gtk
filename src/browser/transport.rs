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

/// Exchange with a five-second default, or an explicit wait timeout plus response margin; admit at most sixteen requests.
/// Reject excess work immediately rather than retaining an unbounded queue of browser operations.
pub(super) async fn request_async(
    path: &Path,
    request: &Value,
    parent_trace: Option<uuid::Uuid>,
) -> Result<Value, String> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
    let mut activity =
        parent_trace.map(|parent| super::metrics::Activity::child("daemon_exchange", parent));
    let _permit = EXCHANGES.try_acquire().map_err(|_| {
        if let Some(activity) = activity.as_mut() {
            activity.finish("overloaded");
        }
        REJECTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        "Browser command capacity reached".to_string()
    })?;
    if let Some(activity) = activity.as_ref() {
        let request_id = request
            .get("id")
            .and_then(Value::as_str)
            .and_then(|id| id.strip_prefix("cmux-"))
            .and_then(|id| uuid::Uuid::parse_str(id).ok())
            .map(|id| format!("cmux-{id}"));
        crate::diagnostics::record(
            "browser.transport.request",
            serde_json::json!({
                "trace_id": activity.id, "parent_trace_id": parent_trace, "request_id": request_id,
            }),
        );
    }
    let exchange = async {
        let mut stream = cmux_platform::local_socket::Stream::connect(path).await?;
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
    let timeout = if request.get("action").and_then(Value::as_str) == Some("wait") {
        request
            .get("timeout")
            .and_then(Value::as_u64)
            .map(|milliseconds| crate::browser_timeout::wait_budgets(milliseconds).0)
            .unwrap_or(Duration::from_secs(5))
    } else {
        Duration::from_secs(5)
    };
    let result = tokio::time::timeout(timeout, exchange).await;
    if let Some(activity) = activity.as_mut() {
        activity.finish(match &result {
            Ok(Ok(_)) => "success",
            Ok(Err(_)) => "error",
            Err(_) => "timeout",
        });
    }
    result
        .map_err(|_| "Browser command deadline exceeded".to_string())?
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
    let value: Value = serde_json::from_slice(response)?;
    if !value.is_object() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "browser response is not an object",
        ));
    }
    match value.get("success") {
        Some(Value::Bool(false)) => {
            return Err(io::Error::other("browser daemon rejected command"))
        }
        Some(Value::Bool(true)) | None => {}
        Some(_) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid browser success status",
            ))
        }
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Daemon failures and malformed status envelopes cannot become successful cmux responses.
    #[test]
    fn daemon_status_validation() {
        for response in [
            br#"{"success":false,"error":"private page detail"}"#.as_slice(),
            br#"{"success":"true"}"#.as_slice(),
            b"[]".as_slice(),
        ] {
            let error = parse_response(response).unwrap_err();
            assert!(!error.to_string().contains("private page detail"));
        }
        assert!(parse_response(br#"{"success":true,"data":{}}"#).is_ok());
        assert!(parse_response(br#"{"legacy":"response"}"#).is_ok());
    }

    /// Aborting an in-flight exchange closes the real socket instead of retaining a waiting peer.
    #[tokio::test]
    async fn cancelled_exchange_closes_socket() {
        use tokio::io::{AsyncBufReadExt, AsyncReadExt};
        let path = std::env::temp_dir().join(format!("cmux-cancel-{}.sock", uuid::Uuid::new_v4()));
        let listener = cmux_platform::local_socket::Listener::bind(&path).unwrap();
        let client_path = path.clone();
        let client = tokio::spawn(async move {
            request_async(&client_path, &serde_json::json!({"action": "ping"}), None).await
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
        let listener = cmux_platform::local_socket::Listener::bind(&path).unwrap();
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
        let result = request_async(&path, &serde_json::json!({"action": "ping"}), None).await;
        let _ = std::fs::remove_file(path);
        assert_eq!(result.unwrap()["success"], true);
        server.await.unwrap();
    }

    /// A configured wait can complete after the ordinary command deadline on a real connection.
    #[tokio::test]
    async fn explicit_wait_outlives_default_transport_deadline() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
        let path = std::env::temp_dir().join(format!("cmux-wait-{}.sock", uuid::Uuid::new_v4()));
        let listener = cmux_platform::local_socket::Listener::bind(&path).unwrap();
        let payload = serde_json::json!({"action": "wait", "timeout": 12_000});
        let server = async {
            let (peer, _) = listener.accept().await.unwrap();
            let mut reader = tokio::io::BufReader::new(peer);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&line).unwrap()["timeout"],
                12_000
            );
            tokio::time::sleep(Duration::from_secs(6)).await;
            reader
                .get_mut()
                .write_all(b"{\"success\":true}\n")
                .await
                .unwrap();
        };
        let (result, ()) = tokio::time::timeout(Duration::from_secs(15), async {
            tokio::join!(request_async(&path, &payload, None), server)
        })
        .await
        .unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(result.unwrap()["success"], true);
    }

    /// A connected peer that never answers must return a socket timeout instead of blocking indefinitely.
    #[tokio::test]
    async fn silent_peer_times_out() {
        let path = std::env::temp_dir().join(format!("cmux-silent-{}.sock", uuid::Uuid::new_v4()));
        let listener = cmux_platform::local_socket::Listener::bind(&path).unwrap();
        let payload = serde_json::json!({"action":"ping"});
        let request = request_async(&path, &payload, None);
        let server = async {
            let (peer, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(6)).await;
            drop(peer);
        };
        let (result, ()) = tokio::join!(request, server);
        assert!(result.unwrap_err().contains("deadline exceeded"));
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
            let listener = cmux_platform::local_socket::Listener::bind(&path).unwrap();
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
            let result = request_async(&path, &serde_json::json!({"action":"ping"}), None).await;
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
