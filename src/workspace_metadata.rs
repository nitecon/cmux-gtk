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
            if params.get("format").is_some_and(|value| value != "plain")
                || params.get("url").is_some()
                || params.get("panel").is_some()
            {
                return Err("only plain workspace status is currently supported");
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
        if let Some(color) = &status.color {
            label.set_markup(&format!(
                "<span foreground='{color}'>{}</span>",
                gtk4::glib::markup_escape_text(&status.value)
            ));
        }
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
