//! Conservative hookless prompt recognition from the live native grid, never from scrollback or resume state.
use super::model::InputState;
use serde_json::Value;
use std::ops::Range;

/// Locate Codex's shaded composer using complete native rows, independent of footer text or theme RGB values.
/// Missing styles or incomplete coverage leave the older plain prompt checks in force.
fn codex_composer(frame: &Value, row: usize) -> Option<Range<usize>> {
    let spans = frame["row_spans"].as_array()?;
    let styles = frame["styles"].as_array()?;
    let columns = frame["columns"].as_u64().filter(|n| *n > 0 && *n <= 512)?;
    let first = spans
        .iter()
        .find(|s| s["row"].as_u64() == Some(row as u64))?;
    let style = styles.iter().find(|s| s["id"] == first["style_id"])?;
    let background = &style["background"];
    if !matches!(style["background_source"].as_str(), Some("rgb" | "palette"))
        || !background.is_string()
    {
        return None;
    }
    let same_row = |y: usize| {
        let mut end = 0;
        for span in spans.iter().filter(|s| s["row"].as_u64() == Some(y as u64)) {
            let Some(style) = styles.iter().find(|s| s["id"] == span["style_id"]) else {
                return false;
            };
            if span["column"].as_u64() != Some(end) || style["background"] != *background {
                return false;
            }
            let Some(width) = span["cell_width"]
                .as_u64()
                .filter(|n| *n > 0 && *n <= columns)
            else {
                return false;
            };
            end += width;
        }
        end == columns
    };
    if !same_row(row) {
        return None;
    }
    let mut start = row;
    while start > 0 && same_row(start - 1) {
        start -= 1;
    }
    let mut end = row + 1;
    while end < frame["rows"].as_u64()? as usize && same_row(end) {
        end += 1;
    }
    Some(start..end)
}

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
    let composer = (client == "codex")
        .then(|| codex_composer(frame, row as usize))
        .flatten();
    // Interrupt indicators sit above the composer; general dialog words in prior replies are not UI state.
    let activity = lines
        .iter()
        .skip((row as usize).saturating_sub(6))
        .map(|l| l.to_lowercase())
        .collect::<Vec<_>>()
        .join("\n");
    let lower = lines[composer
        .as_ref()
        .map_or((row as usize).saturating_sub(6), |r| r.start)..]
        .join("\n")
        .to_lowercase();
    if ["esc to interrupt", "escape to interrupt", "esc to cancel"]
        .iter()
        .any(|s| activity.contains(s))
        || [
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
    let input = composer.unwrap_or(row as usize..footer_row);
    if input.end > footer_row {
        return InputState::Unknown;
    }
    for (y, l) in lines.iter().enumerate().take(input.end).skip(input.start) {
        if y == row as usize {
            continue;
        }
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

    /// Mirror the observed Codex 0.160.1 composer: shaded padding/input, faint placeholder and separate status/hint rows.
    fn shaded_codex() -> Value {
        json!({"anchor":"screen","rows":8,"columns":64,
            "cursor":{"row":2,"column":2,"visible":true},
            "styles":[
                {"id":0,"background_source":"default","background":"#181818","faint":false},
                {"id":1,"background_source":"rgb","background":"#41454C","faint":false},
                {"id":2,"background_source":"rgb","background":"#41454C","faint":true}],
            "row_spans":[
                {"row":0,"column":0,"style_id":0,"cell_width":64,"text":"The permission check is preserved; repair approved."},
                {"row":1,"column":0,"style_id":1,"cell_width":64,"text":" "},
                {"row":2,"column":0,"style_id":1,"cell_width":2,"text":"› "},
                {"row":2,"column":2,"style_id":2,"cell_width":62,"text":"Ask Codex to do anything"},
                {"row":3,"column":0,"style_id":1,"cell_width":64,"text":" "},
                {"row":4,"column":0,"style_id":0,"cell_width":64,"text":"  GPT-6.1-Sol high · ~/project · Context 77% left"},
                {"row":5,"column":0,"style_id":0,"cell_width":64,"text":"  ← for agents · ? for shortcuts"}]})
    }

    /// Status text is outside editable input; drafts, attachments, dialogs and incomplete native grids still block.
    #[test]
    fn shaded_codex_separates_input_from_status() {
        let frame = shaded_codex();
        assert_eq!(classify("codex", &frame), InputState::EmptyReady);
        let mut changed = frame.clone();
        changed["row_spans"][3]["style_id"] = 1.into();
        assert_eq!(classify("codex", &changed), InputState::Unfinished);
        let mut changed = frame.clone();
        changed["row_spans"][4]["text"] = "a second draft line".into();
        assert_eq!(classify("codex", &changed), InputState::Unfinished);
        let mut changed = frame.clone();
        changed["row_spans"][1]["text"] = "[Image #1]".into();
        assert_eq!(classify("codex", &changed), InputState::Unfinished);
        let mut changed = frame.clone();
        changed["row_spans"][0]["text"] = "Working · esc to interrupt".into();
        assert_eq!(classify("codex", &changed), InputState::Busy);
        let mut changed = frame.clone();
        changed["row_spans"][6]["text"] = "Allow once · ? for shortcuts".into();
        assert_eq!(classify("codex", &changed), InputState::Busy);
        let mut changed = frame.clone();
        changed["row_spans"][2]["cell_width"] = 1.into();
        assert_ne!(classify("codex", &changed), InputState::EmptyReady);
        let mut changed = frame.clone();
        changed["cursor"]["visible"] = false.into();
        assert_eq!(classify("codex", &changed), InputState::Unknown);
        let mut changed = frame;
        changed["styles"][1]["background"] = "#EEEEEE".into();
        changed["styles"][2]["background"] = "#EEEEEE".into();
        assert_eq!(classify("codex", &changed), InputState::EmptyReady);
    }
}
