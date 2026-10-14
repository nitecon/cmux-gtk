//! Linux home, runtime, socket and shell path policy.

use std::path::{Path, PathBuf};

/// Retain the existing HOME-relative fallback when an XDG override is absent.
pub(super) fn application_directory(_variable: &str, fallback: &str) -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| ".".into())).join(fallback)
}

/// Return the real user's runtime base without creating it.
pub(super) fn runtime_directory() -> PathBuf {
    // SAFETY: getuid takes no arguments and has no caller preconditions.
    PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() }))
}

/// Return the default control socket path shared by desktop and CLI discovery.
pub fn socket_path() -> PathBuf {
    crate::paths::runtime_dir().join("cmux.sock")
}

/// Borrow the working directory unchanged for POSIX shells.
pub fn terminal_working_directory(path: &Path) -> std::borrow::Cow<'_, Path> {
    std::borrow::Cow::Borrowed(path)
}

/// Find a regular-file candidate within one PATH directory without executing it.
pub(super) fn command_candidate(directory: &Path, name: &str) -> Option<PathBuf> {
    let candidate = directory.join(name);
    candidate.is_file().then_some(candidate)
}
