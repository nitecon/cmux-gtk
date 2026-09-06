//! Serialized SSH JSONL writes with terminal connection failure on partial I/O or cancellation.
use std::{io, time::Duration};
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::sync::{watch, Mutex};

/// Own one connection writer and a persistent failure signal observed by its routing owner.
pub(super) struct RpcWriter<W> {
    writer: Mutex<W>,
    failed: watch::Sender<bool>,
    workspace_id: u64,
    connection_id: uuid::Uuid,
}

impl<W: AsyncWrite + Unpin> RpcWriter<W> {
    /// Identify the owning workspace for metadata-only request correlation.
    pub(super) fn workspace_id(&self) -> u64 {
        self.workspace_id
    }

    /// Start a usable connection writer; the parent routing scope owns its lifetime.
    pub(super) fn new(writer: W, workspace_id: u64, connection_id: uuid::Uuid) -> Self {
        Self {
            writer: Mutex::new(writer),
            failed: watch::channel(false).0,
            workspace_id,
            connection_id,
        }
    }

    /// Return the parent connection identity shared by setup request lifetimes.
    pub(super) fn connection_id(&self) -> uuid::Uuid {
        self.connection_id
    }

    /// Serialize within four MiB and allow ten seconds for lock admission, write and flush together.
    /// Failed or cancelled admitted writes retire the whole connection to prevent frame interleaving.
    pub(super) async fn send(&self, value: &serde_json::Value) -> io::Result<()> {
        self.send_with_timeout(value, Duration::from_secs(10)).await
    }

    /// Apply one total I/O budget; encoding overflow returns before any bytes enter the transport.
    async fn send_with_timeout(
        &self,
        value: &serde_json::Value,
        timeout: Duration,
    ) -> io::Result<()> {
        let line = crate::bounded_json::json_line(value, 4 * 1024 * 1024).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "SSH request exceeds byte limit",
            )
        })?;
        let mut guard = RetireOnDrop {
            failed: &self.failed,
            workspace_id: self.workspace_id,
            reason: "cancelled",
            armed: true,
        };
        let result = tokio::time::timeout(timeout, async {
            let mut writer = self.writer.lock().await;
            if *self.failed.borrow() {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "SSH writer retired",
                ));
            }
            writer.write_all(&line).await?;
            writer.flush().await
        })
        .await
        .unwrap_or_else(|_| {
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "SSH write deadline exceeded",
            ))
        });
        guard.armed = result.is_err();
        guard.reason = if result
            .as_ref()
            .is_err_and(|error| error.kind() == io::ErrorKind::TimedOut)
        {
            "timeout"
        } else {
            "io_error"
        };
        result
    }

    /// Wait for persistent retirement, including failures that occurred before this observer started.
    pub(super) async fn failed(&self) {
        let mut receiver = self.failed.subscribe();
        let _ = receiver.wait_for(|failed| *failed).await;
    }

    /// Retire a setup request whose remote side effects cannot be established from its reply.
    pub(super) fn retire_unanswered_request(&self) {
        drop(RetireOnDrop {
            failed: &self.failed,
            workspace_id: self.workspace_id,
            reason: "unanswered_request",
            armed: true,
        });
    }
}

/// Mark cancellation as transport failure even when a write future is dropped midway through a frame.
struct RetireOnDrop<'a> {
    failed: &'a watch::Sender<bool>,
    workspace_id: u64,
    reason: &'static str,
    armed: bool,
}

impl Drop for RetireOnDrop<'_> {
    /// Publish the first retirement without request contents; later senders observe the persistent state.
    fn drop(&mut self) {
        if self.armed && !self.failed.send_replace(true) {
            crate::diagnostics::record(
                "ssh.write.retired",
                serde_json::json!({
                    "workspace_id": self.workspace_id, "reason": self.reason,
                }),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

    /// Concurrent callers produce complete JSON lines rather than interleaved fragments.
    #[tokio::test]
    async fn serializes_complete_requests() {
        let (pipe, reader) = tokio::io::duplex(1024);
        let writer = RpcWriter::new(pipe, 0, uuid::Uuid::new_v4());
        let a = serde_json::json!({"id": 1, "text": "λ\n"});
        let b = serde_json::json!({"id": 2});
        let (first, second) = tokio::join!(writer.send(&a), writer.send(&b));
        first.unwrap();
        second.unwrap();
        drop(writer);
        let mut lines = BufReader::new(reader).lines();
        let mut ids = vec![];
        while let Some(line) = lines.next_line().await.unwrap() {
            let value: serde_json::Value = serde_json::from_str(&line).unwrap();
            ids.push(value["id"].as_u64().unwrap());
        }
        ids.sort();
        assert_eq!(ids, [1, 2]);
    }

    /// Timeout after partial output permanently retires the writer and wakes even a late observer.
    #[tokio::test]
    async fn timeout_retires_partial_frame() {
        let (pipe, mut reader) = tokio::io::duplex(8);
        let writer = RpcWriter::new(pipe, 0, uuid::Uuid::new_v4());
        let value = serde_json::json!({"data": "x".repeat(128)});
        assert_eq!(
            writer
                .send_with_timeout(&value, Duration::from_millis(30))
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        tokio::time::timeout(Duration::from_secs(1), writer.failed())
            .await
            .unwrap();
        assert_eq!(
            writer
                .send(&serde_json::json!({}))
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::BrokenPipe
        );
        drop(writer);
        let mut bytes = vec![];
        reader.read_to_end(&mut bytes).await.unwrap();
        assert_eq!(bytes.len(), 8);
    }

    /// Aborting a future after actual pipe delivery poisons subsequent writes without keeping a pending task.
    #[tokio::test]
    async fn cancellation_retires_partial_frame() {
        let (pipe, mut reader) = tokio::io::duplex(8);
        let writer = Arc::new(RpcWriter::new(pipe, 0, uuid::Uuid::new_v4()));
        let owned = writer.clone();
        let task = tokio::spawn(async move {
            owned
                .send(&serde_json::json!({"data": "x".repeat(128)}))
                .await
        });
        let mut first = [0; 8];
        tokio::time::timeout(Duration::from_secs(1), reader.read_exact(&mut first))
            .await
            .unwrap()
            .unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        tokio::time::timeout(Duration::from_secs(1), writer.failed())
            .await
            .unwrap();
        assert_eq!(
            writer
                .send(&serde_json::json!({}))
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::BrokenPipe
        );
    }

    /// Lock waiting consumes the same deadline as transport I/O; oversized local input sends no bytes.
    #[tokio::test]
    async fn bounds_admission_and_encoding() {
        let (pipe, _reader) = tokio::io::duplex(16);
        let writer = RpcWriter::new(pipe, 0, uuid::Uuid::new_v4());
        assert_eq!(
            writer
                .send(&serde_json::json!("x".repeat(4 * 1024 * 1024)))
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(!*writer.failed.borrow());
        let _lock = writer.writer.lock().await;
        assert_eq!(
            writer
                .send_with_timeout(&serde_json::json!({}), Duration::from_millis(30))
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        assert!(*writer.failed.borrow());
    }
}
