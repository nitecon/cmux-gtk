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
    let (thread_options, tui_args) = options(args)?;
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
        json!({"surface_id":surface,"executable":executable,"thread_options":thread_options}),
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
            .args(tui_args)
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

/// Native remote resume rejects model/permission overrides; apply explicit choices at thread creation.
fn options(args: &[String]) -> Result<(serde_json::Value, Vec<String>), CliError> {
    let mut options = json!({});
    let mut tui = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let (name, inline) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(name, value)| (name, Some(value)));
        let field = match name {
            "-m" | "--model" => Some("model"),
            "-s" | "--sandbox" => Some("sandbox"),
            "-a" | "--ask-for-approval" => Some("approvalPolicy"),
            _ => None,
        };
        if let Some(field) = field {
            let value = inline
                .or_else(|| args.next().map(String::as_str))
                .filter(|s| !s.is_empty())
                .ok_or_else(|| CliError::Command(format!("Missing value for {name}")))?;
            options[field] = json!(value);
        } else if matches!(
            name,
            "--yolo" | "--dangerously-bypass-approvals-and-sandbox"
        ) {
            options["approvalPolicy"] = json!("never");
            options["sandbox"] = json!("danger-full-access");
        } else {
            tui.push(arg.clone());
        }
    }
    Ok((options, tui))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Explicit permissions configure the new thread and never reach native remote resume.
    #[test]
    fn remote_thread_options_are_explicit() {
        let args = [
            "--sandbox",
            "read-only",
            "-a",
            "never",
            "--model=test",
            "--no-alt-screen",
        ]
        .map(str::to_owned);
        let (params, tui) = options(&args).unwrap();
        assert_eq!(
            params,
            json!({"sandbox":"read-only","approvalPolicy":"never","model":"test"})
        );
        assert_eq!(tui, ["--no-alt-screen"]);
        assert_eq!(options(&[]).unwrap().0, json!({}));
        assert!(options(&["--model".into()]).is_err());
    }
}
