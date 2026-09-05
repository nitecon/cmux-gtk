//! Synchronous Unix socket JSON-RPC client for the cmux CLI.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

/// Errors from CLI socket operations.
#[derive(Debug)]
pub enum CliError {
    /// Could not connect to the socket.
    ConnectionError(String),
    /// The server returned an error response.
    CommandError(String),
    /// Unexpected protocol-level error (malformed response, timeout, etc).
    ProtocolError(String),
}

impl std::fmt::Display for CliError {
    /// Render the underlying transport, protocol or application error for command-line output.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliError::ConnectionError(msg) => write!(f, "{}", msg),
            CliError::CommandError(msg) => write!(f, "{}", msg),
            CliError::ProtocolError(msg) => write!(f, "{}", msg),
        }
    }
}

/// A synchronous Unix socket JSON-RPC client.
pub struct SocketClient {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next_id: u64,
    usable: bool,
    timeout: Duration,
    last_trace_id: Option<uuid::Uuid>,
}

impl SocketClient {
    /// Connect to the cmux socket at the given path with the specified timeout.
    pub fn connect(path: &str, timeout: Duration) -> Result<Self, CliError> {
        let stream = UnixStream::connect(path)
            .map_err(|e| CliError::ConnectionError(format!("cannot connect to {}: {}", path, e)))?;
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|e| CliError::ConnectionError(format!("set_read_timeout: {}", e)))?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(|e| CliError::ConnectionError(format!("set_write_timeout: {}", e)))?;
        let writer = stream
            .try_clone()
            .map_err(|e| CliError::ConnectionError(format!("clone stream: {}", e)))?;
        Ok(Self {
            reader: BufReader::new(stream),
            writer,
            next_id: 1,
            usable: true,
            timeout,
            last_trace_id: None,
        })
    }

    /// Return the correlation ID of the most recent call, including failed calls.
    pub fn last_trace_id(&self) -> Option<uuid::Uuid> {
        self.last_trace_id
    }

    /// Send a JSON-RPC call and return the result value.
    ///
    /// On success (ok: true), returns the `result` field.
    /// On error (ok: false), returns `Err(CliError::CommandError(...))`.
    pub fn call(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, CliError> {
        if !self.usable {
            return Err(CliError::ProtocolError(
                "connection retired after a protocol failure".into(),
            ));
        }
        let result = self.exchange(method, params);
        if matches!(result, Err(CliError::ProtocolError(_))) {
            self.usable = false;
            let _ = self.writer.shutdown(std::net::Shutdown::Both);
        }
        result
    }

    /// Execute one exchange; the caller retires transport/protocol failures but preserves server errors.
    fn exchange(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, CliError> {
        let started = Instant::now();
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| CliError::ProtocolError("request ID space exhausted".into()))?;

        let trace_id = uuid::Uuid::new_v4();
        self.last_trace_id = Some(trace_id);
        let request = serde_json::json!({
            "trace_id": trace_id,
            "id": id,
            "method": method,
            "params": params,
        });

        let mut line = request.to_string();
        line.push('\n');

        write_request(
            &mut self.writer,
            line.as_bytes(),
            remaining_budget(started, self.timeout)?,
        )?;
        let response = read_response(
            &mut self.reader,
            remaining_budget(started, self.timeout)?,
            MAX_RESPONSE_BYTES,
        )?;
        decode_response(&response, id)
    }
}

/// Return the remaining operation budget without permitting zero-length socket timeouts.
fn remaining_budget(started: Instant, timeout: Duration) -> Result<Duration, CliError> {
    timeout
        .checked_sub(started.elapsed())
        .filter(|value| !value.is_zero())
        .ok_or_else(|| CliError::ProtocolError("exchange deadline exceeded".into()))
}

/// Write a complete request while reducing the socket timeout after each partial write.
fn write_request(
    stream: &mut UnixStream,
    mut bytes: &[u8],
    timeout: Duration,
) -> Result<(), CliError> {
    let started = Instant::now();
    while !bytes.is_empty() {
        stream
            .set_write_timeout(Some(remaining_budget(started, timeout)?))
            .map_err(|error| CliError::ProtocolError(format!("set request timeout: {error}")))?;
        match stream.write(bytes) {
            Ok(0) => {
                return Err(CliError::ProtocolError(
                    "server closed while writing request".into(),
                ))
            }
            Ok(count) => bytes = &bytes[count..],
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(CliError::ProtocolError(format!("write failed: {error}"))),
        }
    }
    Ok(())
}

/// Validate a numbered v2 envelope, distinguishing malformed replies from valid server errors.
fn decode_response(response: &[u8], id: u64) -> Result<serde_json::Value, CliError> {
    let resp: serde_json::Value = serde_json::from_slice(response)
        .map_err(|error| CliError::ProtocolError(format!("invalid JSON response: {error}")))?;
    if resp.get("id").and_then(serde_json::Value::as_u64) != Some(id) {
        return Err(CliError::ProtocolError(
            "response ID does not match request".into(),
        ));
    }
    match resp.get("ok").and_then(serde_json::Value::as_bool) {
        Some(true) => Ok(resp
            .get("result")
            .cloned()
            .unwrap_or(serde_json::Value::Null)),
        Some(false) => {
            let error = resp
                .get("error")
                .and_then(serde_json::Value::as_object)
                .ok_or_else(|| {
                    CliError::ProtocolError("failed response has no error object".into())
                })?;
            for key in ["code", "message"] {
                if error.get(key).is_some_and(|value| !value.is_string()) {
                    return Err(CliError::ProtocolError(format!(
                        "error {key} must be a string"
                    )));
                }
            }
            let message = error
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown error");
            Err(CliError::CommandError(message.to_owned()))
        }
        None => Err(CliError::ProtocolError(
            "response has no boolean ok field".into(),
        )),
    }
}

/// Read a newline-delimited response within one time/byte budget without overreading later replies.
fn read_response(
    reader: &mut BufReader<UnixStream>,
    timeout: Duration,
    limit: usize,
) -> Result<Vec<u8>, CliError> {
    let started = Instant::now();
    let mut response = Vec::new();
    loop {
        let remaining = remaining_budget(started, timeout)?;
        reader
            .get_ref()
            .set_read_timeout(Some(remaining))
            .map_err(|error| CliError::ProtocolError(format!("set response timeout: {error}")))?;
        let available = reader
            .fill_buf()
            .map_err(|error| CliError::ProtocolError(format!("read failed: {error}")))?;
        if available.is_empty() {
            return Err(CliError::ProtocolError(
                "server closed before response newline".into(),
            ));
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let count = newline.unwrap_or(available.len());
        if count > limit.saturating_sub(response.len()) {
            return Err(CliError::ProtocolError(
                "response exceeds byte limit".into(),
            ));
        }
        response.extend_from_slice(&available[..count]);
        reader.consume(count + usize::from(newline.is_some()));
        if newline.is_some() {
            return Ok(response);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Construct a client and peer socket without filesystem discovery for protocol behavior tests.
    fn client_pair() -> (SocketClient, UnixStream) {
        let (stream, peer) = UnixStream::pair().unwrap();
        let client = SocketClient {
            writer: stream.try_clone().unwrap(),
            reader: BufReader::new(stream),
            next_id: 1,
            usable: true,
            timeout: Duration::from_secs(1),
            last_trace_id: None,
        };
        (client, peer)
    }

    /// Malformed envelopes retire the connection before a later response can be mistaken for success.
    #[test]
    fn malformed_reply_retires_connection() {
        for reply in [
            r#"[]"#,
            r#"{"id":true,"ok":true}"#,
            r#"{"id":2,"ok":true}"#,
            r#"{"id":1,"ok":"true"}"#,
            r#"{"id":1,"ok":false,"error":[]}"#,
        ] {
            let (mut client, mut peer) = client_pair();
            writeln!(peer, "{reply}").unwrap();
            assert!(matches!(
                client.call("system.ping", serde_json::json!({})),
                Err(CliError::ProtocolError(_))
            ));
            let error = client
                .call("system.ping", serde_json::json!({}))
                .unwrap_err();
            assert!(error.to_string().contains("retired"));
        }
    }

    /// A valid server error preserves response framing for the next numbered request.
    #[test]
    fn server_error_preserves_connection() {
        let (mut client, mut peer) = client_pair();
        peer.write_all(b"{\"id\":1,\"ok\":false,\"error\":{\"message\":\"missing\"}}\n{\"id\":2,\"ok\":true,\"result\":{\"pong\":true}}\n").unwrap();
        assert!(matches!(
            client.call("missing", serde_json::json!({})),
            Err(CliError::CommandError(_))
        ));
        assert_eq!(
            client.call("system.ping", serde_json::json!({})).unwrap(),
            serde_json::json!({"pong": true})
        );
    }

    /// Write exact request bytes and time out when a live peer stops consuming a full socket.
    #[test]
    fn bounded_request_writes() {
        use std::io::Read;
        let (mut stream, mut peer) = UnixStream::pair().unwrap();
        write_request(&mut stream, b"request\n", Duration::from_secs(1)).unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let mut bytes = [0; 8];
        peer.read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"request\n");
        assert!(write_request(
            &mut stream,
            &vec![b'x'; 8 * 1024 * 1024],
            Duration::from_millis(30)
        )
        .is_err());
    }

    /// Preserve fragmented UTF-8 bytes and leave coalesced response lines for the next read.
    #[test]
    fn response_boundaries() {
        let (stream, mut peer) = UnixStream::pair().unwrap();
        peer.write_all("€\nnext\n".as_bytes()).unwrap();
        let mut reader = BufReader::with_capacity(1, stream);
        assert_eq!(
            read_response(&mut reader, Duration::from_secs(1), 3).unwrap(),
            "€".as_bytes()
        );
        assert_eq!(
            read_response(&mut reader, Duration::from_secs(1), 4).unwrap(),
            b"next"
        );
    }

    /// Reject oversized lines, missing delimiters and peers that never produce bytes.
    #[test]
    fn response_limits() {
        let (stream, mut peer) = UnixStream::pair().unwrap();
        peer.write_all(b"12345\n").unwrap();
        assert!(
            read_response(&mut BufReader::new(stream), Duration::from_secs(1), 4)
                .unwrap_err()
                .to_string()
                .contains("byte limit")
        );
        let (stream, peer) = UnixStream::pair().unwrap();
        assert!(read_response(&mut BufReader::new(stream), Duration::from_millis(30), 4).is_err());
        drop(peer);
        let (stream, mut peer) = UnixStream::pair().unwrap();
        peer.write_all(b"123").unwrap();
        drop(peer);
        assert!(
            read_response(&mut BufReader::new(stream), Duration::from_secs(1), 4)
                .unwrap_err()
                .to_string()
                .contains("newline")
        );
    }
}
