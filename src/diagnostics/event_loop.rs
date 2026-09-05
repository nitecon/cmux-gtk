//! Constant-space GTK responsiveness sampling readable from background diagnostics.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static LAST_TICK_MS: AtomicU64 = AtomicU64::new(u64::MAX);
static LAST_DELAY_US: AtomicU64 = AtomicU64::new(0);
static MAX_DELAY_US: AtomicU64 = AtomicU64::new(0);
const INTERVAL: Duration = Duration::from_secs(1);

/// Schedule a GTK-thread heartbeat; remove its source when the application loop exits.
pub(super) fn start() -> gtk4::glib::SourceId {
    let mut previous = Instant::now();
    gtk4::glib::timeout_add_local(INTERVAL, move || {
        let now = Instant::now();
        let delay = now.duration_since(previous).saturating_sub(INTERVAL);
        previous = now;
        let delay_us = delay.as_micros().min(u64::MAX as u128) as u64;
        LAST_DELAY_US.store(delay_us, Ordering::Relaxed);
        MAX_DELAY_US.fetch_max(delay_us, Ordering::Relaxed);
        let elapsed = super::STARTED.get_or_init(Instant::now).elapsed();
        LAST_TICK_MS.store(elapsed.as_millis() as u64, Ordering::Release);
        gtk4::glib::ControlFlow::Continue
    })
}

/// Read heartbeat age off GTK so a stalled loop remains detectable during a stall.
pub(super) fn snapshot() -> serde_json::Value {
    let tick = LAST_TICK_MS.load(Ordering::Acquire);
    if tick == u64::MAX {
        return serde_json::json!({"sampled": false, "interval_ms": INTERVAL.as_millis()});
    }
    let now = super::STARTED
        .get_or_init(Instant::now)
        .elapsed()
        .as_millis() as u64;
    serde_json::json!({
        "sampled": true, "interval_ms": INTERVAL.as_millis(),
        "sample_age_ms": now.saturating_sub(tick),
        "last_delay_us": LAST_DELAY_US.load(Ordering::Relaxed),
        "max_delay_us": MAX_DELAY_US.load(Ordering::Relaxed),
    })
}
