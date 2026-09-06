//! Shared Linux application locations and command search paths.

use std::ffi::OsStr;
use std::path::PathBuf;

/// Resolve an XDG application directory, using the home-relative default.
///
/// Empty or relative XDG overrides are ignored, as required by XDG. Missing
/// HOME preserves the existing relative-directory fallback used by the app.
fn application_directory(variable: &str, fallback: &str) -> PathBuf {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| ".".into())).join(fallback)
        })
        .join("cmux")
}

/// Return the cmux configuration directory without creating it.
pub fn config_dir() -> PathBuf {
    application_directory("XDG_CONFIG_HOME", ".config")
}

/// Return the cmux persistent-data directory without creating it.
pub fn data_dir() -> PathBuf {
    application_directory("XDG_DATA_HOME", ".local/share")
}

/// Return the cmux diagnostic-state directory without creating it.
pub fn state_dir() -> PathBuf {
    application_directory("XDG_STATE_HOME", ".local/state")
}

/// Return the cmux cache directory without creating it.
pub fn cache_dir() -> PathBuf {
    application_directory("XDG_CACHE_HOME", ".cache")
}

/// Return the private runtime directory; the caller sets access permissions.
pub fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| {
            // SAFETY: getuid takes no arguments and has no caller preconditions.
            PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() }))
        })
        .join("cmux")
}

/// Return the default control socket path shared by desktop and CLI discovery.
pub fn socket_path() -> PathBuf {
    runtime_dir().join("cmux.sock")
}

/// Return the runtime discovery marker path shared by the listener and CLI.
pub fn socket_marker_path() -> PathBuf {
    runtime_dir().join("last-socket-path")
}

/// Find the first regular-file candidate on PATH without launching it.
/// Preserve PATH order and relative/empty entries; return None when PATH is absent.
/// This is discovery only: actual execution validates permissions and binary format.
pub fn find_command_on_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|path| find_command_in(name, &path))
}

/// Search a supplied OS-native path list without reading or mutating process environment.
fn find_command_in(name: &str, search_path: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(search_path)
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Search real paths in order, following file symlinks while skipping directories and missing files.
    #[test]
    fn command_search_preserves_path_order() {
        let root = std::env::temp_dir().join(format!(
            "cmux-command-path-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("first")).unwrap();
        std::fs::create_dir_all(root.join("second")).unwrap();
        let first = root.join("first");
        let second = root.join("second");
        std::fs::create_dir(first.join("tool")).unwrap();
        std::fs::write(second.join("tool"), b"candidate").unwrap();
        let search = std::env::join_paths([&first, &second]).unwrap();
        assert_eq!(find_command_in("tool", &search), Some(second.join("tool")));
        std::fs::remove_dir(first.join("tool")).unwrap();
        std::os::unix::fs::symlink(second.join("tool"), first.join("tool")).unwrap();
        assert_eq!(find_command_in("tool", &search), Some(first.join("tool")));
        std::fs::remove_file(second.join("tool")).unwrap();
        assert!(find_command_in("tool", &search).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
