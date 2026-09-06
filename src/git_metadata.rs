//! Bounded local Git discovery, independent of agent-published sidebar metadata.
use gtk4::prelude::*;
use std::{path::PathBuf, rc::Rc, time::Duration};

/// Latest successful local repository observation; never persisted as authoritative session state.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct GitMetadata {
    pub branch: String,
    pub dirty: bool,
}

/// Parse Git's stable porcelain-v2 branch headers without retaining filenames.
fn parse(bytes: &[u8]) -> Option<GitMetadata> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut branch = None;
    let mut dirty = false;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("# branch.head ") {
            if value.len() > 1024 || value.chars().any(char::is_control) {
                return None;
            }
            branch = Some(value.to_owned());
        } else if matches!(line.as_bytes().first(), Some(b'1' | b'2' | b'u' | b'?')) {
            dirty = true;
        }
    }
    Some(GitMetadata {
        branch: branch?,
        dirty,
    })
}

/// Probe one local directory with one owned subprocess, bounded output and a two-second execution budget.
async fn probe(directory: PathBuf, workspace_id: uuid::Uuid) -> Option<GitMetadata> {
    let mut command = tokio::process::Command::new("git");
    command
        .args([
            "--no-optional-locks",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-C",
        ])
        .arg(directory)
        .args([
            "status",
            "--porcelain=v2",
            "--branch",
            "--untracked-files=normal",
        ]);
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    command.env("GIT_TERMINAL_PROMPT", "0");
    let started = std::time::Instant::now();
    let output = crate::task::run_output(
        command,
        Duration::from_secs(2),
        256 * 1024,
        4096,
        cleanup_failed,
    )
    .await;
    let value = output
        .as_ref()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| parse(&out.stdout));
    crate::diagnostics::record(
        "workspace.git.probe",
        serde_json::json!({
            "workspace_id":workspace_id,
        "duration_us":started.elapsed().as_micros() as u64,
            "outcome":if value.is_some() { "repository" } else if output.is_err() { "error" } else { "unavailable" }
        }),
    );
    value
}

/// Report bounded cleanup failures without paths, branch names or file contents.
fn cleanup_failed(error: &std::io::Error) {
    crate::diagnostics::record(
        "workspace.git.cleanup_failed",
        serde_json::json!({"os_error":error.raw_os_error()}),
    );
}

/// Resolve the selected terminal's reported CWD, falling back to the workspace launch directory.
fn directory(state: &crate::app_state::AppState, index: usize) -> Option<PathBuf> {
    let workspace = state.workspaces.get(index)?;
    if workspace.remote_target.is_some() {
        return None;
    }
    let native = state.split_engines.get(index).and_then(|engine| {
        engine
            .active_pane_uuid()
            .and_then(|id| engine.find_surface_by_uuid(&id))
    });
    let current = native
        .map(|pointer| crate::ghostty::registry::working_directory(pointer as usize))
        .filter(|value| !value.is_empty());
    current
        .map(PathBuf::from)
        .or_else(|| workspace.working_directory.clone())
}

/// Refresh only the dedicated Git label; no terminal focus, row ownership or session-save changes.
pub fn render(label: &gtk4::Label, value: Option<&GitMetadata>) {
    label.set_text(
        &value
            .map(|value| format!("{}{}", value.branch, if value.dirty { " •" } else { "" }))
            .unwrap_or_default(),
    );
    label.set_visible(value.is_some());
}

/// Poll workspaces round-robin with a single in-flight probe; window destruction cancels owned work.
pub fn start(state: &crate::app_state::AppStateRef, window: &gtk4::ApplicationWindow) {
    let Some(runtime) = state.borrow().runtime_handle.clone() else {
        return;
    };
    let state = Rc::downgrade(state);
    let task = glib::MainContext::default().spawn_local(async move {
        let mut cursor = 0usize;
        loop {
            glib::timeout_future(Duration::from_secs(1)).await;
            let target = {
                let Some(state) = state.upgrade() else {
                    break;
                };
                let state = state.borrow();
                if state.workspaces.is_empty() {
                    continue;
                }
                let index = cursor % state.workspaces.len();
                cursor = cursor.wrapping_add(1);
                directory(&state, index).map(|directory| (state.workspaces[index].uuid, directory))
            };
            let Some((id, path)) = target else {
                continue;
            };
            let worker = runtime.spawn(probe(path.clone(), id));
            let _cancel = crate::task::AbortOnDrop(worker.abort_handle());
            let value = worker.await.unwrap_or_default();
            let Some(state) = state.upgrade() else {
                break;
            };
            let mut state = state.borrow_mut();
            let Some(index) = state
                .workspaces
                .iter()
                .position(|workspace| workspace.uuid == id)
            else {
                continue;
            };
            if directory(&state, index).as_ref() != Some(&path) {
                continue;
            }
            if state.workspaces[index].git == value {
                continue;
            }
            state.workspaces[index].git = value;
            if let Some(container) = state
                .sidebar_list
                .row_at_index(index as i32)
                .and_then(|row| row.child())
                .and_then(|row| row.first_child())
            {
                let mut child = container.first_child();
                while let Some(widget) = child {
                    child = widget.next_sibling();
                    if widget.has_css_class("workspace-git") {
                        if let Ok(label) = widget.downcast::<gtk4::Label>() {
                            render(&label, state.workspaces[index].git.as_ref());
                        }
                        break;
                    }
                }
            }
        }
    });
    window.connect_destroy(move |_| task.abort());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Porcelain headers retain branch identity while untracked, conflict and ordinary records mark dirty.
    #[test]
    fn parse_branch_and_changes() {
        assert_eq!(
            parse(b"# branch.oid (initial)\n# branch.head main\n"),
            Some(GitMetadata {
                branch: "main".into(),
                dirty: false
            })
        );
        for record in [
            "? new",
            "1 M. ignored fields",
            "2 R. ignored fields",
            "u UU ignored fields",
        ] {
            assert!(
                parse(format!("# branch.head feature\n{record}\n").as_bytes())
                    .unwrap()
                    .dirty
            );
        }
        assert_eq!(
            parse(b"# branch.head (detached)\n").unwrap().branch,
            "(detached)"
        );
        assert!(parse(b"fatal: not a repository").is_none());
        assert!(parse(format!("# branch.head {}\n", "x".repeat(1025)).as_bytes()).is_none());
    }
}
