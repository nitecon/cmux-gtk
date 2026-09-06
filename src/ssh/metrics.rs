//! Connection-attempt identity and cancellation-aware lifecycle records without remote target data.
use std::time::Instant;

/// Accept legacy untraced replies or validate echoed identity and bounded integer handler timing together.
pub(super) fn remote_timing(
    response: &serde_json::Value,
    trace_id: uuid::Uuid,
) -> Result<Option<u64>, &'static str> {
    let Some(trace) = response.get("trace_id") else {
        return Ok(None);
    };
    if trace
        .as_str()
        .and_then(|value| uuid::Uuid::parse_str(value).ok())
        != Some(trace_id)
    {
        return Err("trace identity mismatch");
    }
    response
        .get("handler_duration_us")
        .and_then(|value| value.as_u64())
        .map(Some)
        .ok_or("handler duration invalid")
}

/// Own one attempt from GTK admission through deployment, transport and reconnect backoff.
pub(super) struct Attempt {
    pub id: uuid::Uuid,
    workspace_id: u64,
    attempt: u32,
    started: Instant,
    phase_started: Instant,
    phase: &'static str,
    outcome: &'static str,
}

impl Attempt {
    /// Generate a new attempt identity; unfinished scope exit records cancellation.
    pub fn begin(workspace_id: u64, attempt: u32) -> Self {
        let started = Instant::now();
        let result = Self {
            id: uuid::Uuid::new_v4(),
            workspace_id,
            attempt,
            started,
            phase_started: started,
            phase: "gtk_admission",
            outcome: "cancelled",
        };
        crate::diagnostics::record(
            "ssh.connection.begin",
            serde_json::json!({
                "trace_id": result.id, "workspace_id": workspace_id, "attempt": attempt,
            }),
        );
        result
    }

    /// Mark entry to a fixed lifecycle phase; elapsed times separate deployment, handshake and routing waits.
    pub fn phase(&mut self, phase: &'static str) {
        self.phase = phase;
        self.phase_started = Instant::now();
        crate::diagnostics::record(
            "ssh.connection.stage",
            serde_json::json!({
                "trace_id": self.id, "workspace_id": self.workspace_id, "phase": phase,
                "elapsed_us": self.started.elapsed().as_micros(),
            }),
        );
    }

    /// Set a terminal category immediately before normal scope exit.
    pub fn finish(&mut self, outcome: &'static str) {
        self.outcome = outcome;
    }
}

impl Drop for Attempt {
    /// Emit the terminal outcome and current phase even when its owning future is aborted.
    fn drop(&mut self) {
        crate::diagnostics::record(
            "ssh.connection.complete",
            serde_json::json!({
                "trace_id": self.id, "workspace_id": self.workspace_id, "attempt": self.attempt,
                "phase": self.phase, "outcome": self.outcome,
                "duration_us": self.started.elapsed().as_micros(),
                "stage_duration_us": self.phase_started.elapsed().as_micros(),
            }),
        );
    }
}
