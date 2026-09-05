//! Bound formatting and serialization before allocating complete diagnostic payloads.

use std::fmt::{self, Write as _};

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

pub(super) use crate::bounded_json::json_line;

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
}
