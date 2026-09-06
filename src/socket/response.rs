//! Shared response envelopes for worker validation and GTK command completion.
use serde_json::{json, Value};

/// Maximum encoded response bytes, including its line delimiter.
pub(super) const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

/// Bound JSON allocation before transport; preserve identity in an oversized-response error when it fits.
/// Extremely large request identities fall back to null. Result construction is a separate bound.
/// Encoding overflow marks the retained request as failed and emits its trace identity.
pub(super) fn encode(
    mut response: Value,
    operation: Option<&mut crate::diagnostics::Operation>,
) -> String {
    let bytes = crate::bounded_json::json_line(&response, MAX_RESPONSE_BYTES).or_else(|| {
        let trace_id = operation.map(|operation| {
            operation.finish(false);
            operation.id
        });
        crate::diagnostics::record(
            "rpc.response.oversized",
            json!({"trace_id": trace_id, "limit_bytes": MAX_RESPONSE_BYTES}),
        );
        let id = response
            .get_mut("id")
            .map(Value::take)
            .unwrap_or(Value::Null);
        drop(response);
        crate::bounded_json::json_line(
            &err(
                id,
                "response_too_large",
                "response exceeds encoded byte limit",
            ),
            MAX_RESPONSE_BYTES,
        )
    });
    match bytes {
        Some(mut bytes) => {
            // The transport owns the newline write.
            bytes.pop();
            // serde_json always produces UTF-8; keep this conversion checked.
            String::from_utf8(bytes).unwrap_or_else(|_| fallback_error().to_owned())
        }
        None => fallback_error().to_owned(),
    }
}

/// Return a small valid error when even the echoed identity exceeds the response budget.
fn fallback_error() -> &'static str {
    r#"{"id":null,"ok":false,"error":{"code":"response_too_large","message":"response exceeds encoded byte limit"}}"#
}

/// Move the request identity and result into a successful protocol response.
pub(super) fn ok(req_id: Value, result: Value) -> Value {
    json!({"id": req_id, "ok": true, "result": result})
}

/// Build a failed response, preserving request identity and copying public error details.
pub(super) fn err(req_id: Value, code: &str, message: &str) -> Value {
    json!({"id": req_id, "ok": false, "error": {"code": code, "message": message}})
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A complete line may exactly fill the budget; one additional payload byte returns a small error.
    #[test]
    fn exact_line_budget() {
        let overhead = encode(ok(json!(1), json!("")), None).len() + 1;
        let payload = "x".repeat(MAX_RESPONSE_BYTES - overhead);
        let encoded = encode(ok(json!(1), json!(payload)), None);
        assert_eq!(encoded.len() + 1, MAX_RESPONSE_BYTES);
        let decoded: Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded["ok"], true);
        let oversized = encode(ok(json!(1), json!(format!("{payload}x"))), None);
        let decoded: Value = serde_json::from_str(&oversized).unwrap();
        assert_eq!(decoded["id"], 1);
        assert_eq!(decoded["error"]["code"], "response_too_large");
    }

    /// Escaping overflow becomes a valid correlated error, rather than a partial serialized result.
    #[test]
    fn oversized_escaped_payload() {
        let encoded = encode(
            ok(
                json!(42),
                json!({"text": "\u{0001}".repeat(MAX_RESPONSE_BYTES / 2)}),
            ),
            None,
        );
        let decoded: Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded["id"], 42);
        assert_eq!(decoded["error"]["code"], "response_too_large");
        assert!(encoded.len() < MAX_RESPONSE_BYTES);
    }

    /// Ordinary Unicode responses round-trip and oversized identities use the bounded null-id fallback.
    #[test]
    fn response_identity_and_unicode() {
        let value = ok(json!("request"), json!({"text": "日本語\n"}));
        assert_eq!(
            serde_json::from_str::<Value>(&encode(value.clone(), None)).unwrap(),
            value
        );
        let encoded = encode(ok(json!("x".repeat(MAX_RESPONSE_BYTES)), Value::Null), None);
        let decoded: Value = serde_json::from_str(&encoded).unwrap();
        assert!(decoded["id"].is_null());
        assert_eq!(decoded["ok"], false);
    }
}
