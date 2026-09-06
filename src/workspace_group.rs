use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const MAX_GROUPS: usize = 128;
pub const MAX_GROUP_NAME_BYTES: usize = 96;

/// Persisted presentation and collapse state for an ordered workspace group.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceGroup {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub collapsed: bool,
}

impl WorkspaceGroup {
    pub fn new(name: String, color: Option<String>) -> Result<Self, &'static str> {
        validate_name(&name)?;
        validate_color(color.as_deref())?;
        Ok(Self {
            id: Uuid::new_v4(),
            name,
            color,
            collapsed: false,
        })
    }

    /// Sanitize persisted data without allowing malformed color values into CSS.
    pub fn validated(mut self) -> Option<Self> {
        if validate_name(&self.name).is_err() {
            return None;
        }
        if validate_color(self.color.as_deref()).is_err() {
            self.color = None;
        }
        Some(self)
    }
}

pub fn validate_name(name: &str) -> Result<(), &'static str> {
    let name = name.trim();
    if name.is_empty() || name.len() > MAX_GROUP_NAME_BYTES || name.chars().any(char::is_control) {
        return Err("group name must contain 1..96 bytes without control characters");
    }
    Ok(())
}

pub fn validate_color(color: Option<&str>) -> Result<(), &'static str> {
    if color.is_some_and(|value| !crate::workspace::valid_workspace_color(value)) {
        return Err("group color must be a #RRGGBB value");
    }
    Ok(())
}
