//! Linux control-socket discovery shared by command-line adapters.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Discover the cmux socket path using the standard search chain.
///
/// Nonempty environment overrides win even when absent on disk. Otherwise try
/// the XDG socket, bounded marker, fixed debug path and newest tagged debug path.
/// Filesystem failures are skipped. Discovery preserves existing path-existence
/// checks; connecting to the selected endpoint validates its socket behavior.
pub fn discover_socket() -> Option<String> {
    // 1. CMUX_SOCKET env var
    if let Ok(val) = std::env::var("CMUX_SOCKET") {
        if !val.is_empty() {
            return Some(val);
        }
    }

    // Also check CMUX_SOCKET_PATH for backwards compat with Python client
    if let Ok(val) = std::env::var("CMUX_SOCKET_PATH") {
        if !val.is_empty() {
            return Some(val);
        }
    }

    // 2. $XDG_RUNTIME_DIR/cmux/cmux.sock (fallback /run/user/{uid}/cmux/cmux.sock)
    let runtime_dir = crate::paths::runtime_dir();
    let xdg_socket = crate::paths::socket_path().to_string_lossy().into_owned();
    if Path::new(&xdg_socket).exists() {
        return Some(xdg_socket);
    }

    // 3. $XDG_RUNTIME_DIR/cmux/last-socket-path marker file
    let marker = runtime_dir.join("last-socket-path");
    if let Some(path) = read_marker(&marker) {
        return Some(path.to_string_lossy().into_owned());
    }

    // 4. /tmp/cmux-debug.sock
    let debug_sock = "/tmp/cmux-debug.sock";
    if Path::new(debug_sock).exists() {
        return Some(debug_sock.to_string());
    }

    // 5. Tagged debug sockets, retaining only the newest candidate.
    newest_debug_path(Path::new("/tmp")).map(|path| path.to_string_lossy().into_owned())
}

/// Read at most 4097 bytes and reject oversized, non-UTF-8, empty or missing marker targets.
fn read_marker(marker: &Path) -> Option<PathBuf> {
    const MAX_MARKER_BYTES: u64 = 4096;
    let mut contents = Vec::new();
    File::open(marker)
        .ok()?
        .take(MAX_MARKER_BYTES + 1)
        .read_to_end(&mut contents)
        .ok()?;
    if contents.len() > MAX_MARKER_BYTES as usize {
        return None;
    }
    let target = std::str::from_utf8(&contents).ok()?.trim();
    if target.is_empty() {
        return None;
    }
    let path = PathBuf::from(target);
    path.exists().then_some(path)
}

/// Keep one newest matching entry, preserving the first enumerated entry on equal timestamps.
fn newest_debug_path(directory: &Path) -> Option<PathBuf> {
    let mut newest = None;
    for entry in std::fs::read_dir(directory).ok()?.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("cmux-debug-") || !name.ends_with(".sock") {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let modified = metadata.modified().unwrap_or(std::time::UNIX_EPOCH);
        if newest
            .as_ref()
            .is_none_or(|(_, previous)| modified > *previous)
        {
            newest = Some((entry.path(), modified));
        }
    }
    newest.map(|(path, _)| path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    /// Own a unique filesystem fixture without changing process environment or shared debug paths.
    struct Directory(PathBuf);

    impl Directory {
        /// Create a fixture directory for this test invocation.
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path =
                std::env::temp_dir().join(format!("cmux-discovery-{}-{nonce}", std::process::id()));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Directory {
        /// Remove only fixture-owned entries, including when an assertion unwinds.
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Marker targets preserve Unicode and trimming; malformed or oversized files are ignored.
    #[test]
    fn bounded_marker_targets() {
        let root = Directory::new();
        let target = root.0.join("日本語.sock");
        File::create(&target).unwrap();
        let marker = root.0.join("marker");
        std::fs::write(&marker, format!(" {}\n", target.display())).unwrap();
        assert_eq!(read_marker(&marker), Some(target.clone()));
        for contents in [
            vec![],
            vec![b' '; 4097],
            vec![0xff],
            b"/missing/cmux.sock".to_vec(),
        ] {
            std::fs::write(&marker, contents).unwrap();
            assert_eq!(read_marker(&marker), None);
        }
        std::fs::remove_file(&marker).unwrap();
        assert_eq!(read_marker(&marker), None);
    }

    /// Discovery skips unrelated filenames and picks the newest matching candidate without sorting.
    #[test]
    fn newest_tagged_candidate() {
        let root = Directory::new();
        for (name, seconds) in [
            ("cmux-debug-old.sock", 1),
            ("cmux-debug-new.sock", 2),
            ("other.sock", 3),
        ] {
            File::create(root.0.join(name))
                .unwrap()
                .set_times(
                    std::fs::FileTimes::new()
                        .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)),
                )
                .unwrap();
        }
        let newest = root.0.join("cmux-debug-new.sock");
        assert_eq!(newest_debug_path(&root.0), Some(newest.clone()));
        std::fs::remove_file(newest).unwrap();
        assert_eq!(
            newest_debug_path(&root.0),
            Some(root.0.join("cmux-debug-old.sock"))
        );
        assert_eq!(newest_debug_path(&root.0.join("missing")), None);
    }
}
