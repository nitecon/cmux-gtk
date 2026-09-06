//! Bounded agent-owned sidebar status and progress, independent of terminal focus.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One agent status entry, rendered as plain text with an optional themed icon and validated color.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Status {
    pub value: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub format: Format,
    #[serde(default)]
    pub url: Option<String>,
}

/// Sidebar presentation format; absent fields preserve older plain-text snapshots.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    #[default]
    Plain,
    Markdown,
}

/// Only bounded HTTP(S) destinations may become actionable links, matching upstream status URLs.
fn valid_url(value: &str) -> bool {
    value.len() <= 2048
        && !value.chars().any(char::is_control)
        && reqwest::Url::parse(value)
            .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
}

/// Determinate progress; finite values are clamped to the visible zero-to-one range.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Progress {
    pub value: f64,
    #[serde(default)]
    pub label: String,
}

/// At most 32 keyed entries plus one progress record per workspace.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Metadata {
    #[serde(default)]
    pub statuses: BTreeMap<String, Status>,
    #[serde(default)]
    pub progress: Option<Progress>,
}

/// Reject oversized/non-displayable metadata before crossing the GTK bridge.
fn valid_text(text: &str, max: usize) -> bool {
    text.len() <= max && !text.contains('\0')
}

impl Status {
    /// Validate all styling fields without parsing user-provided markup or paths.
    fn valid(&self) -> bool {
        valid_text(&self.value, 1024)
            && self.url.as_ref().is_none_or(|url| valid_url(url))
            && self.icon.as_ref().is_none_or(|icon| valid_text(icon, 128))
            && self
                .color
                .as_ref()
                .is_none_or(|color| crate::workspace::valid_workspace_color(color))
    }
}
impl Metadata {
    /// Bound loaded state and discard invalid entries without rejecting the workspace layout.
    pub fn validated(mut self) -> Self {
        self.statuses
            .retain(|key, status| valid_text(key, 64) && !key.is_empty() && status.valid());
        while self.statuses.len() > 32 {
            self.statuses.pop_last();
        }
        self.progress = self
            .progress
            .filter(|p| p.value.is_finite() && valid_text(&p.label, 512))
            .map(|mut p| {
                p.value = p.value.clamp(0.0, 1.0);
                p
            });
        self
    }
}

/// Validated metadata operation; fetching has no mutation or session-save side effect.
pub enum Action {
    Get,
    SetStatus(String, Status),
    ClearStatus(String),
    SetProgress(Progress),
    ClearProgress,
}

/// Decode bounded command content; explicit workspace UUID parsing belongs to the transport boundary.
pub fn parse(method: &str, params: &serde_json::Value) -> Result<Action, &'static str> {
    let key = || {
        params
            .get("key")
            .and_then(|value| value.as_str())
            .filter(|key| !key.is_empty() && valid_text(key, 64))
            .map(str::to_owned)
            .ok_or("invalid status key")
    };
    match method {
        "sidebar.metadata" => Ok(Action::Get),
        "sidebar.set_status" => {
            if params.get("panel").is_some() {
                return Err("panel-owned status is not yet supported");
            }
            let status: Status =
                Status::deserialize(params).map_err(|_| "invalid status fields")?;
            if !status.valid() {
                return Err("status fields exceed limits or contain invalid styling");
            }
            Ok(Action::SetStatus(key()?, status))
        }
        "sidebar.clear_status" => Ok(Action::ClearStatus(key()?)),
        "sidebar.set_progress" => {
            let mut progress: Progress =
                Progress::deserialize(params).map_err(|_| "invalid progress fields")?;
            if !progress.value.is_finite() || !valid_text(&progress.label, 512) {
                return Err("invalid progress fields");
            }
            progress.value = progress.value.clamp(0.0, 1.0);
            Ok(Action::SetProgress(progress))
        }
        "sidebar.clear_progress" => Ok(Action::ClearProgress),
        _ => Err("unknown metadata operation"),
    }
}

/// Convert bounded CommonMark to GTK label markup without accepting HTML or fetching resources.
/// Block boundaries collapse to spaces for inline sidebar layout; image alt text remains visible.
fn inline_markdown(value: &str, links: bool) -> String {
    use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
    let mut output = String::new();
    let mut closing = Vec::new();
    for event in Parser::new_ext(value, Options::ENABLE_STRIKETHROUGH) {
        match event {
            Event::Start(tag) => {
                let (open, close) = match tag {
                    Tag::Strong => ("<b>".to_owned(), "</b>"),
                    Tag::Emphasis => ("<i>".to_owned(), "</i>"),
                    Tag::Strikethrough => ("<s>".to_owned(), "</s>"),
                    Tag::CodeBlock(_) => ("<tt>".to_owned(), "</tt>"),
                    Tag::Link { dest_url, .. } if links && valid_url(&dest_url) => (
                        format!("<a href=\"{}\">", glib::markup_escape_text(&dest_url)),
                        "</a>",
                    ),
                    _ => (String::new(), ""),
                };
                output.push_str(&open);
                closing.push(close);
            }
            Event::End(tag) => {
                output.push_str(closing.pop().unwrap_or_default());
                if matches!(
                    tag,
                    TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::Item | TagEnd::CodeBlock
                ) {
                    output.push(' ');
                }
            }
            Event::Text(text)
            | Event::Html(text)
            | Event::InlineHtml(text)
            | Event::FootnoteReference(text)
            | Event::InlineMath(text)
            | Event::DisplayMath(text) => {
                output.push_str(&glib::markup_escape_text(&text));
            }
            Event::Code(text) => {
                output.push_str(&format!("<tt>{}</tt>", glib::markup_escape_text(&text)))
            }
            Event::SoftBreak | Event::HardBreak | Event::Rule => output.push(' '),
            Event::TaskListMarker(done) => output.push_str(if done { "☑ " } else { "☐ " }),
        }
    }
    output.trim_end().to_owned()
}

/// Produce escaped label markup; an explicit row URL takes precedence over embedded links.
fn status_markup(status: &Status) -> String {
    let mut markup = match status.format {
        Format::Plain => glib::markup_escape_text(&status.value).to_string(),
        Format::Markdown => inline_markdown(&status.value, status.url.is_none()),
    };
    if let Some(color) = &status.color {
        markup = format!("<span foreground='{color}'>{markup}</span>");
    }
    if let Some(url) = &status.url {
        markup = format!("<a href=\"{}\">{markup}</a>", glib::markup_escape_text(url));
    }
    markup
}

/// Mutate a resolved workspace without changing selection; reject new keys at capacity without eviction.
pub fn apply(metadata: &mut Metadata, action: Action) -> Result<bool, &'static str> {
    match action {
        Action::Get => return Ok(false),
        Action::SetStatus(key, status) => {
            if metadata.statuses.len() >= 32 && !metadata.statuses.contains_key(&key) {
                return Err("status entry limit reached");
            }
            metadata.statuses.insert(key, status);
        }
        Action::ClearStatus(key) => {
            metadata.statuses.remove(&key);
        }
        Action::SetProgress(progress) => metadata.progress = Some(progress),
        Action::ClearProgress => metadata.progress = None,
    }
    Ok(true)
}

/// Render bounded plain-text rows in priority order and an optional progress bar into a dedicated box.
pub fn render(container: &gtk4::Box, metadata: &Metadata) {
    use gtk4::prelude::*;
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
    let mut entries: Vec<_> = metadata.statuses.iter().collect();
    entries.sort_by(|(a, av), (b, bv)| bv.priority.cmp(&av.priority).then_with(|| a.cmp(b)));
    for (key, status) in entries {
        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        if let Some(icon) = &status.icon {
            row.append(&gtk4::Image::from_icon_name(icon));
        }
        let label = gtk4::Label::new(Some(&status.value));
        label.set_xalign(0.0);
        label.set_single_line_mode(true);
        label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        label.set_max_width_chars(28);
        label.set_tooltip_text(Some(&format!("{key}: {}", status.value)));
        label.set_markup(&status_markup(status));
        row.append(&label);
        container.append(&row);
    }
    if let Some(progress) = &metadata.progress {
        let bar = gtk4::ProgressBar::new();
        bar.set_fraction(progress.value);
        bar.set_text(Some(&progress.label));
        bar.set_show_text(!progress.label.is_empty());
        container.append(&bar);
    }
    container.set_visible(!metadata.statuses.is_empty() || metadata.progress.is_some());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Preserve inline formatting while preventing raw HTML and non-web links becoming active.
    #[test]
    fn markdown_escapes_untrusted_content() {
        let markup = inline_markdown("**bold _italic_** `a<b` <span>text</span> [bad](file:///tmp/x) [web](https://example.com/?x=1&y=2)", true);
        assert!(markup.contains("<b>bold <i>italic</i></b>"));
        assert!(markup.contains("<tt>a&lt;b</tt>"));
        assert!(markup.contains("&lt;span&gt;text&lt;/span&gt;"));
        assert!(!markup.contains("href=\"file:"));
        assert!(markup.contains("<a href=\"https://example.com/?x=1&amp;y=2\">web</a>"));
    }

    /// A whole-row destination must not create nested anchors; old records default to plain text.
    #[test]
    fn row_link_precedes_markdown_links_and_old_state_loads() {
        let old: Status =
            serde_json::from_value(serde_json::json!({"value":"<b>literal</b>"})).unwrap();
        assert_eq!(status_markup(&old), "&lt;b&gt;literal&lt;/b&gt;");
        let linked: Status = serde_json::from_value(serde_json::json!({
            "value":"[inside](https://inside.example)", "format":"markdown", "url":"https://outside.example/?a=1&b=2"
        })).unwrap();
        let markup = status_markup(&linked);
        assert_eq!(markup.matches("<a ").count(), 1);
        assert!(markup.contains("outside.example/?a=1&amp;b=2"));
        assert!(!markup.contains("inside.example"));
    }

    /// Reject active non-web schemes, invalid formats and oversized destinations at the worker boundary.
    #[test]
    fn rejects_invalid_destinations_and_formats() {
        for url in [
            "file:///tmp/a",
            "javascript:alert(1)",
            "https://",
            "https://example.com/\npath",
        ] {
            assert!(parse(
                "sidebar.set_status",
                &serde_json::json!({"key":"x","value":"x","url":url})
            )
            .is_err());
        }
        assert!(!valid_url(&format!(
            "https://example.com/{}",
            "x".repeat(2048)
        )));
        assert!(parse(
            "sidebar.set_status",
            &serde_json::json!({"key":"x","value":"x","format":"html"})
        )
        .is_err());
    }
}
