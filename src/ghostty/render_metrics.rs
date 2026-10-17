//! Content-free redraw scheduling and GTK draw timing, retained in constant space.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

static REQUESTS: AtomicU64 = AtomicU64::new(0);
static DRAWS: AtomicU64 = AtomicU64::new(0);
static DRAW_TOTAL_US: AtomicU64 = AtomicU64::new(0);
static DRAW_MAX_US: AtomicU64 = AtomicU64::new(0);

/// Count a native redraw action before GTK coalesces it into a frame.
pub(super) fn requested() {
    REQUESTS.fetch_add(1, Ordering::Relaxed);
}

/// Record one completed Ghostty draw on GTK. This measures wall time including driver waits,
/// not GPU completion or desktop presentation; no terminal payload is retained.
pub(super) fn drawn(started: Instant) {
    let micros = started.elapsed().as_micros().min(u64::MAX as u128) as u64;
    DRAW_TOTAL_US.fetch_add(micros, Ordering::Relaxed);
    DRAW_MAX_US.fetch_max(micros, Ordering::Relaxed);
    DRAWS.fetch_add(1, Ordering::Relaxed);
}

/// Read process-wide counters off GTK; interval deltas distinguish scheduling from slow draws.
pub(crate) fn snapshot() -> serde_json::Value {
    serde_json::json!({
        "requests": REQUESTS.load(Ordering::Relaxed),
        "draws": DRAWS.load(Ordering::Relaxed),
        "draw_total_us": DRAW_TOTAL_US.load(Ordering::Relaxed),
        "draw_max_us": DRAW_MAX_US.load(Ordering::Relaxed),
    })
}
