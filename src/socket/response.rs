//! Shared response envelopes for worker validation and GTK command completion.
use serde_json::{json, Value};

/// Move the request identity and result into a successful protocol response.
pub(super) fn ok(req_id: Value, result: Value) -> Value {
    json!({"id": req_id, "ok": true, "result": result})
}

/// Build a failed response, preserving request identity and copying public error details.
pub(super) fn err(req_id: Value, code: &str, message: &str) -> Value {
    json!({"id": req_id, "ok": false, "error": {"code": code, "message": message}})
}
