//! Bounded diff preparation and composition over the public browser/surface APIs.

use super::socket_client::{CliError, SocketClient};
use super::{args::DiffLayout, args::DiffSource, browser_address, comments};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::io::{IsTerminal, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const MAX_PATCH_BYTES: usize = 32 * 1024 * 1024;
const MAX_VIEWERS: usize = 64;
const MAX_VIEWER_BYTES: u64 = 256 * 1024 * 1024;
const MAX_BASELINE_PATHS: usize = 10_000;
const MAX_BASELINE_CHANGED_BYTES: u64 = 256 * 1024 * 1024;

pub(super) struct PrepareRequest<'a> {
    pub input: Option<&'a str>,
    pub source: Option<DiffSource>,
    pub unstaged: bool,
    pub staged: bool,
    pub branch: bool,
    pub last_turn: bool,
    pub cwd: Option<&'a Path>,
    pub base: Option<&'a str>,
    pub surface: Option<&'a str>,
    pub session: Option<&'a str>,
    pub title: Option<&'a str>,
    pub layout: DiffLayout,
    pub font_size: Option<f64>,
}

pub(super) struct PreparedDiff {
    path: PathBuf,
    url: String,
    title: String,
    source: String,
    layout: DiffLayout,
}

pub(super) struct PreparedDocument {
    pub path: PathBuf,
    pub url: String,
    pub title: String,
    pub metadata: serde_json::Map<String, Value>,
}

#[derive(Serialize)]
struct ViewerConfig<'a> {
    title: &'a str,
    source: &'a str,
    patch: &'a str,
    layout: DiffLayout,
    font_size: f64,
    repo_root: Option<&'a str>,
    comments: &'a [comments::Comment],
}

pub(super) fn prepare(request: PrepareRequest<'_>) -> Result<PreparedDiff, CliError> {
    if request
        .font_size
        .is_some_and(|size| !size.is_finite() || !(1.0..=96.0).contains(&size))
    {
        return Err(CliError::Command(
            "--font-size must be a positive number no larger than 96".into(),
        ));
    }
    let source = selected_source(&request)?;
    let repository = if source.is_some() {
        Some(repository_root(
            request.cwd.unwrap_or_else(|| Path::new(".")),
        )?)
    } else {
        None
    };
    let review_comments = repository
        .as_deref()
        .map(comments::for_viewer)
        .transpose()?
        .unwrap_or_default();
    let repository_label = repository.as_ref().map(|path| path.to_string_lossy());
    let ambient_surface = std::env::var("CMUX_SURFACE_ID").ok();
    let surface = request.surface.or(ambient_surface.as_deref());
    let (patch, source_label, default_title) = match source {
        None => read_patch_input(request.input)?,
        Some(source) => {
            read_git_source(source, request.cwd, request.base, surface, request.session)?
        }
    };
    if patch.trim().is_empty() && source.is_none() {
        return Err(CliError::Command("diff input is empty".into()));
    }
    let title = request.title.unwrap_or(&default_title).trim();
    if title.is_empty() || title.len() > 256 || title.chars().any(char::is_control) {
        return Err(CliError::Command(
            "diff title must contain 1-256 printable bytes".into(),
        ));
    }
    let directory = cmux_platform::paths::data_dir().join("diffs");
    cmux_platform::filesystem::create_private_directory(&directory)
        .map_err(|error| CliError::Command(format!("create diff directory: {error}")))?;
    let path = directory.join(format!("diff-{}.html", uuid::Uuid::new_v4()));
    let config = ViewerConfig {
        title,
        source: &source_label,
        patch: &patch,
        layout: request.layout,
        font_size: request.font_size.unwrap_or(13.0),
        repo_root: repository_label.as_deref(),
        comments: &review_comments,
    };
    let mut config = serde_json::to_string(&config)
        .map_err(|error| CliError::Command(format!("encode diff viewer: {error}")))?;
    // A JSON string embedded in a script must not be able to terminate that script.
    config = config
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    let html = VIEWER_HTML.replace("__CMUX_DIFF_CONFIG__", &config);
    prune_viewers(&directory, html.len() as u64)?;
    cmux_platform::filesystem::atomic_write(&path, html.as_bytes())
        .map_err(|error| CliError::Command(format!("write diff viewer: {error}")))?;
    let url = browser_address::normalize(path.to_string_lossy().as_ref());
    Ok(PreparedDiff {
        path,
        url,
        title: title.to_owned(),
        source: source_label,
        layout: request.layout,
    })
}

fn selected_source(request: &PrepareRequest<'_>) -> Result<Option<DiffSource>, CliError> {
    let flags = [
        request.unstaged.then_some(DiffSource::Unstaged),
        request.staged.then_some(DiffSource::Staged),
        request.branch.then_some(DiffSource::Branch),
        request.last_turn.then_some(DiffSource::LastTurn),
    ];
    let flag = flags.into_iter().flatten().next();
    if request.source.is_some() && flag.is_some() {
        return Err(CliError::Command("diff accepts only one Git source".into()));
    }
    if request.input.is_some() && (request.source.is_some() || flag.is_some()) {
        return Err(CliError::Command(
            "diff accepts either a patch file or a Git source".into(),
        ));
    }
    Ok(request.source.or(flag))
}

fn read_patch_input(input: Option<&str>) -> Result<(String, String, String), CliError> {
    match input {
        Some("-") | None => {
            if input.is_none() && std::io::stdin().is_terminal() {
                return Err(CliError::Command(
                    "diff requires a patch file, piped stdin, or a Git source".into(),
                ));
            }
            let patch = read_bounded(std::io::stdin().lock(), "stdin")?;
            Ok((patch, "stdin".into(), "cmux diff".into()))
        }
        Some(input) => {
            let path = PathBuf::from(input);
            let file = cmux_platform::filesystem::open_regular_read(&path)
                .map_err(|error| CliError::Command(format!("open {}: {error}", path.display())))?;
            let patch = read_bounded(file, &path.display().to_string())?;
            let title = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("cmux diff")
                .to_owned();
            Ok((patch, path.display().to_string(), title))
        }
    }
}

fn read_bounded(mut reader: impl Read, label: &str) -> Result<String, CliError> {
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take(MAX_PATCH_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| CliError::Command(format!("read diff {label}: {error}")))?;
    if bytes.len() > MAX_PATCH_BYTES {
        return Err(CliError::Command(format!(
            "diff {label} exceeds the {} MiB limit",
            MAX_PATCH_BYTES / 1024 / 1024
        )));
    }
    String::from_utf8(bytes).map_err(|_| CliError::Command(format!("diff {label} is not UTF-8")))
}

fn read_git_source(
    source: DiffSource,
    cwd: Option<&Path>,
    base: Option<&str>,
    surface: Option<&str>,
    session: Option<&str>,
) -> Result<(String, String, String), CliError> {
    let root = repository_root(cwd.unwrap_or_else(|| Path::new(".")))?;
    if matches!(source, DiffSource::LastTurn) {
        let surface = surface.ok_or_else(|| {
            CliError::Command("cmux diff --last-turn requires --surface or CMUX_SURFACE_ID".into())
        })?;
        let reference = baseline_ref(surface, session)?;
        let baseline = match git_text(&root, &["rev-parse", "--verify", &reference], 64 * 1024) {
            Ok(value) => value.trim().to_owned(),
            Err(_) => {
                return Ok((
                    String::new(),
                    format!("git last-turn {surface}"),
                    "Last turn changes".into(),
                ))
            }
        };
        let current = snapshot_tree(&root, Instant::now() + Duration::from_secs(30))?;
        let patch = git_text(
            &root,
            &[
                "diff",
                "--no-ext-diff",
                "--binary",
                &baseline,
                &current,
                "--",
            ],
            MAX_PATCH_BYTES,
        )?;
        return Ok((
            patch,
            format!("git last-turn {surface}"),
            "Last turn changes".into(),
        ));
    }
    let (tail, label, title): (Vec<String>, String, &str) = match source {
        DiffSource::Unstaged => (vec!["--".into()], "git unstaged".into(), "Unstaged changes"),
        DiffSource::Staged => (
            vec!["--cached".into(), "--".into()],
            "git staged".into(),
            "Staged changes",
        ),
        DiffSource::Branch => {
            let base = match base {
                Some(value) => value.to_owned(),
                None => default_branch_base(&root)?,
            };
            let merge_base = git_text(&root, &["merge-base", "HEAD", &base], 64 * 1024)?;
            (
                vec![merge_base.trim().into(), "--".into()],
                format!("git branch {base}"),
                "Branch changes",
            )
        }
        DiffSource::LastTurn => unreachable!(),
    };
    let mut args = vec![
        "diff".to_owned(),
        "--no-ext-diff".to_owned(),
        "--binary".to_owned(),
    ];
    args.extend(tail);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let patch = git_text(&root, &refs, MAX_PATCH_BYTES)?;
    Ok((patch, label, title.into()))
}

pub(super) fn repository_root(cwd: &Path) -> Result<PathBuf, CliError> {
    let root = git_text(cwd, &["rev-parse", "--show-toplevel"], 64 * 1024)?;
    let root = PathBuf::from(root.trim());
    std::fs::canonicalize(&root)
        .map_err(|error| CliError::Command(format!("canonicalize Git repository: {error}")))
}

fn default_branch_base(root: &Path) -> Result<String, CliError> {
    for args in [
        ["rev-parse", "--abbrev-ref", "@{upstream}"],
        ["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    ] {
        if let Ok(value) = git_text(root, &args, 64 * 1024) {
            let value = value.trim();
            if !value.is_empty() {
                return Ok(value.to_owned());
            }
        }
    }
    for candidate in ["origin/main", "origin/master", "main", "master"] {
        if git_text(root, &["rev-parse", "--verify", candidate], 64 * 1024).is_ok() {
            return Ok(candidate.into());
        }
    }
    Err(CliError::Command(
        "cannot determine branch diff base; pass --base".into(),
    ))
}

fn git_text(cwd: &Path, args: &[&str], limit: usize) -> Result<String, CliError> {
    git_text_with(cwd, args, limit, &[], Duration::from_secs(30))
}

fn git_text_with(
    cwd: &Path,
    args: &[&str],
    limit: usize,
    environment: &[(&str, &str)],
    deadline: Duration,
) -> Result<String, CliError> {
    let directory = cmux_platform::paths::state_dir().join("diff-work");
    cmux_platform::filesystem::create_private_directory(&directory)
        .map_err(|error| CliError::Command(format!("create diff work directory: {error}")))?;
    let token = uuid::Uuid::new_v4();
    let stdout_path = directory.join(format!("{token}.out"));
    let stderr_path = directory.join(format!("{token}.err"));
    let stdout = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&stdout_path)
        .map_err(|error| CliError::Command(format!("create Git output: {error}")))?;
    let stderr = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&stderr_path)
    {
        Ok(file) => file,
        Err(error) => {
            let _ = std::fs::remove_file(&stdout_path);
            return Err(CliError::Command(format!(
                "create Git error output: {error}"
            )));
        }
    };
    let cleanup = TemporaryGitOutput {
        stdout: stdout_path.clone(),
        stderr: stderr_path.clone(),
    };
    let mut child = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .envs(environment.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|error| CliError::Command(format!("start git: {error}")))?;
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| CliError::Command(format!("wait for git: {error}")))?
        {
            break status;
        }
        let output_size = std::fs::metadata(&stdout_path)
            .map(|value| value.len())
            .unwrap_or(0);
        let error_size = std::fs::metadata(&stderr_path)
            .map(|value| value.len())
            .unwrap_or(0);
        if output_size > limit as u64 || error_size > 64 * 1024 {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CliError::Command(format!(
                "git output exceeds the {} MiB diff limit",
                limit / 1024 / 1024
            )));
        }
        if started.elapsed() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CliError::Command(format!(
                "git {} exceeded its {} second deadline",
                args.first().copied().unwrap_or("command"),
                deadline.as_secs()
            )));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if !status.success() {
        let error = cmux_platform::filesystem::read_text_bounded(&stderr_path, 64 * 1024)
            .unwrap_or_else(|_| "Git command failed".into());
        return Err(CliError::Command(format!(
            "git {} failed: {}",
            args.first().copied().unwrap_or("command"),
            error.trim().chars().take(2048).collect::<String>()
        )));
    }
    let output = cmux_platform::filesystem::read_text_bounded(&stdout_path, limit)
        .map_err(|error| CliError::Command(format!("read Git output: {error}")))?;
    drop(cleanup);
    Ok(output)
}

/// Record the repository state at an accepted agent prompt boundary.
pub(super) fn record_baseline(cwd: &Path, surface: &str, session: &str) -> Result<(), CliError> {
    let deadline = Instant::now() + Duration::from_secs(4);
    let root = git_text_with(
        cwd,
        &["rev-parse", "--show-toplevel"],
        64 * 1024,
        &[],
        remaining(deadline)?,
    )?;
    let root = PathBuf::from(root.trim());
    let tree = snapshot_tree(&root, deadline)?;
    for reference in [
        baseline_ref(surface, None)?,
        baseline_ref(surface, Some(session))?,
    ] {
        git_text_with(
            &root,
            &["update-ref", &reference, &tree],
            64 * 1024,
            &[],
            remaining(deadline)?,
        )?;
    }
    Ok(())
}

fn baseline_ref(surface: &str, session: Option<&str>) -> Result<String, CliError> {
    let surface = uuid::Uuid::parse_str(surface)
        .map_err(|_| CliError::Command("last-turn surface must be a UUID".into()))?;
    let suffix = match session.filter(|value| !value.trim().is_empty()) {
        Some(session) => {
            let digest = Sha256::digest(session.as_bytes());
            let hash = digest[..12]
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            format!("session-{hash}")
        }
        None => "latest".into(),
    };
    Ok(format!("refs/cmux/last-turn/{surface}/{suffix}"))
}

fn snapshot_tree(root: &Path, deadline: Instant) -> Result<String, CliError> {
    validate_snapshot_size(root, deadline)?;
    let directory = cmux_platform::paths::state_dir().join("diff-work");
    cmux_platform::filesystem::create_private_directory(&directory)
        .map_err(|error| CliError::Command(format!("create diff work directory: {error}")))?;
    let index = directory.join(format!("{}.index", uuid::Uuid::new_v4()));
    let index_text = index.to_string_lossy().into_owned();
    let cleanup = TemporaryIndex(index);
    let environment = [("GIT_INDEX_FILE", index_text.as_str())];
    let read_tree = if git_text_with(
        root,
        &["rev-parse", "--verify", "HEAD"],
        64 * 1024,
        &[],
        remaining(deadline)?,
    )
    .is_ok()
    {
        ["read-tree", "HEAD"]
    } else {
        ["read-tree", "--empty"]
    };
    git_text_with(
        root,
        &read_tree,
        64 * 1024,
        &environment,
        remaining(deadline)?,
    )?;
    git_text_with(
        root,
        &["add", "-A", "--", "."],
        64 * 1024,
        &environment,
        remaining(deadline)?,
    )?;
    let tree = git_text_with(
        root,
        &["write-tree"],
        64 * 1024,
        &environment,
        remaining(deadline)?,
    )?;
    drop(cleanup);
    Ok(tree.trim().to_owned())
}

fn validate_snapshot_size(root: &Path, deadline: Instant) -> Result<(), CliError> {
    let commands: [&[&str]; 3] = [
        &["diff", "--name-only", "-z", "--"],
        &["diff", "--cached", "--name-only", "-z", "--"],
        &["ls-files", "--others", "--exclude-standard", "-z"],
    ];
    let mut paths = BTreeSet::new();
    for args in commands {
        let names = git_text_with(root, args, 4 * 1024 * 1024, &[], remaining(deadline)?)?;
        paths.extend(
            names
                .split('\0')
                .filter(|value| !value.is_empty())
                .map(str::to_owned),
        );
        if paths.len() > MAX_BASELINE_PATHS {
            return Err(CliError::Command(format!(
                "last-turn baseline exceeds {MAX_BASELINE_PATHS} changed paths"
            )));
        }
    }
    let mut bytes = 0_u64;
    for relative in paths {
        let path = Path::new(&relative);
        if path.is_absolute()
            || path
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            return Err(CliError::Command(
                "Git returned an invalid changed path".into(),
            ));
        }
        match std::fs::symlink_metadata(root.join(path)) {
            Ok(metadata) if metadata.is_file() => bytes = bytes.saturating_add(metadata.len()),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(CliError::Command(format!("inspect changed path: {error}"))),
        }
        if bytes > MAX_BASELINE_CHANGED_BYTES {
            return Err(CliError::Command(
                "last-turn changed files exceed 256 MiB".into(),
            ));
        }
    }
    Ok(())
}

fn remaining(deadline: Instant) -> Result<Duration, CliError> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| CliError::Command("last-turn baseline exceeded its deadline".into()))
}

struct TemporaryIndex(PathBuf);

impl Drop for TemporaryIndex {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        let mut lock = self.0.as_os_str().to_owned();
        lock.push(".lock");
        let _ = std::fs::remove_file(PathBuf::from(lock));
    }
}

struct TemporaryGitOutput {
    stdout: PathBuf,
    stderr: PathBuf,
}

impl Drop for TemporaryGitOutput {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.stdout);
        let _ = std::fs::remove_file(&self.stderr);
    }
}

fn prune_viewers(directory: &Path, incoming: u64) -> Result<(), CliError> {
    let mut entries = std::fs::read_dir(directory)
        .map_err(|error| CliError::Command(format!("scan diff directory: {error}")))?
        .flatten()
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            (metadata.is_file()
                && entry.file_name().to_string_lossy().starts_with("diff-")
                && entry
                    .path()
                    .extension()
                    .is_some_and(|value| value == "html"))
            .then(|| (metadata.modified().ok(), metadata.len(), entry.path()))
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.0);
    let mut total = entries.iter().map(|entry| entry.1).sum::<u64>();
    while entries.len() >= MAX_VIEWERS || total.saturating_add(incoming) > MAX_VIEWER_BYTES {
        let (_, size, path) = entries.remove(0);
        std::fs::remove_file(&path)
            .map_err(|error| CliError::Command(format!("prune {}: {error}", path.display())))?;
        total = total.saturating_sub(size);
    }
    Ok(())
}

pub(super) fn open(
    client: &mut SocketClient,
    prepared: PreparedDiff,
    workspace: Option<&str>,
    surface: Option<&str>,
    focus: bool,
    json_output: bool,
) -> Result<(), CliError> {
    let mut metadata = serde_json::Map::new();
    metadata.insert("source".into(), Value::String(prepared.source));
    metadata.insert(
        "layout".into(),
        serde_json::to_value(prepared.layout)
            .map_err(|error| CliError::Output(error.to_string()))?,
    );
    open_document(
        client,
        PreparedDocument {
            path: prepared.path,
            url: prepared.url,
            title: prepared.title,
            metadata,
        },
        workspace,
        surface,
        focus,
        json_output,
        "diff",
    )
}

pub(super) fn open_document(
    client: &mut SocketClient,
    prepared: PreparedDocument,
    workspace: Option<&str>,
    surface: Option<&str>,
    focus: bool,
    json_output: bool,
    subject: &str,
) -> Result<(), CliError> {
    let caller_surface = surface
        .map(str::to_owned)
        .or_else(|| std::env::var("CMUX_SURFACE_ID").ok());
    let workspace = workspace
        .map(str::to_owned)
        .or_else(|| std::env::var("CMUX_WORKSPACE_ID").ok());
    let panes_before = client.call("pane.list", json!({}))?;
    let previous_focus = focused_surface(&panes_before);
    let target_pane = caller_surface
        .as_deref()
        .and_then(|surface| pane_for_surface(&panes_before, surface));
    if caller_surface.is_some() && target_pane.is_none() {
        return Err(CliError::Command(format!(
            "{subject} target surface not found"
        )));
    }
    let opened = client.call(
        "browser.open",
        json!({"url":prepared.url,"workspace":workspace}),
    )?;
    let uuid = opened
        .get("uuid")
        .and_then(Value::as_str)
        .ok_or_else(|| CliError::Protocol("browser.open response omitted surface UUID".into()))?
        .to_owned();
    let placed = (|| {
        let panes = client.call("pane.list", json!({}))?;
        let source_pane = pane_for_surface(&panes, &uuid)
            .ok_or_else(|| CliError::Protocol("new browser surface has no pane".into()))?;
        let target_pane = target_pane.unwrap_or_else(|| source_pane.clone());
        if target_pane != source_pane {
            client.call(
                "surface.move",
                json!({"id":uuid,"workspace":workspace,"pane":target_pane,"focus":false}),
            )?;
        }
        client.call(
            "surface.drag_to_split",
            json!({"id":uuid,"pane":target_pane,"direction":"right"}),
        )
    })();
    let split = match placed {
        Ok(value) => value,
        Err(error) => {
            let _ = client.call("browser.close", json!({"surface_ref":uuid}));
            return Err(error);
        }
    };
    if focus {
        client.call("surface.focus", json!({"id":uuid}))?;
    } else if let Some(previous) = previous_focus {
        client.call("surface.focus", json!({"id":previous}))?;
    }
    let mut result = prepared.metadata;
    result.insert("uuid".into(), Value::String(uuid.clone()));
    result.insert(
        "surface_ref".into(),
        opened.get("surface_ref").cloned().unwrap_or(Value::Null),
    );
    result.insert(
        "pane".into(),
        split.get("pane").cloned().unwrap_or(Value::Null),
    );
    result.insert("path".into(), json!(prepared.path));
    result.insert("url".into(), Value::String(prepared.url));
    result.insert("title".into(), Value::String(prepared.title));
    let result = Value::Object(result);
    if json_output {
        println!(
            "{}",
            serde_json::to_string(&result).map_err(|error| CliError::Output(error.to_string()))?
        );
    } else {
        println!(
            "OK surface={uuid} pane={}",
            split
                .get("pane")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        );
    }
    Ok(())
}

fn pane_for_surface(payload: &Value, surface: &str) -> Option<String> {
    payload.get("panes")?.as_array()?.iter().find_map(|pane| {
        pane.get("surface_ids")
            .and_then(Value::as_array)
            .is_some_and(|ids| ids.iter().any(|id| id.as_str() == Some(surface)))
            .then(|| pane.get("id")?.as_str().map(str::to_owned))
            .flatten()
    })
}

fn focused_surface(payload: &Value) -> Option<String> {
    payload.get("panes")?.as_array()?.iter().find_map(|pane| {
        pane.get("focused")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            .then(|| pane.get("active_surface_uuid")?.as_str().map(str::to_owned))
            .flatten()
    })
}

const VIEWER_HTML: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>cmux diff</title><style>
:root{color-scheme:light dark;--bg:#101216;--panel:#171a20;--fg:#d7dae0;--muted:#89909b;--border:#303640;--add:#143d2a;--del:#4b2027;--accent:#78a9ff;--comment:#312813}*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);font:13px ui-monospace,SFMono-Regular,Consolas,monospace}header{height:46px;display:flex;align-items:center;gap:10px;padding:7px 12px;border-bottom:1px solid var(--border);position:sticky;top:0;background:var(--panel);z-index:3}button,input,textarea{font:inherit;color:inherit;background:#222731;border:1px solid var(--border);border-radius:5px;padding:5px 8px}button{cursor:pointer}button.active{border-color:var(--accent)}#source{color:var(--muted);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;flex:1}#comment-count{color:#e9c46a;white-space:nowrap}#body{display:grid;grid-template-columns:260px minmax(0,1fr);height:calc(100vh - 46px)}nav{overflow:auto;border-right:1px solid var(--border);padding:8px}nav button{display:block;width:100%;text-align:left;border:0;background:transparent;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.file{overflow:auto}.file-title{padding:10px 12px;background:var(--panel);border-bottom:1px solid var(--border);position:sticky;top:0}.line{display:grid;grid-template-columns:54px 54px minmax(0,1fr);min-height:20px;border-bottom:1px solid #ffffff08}.line,.split-side{cursor:pointer}.line.add{background:var(--add)}.line.del{background:var(--del)}.line.hunk{color:#9ab7e8;background:#182437}.no{color:var(--muted);text-align:right;padding:2px 7px;border-right:1px solid var(--border);user-select:none}.code{white-space:pre-wrap;overflow-wrap:anywhere;padding:2px 8px}.split-line{display:grid;grid-template-columns:1fr 1fr}.split-side{display:grid;grid-template-columns:54px minmax(0,1fr);min-width:0;border-bottom:1px solid #ffffff08}.split-side.add{background:var(--add)}.split-side.del{background:var(--del)}.review-comment{margin:5px 12px 8px 116px;padding:8px 10px;border:1px solid #735f2c;border-radius:6px;background:var(--comment);white-space:pre-wrap}.comment-meta{display:flex;align-items:center;gap:8px;margin-bottom:5px;color:#e9c46a}.comment-meta button{margin-left:auto;padding:2px 6px}.comment-message{overflow-wrap:anywhere}dialog{color:var(--fg);background:var(--panel);border:1px solid var(--border);border-radius:8px;width:min(560px,90vw)}dialog::backdrop{background:#0009}dialog form{display:grid;gap:10px}dialog textarea{min-height:120px;resize:vertical}dialog menu{display:flex;justify-content:flex-end;gap:8px;margin:0;padding:0}#comment-anchor,#comment-status{color:var(--muted)}#empty,#pagebar{padding:30px;color:var(--muted);text-align:center}mark{background:#8d6b12;color:inherit}@media(max-width:760px){#body{grid-template-columns:1fr}nav{display:none}.review-comment{margin-left:12px}}
</style></head><body><header><strong id="title"></strong><span id="source"></span><span id="comment-count"></span><button id="unified">Unified</button><button id="split">Split</button><input id="find" type="search" placeholder="Find"><button id="copy">Copy patch</button></header><div id="body"><nav id="files"></nav><main class="file"><div class="file-title" id="file-title"></div><div id="viewer"></div><div id="pagebar"></div></main></div><dialog id="comment-dialog"><form id="comment-form"><strong>Add review comment</strong><span id="comment-anchor"></span><textarea id="comment-message" maxlength="4096" required placeholder="Describe the requested change"></textarea><span id="comment-status"></span><menu><button type="button" id="comment-cancel">Cancel</button><button type="submit">Save comment</button></menu></form></dialog>
<script>const C=__CMUX_DIFF_CONFIG__;document.title=C.title;document.getElementById('title').textContent=C.title;document.getElementById('source').textContent=C.source;document.body.style.fontSize=C.fontSize+'px';
const parse=p=>{const out=[];if(!p.trim())return out;let f=null,old=0,neu=0;for(const text of p.split(/\n/)){if(text.startsWith('diff --git ')){f={name:text.replace(/^diff --git a\//,'').replace(/ b\/.*$/,''),lines:[]};out.push(f)}if(!f){f={name:'Patch',lines:[]};out.push(f)}let kind='ctx',o='',n='';const h=text.match(/^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/);if(h){old=+h[1];neu=+h[2];kind='hunk'}else if(text.startsWith('+')&&!text.startsWith('+++')){kind='add';n=neu++}else if(text.startsWith('-')&&!text.startsWith('---')){kind='del';o=old++}else if(!text.startsWith('\\')){o=old++;n=neu++}f.lines.push({text,kind,o,n})}return out};
const files=parse(C.patch),list=document.getElementById('files'),view=document.getElementById('viewer'),title=document.getElementById('file-title'),bar=document.getElementById('pagebar');let selected=0,page=0,layout=C.layout,query='';const PAGE=4000,commentCount=document.getElementById('comment-count');commentCount.textContent=C.comments.length?`${C.comments.length} pending comment${C.comments.length===1?'':'s'}`:'';commentCount.title=C.repoRoot||'';
const esc=s=>{const x=document.createElement('span');x.textContent=s;if(!query)return x;const q=query.toLowerCase(),v=s.toLowerCase();let at=0,pos;const frag=document.createDocumentFragment();while((pos=v.indexOf(q,at))>=0){frag.append(document.createTextNode(s.slice(at,pos)));const m=document.createElement('mark');m.textContent=s.slice(pos,pos+query.length);frag.append(m);at=pos+query.length}frag.append(document.createTextNode(s.slice(at)));return frag};
function line(row){const e=document.createElement('div');e.className='line '+row.kind;for(const value of [row.o,row.n]){const n=document.createElement('span');n.className='no';n.textContent=value;e.append(n)}const c=document.createElement('span');c.className='code';c.append(esc(row.text));e.append(c);if(row.o||row.n)e.onclick=()=>openComposer(row,row.n?'new':'old');return e}function side(row,which){const e=document.createElement('div');e.className='split-side '+row.kind;const value=which==='old'?row.o:row.n,n=document.createElement('span');n.className='no';n.textContent=value;const c=document.createElement('span');c.className='code';if((which==='old'&&row.kind!=='add')||(which==='new'&&row.kind!=='del'))c.append(esc(row.text));e.append(n,c);if(value)e.onclick=()=>openComposer(row,which);return e}
function anchored(file,row){return C.comments.filter(c=>c.filePath===file&&((c.side==='old'&&row.o===c.endLine)||(c.side==='new'&&row.n===c.endLine)))}function commentCard(c){const e=document.createElement('article');e.className='review-comment';e.dataset.commentId=c.id;const meta=document.createElement('div');meta.className='comment-meta';const label=document.createElement('span');label.textContent=`${c.side} ${c.startLine}${c.endLine===c.startLine?'':`–${c.endLine}`}`;const copy=document.createElement('button');copy.textContent='Copy for agent';copy.onclick=async()=>{await navigator.clipboard.writeText(c.submissionText);copy.textContent='Copied'};meta.append(label,copy);const body=document.createElement('div');body.className='comment-message';body.append(esc(c.message));e.append(meta,body);return e}
const dialog=document.getElementById('comment-dialog'),message=document.getElementById('comment-message'),commentStatus=document.getElementById('comment-status');let anchor=null;function openComposer(row,side){const line=side==='old'?row.o:row.n;if(!line||!C.repoRoot)return;anchor={filePath:files[selected].name,side,startLine:line,endLine:line,lineText:row.text};document.getElementById('comment-anchor').textContent=`${anchor.filePath}:${line} (${side})`;commentStatus.textContent='';message.value='';dialog.showModal();message.focus()}function encodeRequest(value){const bytes=new TextEncoder().encode(JSON.stringify(value));let binary='';for(const byte of bytes)binary+=String.fromCharCode(byte);return btoa(binary).replace(/\+/g,'-').replace(/\//g,'_').replace(/=+$/,'')}document.getElementById('comment-cancel').onclick=()=>dialog.close();document.getElementById('comment-form').onsubmit=e=>{e.preventDefault();if(!anchor||!message.value.trim())return;const id=crypto.randomUUID(),entry={id,...anchor,message:message.value.trim(),submissionText:`Review comment on ${anchor.filePath}:${anchor.endLine} (${anchor.side}):\n${message.value.trim()}`,pending:true};C.comments.push(entry);commentStatus.textContent='Saving…';render();location.hash='cmux-comment='+encodeRequest({repoRoot:C.repoRoot,...entry})};window.cmuxCommentAccepted=id=>{const entry=C.comments.find(c=>c.id===id);if(entry)delete entry.pending;commentStatus.textContent='Saved';setTimeout(()=>dialog.close(),300);render()};window.cmuxCommentRejected=id=>{const at=C.comments.findIndex(c=>c.id===id);if(at>=0)C.comments.splice(at,1);commentStatus.textContent='Could not save comment';render()};
function render(){view.replaceChildren();bar.replaceChildren();commentCount.textContent=C.comments.length?`${C.comments.length} pending comment${C.comments.length===1?'':'s'}`:'';const f=files[selected];title.textContent=f?.name||'No changes';if(!f){const e=document.createElement('div');e.id='empty';e.textContent='No changes to display.';view.append(e);return}const start=page*PAGE,rows=f.lines.slice(start,start+PAGE),frag=document.createDocumentFragment();for(const row of rows){if(layout==='unified')frag.append(line(row));else{const e=document.createElement('div');e.className='split-line';e.append(side(row,'old'),side(row,'new'));frag.append(e)}for(const comment of anchored(f.name,row))frag.append(commentCard(comment))}view.append(frag);const pages=Math.ceil(f.lines.length/PAGE);if(pages>1){const prev=document.createElement('button');prev.textContent='Previous';prev.disabled=page===0;prev.onclick=()=>{page--;render()};const label=document.createElement('span');label.textContent=` Page ${page+1} of ${pages} `;const next=document.createElement('button');next.textContent='Next';next.disabled=page+1>=pages;next.onclick=()=>{page++;render()};bar.append(prev,label,next)}document.getElementById('unified').classList.toggle('active',layout==='unified');document.getElementById('split').classList.toggle('active',layout==='split')}
files.forEach((f,i)=>{const b=document.createElement('button');b.textContent=f.name;b.onclick=()=>{selected=i;page=0;render()};list.append(b)});document.getElementById('unified').onclick=()=>{layout='unified';render()};document.getElementById('split').onclick=()=>{layout='split';render()};document.getElementById('find').oninput=e=>{query=e.target.value;render()};document.getElementById('copy').onclick=async()=>{await navigator.clipboard.writeText(C.patch);document.getElementById('copy').textContent='Copied'};render();</script></body></html>"#;
