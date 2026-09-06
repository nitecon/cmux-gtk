//! Validate the deployed daemon before starting terminal requests on an SSH connection.
use super::writer::RpcWriter;
use std::{io, time::Duration};
use tokio::io::{AsyncBufRead, AsyncWrite};

/// Features negotiated for this connection only; old terminal-capable peers remain usable.
#[derive(Debug, Clone, Copy)]
pub(super) struct Negotiated {
    pub handler_duration_us: Option<u64>,
    pub ports: bool,
    pub forwarding: bool,
}

/// Bound hello write and response together; retain buffered trailing bytes for subsequent routing.
/// Require the expected reply identity, daemon name and terminal capabilities. On any error,
/// the caller must retire the connection rather than reuse potentially consumed protocol bytes.
pub(super) async fn establish<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin>(
    writer: &RpcWriter<W>,
    reader: &mut R,
    timeout: Duration,
) -> io::Result<Negotiated> {
    tokio::time::timeout(timeout, async {
        writer
            .send(
                &serde_json::json!({"jsonrpc":"2.0", "id":1, "method":"hello", "params":{},
                "trace_id": writer.connection_id()}),
            )
            .await?;
        let line = crate::line_reader::next_line(reader, 4 * 1024 * 1024, timeout)
            .await?
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::UnexpectedEof, "SSH hello response missing")
            })?;
        let reply: serde_json::Value = serde_json::from_str(&line)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid SSH hello JSON"))?;
        if reply.get("id").and_then(|id| id.as_u64()) != Some(1)
            || reply.get("ok").and_then(|ok| ok.as_bool()) != Some(true)
            || reply.pointer("/result/name").and_then(|name| name.as_str()) != Some("cmuxd-remote")
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "SSH hello identity or result invalid",
            ));
        }
        let capabilities = reply
            .pointer("/result/capabilities")
            .and_then(|value| value.as_array())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "SSH hello capabilities missing")
            })?;
        for required in ["session.spawn", "proxy.stream", "proxy.stream.push"] {
            if !capabilities
                .iter()
                .any(|value| value.as_str() == Some(required))
            {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "SSH daemon lacks terminal capabilities",
                ));
            }
        }
        let supports = |name| {
            capabilities
                .iter()
                .any(|value| value.as_str() == Some(name))
        };
        let ports = supports("ports.list");
        Ok(Negotiated {
            handler_duration_us: super::metrics::remote_timing(&reply, writer.connection_id())
                .map_err(|message| io::Error::new(io::ErrorKind::InvalidData, message))?,
            ports,
            forwarding: ports
                && supports("proxy.shutdown_write")
                && supports("proxy.stream.half_close"),
        })
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "SSH handshake deadline exceeded"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    /// Build the minimum supported hello envelope; unrelated version fields are optional.
    fn valid_reply() -> serde_json::Value {
        serde_json::json!({"id": 1, "ok": true, "result": {
            "name": "cmuxd-remote", "capabilities": ["session.spawn", "proxy.stream", "proxy.stream.push"]
        }})
    }

    /// Verify the request on a real duplex transport and preserve prefetched data after hello.
    #[tokio::test]
    async fn exchanges_hello_without_losing_following_line() {
        let (client, peer) = tokio::io::duplex(4096);
        let (reader, pipe) = tokio::io::split(client);
        let mut reader = BufReader::new(reader);
        let writer = RpcWriter::new(pipe, 0, uuid::Uuid::new_v4());
        let serve = async {
            let mut peer = BufReader::new(peer);
            let mut line = String::new();
            peer.read_line(&mut line).await.unwrap();
            let request: serde_json::Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["id"], 1);
            assert_eq!(request["method"], "hello");
            assert_eq!(
                request["trace_id"],
                serde_json::json!(writer.connection_id())
            );
            let mut reply = valid_reply();
            reply["trace_id"] = request["trace_id"].clone();
            reply["handler_duration_us"] = serde_json::json!(3);
            let response = format!("{reply}\nfollowing\n");
            peer.get_mut().write_all(response.as_bytes()).await.unwrap();
        };
        let (result, ()) = tokio::join!(
            establish(&writer, &mut reader, Duration::from_secs(1)),
            serve
        );
        let negotiated = result.unwrap();
        assert_eq!(negotiated.handler_duration_us, Some(3));
        assert!(!negotiated.ports && !negotiated.forwarding);
        let mut following = String::new();
        reader.read_line(&mut following).await.unwrap();
        assert_eq!(following, "following\n");
    }

    /// Every connection negotiates optional features independently; partial support never forwards.
    #[tokio::test]
    async fn negotiates_optional_forwarding_per_connection() {
        for (extra, ports, forwarding) in [
            (
                vec![
                    "ports.list",
                    "proxy.shutdown_write",
                    "proxy.stream.half_close",
                ],
                true,
                true,
            ),
            (vec!["ports.list", "proxy.shutdown_write"], true, false),
            (vec!["ports.list", "proxy.stream.half_close"], true, false),
            (
                vec!["proxy.shutdown_write", "proxy.stream.half_close"],
                false,
                false,
            ),
            (vec![], false, false),
        ] {
            let mut reply = valid_reply();
            reply["result"]["capabilities"]
                .as_array_mut()
                .unwrap()
                .extend(extra.into_iter().map(serde_json::Value::from));
            let line = format!("{reply}\n");
            let mut reader = BufReader::new(line.as_bytes());
            let writer = RpcWriter::new(tokio::io::sink(), 0, uuid::Uuid::new_v4());
            let negotiated = establish(&writer, &mut reader, Duration::from_secs(1))
                .await
                .unwrap();
            assert_eq!(negotiated.ports, ports);
            assert_eq!(negotiated.forwarding, forwarding);
        }
    }

    /// Reject wrong IDs, failed replies, wrong daemon names and missing required capabilities.
    #[tokio::test]
    async fn rejects_incompatible_peers() {
        for (pointer, value, kind) in [
            ("/id", serde_json::json!(2), io::ErrorKind::InvalidData),
            ("/ok", serde_json::json!(false), io::ErrorKind::InvalidData),
            (
                "/result/name",
                serde_json::json!("other"),
                io::ErrorKind::InvalidData,
            ),
            (
                "/result/capabilities",
                serde_json::json!([]),
                io::ErrorKind::Unsupported,
            ),
            (
                "/result/capabilities",
                serde_json::json!(null),
                io::ErrorKind::InvalidData,
            ),
        ] {
            let mut reply = valid_reply();
            *reply.pointer_mut(pointer).unwrap() = value;
            let line = format!("{reply}\n");
            let mut reader = BufReader::new(line.as_bytes());
            let writer = RpcWriter::new(tokio::io::sink(), 0, uuid::Uuid::new_v4());
            assert_eq!(
                establish(&writer, &mut reader, Duration::from_secs(1))
                    .await
                    .unwrap_err()
                    .kind(),
                kind
            );
        }
    }

    /// Silent peers expire despite idle framing policy; malformed JSON and EOF fail immediately.
    #[tokio::test]
    async fn bounds_missing_or_malformed_hello() {
        let (pipe, _peer) = tokio::io::duplex(16);
        let mut reader = BufReader::new(pipe);
        let writer = RpcWriter::new(tokio::io::sink(), 0, uuid::Uuid::new_v4());
        assert_eq!(
            establish(&writer, &mut reader, Duration::from_millis(30))
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        for (bytes, kind) in [
            (&b"{bad}\n"[..], io::ErrorKind::InvalidData),
            (&b""[..], io::ErrorKind::UnexpectedEof),
        ] {
            let mut reader = BufReader::new(bytes);
            assert_eq!(
                establish(&writer, &mut reader, Duration::from_secs(1))
                    .await
                    .unwrap_err()
                    .kind(),
                kind
            );
        }
    }
}
