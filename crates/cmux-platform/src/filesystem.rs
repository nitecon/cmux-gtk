//! Linux filesystem operations shared by persistent application settings.

use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);

/// Load or create a durable, owner-only 32-byte signing key without following symlinks.
/// Performs blocking I/O before the UI starts. Invalid or concurrently incomplete files fail closed.
pub fn load_or_create_secret(path: &Path) -> io::Result<[u8; 32]> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("key has no parent"))?;
    std::fs::create_dir_all(parent)?;
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
    {
        Ok(mut file) => {
            let mut secret = [0u8; 32];
            std::fs::File::open("/dev/urandom")?.read_exact(&mut secret)?;
            file.write_all(&secret)?;
            file.sync_all()?;
            std::fs::File::open(parent)?.sync_all()?;
            Ok(secret)
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let mut file = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(path)?;
            let metadata = file.metadata()?;
            use std::os::unix::fs::MetadataExt;
            // SAFETY: geteuid has no pointer or initialization preconditions.
            let owner = unsafe { libc::geteuid() };
            if !metadata.is_file()
                || metadata.len() != 32
                || metadata.mode() & 0o077 != 0
                || metadata.uid() != owner
            {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "invalid signing key file",
                ));
            }
            let mut secret = [0u8; 32];
            file.read_exact(&mut secret)?;
            Ok(secret)
        }
        Err(error) => Err(error),
    }
}

/// Open a regular file for bounded worker reads, rejecting FIFOs/devices without waiting for a peer.
/// Follows symlinks; validates the opened descriptor so a path replacement cannot bypass the type check.
pub fn open_regular_read(path: &Path) -> io::Result<std::fs::File> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected a regular file",
        ));
    }
    Ok(file)
}

/// Read a complete UTF-8 file without retaining more than limit plus one input bytes.
/// Oversize or invalid UTF-8 returns InvalidData; filesystem errors pass through.
/// Follows symlinks and performs blocking I/O; callers own path selection and worker scheduling.
pub fn read_text_bounded(path: &Path, limit: usize) -> io::Result<String> {
    let capacity = u64::try_from(limit)
        .ok()
        .and_then(|limit| limit.checked_add(1))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid file byte limit"))?;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(capacity)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "file exceeds byte limit",
        ));
    }
    String::from_utf8(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "file is not UTF-8"))
}

/// Create a directory tree and restrict its final directory to its owner.
/// New directories are created with mode 0700 (subject to umask); the final
/// directory is set to exactly 0700, including when it already exists.
/// Ancestor permissions are not changed. Performs blocking filesystem I/O.
pub fn create_private_directory(path: &Path) -> io::Result<()> {
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

/// Restrict an existing file or socket to owner read/write access (0600).
/// Follows symlinks and returns filesystem errors; callers own path selection.
pub fn restrict_file_to_owner(path: &Path) -> io::Result<()> {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

/// Set a staged release executable to owner write and universal read/execute (0755).
/// Follows symlinks and returns filesystem errors; callers validate staged content.
pub fn set_executable_permissions(path: &Path) -> io::Result<()> {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
}

/// Replace a file atomically using a private, uniquely created sibling file.
///
/// Creates missing parent directories. Concurrent writers have independent
/// temporary files; the last completed rename wins. Failed writes remove their
/// temporary file and preserve the destination. This guarantees atomic visibility,
/// not power-loss durability; it does not fsync the file or parent directory.
/// Performs blocking I/O and returns filesystem errors to the caller.
pub fn atomic_write(path: &Path, contents: &[u8]) -> io::Result<()> {
    atomic_write_with(path, |file| file.write_all(contents))
}

/// Make an already replaced file and its directory entry durable before returning.
/// Call after the owning serialized writer has completed its final replacement.
/// Blocking filesystem errors propagate; concurrent replacement is the caller's responsibility.
pub fn sync_file_and_parent(path: &Path) -> io::Result<()> {
    std::fs::File::open(path)?.sync_all()?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::File::open(parent)?.sync_all()
}

/// Stream a replacement into a private sibling file and rename only after callback success.
/// The callback owns flushing any added buffers; errors remove the staging file and retain
/// the destination. Returns the callback result after replacement. Blocking, without fsync.
pub fn atomic_write_with<T>(
    path: &Path,
    write: impl FnOnce(&mut std::fs::File) -> io::Result<T>,
) -> io::Result<T> {
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "destination has no filename")
    })?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    for _ in 0..32 {
        let mut temporary_name = name.to_os_string();
        temporary_name.push(format!(
            ".{}.{}.tmp",
            std::process::id(),
            NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed)
        ));
        let temporary = parent.join(temporary_name);
        let mut file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        let result = write(&mut file).and_then(|value| {
            drop(file);
            std::fs::rename(&temporary, path).map(|_| value)
        });
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        return result;
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a private temporary file",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real key round-trips privately; symlinks, permissive modes and partial files fail closed.
    #[test]
    fn signing_key_is_private_persistent_and_validated() {
        let root = directory();
        let key = root.join("key");
        let first = load_or_create_secret(&key).unwrap();
        assert_eq!(first, load_or_create_secret(&key).unwrap());
        assert_eq!(
            std::fs::metadata(&key).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let link = root.join("link");
        std::os::unix::fs::symlink(&key, &link).unwrap();
        assert!(load_or_create_secret(&link).is_err());
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(load_or_create_secret(&key).is_err());
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::write(&key, b"partial").unwrap();
        assert!(load_or_create_secret(&key).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    /// Build a per-test directory without mutating shared environment variables.
    fn directory() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "cmux-atomic-{}-{}",
            std::process::id(),
            NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed)
        ))
    }

    /// Real files enforce byte boundaries, UTF-8 validity and ordinary filesystem error behavior.
    #[test]
    fn bounded_text_reads_validate_complete_contents() {
        let root = directory();
        create_private_directory(&root).unwrap();
        let file = root.join("metadata");
        std::fs::write(&file, "λ").unwrap();
        assert_eq!(read_text_bounded(&file, 2).unwrap(), "λ");
        assert_eq!(
            read_text_bounded(&file, 1).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        std::fs::write(&file, [0xff]).unwrap();
        assert_eq!(
            read_text_bounded(&file, 1).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        std::fs::write(&file, b"").unwrap();
        assert_eq!(read_text_bounded(&file, 0).unwrap(), "");
        std::fs::remove_file(&file).unwrap();
        assert_eq!(
            read_text_bounded(&file, 4).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        std::fs::remove_dir(root).unwrap();
    }

    /// A failed streaming callback preserves the old destination and removes its partial staging file.
    #[test]
    fn streamed_replacement_preserves_destination_on_error() {
        let root = directory();
        create_private_directory(&root).unwrap();
        let path = root.join("snapshot");
        atomic_write(&path, b"original").unwrap();
        let failed: io::Result<()> = atomic_write_with(&path, |file| {
            file.write_all(b"partial")?;
            Err(io::Error::other("fixture serialization failure"))
        });
        assert!(failed.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        assert_eq!(
            atomic_write_with(&path, |file| {
                file.write_all(b"complete")?;
                Ok(42)
            })
            .unwrap(),
            42
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"complete");
        std::fs::remove_dir_all(root).unwrap();
    }

    /// Private paths and staged binaries receive exact access modes, including existing paths.
    #[test]
    fn access_policies_apply_to_existing_paths() {
        let root = directory();
        create_private_directory(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        create_private_directory(&root).unwrap();
        assert_eq!(
            std::fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let file = root.join("binary");
        std::fs::write(&file, b"payload").unwrap();
        set_executable_permissions(&file).unwrap();
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o755
        );
        restrict_file_to_owner(&file).unwrap();
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let missing = root.join("missing");
        assert_eq!(
            restrict_file_to_owner(&missing).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(
            set_executable_permissions(&missing).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    /// Concurrent saves expose one complete payload and leave no temporary files.
    #[test]
    fn concurrent_replacement_is_complete() {
        let directory = directory();
        let target = directory.join("state.json");
        let threads: Vec<_> = (0..8u8)
            .map(|byte| {
                let target = target.clone();
                std::thread::spawn(move || atomic_write(&target, &vec![byte; 4096]).unwrap())
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        let bytes = std::fs::read(&target).unwrap();
        assert_eq!(bytes.len(), 4096);
        assert!(bytes.iter().all(|byte| *byte == bytes[0]));
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
        std::fs::remove_dir_all(directory).unwrap();
    }

    /// A failed replacement preserves its target and removes the private staging file.
    #[test]
    fn failed_rename_cleans_staging() {
        let directory = directory();
        let target = directory.join("occupied");
        std::fs::create_dir_all(&target).unwrap();
        assert!(atomic_write(&target, b"payload").is_err());
        assert!(target.is_dir());
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
