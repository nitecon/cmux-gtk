//! Decode preview envelopes without building a copied JSON value tree.
use base64::Engine as _;
use std::borrow::Cow;

#[derive(serde::Deserialize)]
struct Envelope<'a> {
    #[serde(rename = "type", borrow)]
    kind: Cow<'a, str>,
    #[serde(borrow)]
    data: Cow<'a, str>,
}

/// Decode frame bytes, ignoring unrelated/malformed envelopes and reporting invalid base64 separately.
/// Cow borrows ordinary base64 text and still accepts valid JSON string escapes.
pub(super) fn decode(text: &str) -> Result<Option<glib::Bytes>, base64::DecodeError> {
    let Ok(envelope) = serde_json::from_str::<Envelope<'_>>(text) else {
        return Ok(None);
    };
    if envelope.kind != "frame" {
        return Ok(None);
    }
    base64::engine::general_purpose::STANDARD
        .decode(envelope.data.as_bytes())
        .map(|bytes| Some(glib::Bytes::from_owned(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Preserve ordinary and JSON-escaped base64 payloads while distinguishing nonframes and invalid data.
    #[test]
    fn frame_envelopes() {
        for text in [
            r#"{"type":"frame","data":"/w=="}"#,
            r#"{"type":"frame","data":"\/w=="}"#,
        ] {
            assert_eq!(decode(text).unwrap().unwrap().as_ref(), &[255]);
        }
        assert!(decode(r#"{"type":"status","data":"invalid"}"#)
            .unwrap()
            .is_none());
        assert!(decode(r#"{"type":"frame"}"#).unwrap().is_none());
        assert!(decode("not json").unwrap().is_none());
        assert!(decode(r#"{"type":"frame","data":"invalid"}"#).is_err());
    }
}
