//! Bounded upstream-compatible workspace layout data; no filesystem or launch side effects.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Workspace launch settings with validated nested pane topology.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Workspace {
    pub name: Option<String>,
    pub cwd: Option<String>,
    pub color: Option<String>,
    pub setup: Option<String>,
    pub env: BTreeMap<String, String>,
    pub layout: Option<Layout>,
}

/// Preserve upstream pane/split object shapes while excluding structurally ambiguous nodes.
#[derive(Clone, Debug, Serialize, PartialEq)]
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
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Horizontal,
    Vertical,
}

/// A nonempty sibling surface list, bounded across the entire workspace.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Pane {
    pub surfaces: Vec<Surface>,
}

/// Project surfaces remain recognized even though their Linux renderer is still outstanding.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SurfaceType {
    Terminal,
    Browser,
    Project,
}

/// Surface-specific launch fields; callers must protect managed environment variables when launching.
#[derive(Clone, Debug, Serialize, PartialEq)]
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

/// Platform-neutral prepared tree shared by the offline CLI and GTK application binary.
#[allow(dead_code)] // The offline CLI validates layouts but never builds desktop panes.
pub(crate) enum PreparedLayout {
    Pane {
        active_surface_uuid: uuid::Uuid,
        surfaces: Vec<PreparedSurface>,
    },
    Split {
        direction: Direction,
        split: f64,
        children: Box<(PreparedLayout, PreparedLayout)>,
    },
}

/// Validated launch data with resolved terminal directories and stable surface identities.
#[allow(dead_code)] // The offline CLI validates surfaces but never launches them.
pub(crate) enum PreparedSurface {
    Terminal {
        uuid: uuid::Uuid,
        cwd: std::path::PathBuf,
        environment: BTreeMap<String, String>,
        initial_input: Option<String>,
    },
    Browser {
        uuid: uuid::Uuid,
        url: String,
    },
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

/// Recursively validate the shared Linux depth cap and 128 surfaces before execution.
fn layout(value: &Value, depth: usize, surfaces: &mut usize) -> Result<Layout, String> {
    if depth > super::MAX_LAYOUT_DEPTH || !value.is_object() {
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

/// Convert a validated layout into the single pane-tree format used by live state and sessions.
/// Directory checks finish on the caller's worker before any GTK objects are allocated.
#[allow(dead_code)] // Used by the desktop binary; the shared offline parser omits GTK state.
pub(crate) fn prepare_tree(
    layout: &Layout,
    base: &std::path::Path,
    setup: Option<&str>,
) -> Result<(PreparedLayout, String), String> {
    fn node(
        layout: &Layout,
        base: &std::path::Path,
        pending_setup: &mut Option<String>,
        first: &mut Option<uuid::Uuid>,
        focused: &mut Option<uuid::Uuid>,
    ) -> Result<PreparedLayout, String> {
        match layout {
            Layout::Pane { pane } => {
                let mut active = None;
                let mut pane_first = None;
                let mut surfaces = Vec::with_capacity(pane.surfaces.len());
                for surface in &pane.surfaces {
                    let uuid = uuid::Uuid::new_v4();
                    first.get_or_insert(uuid);
                    pane_first.get_or_insert(uuid);
                    if surface.focus == Some(true) {
                        active = Some(uuid);
                        *focused = Some(uuid);
                    }
                    let saved = match surface.kind {
                        SurfaceType::Terminal => {
                            let candidate = surface
                                .cwd
                                .as_deref()
                                .map(std::path::PathBuf::from)
                                .unwrap_or_else(|| base.to_owned());
                            let candidate = if candidate.is_absolute() {
                                candidate
                            } else {
                                base.join(candidate)
                            };
                            let directory = candidate
                                .canonicalize()
                                .map_err(|error| error.to_string())?;
                            if !directory.is_dir() {
                                return Err("workspace surface directory is not a directory".into());
                            }
                            let mut input = pending_setup.take().into_iter().collect::<Vec<_>>();
                            input.extend(surface.command.clone());
                            PreparedSurface::Terminal {
                                uuid,
                                cwd: directory,
                                environment: surface.env.clone(),
                                initial_input: (!input.is_empty()).then(|| input.join("\n")),
                            }
                        }
                        SurfaceType::Browser => PreparedSurface::Browser {
                            uuid,
                            url: surface.url.clone().unwrap_or_else(|| "about:blank".into()),
                        },
                        SurfaceType::Project => {
                            return Err("project surfaces are not available on Linux yet".into())
                        }
                    };
                    surfaces.push(saved);
                }
                Ok(PreparedLayout::Pane {
                    active_surface_uuid: active.or(pane_first).expect("validated pane is nonempty"),
                    surfaces,
                })
            }
            Layout::Split {
                direction,
                split,
                children,
            } => Ok(PreparedLayout::Split {
                direction: direction.clone(),
                split: *split,
                children: Box::new((
                    node(&children[0], base, pending_setup, first, focused)?,
                    node(&children[1], base, pending_setup, first, focused)?,
                )),
            }),
        }
    }
    let mut setup = setup.map(str::to_owned);
    let mut first = None;
    let mut focused = None;
    let tree = node(layout, base, &mut setup, &mut first, &mut focused)?;
    Ok((
        tree,
        focused
            .or(first)
            .expect("validated layouts contain a surface")
            .to_string(),
    ))
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

    /// Preparation resolves each terminal CWD and consumes setup at the first terminal only.
    #[test]
    fn prepares_stable_layout_identity_and_one_shot_input() {
        let root = std::env::temp_dir().join(format!("cmux-layout-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("left")).unwrap();
        std::fs::create_dir_all(root.join("right")).unwrap();
        let workspace = parse(&serde_json::json!({"layout":{"direction":"horizontal","children":[
            {"pane":{"surfaces":[{"type":"terminal","cwd":"left","command":"left"}]}},
            {"pane":{"surfaces":[{"type":"terminal","cwd":"right","command":"right","focus":true}]}}
        ]}})).unwrap();
        let (tree, active) =
            prepare_tree(workspace.layout.as_ref().unwrap(), &root, Some("setup")).unwrap();
        let PreparedLayout::Split { children, .. } = tree else {
            panic!("split missing")
        };
        let (
            PreparedLayout::Pane { surfaces: left, .. },
            PreparedLayout::Pane {
                surfaces: right,
                active_surface_uuid,
            },
        ) = *children
        else {
            panic!("panes missing")
        };
        let PreparedSurface::Terminal {
            cwd, initial_input, ..
        } = &left[0]
        else {
            panic!("left terminal missing")
        };
        assert_eq!(cwd, &root.join("left").canonicalize().unwrap());
        assert_eq!(initial_input.as_deref(), Some("setup\nleft"));
        let PreparedSurface::Terminal { initial_input, .. } = &right[0] else {
            panic!("right terminal missing")
        };
        assert_eq!(initial_input.as_deref(), Some("right"));
        assert_eq!(active, active_surface_uuid.to_string());
        std::fs::remove_dir_all(root).unwrap();
    }
}
