//! Self-update support for direct Linux binary installations.

use anyhow::{bail, Context, Result};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, Write};
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

use cmux_platform::installation::{method as install_method, InstallMethod};

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

/// Check the latest release and replace a direct installation when newer.
/// Performs blocking network and filesystem I/O; callers own worker placement.
fn update_if_available(verbose: bool) -> Result<()> {
    let current = Version::parse(CURRENT_VERSION.trim_start_matches('v'))
        .context("invalid compiled cmux version")?;
    if verbose {
        eprintln!("cmux: current version v{current}");
    }

    let client = http_client()?;
    let release_response = client
        .get(format!(
            "https://api.github.com/repos/{GITHUB_REPO}/releases/latest"
        ))
        .send()
        .context("GitHub release request failed")?
        .error_for_status()
        .context("GitHub release request returned an error")?;
    let release: GitHubRelease =
        serde_json::from_slice(&read_metadata(release_response, 1024 * 1024)?)
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
        .context("release download returned an error")?;
    let checksum = client
        .get(&checksum_asset.browser_download_url)
        .send()
        .context("release checksum download failed")?
        .error_for_status()
        .context("release checksum download returned an error")?;
    let checksum = String::from_utf8(read_metadata(checksum, 4096)?)
        .context("release checksum is not UTF-8")?;
    install_archive(archive, &checksum)?;
    touch_marker(&marker_path()?);
    eprintln!("cmux: updated to v{latest}; restart cmux to use it");
    Ok(())
}

/// Read small release metadata with a caller-specified cap, rejecting overflow before parsing.
fn read_metadata(source: impl Read, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let capacity = limit
        .checked_add(1)
        .context("invalid metadata byte limit")?;
    source
        .take(capacity)
        .read_to_end(&mut bytes)
        .context("failed to read release metadata")?;
    if bytes.len() as u64 > limit {
        bail!("release metadata exceeds byte limit");
    }
    Ok(bytes)
}

/// Stream the archive through a 64-KiB buffer to staging while computing its SHA-256.
/// Invalid checksums or read/write failures prevent extraction; the caller owns staging cleanup.
fn download_verified(
    mut source: impl Read,
    mut destination: impl Write,
    checksum_file: &str,
) -> Result<()> {
    let expected = checksum_file
        .split_whitespace()
        .next()
        .context("release checksum is empty")?;
    if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("release checksum is invalid");
    }
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = source
            .read(&mut buffer)
            .context("failed to read release archive")?;
        if count == 0 {
            break;
        }
        destination
            .write_all(&buffer[..count])
            .context("failed to stage release archive")?;
        hash.update(&buffer[..count]);
    }
    destination
        .flush()
        .context("failed to flush release archive")?;
    if !format!("{:x}", hash.finalize()).eq_ignore_ascii_case(expected) {
        bail!("release archive checksum mismatch; the installed binaries were not changed");
    }
    Ok(())
}

/// Construct the updater HTTP client with bounded connection and request deadlines.
fn http_client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .user_agent("cmux-gtk-updater")
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .context("failed to create update client")
}

/// Map the current CPU architecture to release asset naming or return an unsupported error.
fn release_arch() -> Result<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Ok("x86_64"),
        "aarch64" => Ok("aarch64"),
        arch => bail!("unsupported update architecture: {arch}"),
    }
}

/// Stage both executables, validate them, then replace companions before the CLI.
/// Cleans staging after success or failure; individual renames are atomic, the pair is not.
fn install_archive(source: impl Read, checksum: &str) -> Result<()> {
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
    let staging_dir = install_dir.join(format!(".cmux-update-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&staging_dir).with_context(|| {
        format!(
            "cannot stage an update in {}; move cmux to a user-writable directory such as ~/.local/bin",
            install_dir.display()
        )
    })?;

    let result = (|| -> Result<()> {
        cmux_platform::filesystem::create_private_directory(&staging_dir)?;
        let mut download = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(staging_dir.join("archive.tar.gz"))
            .context("cannot create staged archive")?;
        download_verified(source, &mut download, checksum)?;
        download.rewind().context("cannot rewind staged archive")?;
        let decoder = flate2::read::GzDecoder::new(download);
        let mut archive = tar::Archive::new(decoder);
        let mut staged = Vec::new();
        for entry in archive.entries().context("invalid release archive")? {
            let mut entry = entry.context("invalid release archive entry")?;
            let path = entry.path().context("invalid release archive path")?;
            let Some(name) = path
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
            else {
                continue;
            };
            if !BINARY_NAMES.contains(&name.as_str()) {
                continue;
            }
            let staging = staging_dir.join(&name);
            entry
                .unpack(&staging)
                .with_context(|| format!("failed to extract {name}"))?;
            cmux_platform::filesystem::set_executable_permissions(&staging)?;
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

/// Run the staged executable version preflight and reject incompatible binaries.
fn validate_staged_binary(name: &str, path: &Path) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("cannot initialize update preflight runtime")?;
    let mut command = tokio::process::Command::new(path);
    command.arg("--version");
    let output = runtime
        .block_on(crate::task::run_output(
            command,
            std::time::Duration::from_secs(5),
            4096,
            4096,
            |error| eprintln!("cmux: preflight cleanup failed: {:?}", error.kind()),
        ))
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

/// Create the shared update-cache directory and return its last-check marker path.
fn marker_path() -> Result<PathBuf> {
    let cache = cmux_platform::paths::cache_dir();
    std::fs::create_dir_all(&cache)?;
    Ok(cache.join("last-update-check"))
}

/// Allow an update check when the marker is absent, unreadable or older than the interval.
fn should_check(marker: &Path) -> bool {
    marker
        .metadata()
        .and_then(|metadata| metadata.modified())
        .map(|modified| modified.elapsed().unwrap_or_default().as_secs() >= CHECK_INTERVAL_SECS)
        .unwrap_or(true)
}

/// Record a completed update check by replacing its timestamp marker.
fn touch_marker(marker: &Path) {
    let _ = std::fs::write(marker, "");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Preserve all streamed bytes and reject checksum mismatches and metadata overflow.
    #[test]
    fn streamed_archive_and_metadata_bounds() {
        let payload = vec![0x5a; 256 * 1024 + 17];
        let checksum = format!("{:x}  archive.tar.gz", Sha256::digest(&payload));
        let mut staged = Vec::new();
        download_verified(payload.as_slice(), &mut staged, &checksum).unwrap();
        assert_eq!(staged, payload);
        assert!(download_verified(b"corrupted".as_slice(), std::io::sink(), &checksum).is_err());
        assert!(download_verified(payload.as_slice(), std::io::sink(), "invalid").is_err());
        assert_eq!(read_metadata(b"1234".as_slice(), 4).unwrap(), b"1234");
        assert!(read_metadata(b"12345".as_slice(), 4).is_err());
        assert!(read_metadata(b"".as_slice(), u64::MAX).is_err());
        assert!(download_verified(
            payload.as_slice(),
            std::io::Cursor::new(&mut [0u8; 3][..]),
            &checksum
        )
        .is_err());
    }

    /// Real staged executables must pass version validation within bounded pipes and time.
    #[test]
    fn staged_preflight_rejects_overflow_and_hang() {
        let root = std::env::temp_dir().join(format!("cmux-preflight-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let binary = root.join("cmux");
        for (script, succeeds) in [
            ("#!/bin/sh\nprintf 'cmux 1.0.0\\n'\n", true),
            ("#!/bin/sh\nprintf 'other 1.0.0\\n'\n", false),
            ("#!/bin/sh\nhead -c 4097 /dev/zero\n", false),
            ("#!/bin/sh\nexit 7\n", false),
        ] {
            std::fs::write(&binary, script).unwrap();
            cmux_platform::filesystem::set_executable_permissions(&binary).unwrap();
            assert_eq!(validate_staged_binary("cmux", &binary).is_ok(), succeeds);
        }
        std::fs::write(&binary, "#!/bin/sh\nexec sleep 30\n").unwrap();
        let error = validate_staged_binary("cmux", &binary).unwrap_err();
        assert!(error.chain().any(|source| source
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::TimedOut)));
        std::fs::remove_dir_all(root).unwrap();
    }
}
