//! Shared Linux application locations for configuration, state and IPC.

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
