//! Shared bounded UTF-8 line framing for local requests and SSH daemon responses.
use std::io;
use std::time::Duration;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt};

/// Wait without an idle deadline, then bound one started line; errors require retiring the reader.
pub(crate) async fn next_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    limit: usize,
    timeout: Duration,
) -> io::Result<Option<String>> {
    if reader.fill_buf().await?.is_empty() {
        return Ok(None);
    }
    read_started_line(reader, limit, timeout).await.map(Some)
}

/// Assemble one started UTF-8 line, accepting CRLF and rejecting overflow or incomplete EOF.
pub(crate) async fn read_started_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    limit: usize,
    timeout: Duration,
) -> io::Result<String> {
    let mut bytes = Vec::new();
    let mut limited = reader.take(limit as u64 + 1);
    tokio::time::timeout(timeout, limited.read_until(b'\n', &mut bytes))
        .await
        .map_err(|_| {
            io::Error::new(io::ErrorKind::TimedOut, "line completion deadline exceeded")
        })??;
    if bytes.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "line exceeds byte limit",
        ));
    }
    if bytes.pop() != Some(b'\n') {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "line missing newline",
        ));
    }
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    String::from_utf8(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "line is not UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncWriteExt, BufReader};

    /// Idle connections remain open past the frame budget, which starts only with incoming bytes.
    #[tokio::test]
    async fn idle_then_fragmented_frame() {
        let (reader, mut writer) = tokio::io::duplex(32);
        let mut reader = BufReader::with_capacity(1, reader);
        let read = next_line(&mut reader, 8, Duration::from_millis(30));
        let send = async {
            tokio::time::sleep(Duration::from_millis(60)).await;
            writer.write_all("λ\r\n".as_bytes()).await.unwrap();
        };
        let (line, ()) = tokio::join!(read, send);
        assert_eq!(line.unwrap().as_deref(), Some("λ"));
    }

    /// Unterminated floods, invalid UTF-8 and truncated EOF reject before JSON parsing.
    #[tokio::test]
    async fn rejects_invalid_frames() {
        for (bytes, limit, kind) in [
            (&b"123456789"[..], 8, io::ErrorKind::InvalidData),
            (&b"\xff\n"[..], 8, io::ErrorKind::InvalidData),
            (&b"short"[..], 8, io::ErrorKind::UnexpectedEof),
        ] {
            let mut reader = BufReader::new(bytes);
            assert_eq!(
                next_line(&mut reader, limit, Duration::from_secs(1))
                    .await
                    .unwrap_err()
                    .kind(),
                kind
            );
        }
    }
}
