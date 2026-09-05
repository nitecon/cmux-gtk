//! Structured local diagnostics with bounded delivery, retention and resource samples.

mod bounded;
mod event_loop;
mod writer;

use std::backtrace::Backtrace;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
static WRITER: OnceLock<writer::Sender> = OnceLock::new();
static STARTED: OnceLock<Instant> = OnceLock::new();
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
static RPC_IN_FLIGHT: AtomicU64 = AtomicU64::new(0);
static RPC_SUCCEEDED: AtomicU64 = AtomicU64::new(0);
static RPC_FAILED: AtomicU64 = AtomicU64::new(0);
static RPC_CANCELLED: AtomicU64 = AtomicU64::new(0);
const MAX_RECORD_BYTES: usize = 64 * 1024;
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const QUEUE_CAPACITY: usize = 128;

/// Start the main-thread responsiveness probe and retain its removable source ID.
pub fn start_gtk_probe() -> gtk4::glib::SourceId {
    event_loop::start()
}

/// Start bounded off-thread logging; retain the returned guard until process exit.
/// Failure leaves diagnostics on stderr and does not prevent application startup.
pub fn initialize() -> Option<writer::Guard> {
    STARTED.get_or_init(Instant::now);
    match writer::start(log_path(), QUEUE_CAPACITY, MAX_FILE_BYTES) {
        Ok((sender, guard)) => {
            if WRITER.set(sender).is_ok() {
                Some(guard)
            } else {
                None
            }
        }
        Err(error) => {
            eprintln!("cmux: diagnostic writer unavailable: {error}");
            None
        }
    }
}

/// Preserve the previous panic hook while recording a bounded panic backtrace.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        event(format_args!(
            "PANIC version={} pid={} thread={} {panic_info}\n{}",
            env!("CMUX_VERSION"),
            std::process::id(),
            std::thread::current().name().unwrap_or("unnamed"),
            Backtrace::force_capture(),
        ));
        previous(panic_info);
    }));
}

/// Return the active log path; the preceding rotated file appends `.1`.
pub fn log_path() -> &'static Path {
    LOG_PATH.get_or_init(resolve_log_path).as_path()
}

/// Record a bounded lifecycle message; do not pass terminal or clipboard contents.
pub fn event(args: fmt::Arguments<'_>) {
    let (message, truncated) = bounded::message(args, 4096);
    record(
        "lifecycle",
        serde_json::json!({"message": message, "truncated": truncated}),
    );
}

/// Write structured metadata without blocking GTK; each record has a common envelope.
/// Fields must contain diagnostic metadata only, never user payloads or credentials.
pub fn record(event: &str, fields: serde_json::Value) {
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let record = serde_json::json!({
        "schema": 1, "timestamp_ms": timestamp_ms,
        "sequence": SEQUENCE.fetch_add(1, Ordering::Relaxed),
        "pid": std::process::id(), "version": env!("CMUX_VERSION"),
        "event": event, "fields": fields,
    });
    let Some(bytes) = bounded::json_line(&record, MAX_RECORD_BYTES) else {
        if let Some(writer) = WRITER.get() {
            writer.dropped.fetch_add(1, Ordering::Relaxed);
        }
        return;
    };
    if let Some(writer) = WRITER.get() {
        writer.record(bytes);
    } else {
        eprintln!(
            "cmux: {}",
            String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_RECORD_BYTES)])
        );
    }
}

/// Sample process resources and logger health. Performs blocking procfs reads.
/// Call on a worker. Missing resource data is reported explicitly instead of as zero.
pub fn snapshot() -> serde_json::Value {
    let resources = match cmux_platform::process::resources() {
        Ok(sample) => serde_json::json!({
            "rss_kib": sample.rss_kib, "peak_rss_kib": sample.peak_rss_kib,
            "threads": sample.threads, "file_descriptors": sample.file_descriptors,
        }),
        Err(error) => serde_json::json!({"error": error.to_string()}),
    };
    let writer = WRITER.get();
    serde_json::json!({
        "schema": 1, "version": env!("CMUX_VERSION"), "pid": std::process::id(),
        "build_profile": if cfg!(debug_assertions) { "debug" } else { "release" },
        "gtk_version": format!("{}.{}.{}", gtk4::major_version(), gtk4::minor_version(), gtk4::micro_version()),
        "requested_backend": std::env::var("GDK_BACKEND").unwrap_or_else(|_| "auto".into()),
        "uptime_ms": STARTED.get().map(|started| started.elapsed().as_millis()),
        "resources": resources,
        "gtk_event_loop": event_loop::snapshot(),
        "terminals": {"registered": crate::ghostty::registry::live_count()},
        "rpc": {
            "in_flight": RPC_IN_FLIGHT.load(Ordering::Relaxed),
            "succeeded": RPC_SUCCEEDED.load(Ordering::Relaxed),
            "failed": RPC_FAILED.load(Ordering::Relaxed),
            "cancelled": RPC_CANCELLED.load(Ordering::Relaxed),
        },
        "logging": {
            "active": writer.is_some(),
            "dropped_records": writer.map(|writer| writer.dropped.load(Ordering::Relaxed)).unwrap_or(0),
            "write_failures": writer.map(|writer| writer.failures.load(Ordering::Relaxed)).unwrap_or(0),
            "queue_capacity": QUEUE_CAPACITY, "max_record_bytes": MAX_RECORD_BYTES,
            "max_file_bytes": MAX_FILE_BYTES, "retained_files": 2,
        },
    })
}

/// Sample resources every five seconds off GTK; runtime shutdown cancels the sampler.
pub fn start_sampler(runtime: &tokio::runtime::Handle) {
    runtime.spawn(async {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if let Ok(sample) = tokio::task::spawn_blocking(snapshot).await {
                record("resources", sample);
            }
        }
    });
}

/// Resolve an explicit log override or the shared Linux state directory.
fn resolve_log_path() -> PathBuf {
    if let Some(path) = std::env::var_os("CMUX_LOG").filter(|value| !value.is_empty()) {
        return PathBuf::from(path);
    }
    cmux_platform::paths::state_dir().join("cmux.log")
}

/// Correlate a request lifetime across transport and GTK; unfinished drops are cancelled.
pub struct Operation {
    pub id: uuid::Uuid,
    started: Instant,
    method: String,
    outcome: &'static str,
}

impl Operation {
    /// Begin a request with a valid caller correlation ID or a newly generated ID.
    pub fn begin(method: &str, trace_id: Option<&str>) -> Self {
        RPC_IN_FLIGHT.fetch_add(1, Ordering::Relaxed);
        Self {
            id: trace_id
                .and_then(|id| uuid::Uuid::parse_str(id).ok())
                .unwrap_or_else(uuid::Uuid::new_v4),
            started: Instant::now(),
            method: if method.len() <= 128
                && method
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._".contains(&b))
            {
                method.to_owned()
            } else {
                "invalid_method".into()
            },
            outcome: "cancelled",
        }
    }

    /// Mark an awaited operation so cancellation is distinguishable from an error.
    pub fn pending(&mut self) {
        self.outcome = "cancelled";
    }

    /// Mark completion after the response is available, not merely after enqueueing.
    pub fn finish(&mut self, success: bool) {
        self.outcome = if success { "success" } else { "error" };
    }
}

impl Drop for Operation {
    /// Emit one outcome and duration even when dispatch is cancelled or returns early.
    fn drop(&mut self) {
        RPC_IN_FLIGHT.fetch_sub(1, Ordering::Relaxed);
        let counter = match self.outcome {
            "success" => &RPC_SUCCEEDED,
            "error" => &RPC_FAILED,
            _ => &RPC_CANCELLED,
        };
        counter.fetch_add(1, Ordering::Relaxed);
        record(
            "rpc.complete",
            serde_json::json!({
                "trace_id": self.id, "method": self.method, "outcome": self.outcome,
                "duration_us": self.started.elapsed().as_micros(),
            }),
        );
    }
}
