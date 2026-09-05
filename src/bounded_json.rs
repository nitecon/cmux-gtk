//! Size-limited JSON-line encoding shared by application diagnostics and the CLI.

use std::io;

/// Serialization sink that rejects any write exceeding its fixed record budget.
struct Bytes {
    value: Vec<u8>,
    limit: usize,
}

impl io::Write for Bytes {
    /// Refuse overflow before growing the output buffer.
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit - self.value.len() {
            return Err(io::Error::other("JSON line exceeds byte limit"));
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
pub(crate) fn json_line(value: &serde_json::Value, limit: usize) -> Option<Vec<u8>> {
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
