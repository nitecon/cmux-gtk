//! Lock-free, payload-free forwarding counters, readable even while GTK or network work is stalled.
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

static HANDSHAKES: AtomicU64 = AtomicU64::new(0);
static HANDSHAKE_TIMEOUTS: AtomicU64 = AtomicU64::new(0);
static HANDSHAKE_ERRORS: AtomicU64 = AtomicU64::new(0);
static HANDSHAKE_SUCCESSES: AtomicU64 = AtomicU64::new(0);
static LISTENERS: AtomicU64 = AtomicU64::new(0);
static CLIENTS: AtomicU64 = AtomicU64::new(0);
static SENT: AtomicU64 = AtomicU64::new(0);
static DELIVERED: AtomicU64 = AtomicU64::new(0);
static DATA_REJECTED: AtomicU64 = AtomicU64::new(0);
static CLIENT_REJECTED: AtomicU64 = AtomicU64::new(0);
static CLOSE_REQUESTS: AtomicU64 = AtomicU64::new(0);
static CLOSE_CONFIRMED: AtomicU64 = AtomicU64::new(0);
static CLOSE_FAILED: AtomicU64 = AtomicU64::new(0);

/// Own a task gauge through completion and cancellation, including startup and cleanup work.
pub(super) struct Active(&'static AtomicU64);
impl Active {
    /// Count one SOCKS negotiation task through completion, rejection reply or cancellation.
    pub(super) fn handshake() -> Self {
        HANDSHAKES.fetch_add(1, Relaxed);
        Self(&HANDSHAKES)
    }

    /// Count one listener task, including draining clients after accept admission closes.
    pub(super) fn listener() -> Self {
        LISTENERS.fetch_add(1, Relaxed);
        Self(&LISTENERS)
    }
    /// Count one client task, including remote stream startup.
    pub(super) fn client() -> Self {
        CLIENTS.fetch_add(1, Relaxed);
        Self(&CLIENTS)
    }
}
impl Drop for Active {
    /// Balance the gauge exactly once when an owned task exits or is cancelled.
    fn drop(&mut self) {
        self.0.fetch_sub(1, Relaxed);
    }
}

/// Count bytes in writes acknowledged by the remote RPC handler, not inferred peer consumption.
pub(super) fn sent(bytes: usize) {
    SENT.fetch_add(bytes as u64, Relaxed);
}
/// Count bytes in fully completed local socket writes, excluding partial writes that failed.
pub(super) fn delivered(bytes: usize) {
    DELIVERED.fetch_add(bytes as u64, Relaxed);
}
/// Count malformed or capacity-rejected incoming chunks that retire their route.
pub(super) fn data_rejected() {
    DATA_REJECTED.fetch_add(1, Relaxed);
}
/// Count accepted sockets rejected because the connection-wide client limit is reached.
pub(super) fn client_rejected() {
    CLIENT_REJECTED.fetch_add(1, Relaxed);
}
/// Count requested remote closes; this does not establish remote acknowledgement.
pub(super) fn close_requested() {
    CLOSE_REQUESTS.fetch_add(1, Relaxed);
}

/// Count the observed result of an awaited remote close, separately from queued cancellation fallback.
pub(super) fn close_completed(confirmed: bool) {
    if confirmed {
        CLOSE_CONFIRMED.fetch_add(1, Relaxed);
    } else {
        CLOSE_FAILED.fetch_add(1, Relaxed);
    }
}

/// Count a completed SOCKS protocol negotiation independently from later remote admission.
pub(super) fn handshake_completed(outcome: &str) {
    match outcome {
        "success" => &HANDSHAKE_SUCCESSES,
        "timeout" => &HANDSHAKE_TIMEOUTS,
        _ => &HANDSHAKE_ERRORS,
    }
    .fetch_add(1, Relaxed);
}

/// Snapshot independently sampled aggregate values; per-connection limits are reported separately from gauges.
pub(crate) fn snapshot() -> serde_json::Value {
    serde_json::json!({
        "active_socks_handshakes":HANDSHAKES.load(Relaxed),
        "socks_handshake_timeouts":HANDSHAKE_TIMEOUTS.load(Relaxed),
        "socks_handshake_errors":HANDSHAKE_ERRORS.load(Relaxed),
        "socks_handshake_successes":HANDSHAKE_SUCCESSES.load(Relaxed),
        "socks_handshakes_per_workspace":16, "socks_handshake_deadline_ms":5000,
        "active_listener_tasks":LISTENERS.load(Relaxed), "active_client_tasks":CLIENTS.load(Relaxed),
        "remote_write_acknowledged_bytes":SENT.load(Relaxed), "local_write_completed_bytes":DELIVERED.load(Relaxed),
        "rejected_data_chunks":DATA_REJECTED.load(Relaxed), "rejected_clients":CLIENT_REJECTED.load(Relaxed),
        "close_requests":CLOSE_REQUESTS.load(Relaxed),
        "confirmed_closes":CLOSE_CONFIRMED.load(Relaxed),"failed_closes":CLOSE_FAILED.load(Relaxed), "listeners_per_connection":16,
        "clients_per_connection":16,"queued_chunks_per_client":16,"chunk_bytes":32768,
    })
}
