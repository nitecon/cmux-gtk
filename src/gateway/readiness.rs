//! Conservative hookless prompt recognition from the live native grid, never from scrollback or resume state.
use super::model::InputState;
use serde_json::Value;

/// Classify only supported editable Claude/Codex prompt layouts with a visible caret and shortcuts footer.
/// Unknown layouts, dialogs, multiline drafts and dim disabled prompts remain blocked.
pub fn classify(client: &str, frame: &Value) -> InputState {
    let Some(row) = frame["cursor"]["row"].as_u64() else {
        return InputState::Unknown;
    };
    let Some(column) = frame["cursor"]["column"].as_u64() else {
        return InputState::Unknown;
    };
    if frame["cursor"]["visible"] != true || frame["anchor"] != "screen" {
        return InputState::Unknown;
    }
    let Some(spans) = frame["row_spans"].as_array() else {
        return InputState::Unknown;
    };
    let Some(styles) = frame["styles"].as_array() else {
        return InputState::Unknown;
    };
    let rows = frame["rows"].as_u64().unwrap_or(0);
    if rows == 0 || row >= rows || rows > 200 {
        return InputState::Unknown;
    }
    let mut lines = vec![String::new(); rows as usize];
    for span in spans {
        let Some(y) = span["row"].as_u64().filter(|y| *y < rows) else {
            return InputState::Unknown;
        };
        let Some(x) = span["column"].as_u64().filter(|x| *x <= 512) else {
            return InputState::Unknown;
        };
        let Some(text) = span["text"].as_str() else {
            return InputState::Unknown;
        };
        let line = &mut lines[y as usize];
        let gap = (x as usize).saturating_sub(line.chars().count());
        line.extend(std::iter::repeat_n(' ', gap));
        line.push_str(text);
    }
    let lower = lines
        .iter()
        .skip((row as usize).saturating_sub(6))
        .map(|l| l.to_lowercase())
        .collect::<Vec<_>>()
        .join("\n");
    if [
        "esc to interrupt",
        "escape to interrupt",
        "esc to cancel",
        "connecting",
        "reconnecting",
        "input disabled",
        "viewing sub-agent",
        "permission",
        "approve",
        "allow once",
        "queued message",
    ]
    .iter()
    .any(|s| lower.contains(s))
    {
        return InputState::Busy;
    }
    let line = &lines[row as usize];
    let marker = match client {
        "codex" => '›',
        "claude" => '❯',
        _ => return InputState::Unknown,
    };
    let leading = line.chars().take_while(|c| *c == ' ').count();
    if line.chars().nth(leading) != Some(marker) {
        return InputState::Unknown;
    }
    // Editable providers put the caret immediately after their marker and one blank.
    if column as usize != leading + 2 {
        return InputState::Unfinished;
    }
    let footer = lines
        .iter()
        .enumerate()
        .skip(row as usize + 1)
        .take(4)
        .find(|(_, l)| l.to_lowercase().contains("for shortcuts"));
    let Some((footer_row, _)) = footer else {
        return InputState::Unknown;
    };
    for l in &lines[row as usize + 1..footer_row] {
        if !l.trim().is_empty()
            && !l
                .chars()
                .all(|c| c.is_whitespace() || matches!(c, '─' | '━' | '╭' | '╮' | '╰' | '╯' | '│'))
        {
            return InputState::Unfinished;
        }
    }
    // Codex renders placeholders faint; user drafts use the editable foreground style.
    for span in spans.iter().filter(|s| s["row"].as_u64() == Some(row)) {
        let x = span["column"].as_u64().unwrap_or(0);
        let text = span["text"].as_str().unwrap_or("");
        let id = span["style_id"].as_u64();
        let faint = styles
            .iter()
            .find(|s| s["id"].as_u64() == id)
            .is_some_and(|s| s["faint"] == true);
        let rest: String = text
            .chars()
            .skip((leading + 2).saturating_sub(x as usize))
            .collect();
        if !rest.trim().is_empty() && !faint {
            return InputState::Unfinished;
        }
    }
    InputState::EmptyReady
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A native grid fixture includes caret location and text styles rather than a loose screen substring.
    fn frame(prompt: &str, footer: &str, caret: u64, faint: bool) -> Value {
        json!({"anchor":"screen","rows":4,"cursor":{"row":1,"column":caret,"visible":true},
            "styles":[{"id":0,"faint":false},{"id":1,"faint":faint}],
            "row_spans":[{"row":1,"column":0,"style_id":0,"text":prompt},
                {"row":2,"column":0,"style_id":0,"text":footer}]})
    }

    /// Busy states, user drafts, permission UI and unsupported layouts cannot authorize terminal input.
    #[test]
    fn empty_prompt_requires_live_layout() {
        assert_eq!(
            classify("codex", &frame("› ", "? for shortcuts", 2, false)),
            InputState::EmptyReady
        );
        assert_eq!(
            classify("claude", &frame("❯ ", "? for shortcuts", 2, false)),
            InputState::EmptyReady
        );
        assert_eq!(
            classify("codex", &frame("› draft", "? for shortcuts", 2, false)),
            InputState::Unfinished
        );
        assert_eq!(
            classify("codex", &frame("› ", "esc to interrupt", 2, false)),
            InputState::Busy
        );
        assert_eq!(
            classify(
                "claude",
                &frame("❯ Yes", "allow once · ? for shortcuts", 2, false)
            ),
            InputState::Busy
        );
        assert_eq!(
            classify("codex", &frame("$ ", "? for shortcuts", 2, false)),
            InputState::Unknown
        );
        assert_eq!(
            classify("codex", &frame("› ", "unknown footer", 2, false)),
            InputState::Unknown
        );
        let mut placeholder = frame("› ", "? for shortcuts", 2, true);
        placeholder["row_spans"]
            .as_array_mut()
            .unwrap()
            .push(json!({"row":1,"column":2,"style_id":1,"text":"Explain this codebase"}));
        assert_eq!(classify("codex", &placeholder), InputState::EmptyReady);
        placeholder["cursor"]["visible"] = false.into();
        assert_eq!(classify("codex", &placeholder), InputState::Unknown);
    }
}
