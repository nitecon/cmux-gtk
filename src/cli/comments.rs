//! Durable, bounded diff-review comments keyed by canonical Git repository.

use super::{args::CommentCommands, diff, CliError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_COMMENTS: usize = 512;
const MAX_STORE_BYTES: usize = 1024 * 1024;
const MAX_REPOSITORIES: usize = 128;
const MAX_DIRECTORY_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Comment {
    id: uuid::Uuid,
    file_path: String,
    side: String,
    start_line: u32,
    end_line: u32,
    line_text: String,
    message: String,
    submission_text: String,
    consumed_at: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RepositoryComments {
    repo_root: String,
    comments: Vec<Comment>,
}

struct StorePaths {
    directory: PathBuf,
    data: PathBuf,
    lock: PathBuf,
}

pub(super) fn run(command: &CommentCommands, json_output: bool) -> Result<(), CliError> {
    let repo = match command {
        CommentCommands::List { repo, .. }
        | CommentCommands::Add { repo, .. }
        | CommentCommands::Delete { repo, .. }
        | CommentCommands::Consume { repo, .. } => repo.as_deref(),
    };
    let root = diff::repository_root(repo.unwrap_or_else(|| Path::new(".")))?;
    match command {
        CommentCommands::List { all, .. } => {
            let comments = load(&root)?;
            let listed: Vec<_> = comments
                .into_iter()
                .filter(|comment| *all || comment.consumed_at.is_none())
                .collect();
            print_list(&root, &listed, json_output)
        }
        CommentCommands::Add {
            file,
            side,
            line,
            end_line,
            line_text,
            message,
            ..
        } => {
            validate_anchor(
                file,
                side,
                *line,
                end_line.unwrap_or(*line),
                line_text,
                message,
            )?;
            let now = timestamp()?;
            let comment = Comment {
                id: uuid::Uuid::new_v4(),
                file_path: file.clone(),
                side: side.to_ascii_lowercase(),
                start_line: *line,
                end_line: end_line.unwrap_or(*line),
                line_text: line_text.clone(),
                message: message.clone(),
                submission_text: format!(
                    "Review comment on {}:{} ({}):\n{}",
                    file,
                    end_line.unwrap_or(*line),
                    side.to_ascii_lowercase(),
                    message
                ),
                consumed_at: None,
                created_at: now.clone(),
                updated_at: now,
            };
            mutate(&root, |comments| {
                if comments.len() >= MAX_COMMENTS {
                    if let Some(index) = comments.iter().position(|row| row.consumed_at.is_some()) {
                        comments.remove(index);
                    } else {
                        return Err(std::io::Error::other(
                            "review comment limit reached; consume or delete an entry",
                        ));
                    }
                }
                comments.push(comment.clone());
                Ok(())
            })?;
            print_comment(&root, &comment, json_output)
        }
        CommentCommands::Delete { id, .. } => {
            let id = parse_id(id)?;
            mutate(&root, |comments| {
                let before = comments.len();
                comments.retain(|comment| comment.id != id);
                if comments.len() == before {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "review comment not found",
                    ));
                }
                Ok(())
            })?;
            if json_output {
                println!("{}", json!({"ok": true, "id": id, "repo_root": root}));
            } else {
                println!("Deleted review comment {id}");
            }
            Ok(())
        }
        CommentCommands::Consume { ids, all, .. } => {
            if !*all && ids.is_empty() {
                return Err(CliError::Command(
                    "comments consume requires one or more IDs or --all".into(),
                ));
            }
            let ids = ids
                .iter()
                .map(|id| parse_id(id))
                .collect::<Result<Vec<_>, _>>()?;
            let now = timestamp()?;
            let mut count = 0usize;
            mutate(&root, |comments| {
                for comment in comments {
                    if comment.consumed_at.is_none() && (*all || ids.contains(&comment.id)) {
                        comment.consumed_at = Some(now.clone());
                        comment.updated_at = now.clone();
                        count += 1;
                    }
                }
                Ok(())
            })?;
            if json_output {
                println!(
                    "{}",
                    json!({"ok": true, "consumed": count, "repo_root": root})
                );
            } else {
                println!("Marked {count} review comment(s) consumed");
            }
            Ok(())
        }
    }
}

pub(super) fn for_viewer(root: &Path) -> Result<Vec<Comment>, CliError> {
    load(root).map(|comments| {
        comments
            .into_iter()
            .filter(|comment| comment.consumed_at.is_none())
            .collect()
    })
}

fn validate_anchor(
    file: &str,
    side: &str,
    start: u32,
    end: u32,
    line_text: &str,
    message: &str,
) -> Result<(), CliError> {
    let path = Path::new(file);
    if file.is_empty()
        || file.len() > 4096
        || path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::RootDir))
    {
        return Err(CliError::Command(
            "comment file must be a bounded repository-relative path".into(),
        ));
    }
    if !matches!(side.to_ascii_lowercase().as_str(), "old" | "new") {
        return Err(CliError::Command("comment side must be old or new".into()));
    }
    if start == 0 || end < start || end > 10_000_000 {
        return Err(CliError::Command("comment line range is invalid".into()));
    }
    if line_text.len() > 16 * 1024 || message.is_empty() || message.len() > 64 * 1024 {
        return Err(CliError::Command(
            "comment line text or message exceeds its limit".into(),
        ));
    }
    Ok(())
}

fn parse_id(value: &str) -> Result<uuid::Uuid, CliError> {
    uuid::Uuid::parse_str(value).map_err(|_| CliError::Command("comment ID must be a UUID".into()))
}

fn timestamp() -> Result<String, CliError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .map_err(|error| CliError::Command(format!("read system time: {error}")))
}

fn paths(root: &Path) -> Result<StorePaths, CliError> {
    let directory = cmux_platform::paths::data_dir().join("diff-comments");
    cmux_platform::filesystem::create_private_directory(&directory)
        .map_err(|error| CliError::Command(format!("create comment directory: {error}")))?;
    let canonical = root.to_string_lossy();
    let digest = Sha256::digest(canonical.as_bytes());
    let key = digest[..12]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(StorePaths {
        data: directory.join(format!("{key}.json")),
        lock: directory.join(format!("{key}.lock")),
        directory,
    })
}

fn load(root: &Path) -> Result<Vec<Comment>, CliError> {
    let paths = paths(root)?;
    cmux_platform::filesystem::with_exclusive_lock(&paths.lock, || read_locked(&paths.data, root))
        .map_err(|error| CliError::Command(format!("read review comments: {error}")))
}

fn mutate(
    root: &Path,
    update: impl FnOnce(&mut Vec<Comment>) -> std::io::Result<()>,
) -> Result<(), CliError> {
    let paths = paths(root)?;
    cmux_platform::filesystem::with_exclusive_lock(&paths.lock, || {
        let mut comments = read_locked(&paths.data, root)?;
        update(&mut comments)?;
        let file = RepositoryComments {
            repo_root: root.to_string_lossy().into_owned(),
            comments,
        };
        let encoded = serde_json::to_vec_pretty(&file)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        if encoded.len() > MAX_STORE_BYTES {
            return Err(std::io::Error::other(
                "review comment store exceeds one MiB",
            ));
        }
        cmux_platform::filesystem::atomic_write(&paths.data, &encoded)?;
        cmux_platform::filesystem::sync_file_and_parent(&paths.data)
    })
    .map_err(|error| CliError::Command(format!("save review comments: {error}")))?;
    prune(&paths.directory, &paths.data);
    Ok(())
}

fn read_locked(path: &Path, root: &Path) -> std::io::Result<Vec<Comment>> {
    let text = match cmux_platform::filesystem::read_text_bounded(path, MAX_STORE_BYTES) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let file: RepositoryComments = serde_json::from_str(&text).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid comment JSON")
    })?;
    if file.repo_root != root.to_string_lossy() || file.comments.len() > MAX_COMMENTS {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "comment store identity or count is invalid",
        ));
    }
    Ok(file.comments)
}

fn prune(directory: &Path, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut files: Vec<_> =
        entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let path = entry.path();
                let metadata = entry.metadata().ok()?;
                (metadata.is_file() && path.extension().is_some_and(|ext| ext == "json"))
                    .then_some((path, metadata.modified().ok(), metadata.len()))
            })
            .collect();
    files.sort_by_key(|(_, modified, _)| *modified);
    let mut bytes: u64 = files.iter().map(|(_, _, size)| *size).sum();
    while files.len() > MAX_REPOSITORIES || bytes > MAX_DIRECTORY_BYTES {
        let Some((path, _, size)) = files.first().cloned() else {
            break;
        };
        files.remove(0);
        if path != keep && std::fs::remove_file(path).is_ok() {
            bytes = bytes.saturating_sub(size);
        }
    }
}

fn print_list(root: &Path, comments: &[Comment], json_output: bool) -> Result<(), CliError> {
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "repo_root": root,
                "count": comments.len(),
                "comments": comments,
            }))
            .map_err(|error| CliError::Command(error.to_string()))?
        );
        return Ok(());
    }
    if comments.is_empty() {
        println!("No review comments. (repo: {})", root.display());
        return Ok(());
    }
    println!(
        "{} review comment(s) (repo: {})",
        comments.len(),
        root.display()
    );
    for comment in comments {
        println!(
            "- {}:{} [{}] {}",
            comment.file_path, comment.end_line, comment.side, comment.message
        );
    }
    Ok(())
}

fn print_comment(root: &Path, comment: &Comment, json_output: bool) -> Result<(), CliError> {
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({"repo_root": root, "comment": comment}))
                .map_err(|error| CliError::Command(error.to_string()))?
        );
    } else {
        println!(
            "Saved review comment {} at {}:{}",
            comment.id, comment.file_path, comment.end_line
        );
    }
    Ok(())
}
