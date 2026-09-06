//! Bounded nonblocking admission from GTK callbacks into one SSH connection.
use super::bridge::WriteRequest;
use base64::Engine;
use tokio::sync::{mpsc, watch};

const CAPACITY: usize = 64;
const CHUNK_BYTES: usize = 16 * 1024;
/// Bound remote stream labels before duplicating them into queued frames.
pub(super) const MAX_STREAM_ID: usize = 256;
/// Receiver and persistent overload signal belong to the same connection generation.
pub(super) type Receiver = (mpsc::Receiver<WriteRequest>, watch::Receiver<bool>);

/// Admit bounded input without blocking GTK; overload permanently retires this queue generation.
pub(super) struct Outbound {
    sender: mpsc::Sender<WriteRequest>,
    failed: watch::Sender<bool>,
}

impl Outbound {
    /// Create an empty bounded queue and its failure observation channel.
    pub(super) fn new() -> (Self, Receiver) {
        let (sender, receiver) = mpsc::channel(CAPACITY);
        let (failed, observation) = watch::channel(false);
        (Self { sender, failed }, (receiver, observation))
    }

    /// Reserve every chunk before encoding any input; never enqueue only a prefix of a callback.
    pub(super) fn input(&self, stream_id: &str, bytes: &[u8]) {
        if bytes.is_empty() || *self.failed.borrow() {
            return;
        }
        let chunks = bytes.len().div_ceil(CHUNK_BYTES);
        if chunks > CAPACITY || stream_id.len() > MAX_STREAM_ID {
            self.fail();
            return;
        }
        let Ok(permits) = self.sender.try_reserve_many(chunks) else {
            self.fail();
            return;
        };
        for (permit, bytes) in permits.zip(bytes.chunks(CHUNK_BYTES)) {
            permit.send(WriteRequest {
                stream_id: stream_id.to_owned(),
                data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
                close: false,
                resize: None,
            });
        }
    }

    /// Admit a close/resize command in the same FIFO; unavailable capacity retires the connection.
    pub(super) fn control(&self, request: WriteRequest) {
        if *self.failed.borrow() {
            return;
        }
        if !request.data_base64.is_empty()
            || request.stream_id.len() > MAX_STREAM_ID
            || self.sender.try_send(request).is_err()
        {
            self.fail();
        }
    }

    /// Report overload once without terminal data; routing observes the persistent state and tears down.
    fn fail(&self) {
        if !self.failed.send_replace(true) {
            crate::diagnostics::record(
                "ssh.input.rejected",
                serde_json::json!({
                    "capacity": CAPACITY, "chunk_bytes": CHUNK_BYTES,
                }),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Accepted multi-chunk input round-trips exactly and fills no more than the configured slots.
    #[test]
    fn preserves_chunked_input_order() {
        let (queue, (mut receiver, failure)) = Outbound::new();
        let bytes: Vec<u8> = (0..CAPACITY * CHUNK_BYTES)
            .map(|index| (index % 251) as u8)
            .collect();
        queue.input("stream", &bytes);
        assert_eq!(receiver.len(), CAPACITY);
        assert!(!*failure.borrow());
        let mut delivered = Vec::new();
        while let Ok(request) = receiver.try_recv() {
            assert_eq!(request.stream_id, "stream");
            assert!(!request.close);
            let chunk = base64::engine::general_purpose::STANDARD
                .decode(request.data_base64)
                .unwrap();
            assert!(chunk.len() <= CHUNK_BYTES);
            delivered.extend(chunk);
        }
        assert_eq!(delivered, bytes);
    }

    /// Insufficient space rejects the entire callback and persists failure even after older data drains.
    #[test]
    fn rejects_without_partial_admission() {
        let (queue, (mut receiver, failure)) = Outbound::new();
        for _ in 0..CAPACITY - 1 {
            queue.control(WriteRequest {
                stream_id: "s".into(),
                data_base64: String::new(),
                close: false,
                resize: Some((80, 24)),
            });
        }
        queue.input("s", &vec![b'x'; CHUNK_BYTES + 1]);
        assert!(*failure.borrow());
        assert_eq!(receiver.len(), CAPACITY - 1);
        while let Ok(request) = receiver.try_recv() {
            assert!(request.resize.is_some());
        }
        queue.input("s", b"do not replay");
        assert!(receiver.try_recv().is_err());
    }

    /// Oversized callbacks/stream labels and unavailable receivers fail without queued allocations.
    #[test]
    fn validates_limits_and_closed_receiver() {
        for (stream, bytes) in [
            ("s".to_owned(), vec![0; CAPACITY * CHUNK_BYTES + 1]),
            ("s".repeat(MAX_STREAM_ID + 1), vec![0]),
        ] {
            let (queue, (receiver, failure)) = Outbound::new();
            queue.input(&stream, &bytes);
            assert_eq!(receiver.len(), 0);
            assert!(*failure.borrow());
        }
        let (queue, (receiver, failure)) = Outbound::new();
        drop(receiver);
        queue.input("s", b"x");
        assert!(*failure.borrow());
    }
}
