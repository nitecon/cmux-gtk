//! Bounded, manifest-aware Linux project inspector composed through the browser API.

use super::{browser_address, diff, project_config, socket_client::CliError};
use serde::Serialize;
use serde_json::{json, Value};
use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant};

const MAX_ENTRIES: usize = 5_000;
const MAX_SCANNED_ENTRIES: usize = 20_000;
const MAX_DEPTH: usize = 12;
const MAX_MANIFESTS: usize = 128;
const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
const MAX_TOTAL_MANIFEST_BYTES: usize = 4 * 1024 * 1024;
const MAX_PROJECT_VIEWERS: usize = 32;
const MAX_PROJECT_VIEWER_BYTES: u64 = 128 * 1024 * 1024;
const SCAN_DEADLINE: Duration = Duration::from_secs(3);

#[derive(Serialize)]
struct ProjectConfig<'a> {
    title: &'a str,
    root: &'a str,
    files: &'a [FileEntry],
    targets: &'a [ProjectTarget],
    manifests: &'a [ManifestSummary],
    actions: &'a Value,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileEntry {
    path: String,
    directory: bool,
    depth: usize,
}

#[derive(Serialize)]
struct ProjectTarget {
    kind: String,
    name: String,
    detail: String,
    manifest: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestSummary {
    path: String,
    kind: String,
    size_bytes: u64,
}

struct ProjectScan {
    files: Vec<FileEntry>,
    manifests: Vec<ManifestSummary>,
    targets: Vec<ProjectTarget>,
}

pub(super) struct PreparedProject {
    pub document: diff::PreparedDocument,
}

pub(super) fn prepare(input: &Path) -> Result<PreparedProject, CliError> {
    let canonical = input
        .canonicalize()
        .map_err(|error| CliError::Command(format!("resolve project path: {error}")))?;
    let root = if canonical.is_dir() {
        canonical
    } else if canonical.is_file() {
        canonical
            .parent()
            .ok_or_else(|| CliError::Command("project manifest has no parent".into()))?
            .to_path_buf()
    } else {
        return Err(CliError::Command(
            "project path must be a regular file or directory".into(),
        ));
    };
    let title = root
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("Project")
        .to_owned();
    let root_label = root.to_string_lossy().into_owned();
    let ProjectScan {
        files,
        manifests,
        targets,
    } = scan(&root)?;
    let actions = project_config::resolve(&root, project_config::global_path().as_deref())
        .and_then(|config| serde_json::to_value(config).map_err(|error| error.to_string()))
        .unwrap_or_else(|error| json!({"error": error}));
    let config = ProjectConfig {
        title: &title,
        root: &root_label,
        files: &files,
        targets: &targets,
        manifests: &manifests,
        actions: &actions,
    };
    let mut encoded = serde_json::to_string(&config)
        .map_err(|error| CliError::Command(format!("encode project viewer: {error}")))?;
    encoded = encoded
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    let html = PROJECT_HTML.replace("__CMUX_PROJECT_CONFIG__", &encoded);
    let directory = cmux_platform::paths::data_dir().join("projects");
    cmux_platform::filesystem::create_private_directory(&directory)
        .map_err(|error| CliError::Command(format!("create project view directory: {error}")))?;
    prune(&directory, html.len() as u64)?;
    let path = directory.join(format!("project-{}.html", uuid::Uuid::new_v4()));
    cmux_platform::filesystem::atomic_write(&path, html.as_bytes())
        .map_err(|error| CliError::Command(format!("write project viewer: {error}")))?;
    let url = browser_address::normalize(path.to_string_lossy().as_ref());
    let mut metadata = serde_json::Map::new();
    metadata.insert("project_root".into(), Value::String(root_label));
    metadata.insert("file_count".into(), json!(files.len()));
    metadata.insert("manifest_count".into(), json!(manifests.len()));
    metadata.insert("target_count".into(), json!(targets.len()));
    Ok(PreparedProject {
        document: diff::PreparedDocument {
            path,
            url,
            title,
            metadata,
        },
    })
}

fn scan(root: &Path) -> Result<ProjectScan, CliError> {
    let started = Instant::now();
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    let mut files = Vec::new();
    let mut manifests = Vec::new();
    let mut targets = Vec::new();
    let mut manifest_bytes = 0usize;
    let mut scanned_entries = 0usize;
    while let Some((directory, depth)) = pending.pop() {
        if started.elapsed() > SCAN_DEADLINE {
            return Err(CliError::Command(
                "project scan exceeded its three-second deadline".into(),
            ));
        }
        let reader = std::fs::read_dir(&directory)
            .map_err(|error| CliError::Command(format!("read project directory: {error}")))?;
        let mut entries = Vec::new();
        for entry in reader {
            scanned_entries += 1;
            if scanned_entries > MAX_SCANNED_ENTRIES {
                return Err(CliError::Command(format!(
                    "project scan examined more than {MAX_SCANNED_ENTRIES} entries"
                )));
            }
            entries.push(
                entry.map_err(|error| CliError::Command(format!("read project entry: {error}")))?,
            );
        }
        entries.sort_by_key(std::fs::DirEntry::file_name);
        let mut children = Vec::new();
        for entry in entries {
            if files.len() >= MAX_ENTRIES {
                return Err(CliError::Command(format!(
                    "project contains more than {MAX_ENTRIES} visible entries"
                )));
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if ignored(&name) {
                continue;
            }
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path)
                .map_err(|error| CliError::Command(format!("inspect project entry: {error}")))?;
            if metadata.file_type().is_symlink() {
                continue;
            }
            let relative = path
                .strip_prefix(root)
                .expect("walked project entries remain below root")
                .to_string_lossy()
                .into_owned();
            let directory_entry = metadata.is_dir();
            files.push(FileEntry {
                path: relative.clone(),
                directory: directory_entry,
                depth,
            });
            if directory_entry {
                if depth < MAX_DEPTH {
                    children.push((path, depth + 1));
                }
                continue;
            }
            if manifest_kind(&name).is_none() {
                continue;
            }
            if manifests.len() >= MAX_MANIFESTS {
                return Err(CliError::Command(format!(
                    "project contains more than {MAX_MANIFESTS} supported manifests"
                )));
            }
            let Some(next_total) = manifest_bytes.checked_add(metadata.len() as usize) else {
                return Err(CliError::Command("project manifest size overflow".into()));
            };
            if metadata.len() as usize > MAX_MANIFEST_BYTES || next_total > MAX_TOTAL_MANIFEST_BYTES
            {
                return Err(CliError::Command(
                    "project manifests exceed the four-MiB aggregate limit".into(),
                ));
            }
            manifest_bytes = next_total;
            let kind = manifest_kind(&name).expect("manifest kind checked");
            let mut file = cmux_platform::filesystem::open_regular_read(&path)
                .map_err(|error| CliError::Command(format!("open {relative}: {error}")))?;
            let mut bytes = Vec::new();
            file.by_ref()
                .take((MAX_MANIFEST_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|error| CliError::Command(format!("read {relative}: {error}")))?;
            if bytes.len() > MAX_MANIFEST_BYTES {
                return Err(CliError::Command(format!("{relative} exceeds one MiB")));
            }
            let text = String::from_utf8(bytes)
                .map_err(|_| CliError::Command(format!("{relative} is not UTF-8")))?;
            manifests.push(ManifestSummary {
                path: relative.clone(),
                kind: kind.into(),
                size_bytes: metadata.len(),
            });
            parse_targets(kind, &relative, &text, &mut targets);
        }
        children.reverse();
        pending.extend(children);
    }
    Ok(ProjectScan {
        files,
        manifests,
        targets,
    })
}

fn ignored(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | ".hg"
            | ".svn"
            | "target"
            | "node_modules"
            | ".venv"
            | "venv"
            | "dist"
            | ".next"
            | ".cache"
    )
}

fn manifest_kind(name: &str) -> Option<&'static str> {
    match name {
        "Cargo.toml" => Some("Rust"),
        "go.mod" => Some("Go"),
        "package.json" => Some("Node"),
        "pyproject.toml" => Some("Python"),
        "CMakeLists.txt" => Some("CMake"),
        "Makefile" | "makefile" => Some("Make"),
        "cmux.json" => Some("cmux"),
        _ => None,
    }
}

fn push_target(
    targets: &mut Vec<ProjectTarget>,
    kind: &str,
    name: &str,
    detail: &str,
    manifest: &str,
) {
    if targets.len() >= 512 || name.is_empty() {
        return;
    }
    targets.push(ProjectTarget {
        kind: kind.into(),
        name: name.chars().take(256).collect(),
        detail: detail.chars().take(1024).collect(),
        manifest: manifest.into(),
    });
}

fn parse_targets(kind: &str, manifest: &str, text: &str, targets: &mut Vec<ProjectTarget>) {
    match kind {
        "Rust" | "Python" => {
            let Ok(value) = text.parse::<toml::Value>() else {
                return;
            };
            if let Some(name) = value
                .get("package")
                .or_else(|| value.get("project"))
                .and_then(|table| table.get("name"))
                .and_then(toml::Value::as_str)
            {
                push_target(targets, kind, name, "package", manifest);
            }
            if kind == "Rust" {
                if let Some(bins) = value.get("bin").and_then(toml::Value::as_array) {
                    for bin in bins {
                        if let Some(name) = bin.get("name").and_then(toml::Value::as_str) {
                            push_target(targets, kind, name, "binary", manifest);
                        }
                    }
                }
            } else if let Some(scripts) = value
                .get("project")
                .and_then(|project| project.get("scripts"))
                .and_then(toml::Value::as_table)
            {
                for (name, value) in scripts {
                    push_target(
                        targets,
                        kind,
                        name,
                        value.as_str().unwrap_or("script"),
                        manifest,
                    );
                }
            }
        }
        "Node" => {
            let Ok(value) = serde_json::from_str::<Value>(text) else {
                return;
            };
            if let Some(name) = value.get("name").and_then(Value::as_str) {
                push_target(targets, kind, name, "package", manifest);
            }
            if let Some(scripts) = value.get("scripts").and_then(Value::as_object) {
                for (name, command) in scripts {
                    push_target(
                        targets,
                        kind,
                        name,
                        command.as_str().unwrap_or("script"),
                        manifest,
                    );
                }
            }
        }
        "Go" => {
            if let Some(name) = text.lines().find_map(|line| line.strip_prefix("module ")) {
                push_target(targets, kind, name.trim(), "module", manifest);
            }
        }
        "Make" => {
            for line in text.lines() {
                let Some((name, _)) = line.split_once(':') else {
                    continue;
                };
                if !name.is_empty()
                    && !name.starts_with('.')
                    && name
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
                {
                    push_target(targets, kind, name, "target", manifest);
                }
            }
        }
        "CMake" => {
            for line in text.lines() {
                let trimmed = line.trim();
                for (prefix, detail) in [
                    ("add_executable(", "executable"),
                    ("add_library(", "library"),
                ] {
                    if let Some(rest) = trimmed.strip_prefix(prefix) {
                        let name = rest
                            .split_whitespace()
                            .next()
                            .unwrap_or("")
                            .trim_end_matches(')');
                        push_target(targets, kind, name, detail, manifest);
                    }
                }
            }
        }
        _ => {}
    }
}

fn prune(directory: &Path, incoming: u64) -> Result<(), CliError> {
    if incoming > MAX_PROJECT_VIEWER_BYTES {
        return Err(CliError::Command("project viewer exceeds 128 MiB".into()));
    }
    let mut files = std::fs::read_dir(directory)
        .map_err(|error| CliError::Command(format!("list project viewers: {error}")))?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            (metadata.is_file() && entry.path().extension().is_some_and(|ext| ext == "html"))
                .then_some((entry.path(), metadata.modified().ok(), metadata.len()))
        })
        .collect::<Vec<_>>();
    files.sort_by_key(|(_, modified, _)| *modified);
    let mut bytes = files.iter().map(|(_, _, size)| *size).sum::<u64>();
    while files.len() >= MAX_PROJECT_VIEWERS
        || bytes.saturating_add(incoming) > MAX_PROJECT_VIEWER_BYTES
    {
        let Some((path, _, size)) = files.first().cloned() else {
            break;
        };
        files.remove(0);
        std::fs::remove_file(path)
            .map_err(|error| CliError::Command(format!("prune project viewer: {error}")))?;
        bytes = bytes.saturating_sub(size);
    }
    Ok(())
}

const PROJECT_HTML: &str = r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>cmux project</title><style>
:root{color-scheme:light dark;--bg:#101216;--panel:#171a20;--fg:#d7dae0;--muted:#89909b;--border:#303640;--accent:#78a9ff}*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);font:13px ui-monospace,SFMono-Regular,Consolas,monospace}header{height:48px;display:flex;align-items:center;gap:12px;padding:8px 14px;background:var(--panel);border-bottom:1px solid var(--border)}#root{color:var(--muted);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;flex:1}.tabs{display:flex;gap:4px;padding:8px 14px;border-bottom:1px solid var(--border)}button,input{font:inherit;color:inherit;background:#222731;border:1px solid var(--border);border-radius:5px;padding:5px 9px}button.active{border-color:var(--accent)}main{padding:12px 14px;overflow:auto;height:calc(100vh - 89px)}.row{display:grid;grid-template-columns:minmax(180px,1fr) minmax(100px,.3fr) minmax(220px,1fr);gap:12px;padding:5px 8px;border-bottom:1px solid #ffffff0a}.directory{color:#9ab7e8}.muted{color:var(--muted)}pre{white-space:pre-wrap;overflow-wrap:anywhere;background:var(--panel);border:1px solid var(--border);border-radius:6px;padding:12px}.controls{display:flex;gap:8px;margin-bottom:10px}.hidden{display:none}</style></head><body><header><strong id="title"></strong><span id="root"></span></header><div class="tabs"><button data-tab="files">Files</button><button data-tab="targets">Targets</button><button data-tab="settings">Settings</button><button data-tab="actions">Actions</button></div><main><section id="files"><div class="controls"><input id="find" type="search" placeholder="Filter paths"><span class="muted" id="file-count"></span></div><div id="file-list"></div></section><section id="targets" class="hidden"></section><section id="settings" class="hidden"></section><section id="actions" class="hidden"><pre id="action-json"></pre></section></main><script>
const C=__CMUX_PROJECT_CONFIG__;document.title=C.title+' — cmux project';document.getElementById('title').textContent=C.title;document.getElementById('root').textContent=C.root;const esc=s=>{const e=document.createElement('span');e.textContent=s;return e};function fileRows(query=''){const list=document.getElementById('file-list');list.replaceChildren();const q=query.toLowerCase(),rows=C.files.filter(x=>!q||x.path.toLowerCase().includes(q));for(const x of rows){const e=document.createElement('div');e.className='row '+(x.directory?'directory':'');e.style.paddingLeft=(8+Math.min(x.depth,12)*12)+'px';e.append(esc((x.directory?'▸ ':'')+x.path));list.append(e)}document.getElementById('file-count').textContent=rows.length+' of '+C.files.length+' entries'}function targetRows(){const out=document.getElementById('targets');out.replaceChildren();for(const x of C.targets){const e=document.createElement('div');e.className='row';e.append(esc(x.name),esc(x.kind+' · '+x.detail),esc(x.manifest));out.append(e)}if(!C.targets.length)out.textContent='No supported manifest targets found.'}function settings(){const out=document.getElementById('settings');out.replaceChildren();const summary=[['Project root',C.root],['Files',String(C.files.length)],['Manifests',String(C.manifests.length)],['Targets',String(C.targets.length)]];for(const [key,value] of summary){const e=document.createElement('div');e.className='row';e.append(esc(key),esc(value));out.append(e)}for(const x of C.manifests){const e=document.createElement('div');e.className='row';e.append(esc(x.path),esc(x.kind),esc(x.sizeBytes+' bytes'));out.append(e)}}function select(tab){for(const section of document.querySelectorAll('main section'))section.classList.toggle('hidden',section.id!==tab);for(const button of document.querySelectorAll('.tabs button'))button.classList.toggle('active',button.dataset.tab===tab)}document.querySelectorAll('.tabs button').forEach(button=>button.onclick=()=>select(button.dataset.tab));document.getElementById('find').oninput=e=>fileRows(e.target.value);document.getElementById('action-json').textContent=JSON.stringify(C.actions,null,2);fileRows();targetRows();settings();select('files');</script></body></html>"#;
