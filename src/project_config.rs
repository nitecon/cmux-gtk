//! Read-only project action resolution, independent of GTK and command execution.
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_BYTES: u64 = 256 * 1024;

/// Preserve an action's complete definition and exact winning source for later review/execution.
#[derive(Serialize)]
pub struct Action {
    pub source: PathBuf,
    pub definition: Value,
}

/// Resolved global and nearest-directory actions; loading never runs commands or trusts project files.
#[derive(Serialize)]
pub struct Resolved {
    pub directory: PathBuf,
    pub sources: Vec<PathBuf>,
    pub actions: BTreeMap<String, Action>,
}

/// Read one regular JSON object with a fixed byte budget; absent files are not errors.
fn read(path: &Path) -> Result<Option<Value>, String> {
    let file = match cmux_platform::filesystem::open_regular_read(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("project configuration exceeds 256 KiB".into());
    }
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    if !value.is_object() {
        return Err("project configuration must be a JSON object".into());
    }
    Ok(Some(value))
}

/// Merge whole action entries by ID, preserving local precedence and bounded registry size.
fn merge(resolved: &mut Resolved, source: PathBuf, value: Value) -> Result<(), String> {
    if let Some(actions) = value.get("actions") {
        let actions = actions.as_object().ok_or("actions must be an object")?;
        for (id, definition) in actions {
            if id.is_empty()
                || id.len() > 128
                || id.chars().any(char::is_control)
                || !definition.is_object()
            {
                return Err(
                    "action IDs must be nonempty bounded strings and definitions must be objects"
                        .into(),
                );
            }
            resolved.actions.insert(
                id.clone(),
                Action {
                    source: source.clone(),
                    definition: definition.clone(),
                },
            );
            if resolved.actions.len() > 256 {
                return Err("project action registry exceeds 256 entries".into());
            }
        }
    }
    resolved.sources.push(source);
    Ok(())
}

/// Resolve global config plus the nearest ancestor's preferred .cmux/cmux.json or legacy cmux.json.
/// Caller supplies the global path explicitly; only regular bounded files are read, never executed.
/// Walk at most 64 canonical directory ancestors, failing instead of silently truncating lookup.
pub fn resolve(directory: &Path, global: Option<&Path>) -> Result<Resolved, String> {
    let directory = directory
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !directory.is_dir() {
        return Err("project directory is not a directory".into());
    }
    let mut resolved = Resolved {
        directory: directory.clone(),
        sources: Vec::new(),
        actions: BTreeMap::new(),
    };
    if let Some(global) = global {
        if let Some(value) = read(global)? {
            merge(&mut resolved, global.to_owned(), value)?;
        }
    }
    for (depth, parent) in directory.ancestors().enumerate() {
        if depth >= 64 {
            return Err("project ancestor lookup exceeds 64 directories".into());
        }
        for candidate in [parent.join(".cmux/cmux.json"), parent.join("cmux.json")] {
            if let Some(value) = read(&candidate)? {
                merge(&mut resolved, candidate, value)?;
                return Ok(resolved);
            }
        }
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Preferred nearest files override global entries without executing embedded commands.
    #[test]
    fn nearest_preferred_file_and_global_fallback() {
        let root =
            std::env::temp_dir().join(format!("cmux-project-config-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("project/.cmux")).unwrap();
        std::fs::create_dir_all(root.join("project/nested")).unwrap();
        let global = root.join("global.json");
        std::fs::write(
            &global,
            r#"{"actions":{"global":{"command":"exit 1"},"same":{"title":"global"}}}"#,
        )
        .unwrap();
        std::fs::write(
            root.join("project/cmux.json"),
            r#"{"actions":{"loser":{}}}"#,
        )
        .unwrap();
        std::fs::write(
            root.join("project/.cmux/cmux.json"),
            r#"{"actions":{"same":{"title":"local"}}}"#,
        )
        .unwrap();
        let result = resolve(&root.join("project/nested"), Some(&global));
        std::fs::remove_dir_all(root).unwrap();
        let result = result.unwrap();
        assert_eq!(result.actions.len(), 2);
        assert_eq!(result.actions["same"].definition["title"], "local");
        assert_eq!(result.actions["global"].definition["command"], "exit 1");
        assert!(result.actions["same"]
            .source
            .ends_with("project/.cmux/cmux.json"));
    }
}
