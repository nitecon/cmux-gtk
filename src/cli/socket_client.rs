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
        let id = self.next_id;
        self.next_id += 1;

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

        self.writer
            .write_all(line.as_bytes())
            .map_err(|e| CliError::ProtocolError(format!("write failed: {}", e)))?;

        let response = read_response(&mut self.reader, self.timeout, MAX_RESPONSE_BYTES)?;
        let resp: serde_json::Value = serde_json::from_slice(&response)
            .map_err(|e| CliError::ProtocolError(format!("invalid JSON response: {}", e)))?;

        let ok = resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
        if ok {
            Ok(resp
                .get("result")
                .cloned()
                .unwrap_or(serde_json::Value::Null))
        } else {
            let msg = resp
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("unknown error");
            Err(CliError::CommandError(msg.to_string()))
        }
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
        let remaining = timeout
            .checked_sub(started.elapsed())
            .filter(|value| !value.is_zero())
            .ok_or_else(|| CliError::ProtocolError("response deadline exceeded".into()))?;
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
