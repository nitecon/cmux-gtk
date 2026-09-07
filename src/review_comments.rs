//! Shared durable storage for bounded diff-review comments.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const MAX_COMMENTS: usize = 512;
const MAX_STORE_BYTES: usize = 1024 * 1024;
const MAX_REPOSITORIES: usize = 128;
const MAX_DIRECTORY_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub id: Uuid,
    pub file_path: String,
    pub side: String,
    pub start_line: u32,
    pub end_line: u32,
    pub line_text: String,
    pub message: String,
    pub submission_text: String,
    pub consumed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewComment {
    pub id: Uuid,
    pub file_path: String,
    pub side: String,
    pub start_line: u32,
    pub end_line: u32,
    #[serde(default)]
    pub line_text: String,
    pub message: String,
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

/// Load every retained comment for a canonical repository root.
pub fn load(root: &Path) -> std::io::Result<Vec<Comment>> {
    let root = canonical_root(root)?;
    let paths = paths(&root)?;
    cmux_platform::filesystem::with_exclusive_lock(&paths.lock, || read_locked(&paths.data, &root))
}

/// Load only comments that have not yet been delivered to an agent.
pub fn pending(root: &Path) -> std::io::Result<Vec<Comment>> {
    load(root).map(|comments| {
        comments
            .into_iter()
            .filter(|comment| comment.consumed_at.is_none())
            .collect()
    })
}

/// Insert one validated comment. Replaying the same request UUID is idempotent.
pub fn add(root: &Path, input: NewComment) -> std::io::Result<Comment> {
    validate_anchor(&input)?;
    let root = canonical_root(root)?;
    let now = timestamp()?;
    let comment = Comment {
        id: input.id,
        file_path: input.file_path,
        side: input.side.to_ascii_lowercase(),
        start_line: input.start_line,
        end_line: input.end_line,
        line_text: input.line_text,
        message: input.message,
        submission_text: String::new(),
        consumed_at: None,
        created_at: now.clone(),
        updated_at: now,
    };
    let mut comment = comment;
    comment.submission_text = format!(
        "Review comment on {}:{} ({}):\n{}",
        comment.file_path, comment.end_line, comment.side, comment.message
    );
    let mut result = comment.clone();
    mutate(&root, |comments| {
        if let Some(existing) = comments.iter().find(|row| row.id == comment.id) {
            let same = existing.file_path == comment.file_path
                && existing.side == comment.side
                && existing.start_line == comment.start_line
                && existing.end_line == comment.end_line
                && existing.line_text == comment.line_text
                && existing.message == comment.message;
            if !same {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "review comment request ID conflicts with existing content",
                ));
            }
            result = existing.clone();
            return Ok(());
        }
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
    Ok(result)
}

/// Delete one retained comment by stable UUID.
pub fn delete(root: &Path, id: Uuid) -> std::io::Result<()> {
    let root = canonical_root(root)?;
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
    })
}

/// Mark selected or all pending comments as delivered and return the changed count.
pub fn consume(root: &Path, ids: &[Uuid], all: bool) -> std::io::Result<usize> {
    let root = canonical_root(root)?;
    let now = timestamp()?;
    let mut count = 0usize;
    mutate(&root, |comments| {
        for comment in comments {
            if comment.consumed_at.is_none() && (all || ids.contains(&comment.id)) {
                comment.consumed_at = Some(now.clone());
                comment.updated_at = now.clone();
                count += 1;
            }
        }
        Ok(())
    })?;
    Ok(count)
}

fn validate_anchor(input: &NewComment) -> std::io::Result<()> {
    let path = Path::new(&input.file_path);
    if input.file_path.is_empty()
        || input.file_path.len() > 4096
        || path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::RootDir))
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "comment file must be a bounded repository-relative path",
        ));
    }
    if !matches!(input.side.to_ascii_lowercase().as_str(), "old" | "new") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "comment side must be old or new",
        ));
    }
    if input.start_line == 0 || input.end_line < input.start_line || input.end_line > 10_000_000 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "comment line range is invalid",
        ));
    }
    if input.line_text.len() > 16 * 1024
        || input.message.is_empty()
        || input.message.len() > 64 * 1024
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "comment line text or message exceeds its limit",
        ));
    }
    Ok(())
}

fn canonical_root(root: &Path) -> std::io::Result<PathBuf> {
    std::fs::canonicalize(root).map_err(|error| {
        std::io::Error::new(
            error.kind(),
            format!("canonicalize review repository: {error}"),
        )
    })
}

fn timestamp() -> std::io::Result<String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .map_err(|error| std::io::Error::other(format!("read system time: {error}")))
}

fn paths(root: &Path) -> std::io::Result<StorePaths> {
    let directory = cmux_platform::paths::data_dir().join("diff-comments");
    cmux_platform::filesystem::create_private_directory(&directory)?;
    let digest = Sha256::digest(root.to_string_lossy().as_bytes());
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

fn mutate(
    root: &Path,
    update: impl FnOnce(&mut Vec<Comment>) -> std::io::Result<()>,
) -> std::io::Result<()> {
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
    })?;
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
