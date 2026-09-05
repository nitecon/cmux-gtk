use crate::split_engine::SplitNodeData;
use std::path::{Path, PathBuf};

/// Serializable snapshot of a single workspace for session persistence.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct WorkspaceSession {
    pub uuid: String,
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub startup_script: Option<PathBuf>,
    #[serde(default)]
    pub remote_target: Option<String>,
    #[serde(default)]
    pub remote_directory: Option<String>,
    /// Directory all local terminals in this workspace start in.
    #[serde(default)]
    pub working_directory: Option<PathBuf>,
    /// UUID of the active pane in this workspace, if any.
    pub active_pane_uuid: Option<String>,
    /// The full pane layout tree for this workspace.
    pub layout: SplitNodeData,
}

/// Root session data written to session.json.
/// `version: 1` allows forward-compatible schema evolution.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct SessionData {
    pub version: u32,
    /// Index of the active workspace in the workspaces array.
    pub active_index: usize,
    pub workspaces: Vec<WorkspaceSession>,
}

/// Returns the session file path.
/// Respects $XDG_DATA_HOME/cmux/session.json; falls back to ~/.local/share/cmux/session.json.
pub fn session_path() -> PathBuf {
    cmux_platform::paths::data_dir().join("session.json")
}

/// Save session data atomically.
/// Stages a private sibling file before replacement; readers see complete snapshots.
/// Atomic visibility does not imply power-loss durability or a shutdown flush.
pub fn save_session_atomic(data: &SessionData) -> std::io::Result<()> {
    save_session_to(data, &session_path())
}

/// Internal: save to a specific path (used in tests with temp paths).
pub fn save_session_to(data: &SessionData, path: &Path) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(data).map_err(std::io::Error::other)?;
    cmux_platform::filesystem::atomic_write(path, &json)
}

/// Load session from disk. Returns None if the file is missing, empty, or invalid JSON.
/// Never panics -- always returns a usable result for graceful fallback (SESS-04).
pub fn load_session() -> Option<SessionData> {
    load_session_from(&session_path())
}

/// Internal: load from a specific path (used in tests with temp paths).
pub fn load_session_from(path: &Path) -> Option<SessionData> {
    let content = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("cmux: no session file at {}", path.display());
            return None;
        }
        Err(e) => {
            eprintln!("cmux: session file read error: {e}");
            return None;
        }
    };
    match serde_json::from_str::<SessionData>(&content) {
        Ok(data) => {
            if data.version != 1 && data.version != 2 && data.version != 3 {
                eprintln!(
                    "cmux: session version {} not supported, ignoring",
                    data.version
                );
                return None;
            }
            Some(data)
        }
        Err(e) => {
            eprintln!("cmux: session JSON invalid: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::split_engine::SplitNodeData;

    /// Construct a minimal serializable workspace for persistence scenarios.
    fn dummy_session(name: &str) -> SessionData {
        SessionData {
            version: 1,
            active_index: 0,
            workspaces: vec![WorkspaceSession {
                uuid: "test-uuid-1".to_string(),
                name: name.to_string(),
                color: None,
                startup_script: None,
                remote_target: None,
                remote_directory: None,
                working_directory: Some(PathBuf::from("/tmp")),
                active_pane_uuid: None,
                layout: SplitNodeData::Leaf {
                    pane_id: 1000,
                    surface_uuid: uuid::Uuid::nil(),
                    shell: "/bin/sh".to_string(),
                    cwd: "/tmp".to_string(),
                },
            }],
        }
    }

    /// SESS-01: save_session_to must write session.json to disk for valid data.
    /// Verifies the full trigger -> write path, not just Ok(()) return.
    #[test]
    fn test_save_triggered() {
        let dir = std::env::temp_dir().join(format!("cmux-test-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.json");
        let data = dummy_session("TestWorkspace");
        let result = save_session_to(&data, &path);
        assert!(result.is_ok(), "save_session_to failed: {:?}", result);
        // The file must exist on disk -- not just Ok(()), but actually written.
        assert!(
            path.exists(),
            "session.json not created on disk after save_session_to"
        );
        // The content must be valid JSON with the correct workspace name.
        let content = std::fs::read_to_string(&path).expect("could not read session.json");
        let parsed: SessionData =
            serde_json::from_str(&content).expect("session.json is not valid JSON");
        assert_eq!(parsed.workspaces[0].name, "TestWorkspace");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SESS-02: Full roundtrip -- save then load must reproduce the workspace name.
    #[test]
    fn test_restore_roundtrip() {
        let dir = std::env::temp_dir().join(format!("cmux-test-rt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.json");

        let data = dummy_session("MyWorkspace");
        save_session_to(&data, &path).expect("save failed");

        let loaded = load_session_from(&path).expect("load returned None");
        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.workspaces.len(), 1);
        assert_eq!(loaded.workspaces[0].name, "MyWorkspace");
        assert_eq!(
            loaded.workspaces[0].working_directory,
            Some(PathBuf::from("/tmp"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Older session records remain readable without a workspace launch directory.
    #[test]
    fn test_legacy_session_without_working_directory() {
        let json = r#"{"version":2,"active_index":0,"workspaces":[{"uuid":"test-uuid-1","name":"Legacy","active_pane_uuid":null,"layout":{"type":"Leaf","pane_id":1000,"surface_uuid":"00000000-0000-0000-0000-000000000000","shell":"/bin/sh","cwd":"/tmp"}}]}"#;
        let parsed: SessionData = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.workspaces[0].working_directory, None);
    }

    /// SESS-03: Atomic write -- the .tmp file is gone after a successful rename.
    #[test]
    fn test_atomic_write() {
        let dir = std::env::temp_dir().join(format!("cmux-test-atomic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.json");

        let data = dummy_session("AtomicTest");
        save_session_to(&data, &path).unwrap();

        // After successful save: session.json exists, .tmp must be gone (renamed).
        assert!(path.exists(), "session.json must exist after save");
        assert_eq!(
            std::fs::read_dir(&dir).unwrap().count(),
            1,
            "temporary files must be removed"
        );
        assert_eq!(
            load_session_from(&path).unwrap().workspaces[0].name,
            "AtomicTest"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SESS-04: load_session returns None for missing file without panic.
    #[test]
    fn test_graceful_fallback() {
        let path = std::path::PathBuf::from("/tmp/cmux-nonexistent-session-xyz.json");
        let result = load_session_from(&path);
        assert!(
            result.is_none(),
            "load_session_from must return None for missing file"
        );
    }
}
