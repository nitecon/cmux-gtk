//! Linux installation ownership used to protect package-managed executables.

use std::path::Path;

/// The installation mechanism responsible for replacing application binaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallMethod {
    /// A direct binary installation may use the application's updater.
    SelfManaged,
    /// Homebrew owns installation and upgrades.
    Homebrew,
    /// A system package manager owns installation and upgrades.
    SystemPackage,
    /// An AppImage bundle requires image-level replacement.
    AppImage,
}

/// Detect the running executable's owner, resolving symlinks before classification.
///
/// If executable discovery fails, preserve direct-install behavior. An APPIMAGE
/// environment marker takes precedence over executable location.
pub fn method() -> InstallMethod {
    if std::env::var_os("APPIMAGE").is_some() {
        return InstallMethod::AppImage;
    }
    std::env::current_exe()
        .and_then(|path| path.canonicalize())
        .map(|path| classify(&path))
        .unwrap_or(InstallMethod::SelfManaged)
}

/// Classify a canonical executable location without reading process-global state.
fn classify(executable: &Path) -> InstallMethod {
    let rendered = executable.to_string_lossy();
    if rendered.contains("/.linuxbrew/")
        || rendered.contains("/homebrew/")
        || rendered.contains("/Cellar/")
    {
        InstallMethod::Homebrew
    } else if executable.starts_with("/usr/bin")
        || executable.starts_with("/usr/lib")
        || executable.starts_with("/usr/lib64")
    {
        InstallMethod::SystemPackage
    } else if rendered.contains("/.mount_") {
        InstallMethod::AppImage
    } else {
        InstallMethod::SelfManaged
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Preserve package ownership while distinguishing similarly prefixed paths.
    #[test]
    fn recognizes_managed_installations() {
        for (path, expected) in [
            ("/usr/bin/cmux", InstallMethod::SystemPackage),
            ("/usr/lib/cmux/cmux-app", InstallMethod::SystemPackage),
            (
                "/home/linuxbrew/.linuxbrew/bin/cmux",
                InstallMethod::Homebrew,
            ),
            ("/tmp/.mount_cmux123/usr/bin/cmux", InstallMethod::AppImage),
            ("/usr/binaries/cmux", InstallMethod::SelfManaged),
            ("/home/user/.local/bin/cmux", InstallMethod::SelfManaged),
        ] {
            assert_eq!(classify(Path::new(path)), expected, "{path}");
        }
    }
}
