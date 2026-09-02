//! Self-update support for direct Linux binary installations.

use anyhow::{bail, Context, Result};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const GITHUB_REPO: &str = "nitecon/cmux-gtk";
const CURRENT_VERSION: &str = env!("CMUX_VERSION");
const CHECK_INTERVAL_SECS: u64 = 3600;
const BINARY_NAMES: &[&str] = &["cmux", "cmux-app"];

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallMethod {
    SelfManaged,
    Homebrew,
    SystemPackage,
    AppImage,
}

/// Run a silent, rate-limited update check for manually installed binaries.
pub fn spawn_auto_update() {
    if env!("CMUX_RELEASE_BUILD") != "1"
        || std::env::var("CMUX_NO_UPDATE").as_deref() == Ok("1")
        || install_method() != InstallMethod::SelfManaged
    {
        return;
    }

    let Ok(marker) = marker_path() else {
        return;
    };
    if !should_check(&marker) {
        return;
    }
    touch_marker(&marker);

    std::thread::spawn(|| {
        if let Err(error) = update_if_available(false) {
            if std::env::var("CMUX_UPDATE_DEBUG").as_deref() == Ok("1") {
                eprintln!("cmux: update check failed: {error:#}");
            }
        }
    });
}

/// Update an unpacked/manual installation, or explain the package-manager path.
pub fn manual_update() -> Result<()> {
    match install_method() {
        InstallMethod::Homebrew => {
            bail!("this cmux is managed by Homebrew; run: brew upgrade --cask cmux-gtk")
        }
        InstallMethod::SystemPackage => {
            bail!("this cmux is managed by a system package; run your package manager upgrade (for example: sudo apt install --only-upgrade cmux-gtk or sudo dnf upgrade cmux-gtk)")
        }
        InstallMethod::AppImage => {
            bail!("this cmux is running from an AppImage; download the latest AppImage release")
        }
        InstallMethod::SelfManaged => update_if_available(true),
    }
}

fn update_if_available(verbose: bool) -> Result<()> {
    let current = Version::parse(CURRENT_VERSION.trim_start_matches('v'))
        .context("invalid compiled cmux version")?;
    if verbose {
        eprintln!("cmux: current version v{current}");
    }

    let client = http_client()?;
    let release = client
        .get(format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest"))
        .send()
        .context("GitHub release request failed")?
        .error_for_status()
        .context("GitHub release request returned an error")?
        .json::<GitHubRelease>()
        .context("invalid GitHub release response")?;
    let latest = Version::parse(release.tag_name.trim_start_matches('v'))
        .context("invalid latest release version")?;

    if latest <= current {
        if verbose {
            eprintln!("cmux: already up to date (v{current})");
        }
        return Ok(());
    }

    let asset_name = format!("cmux-gtk-linux-{}.tar.gz", release_arch()?);
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == asset_name)
        .with_context(|| format!("release v{latest} has no {asset_name} asset"))?;
    let checksum_name = format!("{asset_name}.sha256");
    let checksum_asset = release
        .assets
        .iter()
        .find(|asset| asset.name == checksum_name)
        .with_context(|| format!("release v{latest} has no {checksum_name} asset"))?;
    eprintln!("cmux: updating v{current} -> v{latest}");

    let archive = client
        .get(&asset.browser_download_url)
        .send()
        .context("release download failed")?
        .error_for_status()
        .context("release download returned an error")?
        .bytes()
        .context("failed to read release archive")?;
    let checksum = client
        .get(&checksum_asset.browser_download_url)
        .send()
        .context("release checksum download failed")?
        .error_for_status()
        .context("release checksum download returned an error")?
        .text()
        .context("failed to read release checksum")?;
    verify_checksum(&archive, &checksum)?;
    install_archive(&archive)?;
    touch_marker(&marker_path()?);
    eprintln!("cmux: updated to v{latest}; restart cmux to use it");
    Ok(())
}

fn verify_checksum(archive: &[u8], checksum_file: &str) -> Result<()> {
    let expected = checksum_file
        .split_whitespace()
        .next()
        .context("release checksum is empty")?;
    if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("release checksum is invalid");
    }
    let actual = format!("{:x}", Sha256::digest(archive));
    if !actual.eq_ignore_ascii_case(expected) {
        bail!("release archive checksum mismatch; the installed binaries were not changed");
    }
    Ok(())
}

fn http_client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .user_agent("cmux-gtk-updater")
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .context("failed to create update client")
}

fn release_arch() -> Result<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Ok("x86_64"),
        "aarch64" => Ok("aarch64"),
        arch => bail!("unsupported update architecture: {arch}"),
    }
}

fn install_archive(bytes: &[u8]) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let current_exe = std::env::current_exe()
        .context("cannot locate the running cmux executable")?
        .canonicalize()
        .context("cannot resolve the running cmux executable")?;
    let install_dir = current_exe
        .parent()
        .context("cmux executable has no parent directory")?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let staging_dir = install_dir.join(format!(
        ".cmux-update-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&staging_dir).with_context(|| {
        format!(
            "cannot stage an update in {}; move cmux to a user-writable directory such as ~/.local/bin",
            install_dir.display()
        )
    })?;

    let result = (|| -> Result<()> {
        let decoder = flate2::read::GzDecoder::new(bytes);
        let mut archive = tar::Archive::new(decoder);
        let mut staged = Vec::new();
        for entry in archive.entries().context("invalid release archive")? {
            let mut entry = entry.context("invalid release archive entry")?;
            let path = entry.path().context("invalid release archive path")?;
            let Some(name) = path.file_name().and_then(|name| name.to_str()).map(str::to_owned) else {
                continue;
            };
            if !BINARY_NAMES.contains(&name.as_str()) {
                continue;
            }
            let staging = staging_dir.join(&name);
            entry
                .unpack(&staging)
                .with_context(|| format!("failed to extract {name}"))?;
            std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o755))?;
            staged.push((name.clone(), staging, install_dir.join(name)));
        }

        for required in BINARY_NAMES {
            if !staged.iter().any(|(name, _, _)| name == required) {
                bail!("release archive does not contain {required}");
            }
        }
        for (name, staging, _) in &staged {
            validate_staged_binary(name, staging)?;
        }

        // Replace the companion first and the currently running CLI last.
        staged.sort_by_key(|(name, _, _)| name == "cmux");
        for (name, staging, target) in staged {
            std::fs::rename(&staging, &target)
                .with_context(|| format!("failed to replace {name} at {}", target.display()))?;
        }
        Ok(())
    })();

    let _ = std::fs::remove_dir_all(&staging_dir);
    result
}

fn validate_staged_binary(name: &str, path: &Path) -> Result<()> {
    let output = std::process::Command::new(path)
        .arg("--version")
        .output()
        .with_context(|| {
            format!(
                "downloaded {name} cannot run on this host; the installed binaries were not changed"
            )
        })?;
    if !output.status.success() {
        bail!(
            "downloaded {name} failed its compatibility preflight; the installed binaries were not changed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.trim().starts_with(name) {
        bail!(
            "downloaded {name} returned unexpected version output; the installed binaries were not changed"
        );
    }
    Ok(())
}

fn install_method() -> InstallMethod {
    if std::env::var_os("APPIMAGE").is_some() {
        return InstallMethod::AppImage;
    }
    let Ok(exe) = std::env::current_exe().and_then(|path| path.canonicalize()) else {
        return InstallMethod::SelfManaged;
    };
    let rendered = exe.to_string_lossy();
    if rendered.contains("/.linuxbrew/")
        || rendered.contains("/homebrew/")
        || rendered.contains("/Cellar/")
    {
        InstallMethod::Homebrew
    } else if exe.starts_with("/usr/bin")
        || exe.starts_with("/usr/lib")
        || exe.starts_with("/usr/lib64")
    {
        InstallMethod::SystemPackage
    } else if rendered.contains("/.mount_") {
        InstallMethod::AppImage
    } else {
        InstallMethod::SelfManaged
    }
}

fn marker_path() -> Result<PathBuf> {
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache"))
        })
        .context("cannot locate the update cache directory")?
        .join("cmux");
    std::fs::create_dir_all(&cache)?;
    Ok(cache.join("last-update-check"))
}

fn should_check(marker: &Path) -> bool {
    marker
        .metadata()
        .and_then(|metadata| metadata.modified())
        .map(|modified| modified.elapsed().unwrap_or_default().as_secs() >= CHECK_INTERVAL_SECS)
        .unwrap_or(true)
}

fn touch_marker(marker: &Path) {
    let _ = std::fs::write(marker, "");
}
