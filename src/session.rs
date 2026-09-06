use crate::split_engine::SplitNodeData;
use std::io::{BufReader, BufWriter, Read, Write};
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
/// The loader accepts schema versions 1 through 3; newer versions fall back to a fresh session.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct SessionData {
    pub version: u32,
    /// Index of the active workspace in the workspaces array.
    pub active_index: usize,
    pub workspaces: Vec<WorkspaceSession>,
    #[serde(default)]
    pub resume_policy: crate::resume_policy::ResumePolicy,
    #[serde(default)]
    pub inbox: crate::inbox::Inbox,
}

/// Immutable snapshot shared by GTK publication and the worker without cloning the pane tree.
pub type Snapshot = std::sync::Arc<SessionData>;

/// Coalesce snapshots over a 500-ms window and serialize blocking writes one at a time.
/// Finish interrupts debounce, saves the latest snapshot after any older write and syncs it to disk.
/// Only this worker writes the destination; callers stop GTK publication before requesting finish.
pub async fn write_snapshots(
    mut receiver: tokio::sync::watch::Receiver<Option<Snapshot>>,
    path: PathBuf,
    mut finish: tokio::sync::oneshot::Receiver<()>,
) -> std::io::Result<()> {
    loop {
        let stopping = tokio::select! {
            biased;
            _ = &mut finish => true,
            changed = receiver.changed() => {
                if changed.is_err() {
                    true
                } else {
                    tokio::select! {
                        biased;
                        _ = &mut finish => true,
                        _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => false,
                    }
                }
            }
        };
        let latest = receiver.borrow_and_update().clone();
        if let Some(snapshot) = latest {
            let path = path.clone();
            let result = tokio::task::spawn_blocking(move || {
                save_session_to(&snapshot, &path)?;
                if stopping {
                    cmux_platform::filesystem::sync_file_and_parent(&path)?;
                }
                Ok(())
            })
            .await
            .map_err(std::io::Error::other)
            .and_then(|result| result);
            if stopping {
                return result;
            }
            if let Err(error) = result {
                eprintln!("cmux: session save failed: {error}");
            }
        }
        if stopping {
            return Ok(());
        }
    }
}

/// Returns the session file path.
/// Respects $XDG_DATA_HOME/cmux/session.json; falls back to ~/.local/share/cmux/session.json.
pub fn session_path() -> PathBuf {
    cmux_platform::paths::data_dir().join("session.json")
}

/// Stream pretty JSON through a 64-KiB buffer into an atomic replacement.
/// Record combined serialization/write timing and counts without paths or content.
pub fn save_session_to(data: &SessionData, path: &Path) -> std::io::Result<()> {
    let started = std::time::Instant::now();
    let mut serialization_write_us = None;
    let result = cmux_platform::filesystem::atomic_write_with(path, |file| {
        let serialization_started = std::time::Instant::now();
        let mut writer = BufWriter::with_capacity(64 * 1024, file);
        let encoded = serde_json::to_writer_pretty(&mut writer, data)
            .map_err(std::io::Error::other)
            .and_then(|_| writer.flush());
        serialization_write_us = Some(serialization_started.elapsed().as_micros() as u64);
        encoded?;
        writer.get_ref().metadata().map(|metadata| metadata.len())
    });
    let bytes = result.as_ref().ok().copied();
    crate::diagnostics::record(
        "session.save",
        serde_json::json!({
            "outcome": if result.is_ok() { "success" } else { "error" },
            "workspaces": data.workspaces.len(),
            "bytes": bytes,
            "serialization_write_us": serialization_write_us,
            "duration_us": started.elapsed().as_micros() as u64,
            "error_kind": result.as_ref().err().map(|error| format!("{:?}", error.kind())),
        }),
    );
    result.map(|_| ())
}

/// Load session from disk. Returns None if the file is missing, empty, or invalid JSON.
/// Never panics -- always returns a usable result for graceful fallback (SESS-04).
pub fn load_session() -> Option<SessionData> {
    load_session_from(&session_path())
}

/// Select startup state and archive a valid normal-launch snapshot before any live autosaves.
/// Explicit previous-session recovery leaves its backup untouched; missing/invalid backup fails closed.
/// Blocking I/O runs before GTK activation, using the same atomic durable writer as live snapshots.
pub fn load_startup_session(
    previous: bool,
) -> Result<(Option<SessionData>, Option<RunMarker>), &'static str> {
    let path = session_path();
    let backup = path.with_file_name("session.previous.json");
    if previous {
        let session = load_session_from(&backup).ok_or("no valid previous session is available")?;
        return Ok((Some(session), RunMarker::begin(&path)));
    }
    let unclean = path
        .with_file_name("session.running")
        .try_exists()
        .unwrap_or(true);
    let session = load_session();
    if let Some(data) = session
        .as_ref()
        .filter(|data| !unclean && !data.workspaces.is_empty())
    {
        let result = save_session_to(data, &backup)
            .and_then(|()| cmux_platform::filesystem::sync_file_and_parent(&backup));
        crate::diagnostics::record(
            "session.backup",
            serde_json::json!({
                "outcome": if result.is_ok() { "success" } else { "error" },
                "workspaces": data.workspaces.len(),
                "error_kind": result.as_ref().err().map(|error| format!("{:?}", error.kind())),
            }),
        );
        if result.is_err() {
            eprintln!(
                "cmux: could not archive the previous session; continuing with current state"
            );
        }
    }
    crate::diagnostics::record(
        "session.launch",
        serde_json::json!({"previous_unclean": unclean}),
    );
    Ok((session, RunMarker::begin(&path)))
}

/// Owner identity for one launch; deliberately retained after panic, forced exit or failed final save.
pub struct RunMarker {
    path: PathBuf,
    token: String,
}

impl RunMarker {
    /// Publish an owner-only durable launch marker before GTK activation; failure is observable.
    fn begin(session_path: &Path) -> Option<Self> {
        let marker = Self {
            path: session_path.with_file_name("session.running"),
            token: uuid::Uuid::new_v4().to_string(),
        };
        let result = cmux_platform::filesystem::atomic_write_with(&marker.path, |file| {
            file.write_all(marker.token.as_bytes())
        })
        .and_then(|()| cmux_platform::filesystem::sync_file_and_parent(&marker.path));
        if result.is_err() {
            eprintln!("cmux: could not record session launch state");
            return None;
        }
        Some(marker)
    }

    /// Retire only this launch's marker after durable final save, then sync its directory entry.
    pub fn finish(self) -> std::io::Result<()> {
        if cmux_platform::filesystem::read_text_bounded(&self.path, 64)? != self.token {
            return Ok(());
        }
        std::fs::remove_file(&self.path)?;
        if let Some(parent) = self.path.parent() {
            std::fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    }
}

/// Validate UTF-8 across input chunks, including strings the JSON parser ignores.
/// At most three unfinished character bytes survive between reads; EOF validates their completion.
struct Utf8Reader<R> {
    source: R,
    tail: Vec<u8>,
}

impl<R: Read> Read for Utf8Reader<R> {
    /// Forward a chunk only after rejecting invalid sequences; incomplete characters await the next read.
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        let count = self.source.read(output)?;
        let mut chunk = std::mem::take(&mut self.tail);
        chunk.extend_from_slice(&output[..count]);
        match std::str::from_utf8(&chunk) {
            Ok(_) => {}
            Err(error) if error.error_len().is_none() && count != 0 => {
                self.tail.extend_from_slice(&chunk[error.valid_up_to()..]);
            }
            Err(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "session is not UTF-8",
                ))
            }
        }
        Ok(count)
    }
}

/// Stream a session from a path with strict UTF-8 and a 64-KiB input buffer.
/// The deserialized model still scales with session size; no new file-size limit is imposed.
pub fn load_session_from(path: &Path) -> Option<SessionData> {
    let started = std::time::Instant::now();
    let mut outcome = "success";
    let mut error_category = None;
    let mut version = None;
    let result = (|| {
        let source = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                outcome = "missing";
                eprintln!("cmux: no session file at {}", path.display());
                return None;
            }
            Err(e) => {
                outcome = "read_error";
                error_category = Some(format!("{:?}", e.kind()));
                eprintln!("cmux: session file read error: {e}");
                return None;
            }
        };
        let reader = BufReader::with_capacity(
            64 * 1024,
            Utf8Reader {
                source,
                tail: Vec::new(),
            },
        );
        match serde_json::from_reader::<_, SessionData>(reader) {
            Ok(mut data) => {
                version = Some(data.version);
                if data.version != 1 && data.version != 2 && data.version != 3 {
                    outcome = "unsupported_version";
                    eprintln!(
                        "cmux: session version {} not supported, ignoring",
                        data.version
                    );
                    return None;
                }
                crate::scrollback::validate_session(&mut data);
                Some(data)
            }
            Err(e) => {
                outcome = "decode_error";
                error_category = Some(format!("{:?}", e.classify()));
                eprintln!("cmux: session JSON invalid: {e}");
                None
            }
        }
    })();
    crate::diagnostics::record(
        "session.load",
        serde_json::json!({
            "outcome": outcome,
            "duration_us": started.elapsed().as_micros(),
            "version": version,
            "workspaces": result.as_ref().map(|data| data.workspaces.len()),
            "error_category": error_category,
        }),
    );
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::split_engine::SplitNodeData;

    /// Streaming across several buffer flushes preserves escaped UTF-8 and pretty-JSON compatibility.
    #[test]
    fn large_snapshot_streaming_roundtrip() {
        let path =
            std::env::temp_dir().join(format!("cmux-stream-session-{}", uuid::Uuid::new_v4()));
        let name = "λ\"\n".repeat(65536);
        let snapshot = dummy_session(&name);
        save_session_to(&snapshot, &path).unwrap();
        assert_eq!(
            std::fs::read(&path).unwrap(),
            serde_json::to_vec_pretty(&snapshot).unwrap()
        );
        assert_eq!(load_session_from(&path).unwrap().workspaces[0].name, name);
        std::fs::remove_file(path).unwrap();
    }

    /// Chunk boundaries do not change UTF-8 validity, including incomplete EOF and ignored JSON fields.
    #[test]
    fn streamed_session_validates_utf8() {
        let valid = "aλ🦀z".as_bytes();
        for chunk_size in 1..=8 {
            let mut reader = Utf8Reader {
                source: std::io::Cursor::new(valid),
                tail: Vec::new(),
            };
            let mut result = Vec::new();
            let mut chunk = vec![0; chunk_size];
            loop {
                let count = reader.read(&mut chunk).unwrap();
                if count == 0 {
                    break;
                }
                result.extend_from_slice(&chunk[..count]);
            }
            assert_eq!(result, valid);
        }
        for invalid in [&b"\xf0\x9f"[..], &b"\xed\xa0\x80"[..], &b"\xff"[..]] {
            let mut reader = Utf8Reader {
                source: std::io::Cursor::new(invalid),
                tail: Vec::new(),
            };
            assert!(reader.read_to_end(&mut Vec::new()).is_err());
        }
        let path = std::env::temp_dir().join(format!("cmux-session-utf8-{}", uuid::Uuid::new_v4()));
        for input in [
            &b"{\"version\":1,\"active_index\":0,\"workspaces\":[],\"unknown\":\"\xff\"}"[..],
            &b"{\"version\":1,\"active_index\":0,\"workspaces\":[]} trailing"[..],
        ] {
            std::fs::write(&path, input).unwrap();
            assert!(load_session_from(&path).is_none());
        }
        std::fs::remove_file(path).unwrap();
    }

    /// The writer persists the newest burst snapshot and exits after sender closure without lost work.
    #[tokio::test]
    async fn snapshot_writer_coalesces_and_closes() {
        let path =
            std::env::temp_dir().join(format!("cmux-session-worker-{}", uuid::Uuid::new_v4()));
        let (sender, receiver) = tokio::sync::watch::channel(None);
        sender
            .send(Some(std::sync::Arc::new(dummy_session("first"))))
            .unwrap();
        let (_finish, finished) = tokio::sync::oneshot::channel();
        let worker = tokio::spawn(write_snapshots(receiver, path.clone(), finished));
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        sender
            .send(Some(std::sync::Arc::new(dummy_session("latest"))))
            .unwrap();
        drop(sender);
        tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(
            load_session_from(&path).unwrap().workspaces[0].name,
            "latest"
        );
        std::fs::remove_file(path).unwrap();
    }

    /// Finish persists an immediate last mutation even while publishers still hold channel handles.
    #[tokio::test]
    async fn snapshot_finish_flushes_latest_with_live_sender() {
        let path =
            std::env::temp_dir().join(format!("cmux-session-finish-{}", uuid::Uuid::new_v4()));
        let (sender, receiver) = tokio::sync::watch::channel(None);
        let (finish, finished) = tokio::sync::oneshot::channel();
        let worker = tokio::spawn(write_snapshots(receiver, path.clone(), finished));
        sender
            .send(Some(std::sync::Arc::new(dummy_session("old"))))
            .unwrap();
        sender
            .send(Some(std::sync::Arc::new(dummy_session("final"))))
            .unwrap();
        finish.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(
            load_session_from(&path).unwrap().workspaces[0].name,
            "final"
        );
        assert!(sender.is_closed());
        std::fs::remove_file(path).unwrap();
    }

    /// Final-save failures reach the joining owner rather than being reported as a successful flush.
    #[tokio::test]
    async fn snapshot_finish_reports_write_failure() {
        let path = std::env::temp_dir().join(format!("cmux-session-fail-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        let (sender, receiver) = tokio::sync::watch::channel(None);
        let (finish, finished) = tokio::sync::oneshot::channel();
        sender
            .send(Some(std::sync::Arc::new(dummy_session("final"))))
            .unwrap();
        finish.send(()).unwrap();
        assert!(write_snapshots(receiver, path.clone(), finished)
            .await
            .is_err());
        std::fs::remove_dir(path).unwrap();
    }

    /// Construct a minimal serializable workspace for persistence scenarios.
    fn dummy_session(name: &str) -> SessionData {
        SessionData {
            version: 1,
            resume_policy: Default::default(),
            inbox: Default::default(),
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
