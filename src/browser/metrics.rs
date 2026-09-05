//! Process-local preview counters; sampling never touches GTK widgets.
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

static ACTIVE: AtomicU64 = AtomicU64::new(0);
static RECEIVED: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
static PRESENTED: AtomicU64 = AtomicU64::new(0);
static BASE64_ERRORS: AtomicU64 = AtomicU64::new(0);
static TEXTURE_ERRORS: AtomicU64 = AtomicU64::new(0);

/// Track one running stream task, including connection setup, until exit or cancellation.
pub(super) struct Stream;
impl Stream {
    /// Count the task once execution starts; a never-polled task does not count as active.
    pub(super) fn begin() -> Self {
        ACTIVE.fetch_add(1, Relaxed);
        Self
    }
}
impl Drop for Stream {
    /// Balance stream ownership on normal exit and asynchronous cancellation.
    fn drop(&mut self) {
        ACTIVE.fetch_sub(1, Relaxed);
    }
}

/// Count a successfully decoded JPEG payload before latest-frame coalescing.
pub(super) fn received(bytes: usize) {
    RECEIVED.fetch_add(1, Relaxed);
    BYTES.fetch_add(bytes as u64, Relaxed);
}

/// Count rejected base64 data without retaining or logging the frame payload.
pub(super) fn invalid_base64() {
    BASE64_ERRORS.fetch_add(1, Relaxed);
}

/// Count texture assignment or decode failure; assignment does not prove compositor presentation.
pub(super) fn texture(success: bool) {
    if success {
        PRESENTED.fetch_add(1, Relaxed);
    } else {
        TEXTURE_ERRORS.fetch_add(1, Relaxed);
    }
}

/// Read independently sampled cumulative counters without blocking the browser or GTK thread.
pub(crate) fn snapshot() -> serde_json::Value {
    serde_json::json!({
        "active_stream_tasks": ACTIVE.load(Relaxed),
        "jpeg_payloads_received": RECEIVED.load(Relaxed),
        "jpeg_bytes_received": BYTES.load(Relaxed),
        "textures_assigned": PRESENTED.load(Relaxed),
        "base64_errors": BASE64_ERRORS.load(Relaxed),
        "texture_errors": TEXTURE_ERRORS.load(Relaxed),
    })
}
