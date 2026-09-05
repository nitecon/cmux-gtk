//! Optional browser executable discovery without GTK widget dependencies.

use cmux_platform::paths::find_command_on_path;
use std::path::{Path, PathBuf};

/// Find agent-browser binary in PATH or alongside the cmux binary.
pub(super) fn which_agent_browser() -> Option<PathBuf> {
    // Allow deployments and local development to select an exact binary.
    if let Ok(path) = std::env::var("CMUX_AGENT_BROWSER") {
        let candidate = PathBuf::from(path);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    if let Some(candidate) = find_command_on_path("agent-browser") {
        return Some(candidate);
    }
    // Check alongside cmux binary
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("agent-browser");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    // Check FHS install paths used by Debian and Fedora-family packages.
    for candidate in [
        PathBuf::from("/usr/lib/cmux/agent-browser"),
        PathBuf::from("/usr/lib64/cmux/agent-browser"),
    ] {
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    // Desktop launchers do not inherit an interactive shell's NVM PATH. Find
    // independently installed agent-browser versions without pinning one.
    if let Ok(home) = std::env::var("HOME") {
        let versions = PathBuf::from(home).join(".nvm/versions/node");
        return find_nvm_browser(&versions);
    }
    None
}

/// Select the newest numeric NVM version containing agent-browser with one retained candidate.
/// Ignore unreadable entries, nonnumeric version names and missing binaries. Equal versions
/// keep the last encountered candidate, matching the previous stable-sort selection.
fn find_nvm_browser(versions: &Path) -> Option<PathBuf> {
    std::fs::read_dir(versions)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let version = entry
                .file_name()
                .to_string_lossy()
                .trim_start_matches('v')
                .split('.')
                .map(str::parse::<u64>)
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            let path = entry.path().join("bin/agent-browser");
            path.is_file().then_some((version, path))
        })
        .max_by(|left, right| left.0.cmp(&right.0))
        .map(|(_, path)| path)
}

/// Check executable discovery without launching the optional browser service.
pub fn agent_browser_available() -> bool {
    which_agent_browser().is_some()
}

/// Prefer a system Chromium installation in the supported executable-name order.
pub(super) fn find_system_chrome() -> Option<PathBuf> {
    [
        "google-chrome-stable",
        "google-chrome",
        "chromium",
        "chromium-browser",
    ]
    .iter()
    .find_map(|name| find_command_on_path(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Discover actual version-directory candidates by numeric order without environment mutation.
    #[test]
    fn nvm_discovery_ignores_incomplete_installs_and_uses_numeric_versions() {
        let root = std::env::temp_dir().join(format!("cmux-nvm-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        assert!(find_nvm_browser(&root).is_none());
        for version in [
            "v9.9.9",
            "v20.9.0",
            "v20.10.0",
            "v100.0.0-beta",
            "not-a-version",
        ] {
            let bin = root.join(version).join("bin");
            std::fs::create_dir_all(&bin).unwrap();
            std::fs::write(bin.join("agent-browser"), b"fixture").unwrap();
        }
        // A newer partial install and a directory named like the executable are not candidates.
        std::fs::create_dir_all(root.join("v100.0.0/bin")).unwrap();
        std::fs::create_dir_all(root.join("v101.0.0/bin/agent-browser")).unwrap();
        let newest = root.join("v20.10.0/bin/agent-browser");
        assert_eq!(find_nvm_browser(&root), Some(newest.clone()));
        std::fs::remove_file(newest).unwrap();
        assert_eq!(
            find_nvm_browser(&root),
            Some(root.join("v20.9.0/bin/agent-browser"))
        );
        std::fs::remove_dir_all(&root).unwrap();
        assert!(find_nvm_browser(&root).is_none());
    }
}
