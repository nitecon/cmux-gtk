//! Bounded request framing before JSON parsing or GTK dispatch.
use std::io;
use std::time::Duration;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt};

const MAX_REQUEST_BYTES: usize = 4 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Wait for a new request without an idle timeout, then bound its bytes and completion time.
pub(super) async fn next_request<R: AsyncBufRead + Unpin>(
    reader: &mut R,
) -> io::Result<Option<String>> {
    if reader.fill_buf().await?.is_empty() {
        return Ok(None);
    }
    read_started_request(reader, MAX_REQUEST_BYTES, REQUEST_TIMEOUT)
        .await
        .map(Some)
}

/// Assemble one started UTF-8 line, accepting CRLF and rejecting overflow or incomplete EOF.
async fn read_started_request<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    limit: usize,
    timeout: Duration,
) -> io::Result<String> {
    let mut bytes = Vec::new();
    let mut limited = reader.take(limit as u64 + 1);
    tokio::time::timeout(timeout, limited.read_until(b'\n', &mut bytes))
        .await
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "request completion deadline exceeded",
            )
        })??;
    if bytes.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "request exceeds byte limit",
        ));
    }
    if bytes.pop() != Some(b'\n') {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "request missing newline",
        ));
    }
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    String::from_utf8(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "request is not UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncWriteExt, BufReader};

    /// Keep fragmented UTF-8 and coalesced CRLF requests intact at the exact byte boundary.
    #[tokio::test]
    async fn request_boundaries() {
        let (reader, mut writer) = tokio::net::UnixStream::pair().unwrap();
        writer.write_all("€\r\nnext\n".as_bytes()).await.unwrap();
        let mut reader = BufReader::with_capacity(1, reader);
        assert_eq!(
            read_started_request(&mut reader, 5, Duration::from_secs(1))
                .await
                .unwrap(),
            "€"
        );
        assert_eq!(next_request(&mut reader).await.unwrap().unwrap(), "next");
        drop(writer);
        assert!(next_request(&mut reader).await.unwrap().is_none());
    }

    /// A partially sent request times out, and oversized or truncated requests never reach dispatch.
    #[tokio::test]
    async fn request_limits() {
        let (reader, mut writer) = tokio::net::UnixStream::pair().unwrap();
        writer.write_all(b"x").await.unwrap();
        let mut reader = BufReader::new(reader);
        assert_eq!(
            read_started_request(&mut reader, 4, Duration::from_millis(30))
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        writer.write_all(b"12345").await.unwrap();
        assert_eq!(
            read_started_request(&mut reader, 4, Duration::from_secs(1))
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        writer.write_all(b"12").await.unwrap();
        drop(writer);
        assert_eq!(
            next_request(&mut reader).await.unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }
}
