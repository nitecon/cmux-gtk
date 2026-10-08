//! Windows bundle staging and rollback. A staged CLI owns replacement after its caller exits.

use super::*;
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::process::CommandExt;
use std::time::{Duration, Instant};

const MANIFEST: &str = "managed-files.json";
const LOG: &str = ".cmux-update.log";

/// Validate portable paths before using them in either a ZIP or installed ownership manifest.
fn manifest_paths(root: &Path) -> Result<Vec<String>> {
    let data = read_metadata(File::open(root.join(MANIFEST))?, 1024 * 1024)?;
    let paths: Vec<String> = serde_json::from_slice(&data).context("invalid bundle manifest")?;
    if paths.is_empty() || paths.len() > 10_000 {
        bail!("invalid bundle file count");
    }
    let mut seen = BTreeSet::new();
    for path in &paths {
        if !safe_path(path) || !seen.insert(path.to_lowercase()) {
            bail!("unsafe or duplicate bundle path: {path}");
        }
    }
    for required in [
        "cmux.exe",
        "cmux-app.exe",
        "ghostty-internal.dll",
        "build.json",
        MANIFEST,
    ] {
        if !paths.iter().any(|path| path == required) {
            bail!("bundle manifest is missing {required}");
        }
    }
    Ok(paths)
}

/// Reject traversal, Windows aliases/streams, and updater-owned private paths.
fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', ':'])
        && !path.chars().any(char::is_control)
        && path.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.ends_with(['.', ' '])
                && !part.starts_with(".cmux-update")
                && !matches!(
                    part.split('.')
                        .next()
                        .unwrap_or("")
                        .to_ascii_uppercase()
                        .as_str(),
                    "CON"
                        | "PRN"
                        | "AUX"
                        | "NUL"
                        | "COM1"
                        | "COM2"
                        | "COM3"
                        | "COM4"
                        | "COM5"
                        | "COM6"
                        | "COM7"
                        | "COM8"
                        | "COM9"
                        | "LPT1"
                        | "LPT2"
                        | "LPT3"
                        | "LPT4"
                        | "LPT5"
                        | "LPT6"
                        | "LPT7"
                        | "LPT8"
                        | "LPT9"
                )
        })
}

/// Verify every parent stays inside the installation and does not follow a reparse link.
fn check_path(root: &Path, relative: &str) -> Result<()> {
    let mut path = root.to_path_buf();
    for part in relative.split('/') {
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!("bundle path is a link: {}", path.display())
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// Download and verify the complete bundle before starting the staged replacement worker.
pub(super) fn stage_update(source: impl Read, checksum: &str, version: &Version) -> Result<()> {
    let executable = std::env::current_exe()?.canonicalize()?;
    let install = executable
        .parent()
        .context("cannot locate Windows installation")?;
    // Mapped app images cannot be replaced safely; no process is stopped by the updater.
    exclusive_file(&install.join("cmux-app.exe"))
        .context("close CMUX and run cmux --update from a separate Windows Terminal")?;
    let staging = install.join(format!(".cmux-update-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&staging)
        .context("CMUX folder is not writable; move it to a user-writable folder")?;
    let result = (|| -> Result<()> {
        let new = staging.join("new");
        fs::create_dir(&new)?;
        let mut download = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(staging.join("bundle.zip"))?;
        download_verified(source, &mut download, checksum)?;
        download.rewind()?;
        extract_bundle(download, &new)?;
        let paths = manifest_paths(&new)?;
        for path in paths {
            if !new.join(&path).is_file() {
                bail!("bundle is missing {path}");
            }
        }
        let build: serde_json::Value = serde_json::from_slice(&read_metadata(
            File::open(new.join("build.json"))?,
            1024 * 1024,
        )?)?;
        if build["platform"] != format!("windows-{}", release_arch()?) {
            bail!("bundle is for a different platform");
        }
        for name in ["cmux.exe", "cmux-app.exe"] {
            validate_pe(&new.join(name))?;
            let label = name.trim_end_matches(".exe");
            if validate_staged_binary(label, &new.join(name))? != format!("{label} {version}") {
                bail!("bundle executable version does not match release v{version}");
            }
        }
        fs::remove_file(staging.join("bundle.zip"))?;
        let log = File::create(install.join(LOG))?;
        std::process::Command::new(new.join("cmux.exe"))
            .arg("__apply-update")
            .arg(install)
            .arg(&staging)
            .stdin(std::process::Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW; worker owns no UI or terminal.
            .spawn()
            .context("cannot start Windows update worker")?;
        eprintln!("cmux: verified v{version}; applying after this command exits. Check {} for completion, then launch CMUX. No app was restarted.", install.join(LOG).display());
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

/// Extract only regular files with bounded total expansion and exact manifest coverage.
fn extract_bundle(file: File, destination: &Path) -> Result<()> {
    let mut archive = zip::ZipArchive::new(file).context("invalid Windows ZIP")?;
    if archive.len() > 10_000 {
        bail!("bundle contains too many files");
    }
    let mut seen = BTreeSet::new();
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let name = entry.name().to_owned();
        if !safe_path(&name)
            || entry.is_dir()
            || !seen.insert(name.to_lowercase())
            || entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            bail!("unsafe or duplicate ZIP entry: {name}");
        }
        total = total
            .checked_add(entry.size())
            .context("ZIP size overflow")?;
        if total > 512 * 1024 * 1024 {
            bail!("expanded bundle exceeds 512 MiB");
        }
        let target = destination.join(&name);
        fs::create_dir_all(target.parent().context("invalid ZIP parent")?)?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)?;
        let size = entry.size();
        let copied = std::io::copy(&mut entry.by_ref().take(size + 1), &mut output)?;
        if copied != size {
            bail!("invalid ZIP entry size");
        }
    }
    let managed: BTreeSet<_> = manifest_paths(destination)?
        .into_iter()
        .map(|path| path.to_lowercase())
        .collect();
    if seen != managed {
        bail!("ZIP entries do not match bundle manifest");
    }
    Ok(())
}

/// Reject incompatible native PE images before executing the downloaded version preflight.
fn validate_pe(path: &Path) -> Result<()> {
    let mut file = File::open(path)?;
    let mut dos = [0u8; 64];
    file.read_exact(&mut dos)?;
    if &dos[..2] != b"MZ" {
        bail!("bundle executable is not PE");
    }
    let offset = u32::from_le_bytes(dos[60..64].try_into().unwrap());
    file.seek(std::io::SeekFrom::Start(offset.into()))?;
    let mut pe = [0u8; 6];
    file.read_exact(&mut pe)?;
    if &pe[..4] != b"PE\0\0" || pe[4..] != [0x64, 0x86] {
        bail!("bundle executable is not Windows x86-64");
    }
    Ok(())
}

/// Obtain a lock proof, closing it immediately; replacement still handles subsequent races.
fn exclusive_file(path: &Path) -> Result<()> {
    if path.exists() {
        OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(path)?;
    }
    Ok(())
}

/// Apply a staged bundle, backing up all owned files first and rolling back on failure.
/// The worker runs from staging, waits for the original CLI, and never kills or restarts apps.
pub(crate) fn apply_update(install: &Path, staging: &Path) -> Result<()> {
    let install = install.canonicalize()?;
    let staging = staging.canonicalize()?;
    if staging.parent() != Some(install.as_path())
        || !staging
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(".cmux-update-"))
        || std::env::current_exe()?.canonicalize()?
            != staging.join("new/cmux.exe").canonicalize()?
    {
        bail!("update worker must run from its own staged bundle");
    }
    let new = staging.join("new");
    let paths = manifest_paths(&new)?;
    let old_paths = if install.join(MANIFEST).exists() {
        manifest_paths(&install)?
    } else {
        Vec::new()
    };
    let owned: BTreeSet<_> = paths.iter().chain(&old_paths).cloned().collect();
    let deadline = Instant::now() + Duration::from_secs(15);
    while exclusive_file(&install.join("cmux.exe")).is_err() {
        if Instant::now() >= deadline {
            bail!("updating CLI did not exit; installation unchanged");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    for path in &owned {
        check_path(&install, path)?;
        exclusive_file(&install.join(path)).with_context(|| format!("{path} is locked; close CMUX and other CMUX commands, then retry cmux --update; installation unchanged"))?;
    }
    for path in &paths {
        check_path(&new, path)?;
    }
    let backup = staging.join("backup");
    fs::create_dir(&backup)?;
    let mut moved = Vec::new();
    let mut installed = Vec::new();
    let result = (|| -> Result<()> {
        for path in &owned {
            let target = install.join(path);
            if target.is_file() {
                let saved = backup.join(path);
                fs::create_dir_all(saved.parent().context("invalid backup path")?)?;
                fs::rename(&target, &saved)?;
                moved.push(path.clone());
            }
        }
        for path in &paths {
            let target = install.join(path);
            fs::create_dir_all(target.parent().context("invalid install path")?)?;
            fs::rename(new.join(path), &target)?;
            installed.push(path.clone());
        }
        Ok(())
    })();
    if let Err(error) = result {
        // Restore in reverse order. Retain backups if a concurrent lock prevents rollback.
        let mut failures = Vec::new();
        for path in installed.iter().rev() {
            // Mapped worker binaries can be renamed back, but cannot be deleted during rollback.
            if let Err(error) = fs::rename(install.join(path), new.join(path)) {
                failures.push(error.to_string());
            }
        }
        for path in moved.iter().rev() {
            if let Err(error) = fs::rename(backup.join(path), install.join(path)) {
                failures.push(error.to_string());
            }
        }
        bail!("update failed: {error:#}; rollback errors: {failures:?}; recovery files retained in {}", staging.display());
    }
    fs::remove_dir_all(&backup)?;
    println!("Updated CMUX successfully. Launch launch-cmux.cmd to use the new version. Settings and sessions were preserved.");
    // All mapped worker files have moved to the installation; only empty staging parents remain.
    let _ = fs::remove_dir_all(&staging);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real extractor rejects checksum errors, traversal, duplicate aliases and missing files.
    #[test]
    fn verified_zip_requires_exact_safe_manifest() {
        let root = std::env::temp_dir().join(format!("cmux-zip-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let required = [
            "cmux.exe",
            "cmux-app.exe",
            "ghostty-internal.dll",
            "build.json",
            MANIFEST,
        ];
        for (index, extra) in [
            None,
            Some("../outside"),
            Some("CMUX.EXE"),
            Some("unmanaged.txt"),
        ]
        .into_iter()
        .enumerate()
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            for name in required.iter().copied().chain(extra) {
                writer
                    .start_file(name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                let data = if name == MANIFEST {
                    serde_json::to_vec(&required).unwrap()
                } else {
                    b"native fixture".to_vec()
                };
                writer.write_all(&data).unwrap();
            }
            let bytes = writer.finish().unwrap().into_inner();
            let archive = root.join(format!("{index}.zip"));
            let checksum = format!("{:x}", Sha256::digest(&bytes));
            download_verified(bytes.as_slice(), File::create(&archive).unwrap(), &checksum)
                .unwrap();
            assert!(download_verified(bytes.as_slice(), std::io::sink(), &"0".repeat(64)).is_err());
            let destination = root.join(index.to_string());
            fs::create_dir(&destination).unwrap();
            assert_eq!(
                extract_bundle(File::open(archive).unwrap(), &destination).is_ok(),
                index == 0
            );
        }
        assert!(!root.join("outside").exists());
        let not_pe = root.join("not-pe.exe");
        fs::write(&not_pe, [0u8; 64]).unwrap();
        assert!(validate_pe(&not_pe).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
