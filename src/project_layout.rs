//! Bounded upstream-compatible workspace layout data; no filesystem or launch side effects.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Workspace launch settings with validated nested pane topology.
#[derive(Debug, Serialize, PartialEq)]
pub struct Workspace {
    pub name: Option<String>,
    pub cwd: Option<String>,
    pub color: Option<String>,
    pub setup: Option<String>,
    pub env: BTreeMap<String, String>,
    pub layout: Option<Layout>,
}

/// Preserve upstream pane/split object shapes while excluding structurally ambiguous nodes.
#[derive(Debug, Serialize, PartialEq)]
#[serde(untagged)]
pub enum Layout {
    Pane {
        pane: Pane,
    },
    Split {
        direction: Direction,
        split: f64,
        children: Vec<Layout>,
    },
}

/// Split orientation uses upstream horizontal/vertical names.
#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Horizontal,
    Vertical,
}

/// A nonempty sibling surface list, bounded across the entire workspace.
#[derive(Debug, Serialize, PartialEq)]
pub struct Pane {
    pub surfaces: Vec<Surface>,
}

/// Project surfaces remain recognized even though their Linux renderer is still outstanding.
#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SurfaceType {
    Terminal,
    Browser,
    Project,
}

/// Surface-specific launch fields; callers must protect managed environment variables when launching.
#[derive(Debug, Serialize, PartialEq)]
pub struct Surface {
    #[serde(rename = "type")]
    pub kind: SurfaceType,
    pub name: Option<String>,
    pub command: Option<String>,
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
    pub url: Option<String>,
    pub focus: Option<bool>,
}

/// Validate environment names and bounded literal values without expansion or secret logging.
pub(crate) fn environment(value: &Value) -> Result<BTreeMap<String, String>, String> {
    let Some(env) = value.get("env").filter(|value| !value.is_null()) else {
        return Ok(BTreeMap::new());
    };
    let env = env
        .as_object()
        .ok_or("workspace environment must be an object")?;
    if env.len() > 64 {
        return Err("workspace environment exceeds 64 entries".into());
    }
    let mut result = BTreeMap::new();
    let mut bytes = 0usize;
    for (key, value) in env {
        let text = value.as_str().ok_or("environment values must be strings")?;
        if key.is_empty()
            || key.len() > 128
            || !key
                .bytes()
                .enumerate()
                .all(|(i, b)| b == b'_' || b.is_ascii_alphabetic() || (i > 0 && b.is_ascii_digit()))
            || text.contains('\0')
            || text.len() > 16384
        {
            return Err("invalid environment entry".into());
        }
        bytes += key.len() + text.len();
        if bytes > 65536 {
            return Err("workspace environment exceeds 64 KiB".into());
        }
        result.insert(key.clone(), text.into());
    }
    Ok(result)
}

/// Read optional bounded launch text using the shared action validation contract.
fn text(value: &Value, key: &str) -> Result<Option<String>, String> {
    super::string(value, key, false, 16384)
}

/// Recursively validate at most 32 levels and 128 surfaces before producing an executable topology.
fn layout(value: &Value, depth: usize, surfaces: &mut usize) -> Result<Layout, String> {
    if depth > 32 || !value.is_object() {
        return Err("invalid or excessively deep workspace layout".into());
    }
    if let Some(pane) = value.get("pane") {
        if value.get("direction").is_some() {
            return Err("layout cannot contain both pane and direction".into());
        }
        let list = pane
            .get("surfaces")
            .and_then(Value::as_array)
            .ok_or("pane requires surfaces")?;
        if list.is_empty() || list.len() > 128 - *surfaces {
            return Err("workspace requires 1 to 128 surfaces".into());
        }
        *surfaces += list.len();
        let mut entries = Vec::new();
        for entry in list {
            let kind = serde_json::from_value(entry.get("type").cloned().unwrap_or(Value::Null))
                .map_err(|_| "unknown workspace surface type")?;
            let focus = entry
                .get("focus")
                .filter(|value| !value.is_null())
                .map(|value| value.as_bool().ok_or("surface focus must be boolean"))
                .transpose()?;
            entries.push(Surface {
                kind,
                name: text(entry, "name")?,
                command: text(entry, "command")?,
                cwd: text(entry, "cwd")?,
                env: environment(entry)?,
                url: text(entry, "url")?,
                focus,
            });
        }
        return Ok(Layout::Pane {
            pane: Pane { surfaces: entries },
        });
    }
    let direction = serde_json::from_value(value.get("direction").cloned().unwrap_or(Value::Null))
        .map_err(|_| "unknown split direction")?;
    let children = value
        .get("children")
        .and_then(Value::as_array)
        .filter(|children| children.len() == 2)
        .ok_or("split requires exactly two children")?;
    let split = value
        .get("split")
        .filter(|value| !value.is_null())
        .map(|value| {
            value
                .as_f64()
                .filter(|value| value.is_finite())
                .ok_or("invalid split position")
        })
        .transpose()?
        .unwrap_or(0.5)
        .clamp(0.1, 0.9);
    Ok(Layout::Split {
        direction,
        split,
        children: children
            .iter()
            .map(|child| layout(child, depth + 1, surfaces))
            .collect::<Result<_, _>>()?,
    })
}

/// Parse launch text, environment and topology without interpreting commands or applying colors.
pub fn parse(value: &Value) -> Result<Workspace, String> {
    if !value.is_object() {
        return Err("workspace action requires an object".into());
    }
    let mut surfaces = 0;
    Ok(Workspace {
        name: text(value, "name")?,
        cwd: text(value, "cwd")?,
        color: text(value, "color")?,
        setup: text(value, "setup")?,
        env: environment(value)?,
        layout: value
            .get("layout")
            .filter(|value| !value.is_null())
            .map(|value| layout(value, 0, &mut surfaces))
            .transpose()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    /// A mixed terminal/browser split retains launch fields and clamps ratio like upstream.
    #[test]
    fn parses_mixed_layout_and_rejects_ambiguous_nodes() {
        let leaf = serde_json::json!({"pane":{"surfaces":[{"type":"terminal","command":"echo hi"},{"type":"browser","url":"http://localhost:3000"}]}});
        let parsed=parse(&serde_json::json!({"layout":{"direction":"horizontal","split":2,"children":[leaf.clone(),leaf.clone()]}})).unwrap();
        let Layout::Split {
            split, children, ..
        } = parsed.layout.unwrap()
        else {
            panic!("missing split")
        };
        assert_eq!(split, 0.9);
        assert_eq!(children.len(), 2);
        assert!(parse(&serde_json::json!({"layout":{"pane":{"surfaces":[]}}})).is_err());
        assert!(parse(&serde_json::json!({"layout":{"pane":{"surfaces":[{"type":"terminal"}]},"direction":"horizontal"}})).is_err());
        assert!(parse(&serde_json::json!({"env":{"BAD=KEY":"value"}})).is_err());
    }
}
