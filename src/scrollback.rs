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

/// Aggregate retained replay limit across all workspaces in a snapshot.
pub(crate) const SESSION_MAX_BYTES: usize = 16 * 1024 * 1024;
/// Private GLArea cache retains history until native initialization has synchronously replayed it.
pub(crate) const PENDING_KEY: &str = "cmux-pending-scrollback";

/// Validate loaded replay text and enforce one aggregate budget before GTK owns restored state.
pub(crate) fn validate_session(session: &mut crate::session::SessionData) {
    let mut budget = SESSION_MAX_BYTES;
    for workspace in &mut session.workspaces {
        validate_tree(&mut workspace.layout, &mut budget, 0);
    }
}

/// Visit the bounded restore depth; discard oversized history without rejecting unrelated layout state.
fn validate_tree(tree: &mut crate::split_engine::SplitNodeData, budget: &mut usize, depth: usize) {
    use crate::split_engine::{PaneSurfaceData, SplitNodeData};
    if depth > 16 {
        return;
    }
    match tree {
        SplitNodeData::Pane { surfaces, .. } => {
            for surface in surfaces {
                if let PaneSurfaceData::Terminal { scrollback, .. } = surface {
                    *scrollback = scrollback
                        .take()
                        .and_then(|text| replay_text(&text))
                        .filter(|text| text.len() <= *budget);
                    *budget -= scrollback.as_ref().map_or(0, String::len);
                }
            }
        }
        SplitNodeData::Split { start, end, .. } => {
            validate_tree(start, budget, depth + 1);
            validate_tree(end, budget, depth + 1);
        }
        _ => {}
    }
}

/// Preserve history already normalized by validate_session for uninitialized background surfaces.
/// GTK owns the cached allocation; native replay borrows it synchronously before removing the cache.
pub(crate) fn prepare(area: &gtk4::GLArea, text: Option<&str>) {
    use gtk4::prelude::*;
    if let Some(text) = text.filter(|text| text.len() <= MAX_BYTES) {
        // SAFETY: this private key always owns String, read/stolen only on GTK in native initialization.
        unsafe { area.set_data(PENDING_KEY, text.to_owned()) };
    }
}

/// Capture initialized history or retain pending history without realizing a background terminal.
/// The shared budget bounds all copied history; allocation and native capture stay within per-terminal limits.
pub(crate) fn capture(
    area: &gtk4::GLArea,
    surface: Option<crate::ghostty::ffi::ghostty_surface_t>,
    budget: &mut usize,
) -> Option<String> {
    use gtk4::prelude::*;
    if *budget < MAX_BYTES {
        return None;
    }
    let text = if let Some(surface) = surface {
        // SAFETY: the owning GTK snapshot keeps this terminal live without event-loop iteration.
        unsafe { crate::ghostty::text::read_scrollback(surface) }.ok()?
    } else {
        // SAFETY: prepare exclusively stores String under this private key on the same GTK thread.
        unsafe { area.data::<String>(PENDING_KEY) }.map(|text| unsafe { text.as_ref().clone() })?
    };
    if text.len() > *budget {
        return None;
    }
    *budget -= text.len();
    Some(text)
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

    /// Loaded histories share a budget and edited effectful bytes are removed before widget ownership.
    #[test]
    fn loaded_history_is_normalized_and_aggregate_bounded() {
        use crate::split_engine::{PaneSurfaceData, SplitNodeData};
        let mut tree: SplitNodeData = serde_json::from_value(serde_json::json!({
            "type": "Pane", "surfaces": [
                {"type":"Terminal", "surface_uuid":uuid::Uuid::new_v4(), "shell":"", "cwd":"", "scrollback":"\u{1b}]52;c;secret\u{7}visible"},
                {"type":"Terminal", "surface_uuid":uuid::Uuid::new_v4(), "shell":"", "cwd":"", "scrollback":"too much remaining history"}
            ]
        })).unwrap();
        let mut budget = 16;
        validate_tree(&mut tree, &mut budget, 0);
        let SplitNodeData::Pane { surfaces, .. } = tree else {
            panic!("pane");
        };
        let PaneSurfaceData::Terminal { scrollback, .. } = &surfaces[0] else {
            panic!("terminal");
        };
        assert_eq!(scrollback.as_deref(), Some("\x1b[0mvisible\x1b[0m"));
        let PaneSurfaceData::Terminal { scrollback, .. } = &surfaces[1] else {
            panic!("terminal");
        };
        assert!(scrollback.is_none());
        assert_eq!(budget, 1);
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
