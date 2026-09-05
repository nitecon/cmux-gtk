//! Process-local preview counters; sampling never touches GTK widgets.
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

static ACTIVE: AtomicU64 = AtomicU64::new(0);
static RECEIVED: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
static PRESENTED: AtomicU64 = AtomicU64::new(0);
static BASE64_ERRORS: AtomicU64 = AtomicU64::new(0);
static DECODE_OVERLOAD: AtomicU64 = AtomicU64::new(0);
static DECODE_US: AtomicU64 = AtomicU64::new(0);
static DECODE_COUNT: AtomicU64 = AtomicU64::new(0);
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

/// Count frames skipped when both blocking decoder slots are occupied.
pub(super) fn decode_overload() {
    DECODE_OVERLOAD.fetch_add(1, Relaxed);
}

/// Aggregate actual CPU decode completion and failures without per-frame log traffic.
pub(super) fn decoded(duration: std::time::Duration, success: bool) {
    DECODE_COUNT.fetch_add(1, Relaxed);
    DECODE_US.fetch_add(duration.as_micros().min(u64::MAX as u128) as u64, Relaxed);
    if !success {
        TEXTURE_ERRORS.fetch_add(1, Relaxed);
    }
}

/// Read independently sampled cumulative counters without blocking the browser or GTK thread.
pub(crate) fn snapshot() -> serde_json::Value {
    serde_json::json!({
        "decode_overload_drops": DECODE_OVERLOAD.load(Relaxed),
        "decode_attempts": DECODE_COUNT.load(Relaxed),
        "decode_total_us": DECODE_US.load(Relaxed),
        "active_stream_tasks": ACTIVE.load(Relaxed),
        "jpeg_payloads_received": RECEIVED.load(Relaxed),
        "jpeg_bytes_received": BYTES.load(Relaxed),
        "textures_assigned": PRESENTED.load(Relaxed),
        "base64_errors": BASE64_ERRORS.load(Relaxed),
        "texture_errors": TEXTURE_ERRORS.load(Relaxed),
    })
}

/// Correlate a browser navigation or CLI stage without retaining URLs, arguments or output.
/// Unfinished drops report cancellation, including tasks aborted before their next poll.
pub(crate) struct Activity {
    pub(crate) id: uuid::Uuid,
    stage: &'static str,
    started: std::time::Instant,
    outcome: &'static str,
}

impl Activity {
    /// Begin a fixed-name stage, sharing its parent navigation ID when supplied.
    pub(crate) fn begin(stage: &'static str, parent: Option<uuid::Uuid>) -> Self {
        let id = parent.unwrap_or_else(uuid::Uuid::new_v4);
        crate::diagnostics::record(
            "browser.activity.begin",
            serde_json::json!({
                "trace_id": id, "stage": stage,
            }),
        );
        Self {
            id,
            stage,
            started: std::time::Instant::now(),
            outcome: "cancelled",
        }
    }

    /// Record a fixed outcome category for the completion emitted when this stage drops.
    pub(crate) fn finish(&mut self, outcome: &'static str) {
        self.outcome = outcome;
    }
}

impl Drop for Activity {
    /// Emit elapsed time on success, error, early return or cancellation through the bounded writer.
    fn drop(&mut self) {
        crate::diagnostics::record(
            "browser.activity.complete",
            serde_json::json!({
                "trace_id": self.id, "stage": self.stage, "outcome": self.outcome,
                "duration_us": self.started.elapsed().as_micros(),
            }),
        );
    }
}
