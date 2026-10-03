//! Native Git side panel. Bounded read-only Git commands run on Tokio; GTK owns presentation.
use gtk4::{glib, prelude::*};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};

/// One selectable commit with Git's graph prefix and decoration retained for display.
#[derive(Debug)]
struct Version {
    oid: String,
    label: String,
}
/// One changed path; untracked paths need a no-index diff instead of a revision comparison.
#[derive(Clone, Debug)]
struct ChangedFile {
    path: String,
    untracked: bool,
}
/// Owned worker result; commit changes are compared with their first parent (root commits with empty tree).
struct Snapshot {
    versions: Vec<Version>,
    files: Vec<ChangedFile>,
}

/// Log bounded subprocess cleanup failures without touching GTK.
fn cleanup_failed(error: &std::io::Error) {
    eprintln!("cmux: Git view cleanup: {error}");
}

/// Execute literal Git arguments with bounded output/time and no user diff drivers or inherited repository overrides.
async fn git(root: &Path, args: &[&str], diff_exit: bool) -> Result<Vec<u8>, String> {
    let mut command = tokio::process::Command::new("git");
    command
        .args([
            "--no-optional-locks",
            "--literal-pathspecs",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "color.ui=false",
            "-C",
        ])
        .arg(root)
        .args(args);
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    command.env("GIT_TERMINAL_PROMPT", "0");
    let output = crate::task::run_output(
        command,
        Duration::from_secs(10),
        4 * 1024 * 1024,
        8192,
        cleanup_failed,
    )
    .await
    .map_err(|e| e.to_string())?;
    if output.status.success() || (diff_exit && output.status.code() == Some(1)) {
        Ok(output.stdout)
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

/// Decode NUL-delimited paths without silently addressing a different file for non-UTF8 names.
fn paths(bytes: Vec<u8>, untracked: bool) -> Result<Vec<ChangedFile>, String> {
    bytes
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| {
            Ok(ChangedFile {
                path: std::str::from_utf8(p)
                    .map_err(|_| "A filename is not valid UTF-8.")?
                    .to_owned(),
                untracked,
            })
        })
        .collect()
}

/// Resolve subdirectory workspaces to the repository root so every path uses one coordinate system.
async fn repository_root(directory: &Path) -> Result<PathBuf, String> {
    let bytes = git(directory, &["rev-parse", "--show-toplevel"], false).await?;
    let name = std::str::from_utf8(&bytes).map_err(|_| "Repository path is not valid UTF-8.")?;
    Ok(PathBuf::from(name.strip_suffix('\n').unwrap_or(name)))
}

/// Read the graph and selected changeset; Current includes staged, unstaged and untracked paths.
async fn snapshot(directory: &Path, revision: Option<&str>) -> Result<Snapshot, String> {
    let root = repository_root(directory).await?;
    let root = root.as_path();
    let head = git(root, &["rev-parse", "--verify", "HEAD"], false)
        .await
        .is_ok();
    let history = if head {
        git(
            root,
            &[
                "log",
                "--all",
                "HEAD",
                "--graph",
                "--date-order",
                "-n",
                "300",
                "--format=%x1f%H%x1f%h %s %d",
            ],
            false,
        )
        .await?
    } else {
        Vec::new()
    };
    let versions = String::from_utf8_lossy(&history)
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\u{1f}');
            let graph = parts.next()?;
            let oid = parts.next().unwrap_or_default();
            let title = parts.next().unwrap_or_default();
            Some(Version {
                oid: oid.to_owned(),
                label: format!("{graph}{title}"),
            })
        })
        .collect();
    let mut files = if let Some(revision) = revision {
        let parent = format!("{revision}^1");
        let mut args = vec![
            "diff-tree",
            "--root",
            "--no-commit-id",
            "--name-only",
            "-z",
            "-r",
            "--no-renames",
        ];
        if git(root, &["rev-parse", "--verify", &parent], false)
            .await
            .is_ok()
        {
            args.push(&parent);
        }
        args.push(revision);
        paths(git(root, &args, false).await?, false)?
    } else {
        let mut files = paths(
            git(
                root,
                &["diff", "--cached", "--name-only", "-z", "--no-renames"],
                false,
            )
            .await?,
            false,
        )?;
        files.extend(paths(
            git(root, &["diff", "--name-only", "-z", "--no-renames"], false).await?,
            false,
        )?);
        files.extend(paths(
            git(
                root,
                &["ls-files", "--others", "--exclude-standard", "-z"],
                false,
            )
            .await?,
            true,
        )?);
        files
    };
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files.dedup_by(|a, b| a.path == b.path);
    Ok(Snapshot { versions, files })
}

/// Read one file's unified diff. Current separates staged and unstaged edits even if they cancel out.
async fn file_diff(
    directory: &Path,
    revision: Option<&str>,
    file: &ChangedFile,
) -> Result<String, String> {
    let root = repository_root(directory).await?;
    let root = root.as_path();
    let bytes = if file.untracked {
        git(
            root,
            &[
                "diff",
                "--no-index",
                "--no-ext-diff",
                "--no-textconv",
                "--",
                "/dev/null",
                &file.path,
            ],
            true,
        )
        .await?
    } else if let Some(revision) = revision {
        let parent = format!("{revision}^1");
        if git(root, &["rev-parse", "--verify", &parent], false)
            .await
            .is_ok()
        {
            git(
                root,
                &[
                    "diff",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--no-renames",
                    &parent,
                    revision,
                    "--",
                    &file.path,
                ],
                false,
            )
            .await?
        } else {
            git(
                root,
                &[
                    "show",
                    "--format=",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--no-renames",
                    revision,
                    "--",
                    &file.path,
                ],
                false,
            )
            .await?
        }
    } else {
        let staged = git(
            root,
            &[
                "diff",
                "--cached",
                "--no-ext-diff",
                "--no-textconv",
                "--no-renames",
                "--",
                &file.path,
            ],
            false,
        )
        .await?;
        let unstaged = git(
            root,
            &[
                "diff",
                "--no-ext-diff",
                "--no-textconv",
                "--no-renames",
                "--",
                &file.path,
            ],
            false,
        )
        .await?;
        let mut result = Vec::new();
        for (title, patch) in [("Staged", staged), ("Unstaged", unstaged)] {
            if !patch.is_empty() {
                result.extend_from_slice(format!("{title}\n\n").as_bytes());
                result.extend(patch);
                result.push(b'\n');
            }
        }
        result
    };
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Weak presentation handles and a single cancellable request shared by version/file selection.
#[derive(Clone)]
struct View {
    root: PathBuf,
    runtime: tokio::runtime::Handle,
    versions: glib::WeakRef<gtk4::Box>,
    files: glib::WeakRef<gtk4::Box>,
    status: glib::WeakRef<gtk4::Label>,
    diff: glib::WeakRef<gtk4::TextView>,
    request: Rc<RefCell<Option<glib::JoinHandle<()>>>>,
}
impl View {
    /// Cancel the preceding selection before installing its replacement on the GTK context.
    fn cancel(&self) {
        if let Some(task) = self.request.borrow_mut().take() {
            task.abort();
        }
    }
    /// Select Current or a commit, then populate file buttons and its graph without blocking GTK.
    fn select(&self, revision: Option<String>) {
        self.cancel();
        let (Some(files), Some(status), Some(diff)) = (
            self.files.upgrade(),
            self.status.upgrade(),
            self.diff.upgrade(),
        ) else {
            return;
        };
        clear(&files);
        diff.buffer().set_text("");
        status.set_text("Loading changes…");
        let view = self.clone();
        let root = self.root.clone();
        let selected = revision.clone();
        let worker = self
            .runtime
            .spawn(async move { snapshot(&root, selected.as_deref()).await });
        let cancel = crate::task::AbortOnDrop(worker.abort_handle());
        let task = glib::MainContext::default().spawn_local(async move {
            let _cancel = cancel;
            let result = worker.await.unwrap_or_else(|e| Err(e.to_string()));
            let (Some(files), Some(versions), Some(status)) = (
                view.files.upgrade(),
                view.versions.upgrade(),
                view.status.upgrade(),
            ) else {
                return;
            };
            match result {
                Err(error) => status.set_text(&error),
                Ok(snapshot) => {
                    clear(&versions);
                    for version in snapshot.versions {
                        if version.oid.is_empty() {
                            let label = gtk4::Label::new(Some(&version.label));
                            label.set_xalign(0.0);
                            label.add_css_class("monospace");
                            versions.append(&label);
                            continue;
                        }
                        let button = listing_button(&version.label);
                        button.add_css_class("flat");
                        button.add_css_class("monospace");
                        button.set_tooltip_text(Some(&version.oid));
                        if revision.as_ref() == Some(&version.oid) {
                            button.add_css_class("suggested-action");
                        }
                        let view = view.clone();
                        button.connect_clicked(move |_| view.select(Some(version.oid.clone())));
                        versions.append(&button);
                    }
                    status.set_text(&format!(
                        "{} · {} changed files{}",
                        revision.as_deref().map(|s| &s[..8]).unwrap_or("Current"),
                        snapshot.files.len(),
                        if revision.is_some() {
                            " · compared with first parent"
                        } else {
                            ""
                        }
                    ));
                    if let Some(diff) = view.diff.upgrade() {
                        diff.buffer().set_text(if snapshot.files.is_empty() {
                            "No changes."
                        } else {
                            "Select a file to view its diff."
                        });
                    }
                    for file in snapshot.files {
                        let button = listing_button(&file.path);
                        button.add_css_class("flat");
                        button.set_tooltip_text(Some(&file.path));
                        let view = view.clone();
                        let revision = revision.clone();
                        button.connect_clicked(move |_| {
                            view.show_diff(revision.clone(), file.clone())
                        });
                        files.append(&button);
                    }
                }
            }
        });
        *self.request.borrow_mut() = Some(task);
    }
    /// Replace the diff reader asynchronously, aborting stale file work on another selection.
    fn show_diff(&self, revision: Option<String>, file: ChangedFile) {
        self.cancel();
        if let Some(diff) = self.diff.upgrade() {
            diff.buffer().set_text("Loading diff…");
        }
        let root = self.root.clone();
        let worker = self
            .runtime
            .spawn(async move { file_diff(&root, revision.as_deref(), &file).await });
        let cancel = crate::task::AbortOnDrop(worker.abort_handle());
        let diff = self.diff.clone();
        *self.request.borrow_mut() = Some(glib::MainContext::default().spawn_local(async move {
            let _cancel = cancel;
            let result = worker.await.unwrap_or_else(|e| Err(e.to_string()));
            if let Some(diff) = diff.upgrade() {
                render_diff(&diff, &result.unwrap_or_else(|e| e));
            }
        }));
    }
}

/// Remove the previous listing on GTK; button callbacks only hold weak widget references.
fn clear(container: &gtk4::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}
/// Display a selectable unified diff with colored additions, deletions and hunk headers.
fn render_diff(view: &gtk4::TextView, text: &str) {
    let buffer = view.buffer();
    buffer.set_text(text);
    for (line, content) in text.lines().enumerate() {
        let name = if content.starts_with('+') {
            "added"
        } else if content.starts_with('-') {
            "removed"
        } else if content.starts_with("@@") {
            "hunk"
        } else {
            continue;
        };
        if let Some(start) = buffer.iter_at_line(line as i32) {
            let mut end = start;
            end.forward_to_line_end();
            buffer.apply_tag_by_name(name, &start, &end);
        }
    }
    view.scroll_to_iter(&mut buffer.start_iter(), 0.0, false, 0.0, 0.0);
}
/// Left-align and ellipsize long paths/subjects so they never force the side panel wider.
fn listing_button(text: &str) -> gtk4::Button {
    let button = gtk4::Button::new();
    let label = gtk4::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    button.set_child(Some(&label));
    button
}

/// Wrap a GTK child in an expanding scroll area.
fn scroll(child: &impl IsA<gtk4::Widget>) -> gtk4::ScrolledWindow {
    gtk4::ScrolledWindow::builder()
        .child(child)
        .vexpand(true)
        .hexpand(true)
        .build()
}

/// Construct a Git tab with a pinned Current action, graph, file list and diff reader.
/// Requests are cancelled when the root widget is destroyed; remote workspaces have no local fallback.
pub fn create(directory: Option<PathBuf>) -> gtk4::Widget {
    let root = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    root.add_css_class("git-view");
    let current = gtk4::Button::with_label("Current — uncommitted changes");
    current.set_widget_name("git-current");
    root.append(&current);
    let status = gtk4::Label::new(None);
    status.set_wrap(true);
    root.append(&status);
    let versions = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    versions.set_widget_name("git-versions");
    let files = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    files.set_widget_name("git-files");
    versions.set_tooltip_text(Some("Version tree · latest 300 commits across local refs"));
    let lists = gtk4::Paned::new(gtk4::Orientation::Horizontal);
    lists.set_start_child(Some(&scroll(&versions)));
    lists.set_end_child(Some(&scroll(&files)));
    lists.set_position(220);
    lists.set_wide_handle(true);
    let diff = gtk4::TextView::new();
    diff.set_widget_name("git-diff");
    diff.set_editable(false);
    diff.set_monospace(true);
    diff.set_wrap_mode(gtk4::WrapMode::None);
    for (name, color) in [
        ("added", "#82c995"),
        ("removed", "#ee9292"),
        ("hunk", "#89b4fa"),
    ] {
        diff.buffer()
            .create_tag(Some(name), &[("foreground", &color)]);
    }
    let panes = gtk4::Paned::new(gtk4::Orientation::Vertical);
    panes.set_start_child(Some(&lists));
    panes.set_end_child(Some(&scroll(&diff)));
    panes.set_position(240);
    panes.set_wide_handle(true);
    panes.set_vexpand(true);
    root.append(&panes);
    let Some(directory) = directory else {
        status.set_text("This workspace has no local directory.");
        current.set_sensitive(false);
        return root.upcast();
    };
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        status.set_text("Git worker runtime unavailable.");
        return root.upcast();
    };
    let view = View {
        root: directory,
        runtime,
        versions: versions.downgrade(),
        files: files.downgrade(),
        status: status.downgrade(),
        diff: diff.downgrade(),
        request: Default::default(),
    };
    current.connect_clicked({
        let view = view.clone();
        move |_| view.select(None)
    });
    root.connect_destroy({
        let view = view.clone();
        move |_| view.cancel()
    });
    view.select(None);
    root.upcast()
}

/// Return an existing tab to a freshly loaded Current changeset when the header action is invoked.
pub fn select_current(widget: &gtk4::Widget) {
    if let Some(button) = widget.first_child().and_downcast::<gtk4::Button>() {
        button.emit_clicked();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Temporary repositories exercise real Git semantics; teardown removes only this fixture.
    struct Repo(PathBuf);
    impl Repo {
        /// Initialize an isolated repository with deterministic author identity.
        fn new() -> Self {
            let repo =
                Self(std::env::temp_dir().join(format!("cmux-git-view-{}", uuid::Uuid::new_v4())));
            std::fs::create_dir_all(&repo.0).unwrap();
            repo.run(&["init", "-b", "main"]);
            repo.run(&["config", "user.email", "fixture@example.invalid"]);
            repo.run(&["config", "user.name", "Fixture"]);
            repo
        }
        /// Run fixture setup commands, failing with Git's diagnostic output.
        fn run(&self, args: &[&str]) -> String {
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(&self.0)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        }
        /// Write one fixture path without changing unrelated files.
        fn write(&self, path: &str, text: &str) {
            std::fs::write(self.0.join(path), text).unwrap();
        }
        /// Commit all fixture changes and return the exact object identity.
        fn commit(&self, message: &str) -> String {
            self.run(&["add", "."]);
            self.run(&["commit", "-m", message]);
            self.run(&["rev-parse", "HEAD"])
        }
    }
    impl Drop for Repo {
        /// Remove only this test's uniquely named fixture directory.
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Current preserves staged/unstaged reversals, untracked paths, subdirectory roots and unborn history.
    #[tokio::test]
    async fn current_git_changes_include_all_layers() {
        let repo = Repo::new();
        repo.write("tracked.txt", "initial\n");
        repo.run(&["add", "."]);
        let unborn = snapshot(&repo.0, None).await.unwrap();
        assert!(unborn.versions.is_empty());
        assert!(file_diff(&repo.0, None, &unborn.files[0])
            .await
            .unwrap()
            .contains("+initial"));
        repo.commit("initial");
        repo.write("tracked.txt", "staged\n");
        repo.run(&["add", "."]);
        repo.write("tracked.txt", "initial\n");
        repo.write("new [file].txt", "untracked\n");
        std::fs::create_dir(repo.0.join("subdir")).unwrap();
        let root = repo.0.join("subdir");
        let current = snapshot(&root, None).await.unwrap();
        assert_eq!(current.files.len(), 2);
        let tracked = current
            .files
            .iter()
            .find(|file| file.path == "tracked.txt")
            .unwrap();
        let diff = file_diff(&root, None, tracked).await.unwrap();
        assert!(diff.contains("Staged\n") && diff.contains("Unstaged\n"));
        assert!(diff.contains("+staged") && diff.contains("-staged"));
        let untracked = current.files.iter().find(|file| file.untracked).unwrap();
        assert!(file_diff(&root, None, untracked)
            .await
            .unwrap()
            .contains("+untracked"));
        let outside = Repo::new();
        std::fs::remove_dir_all(outside.0.join(".git")).unwrap();
        assert!(snapshot(&outside.0, None).await.is_err());
    }

    /// History selects exact commits, root additions, first-parent merge changes, deletions and binary files.
    #[tokio::test]
    async fn git_history_selects_per_file_diffs() {
        let repo = Repo::new();
        repo.write("base.txt", "base\n");
        let first = repo.commit("root version");
        repo.run(&["checkout", "-b", "topic"]);
        repo.write("topic.txt", "topic\n");
        let topic = repo.commit("topic version");
        repo.run(&["checkout", "main"]);
        repo.write("main.txt", "main\n");
        repo.commit("main version");
        repo.run(&["merge", "--no-ff", "topic", "-m", "merge version"]);
        let merge = repo.run(&["rev-parse", "HEAD"]);
        let root = snapshot(&repo.0, Some(&first)).await.unwrap();
        assert_eq!(root.files.len(), 1);
        assert!(file_diff(&repo.0, Some(&first), &root.files[0])
            .await
            .unwrap()
            .contains("+base"));
        let merged = snapshot(&repo.0, Some(&merge)).await.unwrap();
        assert!(merged.versions.iter().any(|v| v.oid == topic));
        assert!(merged.versions.iter().any(|v| v.label.contains("|")));
        assert_eq!(merged.files[0].path, "topic.txt");
        assert!(file_diff(&repo.0, Some(&merge), &merged.files[0])
            .await
            .unwrap()
            .contains("+topic"));
        std::fs::remove_file(repo.0.join("base.txt")).unwrap();
        std::fs::write(repo.0.join("binary.bin"), b"\0\x01\x02").unwrap();
        let last = repo.commit("delete and binary");
        let snapshot = snapshot(&repo.0, Some(&last)).await.unwrap();
        assert_eq!(snapshot.files.len(), 2);
        assert!(file_diff(&repo.0, Some(&last), &snapshot.files[0])
            .await
            .unwrap()
            .contains("-base"));
        assert!(file_diff(&repo.0, Some(&last), &snapshot.files[1])
            .await
            .unwrap()
            .contains("Binary files"));
    }

    /// Find a named presentation widget without depending on GTK's internal scroller hierarchy.
    fn named(root: &gtk4::Widget, name: &str) -> Option<gtk4::Widget> {
        if root.widget_name() == name {
            return Some(root.clone());
        }
        let mut child = root.first_child();
        while let Some(widget) = child {
            if let Some(found) = named(&widget, name) {
                return Some(found);
            }
            child = widget.next_sibling();
        }
        None
    }
    /// Pump actual GTK delivery until a visible condition is met, with a bounded failure deadline.
    fn until(mut ready: impl FnMut() -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        while !ready() {
            assert!(
                std::time::Instant::now() < deadline,
                "Git view delivery timed out"
            );
            while glib::MainContext::default().pending() {
                glib::MainContext::default().iteration(false);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    /// Read the displayed patch exactly as a user can select/copy it.
    fn displayed(view: &gtk4::TextView) -> String {
        let b = view.buffer();
        b.text(&b.start_iter(), &b.end_iter(), false).to_string()
    }

    /// Real GTK buttons navigate Current/history/files and destruction cancels owned requests.
    #[test]
    #[ignore = "requires GTK display; run in GitHub Actions under Xvfb"]
    fn git_view_navigation_and_lifetime() {
        gtk4::init().unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let _entered = runtime.enter();
        let repo = Repo::new();
        repo.write("file.txt", "original\n");
        repo.commit("original version");
        repo.write("file.txt", "changed\n");
        let widget = create(Some(repo.0.clone()));
        let files = named(&widget, "git-files")
            .unwrap()
            .downcast::<gtk4::Box>()
            .unwrap();
        let versions = named(&widget, "git-versions")
            .unwrap()
            .downcast::<gtk4::Box>()
            .unwrap();
        let diff = named(&widget, "git-diff")
            .unwrap()
            .downcast::<gtk4::TextView>()
            .unwrap();
        until(|| files.first_child().is_some());
        files
            .first_child()
            .unwrap()
            .downcast::<gtk4::Button>()
            .unwrap()
            .emit_clicked();
        until(|| displayed(&diff).contains("+changed"));
        versions
            .first_child()
            .unwrap()
            .downcast::<gtk4::Button>()
            .unwrap()
            .emit_clicked();
        until(|| files.first_child().is_some());
        files
            .first_child()
            .unwrap()
            .downcast::<gtk4::Button>()
            .unwrap()
            .emit_clicked();
        until(|| displayed(&diff).contains("+original"));
        assert!(!displayed(&diff).contains("+changed"));
        select_current(&widget);
        until(|| files.first_child().is_some());
        files
            .first_child()
            .unwrap()
            .downcast::<gtk4::Button>()
            .unwrap()
            .emit_clicked();
        until(|| displayed(&diff).contains("+changed"));
        select_current(&widget);
        select_current(&widget);
        let weak = widget.downgrade();
        drop(widget);
        until(|| weak.upgrade().is_none());
    }
}
