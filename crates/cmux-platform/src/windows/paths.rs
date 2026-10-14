//! Windows application storage, named-pipe identity and native shell path policy.

use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Component, Path, PathBuf, Prefix};

/// Resolve the existing local-app-data fallback when an XDG override is absent.
pub(super) fn application_directory(variable: &str, _fallback: &str) -> PathBuf {
    PathBuf::from(
        std::env::var_os("LOCALAPPDATA")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .unwrap_or_else(|| ".".into()),
    )
    .join(match variable {
        "XDG_CONFIG_HOME" => "config",
        "XDG_DATA_HOME" => "data",
        "XDG_STATE_HOME" => "state",
        _ => "cache",
    })
}

/// Return the temporary runtime base without creating it.
pub(super) fn runtime_directory() -> PathBuf {
    std::env::temp_dir()
}

/// Return the SID-qualified pipe shared by desktop and CLI; user lookup failure is fatal.
pub fn socket_path() -> PathBuf {
    let user = crate::windows::current_sid().expect("Cannot identify current Windows user");
    PathBuf::from(format!(r"\\.\pipe\cmux-{user}-control"))
}

/// Present canonical local drive paths to shells using ordinary DOS syntax.
/// File operations retain their canonical paths; other Windows namespaces are preserved.
pub fn terminal_working_directory(path: &Path) -> std::borrow::Cow<'_, Path> {
    if matches!(path.components().next(), Some(Component::Prefix(prefix))
        if matches!(prefix.kind(), Prefix::VerbatimDisk(_)))
    {
        let ordinary: Vec<u16> = path.as_os_str().encode_wide().skip(4).collect();
        return std::borrow::Cow::Owned(PathBuf::from(std::ffi::OsString::from_wide(&ordinary)));
    }
    std::borrow::Cow::Borrowed(path)
}

/// Search one PATH directory using the existing literal, exe, cmd, bat, com precedence.
pub(super) fn command_candidate(directory: &Path, name: &str) -> Option<PathBuf> {
    let candidate = directory.join(name);
    if candidate.is_file() {
        return Some(candidate);
    }
    if Path::new(name).extension().is_none() {
        for extension in ["exe", "cmd", "bat", "com"] {
            let candidate = directory.join(format!("{name}.{extension}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}
