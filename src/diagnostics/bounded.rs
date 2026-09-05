//! Bound formatting and serialization before allocating complete diagnostic payloads.

use std::fmt::{self, Write as _};
use std::io;

/// UTF-8 formatting sink that stops the formatter at the configured byte limit.
struct Text {
    value: String,
    limit: usize,
}

impl fmt::Write for Text {
    /// Retain a complete UTF-8 prefix and stop subsequent formatting on overflow.
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let mut end = value.len().min(self.limit - self.value.len());
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        self.value.push_str(&value[..end]);
        if end < value.len() {
            Err(fmt::Error)
        } else {
            Ok(())
        }
    }
}

/// Format at most `limit` bytes and report truncation without first building full text.
pub(super) fn message(args: fmt::Arguments<'_>, limit: usize) -> (String, bool) {
    let mut text = Text {
        value: String::with_capacity(limit),
        limit,
    };
    let truncated = text.write_fmt(args).is_err();
    (text.value, truncated)
}

/// Serialization sink that rejects any write exceeding its fixed record budget.
struct Bytes {
    value: Vec<u8>,
    limit: usize,
}

impl io::Write for Bytes {
    /// Refuse overflow before growing the output buffer.
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit - self.value.len() {
            return Err(io::Error::other("diagnostic record exceeds byte limit"));
        }
        self.value.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    /// Memory output has no pending external writes.
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Encode a complete JSONL record within `limit`, reserving space for the newline.
pub(super) fn json_line(value: &serde_json::Value, limit: usize) -> Option<Vec<u8>> {
    let mut bytes = Bytes {
        value: Vec::new(),
        limit: limit.checked_sub(1)?,
    };
    serde_json::to_writer(&mut bytes, value).ok()?;
    bytes.value.push(b'\n');
    Some(bytes.value)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Truncate multi-byte text without invalid UTF-8 or evaluating later format fields.
    #[test]
    fn stops_formatting_at_utf8_boundary() {
        struct Unreachable;
        impl fmt::Display for Unreachable {
            /// Fail if formatting continues after the sink has exhausted its budget.
            fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
                panic!("formatter continued after overflow")
            }
        }
        let (text, truncated) = message(format_args!("{}{}", "ééé", Unreachable), 5);
        assert_eq!(text, "éé");
        assert!(truncated);
        assert_eq!(message(format_args!("small"), 5), ("small".into(), false));
    }

    /// Include JSON escaping and the newline in the byte limit; never emit partial JSON.
    #[test]
    fn bounds_encoded_records() {
        let value = serde_json::json!({"message": "\"\n".repeat(100)});
        assert!(json_line(&value, 100).is_none());
        let full = json_line(&value, 1024).unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&full).unwrap(),
            value
        );
        assert_eq!(json_line(&value, full.len()).unwrap(), full);
        assert!(json_line(&value, full.len() - 1).is_none());
        assert!(json_line(&value, 0).is_none());
    }
}
