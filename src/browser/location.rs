//! Reconcile page-driven navigation with saved browser locations without taking keyboard focus.
use base64::Engine as _;
use gtk4::prelude::*;
use serde::Deserialize;
use std::hash::{Hash, Hasher};
use std::{rc::Rc, time::Duration};

const COMMENT_FRAGMENT: &str = "#cmux-comment=";
const MAX_COMMENT_FRAGMENT_BYTES: usize = 8 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ViewerCommentRequest {
    repo_root: String,
    #[serde(flatten)]
    comment: crate::review_comments::NewComment,
}

/// Decode only comment requests originating from a generated private diff document.
fn viewer_comment_request(
    url: &str,
) -> Result<
    Option<(
        String,
        std::path::PathBuf,
        crate::review_comments::NewComment,
    )>,
    String,
> {
    viewer_comment_request_in(url, &cmux_platform::paths::data_dir().join("diffs"))
}

fn viewer_comment_request_in(
    url: &str,
    expected_directory: &std::path::Path,
) -> Result<
    Option<(
        String,
        std::path::PathBuf,
        crate::review_comments::NewComment,
    )>,
    String,
> {
    let Some((base, encoded)) = url.split_once(COMMENT_FRAGMENT) else {
        return Ok(None);
    };
    if !base.starts_with("file://")
        || !base.ends_with(".html")
        || encoded.is_empty()
        || encoded.len() > MAX_COMMENT_FRAGMENT_BYTES
    {
        return Err("untrusted or oversized diff comment request".into());
    }
    let viewer = url::Url::parse(base)
        .ok()
        .and_then(|url| url.to_file_path().ok())
        .and_then(|path| path.canonicalize().ok())
        .ok_or("diff comment viewer path is unavailable")?;
    let expected_directory = expected_directory
        .canonicalize()
        .map_err(|_| "diff comment viewer directory is unavailable")?;
    if !viewer.is_file() || viewer.parent() != Some(expected_directory.as_path()) {
        return Err("diff comment viewer is outside private storage".into());
    }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| "invalid diff comment request encoding")?;
    let request: ViewerCommentRequest =
        serde_json::from_slice(&bytes).map_err(|_| "invalid diff comment request payload")?;
    if request.repo_root.len() > 4096 {
        return Err("diff comment repository path is oversized".into());
    }
    Ok(Some((
        base.to_owned(),
        std::path::PathBuf::from(request.repo_root),
        request.comment,
    )))
}

/// GTK Entry delegates editing to a child; protect both direct and descendant keyboard focus.
pub(crate) fn is_editing(entry: &gtk4::Entry) -> bool {
    entry
        .root()
        .and_then(|root| root.focus())
        .is_some_and(|focus| focus == *entry || focus.is_ancestor(entry))
}

/// Refresh one live surface per second, round-robin; never initialize suspended pages.
/// One owned CLI worker runs at a time. Window destruction aborts both the loop and its worker.
pub fn start(state: &crate::app_state::AppStateRef, window: &gtk4::ApplicationWindow) {
    let Some(runtime) = state.borrow().runtime_handle.clone() else {
        return;
    };
    let weak = Rc::downgrade(state);
    let task = glib::MainContext::default().spawn_local(async move {
        let mut cursor = 0usize;
        let mut rejected_comments = std::collections::HashSet::new();
        loop {
            glib::timeout_future(Duration::from_secs(1)).await;
            let selected = {
                let Some(state) = weak.upgrade() else {
                    break;
                };
                let state = state.borrow();
                let tabs: Vec<_> = state
                    .split_engines
                    .iter()
                    .flat_map(|engine| engine.browser_tabs())
                    .filter(|widgets| {
                        state
                            .browser_sessions
                            .get(&widgets.uuid)
                            .is_some_and(|browser| browser.binary_path.is_some())
                    })
                    .collect();
                if tabs.is_empty() {
                    continue;
                }
                let widgets = &tabs[cursor % tabs.len()];
                cursor = cursor.wrapping_add(1);
                if is_editing(&widgets.url_entry) {
                    continue;
                }
                let browser = &state.browser_sessions[&widgets.uuid];
                let trace = uuid::Uuid::new_v4();
                (
                    widgets.uuid,
                    browser.session_identity(),
                    widgets.url_entry.text().to_string(),
                    runtime.spawn(browser.current_url_async(trace)),
                )
            };
            let (id, session, original, worker) = selected;
            let _cancel = crate::task::AbortOnDrop(worker.abort_handle());
            let Ok(Ok(Some(url))) = worker.await else {
                continue;
            };
            if let Some(request) = match viewer_comment_request(&url) {
                Ok(value) => value,
                Err(error) => {
                    let mut hasher = std::collections::hash_map::DefaultHasher::new();
                    url.hash(&mut hasher);
                    let identity = (id, hasher.finish());
                    if rejected_comments.len() >= 128 {
                        rejected_comments.clear();
                    }
                    if rejected_comments.insert(identity) {
                        crate::diagnostics::record(
                            "diff.comment.bridge",
                            serde_json::json!({"outcome": "rejected", "reason": error}),
                        );
                    }
                    continue;
                }
            } {
                rejected_comments.retain(|(surface, _)| *surface != id);
                let (base, root, comment) = request;
                let request_id = comment.id;
                let stored = runtime
                    .spawn_blocking(move || crate::review_comments::add(&root, comment))
                    .await;
                let (outcome, reason) = match stored {
                    Ok(Ok(_)) => ("success", None),
                    Ok(Err(error)) => ("error", Some(error.to_string())),
                    Err(error) => ("error", Some(error.to_string())),
                };
                crate::diagnostics::record(
                    "diff.comment.bridge",
                    serde_json::json!({
                        "surface_id": id,
                        "request_id": request_id,
                        "outcome": outcome,
                        "reason": reason,
                    }),
                );
                let Some(state) = weak.upgrade() else {
                    break;
                };
                let acknowledgement = {
                    let state = state.borrow();
                    let Some(browser) = state.browser_sessions.get(&id) else {
                        continue;
                    };
                    if browser.session_identity() != session {
                        continue;
                    }
                    let callback = if outcome == "success" {
                        "cmuxCommentAccepted"
                    } else {
                        "cmuxCommentRejected"
                    };
                    let script = format!(
                        "window.{callback}?.('{request_id}');history.replaceState(null,'',location.pathname+location.search);true"
                    );
                    browser.send_command_async(
                        "evaluate",
                        serde_json::json!({"script": script}),
                        Some(uuid::Uuid::new_v4()),
                    )
                };
                let _ = runtime.spawn(acknowledgement).await;
                if outcome == "success" {
                    let state = state.borrow();
                    if let Some(widgets) = state
                        .split_engines
                        .iter()
                        .flat_map(|engine| engine.browser_tabs())
                        .find(|widgets| widgets.uuid == id)
                    {
                        widgets.url_entry.set_text(&base);
                        state.trigger_session_save();
                    }
                }
                continue;
            }
            if url.len() > 8192 {
                continue;
            }
            let Some(state) = weak.upgrade() else {
                break;
            };
            let state = state.borrow();
            if state
                .browser_sessions
                .get(&id)
                .is_none_or(|browser| browser.session_identity() != session)
            {
                continue;
            }
            let widgets = state
                .split_engines
                .iter()
                .flat_map(|engine| engine.browser_tabs())
                .find(|widgets| widgets.uuid == id);
            if let Some(widgets) = widgets {
                if !is_editing(&widgets.url_entry)
                    && widgets.url_entry.text() == original
                    && url != original
                {
                    widgets.url_entry.set_text(&url);
                    state.trigger_session_save();
                }
            }
        }
    });
    window.connect_destroy(move |_| task.abort());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_comment_bridge_accepts_only_bounded_generated_viewer_fragments() {
        let directory = std::env::temp_dir().join(format!("cmux-comment-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let viewer = directory.join("a.html");
        std::fs::write(&viewer, "viewer").unwrap();
        let payload = serde_json::json!({
            "repoRoot": "/tmp/repo",
            "id": "10000000-0000-4000-8000-000000000001",
            "filePath": "src/main.rs",
            "side": "new",
            "startLine": 4,
            "endLine": 4,
            "lineText": "+line",
            "message": "review this",
        });
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&payload).unwrap());
        let viewer_url = url::Url::from_file_path(&viewer).unwrap();
        let url = format!("{viewer_url}{COMMENT_FRAGMENT}{encoded}");
        let (_, root, request) = viewer_comment_request_in(&url, &directory)
            .unwrap()
            .unwrap();
        assert_eq!(root, std::path::Path::new("/tmp/repo"));
        assert_eq!(request.file_path, "src/main.rs");
        assert!(viewer_comment_request_in(
            &format!("https://example.test/a.html{COMMENT_FRAGMENT}{encoded}"),
            &directory,
        )
        .is_err());
        assert!(
            viewer_comment_request_in(&format!("{viewer_url}#ordinary"), &directory)
                .unwrap()
                .is_none()
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    /// Real GTK entry delegation must be treated as editing; moving focus releases that protection.
    #[test]
    #[ignore = "requires GTK display; run in headless Linux CI"]
    fn browser_address_editing_tracks_delegated_focus() {
        gtk4::init().unwrap();
        let window = gtk4::Window::new();
        let layout = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        let entry = gtk4::Entry::new();
        let other = gtk4::Entry::new();
        layout.append(&entry);
        layout.append(&other);
        window.set_child(Some(&layout));
        window.present();
        assert!(entry.grab_focus());
        assert!(is_editing(&entry));
        assert!(!is_editing(&other));
        assert!(other.grab_focus());
        assert!(!is_editing(&entry));
        assert!(is_editing(&other));
        window.destroy();
    }
}
