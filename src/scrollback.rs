//! Bounded, theme-independent rendered history suitable for terminal output replay.

/// Per-terminal UTF-8 replay budget, including reset sequences.
pub(crate) const MAX_BYTES: usize = 256 * 1024;

/// Retain printable Unicode, whitespace and complete numeric SGR sequences only.
/// OSC/DCS/APC/PM/SOS and other terminal commands are removed, including incomplete frames.
/// Returns None for oversized source data; never truncate in the middle of UTF-8 or an escape.
pub(crate) fn replay_text(source: &str) -> Option<String> {
    if source.len() > MAX_BYTES {
        return None;
    }
    let mut output = String::with_capacity((source.len() + 8).min(MAX_BYTES));
    output.push_str("\x1b[0m");
    let mut characters = source.char_indices().peekable();
    while let Some((start, ch)) = characters.next() {
        let mut keep = None;
        match ch {
            '\x1b' => {
                match characters.next().map(|(_, ch)| ch) {
                    Some('[') => {
                        let mut numeric = true;
                        for (end, ch) in characters.by_ref() {
                            if ('@'..='~').contains(&ch) {
                                if ch == 'm' && numeric && end - start <= 256 {
                                    keep = Some(&source[start..end + 1]);
                                }
                                break;
                            }
                            numeric &= ch.is_ascii_digit() || ch == ';' || ch == ':';
                        }
                    }
                    Some(']' | 'P' | '_' | '^' | 'X') => {
                        let mut escaped = false;
                        for (_, ch) in characters.by_ref() {
                            if ch == '\x07' || (escaped && ch == '\\') {
                                break;
                            }
                            escaped = ch == '\x1b';
                        }
                    }
                    // Consume intermediate bytes of any other escape before its final byte.
                    Some(ch) if (' '..='/').contains(&ch) => {
                        for (_, ch) in characters.by_ref() {
                            if !(' '..='/').contains(&ch) {
                                break;
                            }
                        }
                    }
                    _ => {}
                }
            }
            ch if !ch.is_control() || matches!(ch, '\r' | '\n' | '\t') => {
                keep = Some(&source[start..start + ch.len_utf8()]);
            }
            _ => {}
        }
        if let Some(text) = keep {
            if output.len() + text.len() + 4 > MAX_BYTES {
                break;
            }
            output.push_str(text);
        }
    }
    output.push_str("\x1b[0m");
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Replay retains wide/combining text and rich styling while suppressing effectful terminal commands.
    #[test]
    fn replay_filters_control_channels_and_keeps_styles() {
        let source = "\x1b]11;#ffffff\x07\x1b]52;c;clipboard\x1b\\\x1b]9;notification\x07\x1bPsecret\x1b\\\x1b[2J\x1b[?2004h\x1b[38:2::1:2:3m界e\u{301}\tbold\r\n\x1b[0m";
        let filtered = replay_text(source).unwrap();
        assert_eq!(
            filtered,
            "\x1b[0m\x1b[38:2::1:2:3m界e\u{301}\tbold\r\n\x1b[0m\x1b[0m"
        );
        for unfinished in [
            "visible\x1b]52;c;secret",
            "visible\x1b[38;2;",
            "visible\x1bPprivate",
        ] {
            assert_eq!(replay_text(unfinished).unwrap(), "\x1b[0mvisible\x1b[0m");
        }
    }

    /// Output caps include resets and never split multibyte characters or complete styling sequences.
    #[test]
    fn replay_budget_includes_boundaries() {
        assert!(replay_text(&"x".repeat(MAX_BYTES + 1)).is_none());
        let text = replay_text(&"界".repeat(MAX_BYTES / 3)).unwrap();
        assert!(text.len() <= MAX_BYTES && text.ends_with("\x1b[0m"));
        assert_eq!(text.matches('界').count(), (MAX_BYTES - 8) / 3);
    }
}
