//! Launch the native Codex TUI against a desktop-owned per-pane backend.
use super::{
    discovery,
    socket_client::{CliError, SocketClient},
};
use serde_json::json;
use std::{path::PathBuf, process::Command, time::Duration};

/// Provider flags stay explicit; the launcher exclusively owns endpoint, thread and working directory.
pub(super) fn launch(args: &[String], socket: Option<&str>) -> Result<(), CliError> {
    if args.iter().any(|arg| {
        [
            "--remote",
            "--remote-auth-token-env",
            "--no-daemon",
            "--worktree",
            "--add-dir",
            "--last",
            "--all",
            "--cd",
            "-C",
        ]
        .iter()
        .any(|name| arg == name || arg.starts_with(&format!("{name}=")))
    }) {
        return Err(CliError::Command("CMUX owns the remote endpoint, conversation and pane directory; these launch overrides are unsupported".into()));
    }
    let surface = std::env::var("CMUX_SURFACE_ID")
        .map_err(|_| CliError::Command("Run cmux codex inside a local CMUX terminal".into()))?;
    uuid::Uuid::parse_str(&surface)
        .map_err(|_| CliError::Command("Invalid CMUX surface identity".into()))?;
    let path = socket
        .map(str::to_owned)
        .or_else(discovery::discover_socket)
        .ok_or_else(|| CliError::Connection("No CMUX socket found".into()))?;
    let executable = executable()
        .ok_or_else(|| CliError::Command("Codex executable not found on PATH".into()))?;
    let executable = executable
        .canonicalize()
        .map_err(|error| CliError::Command(error.to_string()))?;
    let mut client = SocketClient::connect(&path, Duration::from_secs(40))?;
    let context = client.call(
        "gateway.codex.start",
        json!({"surface_id":surface,"executable":executable}),
    )?;
    let result = (|| {
        let thread = context["thread_id"]
            .as_str()
            .ok_or_else(|| CliError::Protocol("Missing Codex thread".into()))?;
        let endpoint = context["endpoint"]
            .as_str()
            .ok_or_else(|| CliError::Protocol("Missing Codex endpoint".into()))?;
        let token = context["token"]
            .as_str()
            .ok_or_else(|| CliError::Protocol("Missing Codex capability".into()))?;
        let status = Command::new(&executable)
            .args([
                "resume",
                thread,
                "--remote",
                endpoint,
                "--remote-auth-token-env",
                "CMUX_CODEX_AUTH_TOKEN",
            ])
            .args(args)
            .env("CMUX_CODEX_AUTH_TOKEN", token)
            .status()
            .map_err(|error| CliError::Command(format!("Cannot start Codex TUI: {error}")))?;
        if status.success() {
            Ok(())
        } else {
            Err(CliError::Command(format!("Codex TUI exited with {status}")))
        }
    })();
    // The desktop also retires the backend when this launcher's native generation exits.
    let stopped = client.call("gateway.codex.stop", json!({"surface_id":surface}));
    result.and(stopped.map(|_| ()))
}

/// Resolve the user's installed provider, without copying binaries or editing their configuration.
fn executable() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        cmux_platform::paths::find_command_on_path("codex")
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|path| path.join("codex"))
            .find(|path| {
                path.metadata()
                    .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            })
    }
}
