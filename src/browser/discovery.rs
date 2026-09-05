//! Optional browser executable discovery without GTK widget dependencies.

use std::path::PathBuf;

/// Find agent-browser binary in PATH or alongside the cmux binary.
pub(super) fn which_agent_browser() -> Option<PathBuf> {
    // Allow deployments and local development to select an exact binary.
    if let Ok(path) = std::env::var("CMUX_AGENT_BROWSER") {
        let candidate = PathBuf::from(path);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    if let Some(candidate) = find_on_path("agent-browser") {
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
        if let Ok(entries) = std::fs::read_dir(versions) {
            let mut candidates: Vec<(Vec<u64>, PathBuf)> = entries
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
                .collect();
            candidates.sort_by(|left, right| left.0.cmp(&right.0));
            if let Some((_, candidate)) = candidates.pop() {
                return Some(candidate);
            }
        }
    }
    None
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
    .find_map(|name| find_on_path(name))
}

/// Find the first regular file with this name on PATH; execution validates permissions.
fn find_on_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join(name))
            .find(|candidate| candidate.is_file())
    })
}
