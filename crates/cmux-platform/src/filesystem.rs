//! Linux filesystem operations shared by persistent application settings.

use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);

/// Replace a file atomically using a private, uniquely created sibling file.
///
/// Creates missing parent directories. Concurrent writers have independent
/// temporary files; the last completed rename wins. Failed writes remove their
/// temporary file and preserve the destination. This guarantees atomic visibility,
/// not power-loss durability; it does not fsync the file or parent directory.
/// Performs blocking I/O and returns filesystem errors to the caller.
pub fn atomic_write(path: &Path, contents: &[u8]) -> io::Result<()> {
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
        let result = file.write_all(contents).and_then(|_| {
            drop(file);
            std::fs::rename(&temporary, path)
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

    /// Build a per-test directory without mutating shared environment variables.
    fn directory() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "cmux-atomic-{}-{}",
            std::process::id(),
            NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed)
        ))
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
