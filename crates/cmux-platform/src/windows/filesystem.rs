//! Windows private storage and atomic replacement using native NTFS access control.

use crate::windows::{self, PrivateSecurity};
use std::os::windows::io::FromRawHandle;
use std::{
    io::{self, Read, Write},
    os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, CREATE_NEW, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_GENERIC_WRITE, FILE_SHARE_READ,
};

static NEXT: AtomicU64 = AtomicU64::new(0);

/// Exclusively create an owner-only file; its DACL is applied before the handle becomes visible.
pub fn create_private_file(path: &Path) -> io::Result<std::fs::File> {
    let security = PrivateSecurity::new()?;
    let attributes = security.attributes();
    let name = windows::wide(path.as_os_str());
    // SAFETY: name and security descriptor remain live for synchronous creation.
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            FILE_GENERIC_WRITE,
            FILE_SHARE_READ,
            &attributes,
            CREATE_NEW,
            0,
            std::ptr::null_mut(),
        )
    };
    if handle == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: this transfers sole ownership of the newly created handle to File.
    Ok(unsafe { std::fs::File::from_raw_handle(handle) })
}

/// Load or create an owner-only 32-byte key, rejecting reparse points and permissive existing keys.
pub fn load_or_create_secret(path: &Path) -> io::Result<[u8; 32]> {
    create_private_directory(
        path.parent()
            .ok_or_else(|| io::Error::other("key has no parent"))?,
    )?;
    match create_private_file(path) {
        Ok(mut file) => {
            let mut key = [0; 32];
            getrandom::fill(&mut key).map_err(|error| io::Error::other(error.to_string()))?;
            file.write_all(&key)?;
            file.sync_all()?;
            Ok(key)
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let mut file = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                .open(path)?;
            let metadata = file.metadata()?;
            if !metadata.is_file()
                || metadata.len() != 32
                || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
                || !windows::private_file(file.as_raw_handle())?
            {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "invalid signing key file",
                ));
            }
            let mut key = [0; 32];
            file.read_exact(&mut key)?;
            Ok(key)
        }
        Err(error) => Err(error),
    }
}

/// Open a regular disk file for bounded worker reads.
pub fn open_regular_read(path: &Path) -> io::Result<std::fs::File> {
    let file = std::fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected a regular file",
        ));
    }
    Ok(file)
}

/// Read complete UTF-8 contents with at most limit plus one input bytes.
pub fn read_text_bounded(path: &Path, limit: usize) -> io::Result<String> {
    let cap = u64::try_from(limit)
        .ok()
        .and_then(|n| n.checked_add(1))
        .ok_or_else(|| io::Error::other("invalid byte limit"))?;
    let mut bytes = Vec::new();
    open_regular_read(path)?.take(cap).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "file exceeds byte limit",
        ));
    }
    String::from_utf8(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "file is not UTF-8"))
}

/// Create a directory tree and restrict its final directory to the current user, including inherited children.
pub fn create_private_directory(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)?;
    PrivateSecurity::new()?.restrict(path)
}

/// Replace a file's DACL with current-user access only; native pipes are secured when created.
pub fn restrict_file_to_owner(path: &Path) -> io::Result<()> {
    if path.to_string_lossy().starts_with(r"\\.\pipe\") {
        return Ok(());
    }
    PrivateSecurity::new()?.restrict(path)
}

/// Windows executable permission is governed by its file ACL; retain owner-only access.
pub fn set_executable_permissions(path: &Path) -> io::Result<()> {
    restrict_file_to_owner(path)
}

/// Replace a file via a private sibling; failure preserves the original destination.
pub fn atomic_write(path: &Path, contents: &[u8]) -> io::Result<()> {
    atomic_write_with(path, |file| file.write_all(contents))
}

/// Flush replaced file contents; Windows does not expose POSIX directory fsync through std.
pub fn sync_file_and_parent(path: &Path) -> io::Result<()> {
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)?
        .sync_all()
}

/// Hold a kernel file lock for one short transaction; descriptor drop releases it on unwinding.
pub fn with_exclusive_lock<T>(
    path: &Path,
    transaction: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    if file.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "lock is a reparse point",
        ));
    }
    restrict_file_to_owner(path)?;
    file.lock()?;
    let result = transaction();
    let unlock = file.unlock();
    result.and_then(|value| unlock.map(|_| value))
}

/// Stage callback output in a unique owner-only sibling before atomic replacement.
pub fn atomic_write_with<T>(
    path: &Path,
    write: impl FnOnce(&mut std::fs::File) -> io::Result<T>,
) -> io::Result<T> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("destination has no filename"))?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    for _ in 0..32 {
        let mut name = name.to_os_string();
        name.push(format!(
            ".{}.{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let temp = parent.join(name);
        let mut file = match create_private_file(&temp) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        let result = restrict_file_to_owner(&temp)
            .and_then(|_| write(&mut file))
            .and_then(|value| {
                drop(file);
                std::fs::rename(&temp, path).map(|_| value)
            });
        if result.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        return result;
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "cannot allocate staging file",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exercise NTFS private-key ownership, atomic replacement and rejection of corrupt persistent data.
    #[test]
    fn private_storage_roundtrip() {
        let root = std::env::temp_dir().join(format!(
            "cmux-win-storage-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        create_private_directory(&root).unwrap();
        let key = root.join("key");
        let original = load_or_create_secret(&key).unwrap();
        assert_eq!(load_or_create_secret(&key).unwrap(), original);
        let file = std::fs::File::open(&key).unwrap();
        assert!(windows::private_file(file.as_raw_handle()).unwrap());
        drop(file);
        let value = root.join("value");
        atomic_write(&value, b"before").unwrap();
        assert!(atomic_write_with(&value, |file| {
            file.write_all(b"partial")?;
            Err::<(), _>(io::Error::other("rejected"))
        })
        .is_err());
        assert_eq!(read_text_bounded(&value, 32).unwrap(), "before");
        atomic_write(&value, b"after").unwrap();
        assert_eq!(read_text_bounded(&value, 32).unwrap(), "after");
        std::fs::write(&key, b"invalid").unwrap();
        assert_eq!(
            load_or_create_secret(&key).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
