use crate::task::run_status;
use std::{path::PathBuf, time::Duration};
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
    let mkdir_status = run_status(
        &mut ssh_command(target, "mkdir -p ~/.local/bin"),
        Duration::from_secs(15),
    )
    .await
    .map_err(|e| format!("SSH mkdir failed: {e}"))?;
    if !mkdir_status.success() {
        return Err("Failed to create remote directory".to_string());
    }

    // A prior SSH session may still be running this executable. Never truncate
    // its inode: publish a fully uploaded executable with an atomic rename.
    let staging_name = format!(".local/bin/cmuxd-remote-{}.tmp", uuid::Uuid::new_v4());
    let remote_dest = format!("{target}:~/{staging_name}");
    let scp_status = run_status(
        Command::new("scp")
            .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=10"])
            .arg(&local_path)
            .arg(&remote_dest),
        Duration::from_secs(60),
    )
    .await;
    if !scp_status.as_ref().is_ok_and(|status| status.success()) {
        remove_staged_daemon(target, &staging_name).await;
        return Err(match scp_status {
            Err(error) => format!("scp failed: {error}"),
            Ok(_) => "Failed to deploy remote daemon".into(),
        });
    }

    // staging_name contains only a fixed prefix and a generated UUID.
    let install_command =
        format!("chmod 755 ~/{staging_name} && mv -f ~/{staging_name} ~/.local/bin/cmuxd-remote");
    let chmod_status = run_status(
        &mut ssh_command(target, &install_command),
        Duration::from_secs(15),
    )
    .await;
    if !chmod_status.as_ref().is_ok_and(|status| status.success()) {
        remove_staged_daemon(target, &staging_name).await;
        return Err(match chmod_status {
            Err(error) => format!("SSH publish failed: {error}"),
            Ok(_) => "Failed to publish remote daemon executable".into(),
        });
    }

    Ok(())
}

/// Best-effort removal of a validated remote staging filename after failed deployment.
async fn remove_staged_daemon(target: &str, staging_name: &str) {
    let _ = run_status(
        &mut ssh_command(target, &format!("rm -f ~/{staging_name}")),
        Duration::from_secs(10),
    )
    .await;
}

/// Construct noninteractive SSH control commands using validated targets and internally generated shell text.
fn ssh_command(target: &str, remote_command: &str) -> Command {
    let mut command = Command::new("ssh");
    command.args([
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=10",
        target,
        remote_command,
    ]);
    command
}
