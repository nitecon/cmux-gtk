use std::path::PathBuf;
use tokio::process::Command;

/// Path to the pre-compiled cmuxd-remote binary.
/// Looks in: ~/.local/share/cmux/bin/cmuxd-remote-linux-amd64
pub fn local_daemon_path() -> PathBuf {
    cmux_platform::paths::data_dir().join("bin/cmuxd-remote-linux-amd64")
}

/// Deploy cmuxd-remote to remote host via scp.
/// Copies to ~/.local/bin/cmuxd-remote on the remote.
pub async fn deploy_remote(target: &str) -> Result<(), String> {
    crate::workspace::validate_ssh_target(target)?;
    let local_path = local_daemon_path();
    if !local_path.exists() {
        return Err(format!(
            "cmuxd-remote binary not found at {}. Run: ./scripts/install-cmuxd-remote.sh",
            local_path.display()
        ));
    }

    // Ensure remote directory exists
    let mkdir_status = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            target,
            "mkdir",
            "-p",
            "~/.local/bin",
        ])
        .kill_on_drop(true)
        .status()
        .await
        .map_err(|e| format!("SSH mkdir failed: {e}"))?;
    if !mkdir_status.success() {
        return Err("Failed to create remote directory".to_string());
    }

    // A prior SSH session may still be running this executable. Never truncate
    // its inode: publish a fully uploaded executable with an atomic rename.
    let staging_name = format!(".local/bin/cmuxd-remote-{}.tmp", uuid::Uuid::new_v4());
    let remote_dest = format!("{target}:~/{staging_name}");
    let scp_status = Command::new("scp")
        .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=10"])
        .arg(&local_path)
        .arg(&remote_dest)
        .kill_on_drop(true)
        .status()
        .await
        .map_err(|e| format!("scp failed: {e}"))?;
    if !scp_status.success() {
        remove_staged_daemon(target, &staging_name).await;
        return Err(format!("Failed to deploy remote daemon to {target}"));
    }

    // staging_name contains only a fixed prefix and a generated UUID.
    let install_command =
        format!("chmod 755 ~/{staging_name} && mv -f ~/{staging_name} ~/.local/bin/cmuxd-remote");
    let chmod_status = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            target,
            &install_command,
        ])
        .kill_on_drop(true)
        .status()
        .await
        .map_err(|e| format!("SSH chmod failed: {e}"))?;
    if !chmod_status.success() {
        remove_staged_daemon(target, &staging_name).await;
        return Err("Failed to publish remote daemon executable".to_string());
    }

    Ok(())
}

/// Best-effort removal of a validated remote staging filename after failed deployment.
async fn remove_staged_daemon(target: &str, staging_name: &str) {
    let _ = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            target,
            &format!("rm -f ~/{staging_name}"),
        ])
        .kill_on_drop(true)
        .status()
        .await;
}
