//! Claude Code team launcher and the narrow tmux protocol translated to native cmux panes.

use super::socket_client::{CliError, SocketClient};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

const TEAM_GUIDANCE: &str = "You are Claude Code running inside cmux. Agent teams are enabled. When work can run in parallel, spawn NAMED teammates with distinct short role names in one message; each named teammate opens in its own native cmux split.";

/// Launch Claude with a private tmux shim, preserving argv boundaries and the caller identity.
pub(super) fn launch(args: &[String]) -> Result<(), CliError> {
    let surface = std::env::var("CMUX_SURFACE_ID")
        .map_err(|_| CliError::Command("claude-teams must run inside a cmux terminal".into()))?;
    uuid::Uuid::parse_str(&surface)
        .map_err(|_| CliError::Command("CMUX_SURFACE_ID is not a valid cmux identity".into()))?;
    let cmux = std::env::current_exe()
        .map_err(|error| CliError::Command(format!("cannot resolve cmux executable: {error}")))?;
    let shim = install_shim()?;
    let layout_state = shim.join(format!("{surface}.json"));
    save_layout_state(&layout_state, &TeamLayoutState::default())?;
    let claude = find_executable("claude")
        .map_err(|_| CliError::Command("claude executable not found in PATH".into()))?;
    let mut forwarded = args.to_vec();
    if !forwarded
        .iter()
        .any(|arg| arg == "--teammate-mode" || arg.starts_with("--teammate-mode="))
    {
        forwarded.splice(0..0, ["--teammate-mode".into(), "auto".into()]);
    }
    if !forwarded.iter().any(|arg| {
        [
            "--system-prompt",
            "--system-prompt-file",
            "--append-system-prompt",
            "--append-system-prompt-file",
        ]
        .iter()
        .any(|name| arg == name || arg.starts_with(&format!("{name}=")))
    }) {
        let position = usize::from(
            forwarded
                .first()
                .is_some_and(|arg| arg == "--teammate-mode"),
        ) * 2;
        forwarded.splice(
            position..position,
            ["--append-system-prompt".into(), TEAM_GUIDANCE.into()],
        );
    }
    let inherited = std::env::var("PATH").unwrap_or_default();
    let mut command = std::process::Command::new(claude);
    command
        .args(forwarded)
        .env("PATH", format!("{}:{inherited}", shim.display()))
        .env("CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS", "1")
        .env("TERM_PROGRAM", "cmux")
        .env("TMUX", format!("cmux,{},0", std::process::id()))
        .env("TMUX_PANE", pane_token(&surface))
        .env("CMUX_TEAMS_LEADER_SURFACE", &surface)
        .env("CMUX_TEAMS_LAYOUT_STATE", layout_state)
        .env("CMUX_TEAMS_CMUX_BIN", cmux);
    if exact_option(args, "--dangerously-skip-permissions") {
        command.env("CLAUDE_CODE_SANDBOXED", "1");
    }
    Err(CliError::Command(format!(
        "claude-teams launch failed: {}",
        cmux_platform::process::replace_current(&mut command)
    )))
}

/// Translate the subset of tmux used by Claude Code teams into cmux RPCs.
pub(super) fn tmux_compat(args: &[String], explicit_socket: Option<&str>) -> Result<(), CliError> {
    if matches!(args.first().map(String::as_str), Some("-V" | "-v")) {
        println!("tmux 3.4 (cmux native shim)");
        return Ok(());
    }
    let command = args
        .first()
        .map(String::as_str)
        .ok_or_else(|| CliError::Command("tmux shim requires a command".into()))?;
    let tail = &args[1..];
    if matches!(
        command,
        "start-server"
            | "has-session"
            | "set-option"
            | "set"
            | "set-hook"
            | "refresh-client"
            | "resize-pane"
    ) {
        return Ok(());
    }
    let leader = std::env::var("CMUX_TEAMS_LEADER_SURFACE")
        .or_else(|_| std::env::var("CMUX_SURFACE_ID"))
        .map_err(|_| CliError::Command("tmux shim has no cmux leader surface".into()))?;
    let socket = explicit_socket
        .map(str::to_owned)
        .or_else(|| std::env::var("CMUX_SOCKET_PATH").ok())
        .or_else(cmux_platform::discovery::discover_socket)
        .ok_or_else(|| CliError::Connection("no cmux socket found".into()))?;
    let mut client = SocketClient::connect(&socket, Duration::from_secs(10))?;
    match command {
        "display-message" | "display" => {
            let format = option_value(tail, "-p")
                .or_else(|| tail.last().map(String::as_str))
                .unwrap_or("#{pane_id}");
            println!("{}", render(format, &leader));
        }
        "list-panes" | "lsp" => {
            let format = option_value(tail, "-F").unwrap_or("#{pane_id}");
            let result = client.call("surface.list", serde_json::json!({}))?;
            let rows = result
                .get("surfaces")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| CliError::Protocol("surface.list returned no surfaces".into()))?;
            let workspace = rows
                .iter()
                .find(|row| {
                    row.get("uuid").and_then(serde_json::Value::as_str) == Some(leader.as_str())
                })
                .and_then(|row| row.get("workspace_uuid"))
                .and_then(serde_json::Value::as_str);
            for id in rows
                .iter()
                .filter(|row| {
                    workspace.is_none()
                        || row
                            .get("workspace_uuid")
                            .and_then(serde_json::Value::as_str)
                            == workspace
                })
                .filter_map(|row| row.get("uuid").and_then(serde_json::Value::as_str))
            {
                println!("{}", render(format, id));
            }
        }
        "split-window" | "splitw" => split_window(&mut client, tail, &leader)?,
        "select-layout" => {
            let name = tail.iter().rev().find(|arg| !arg.starts_with('-'));
            if name.is_some_and(|name| name == "main-vertical") {
                let path = layout_state_path()?;
                let mut state = load_layout_state(&path);
                state.main_vertical = true;
                save_layout_state(&path, &state)?;
            }
        }
        "respawn-pane" | "respawnp" => {
            let target = target_surface(option_value(tail, "-t").unwrap_or(&leader));
            let command = positional_command(tail);
            if !command.is_empty() {
                client.call(
                    "surface.send_key",
                    serde_json::json!({"id":target,"key":"\u{3}"}),
                )?;
                client.call(
                    "surface.send_text",
                    serde_json::json!({"id":target,"text":format!("{command}\r")}),
                )?;
            }
        }
        "kill-pane" | "killp" => {
            let target = target_surface(option_value(tail, "-t").unwrap_or(&leader));
            client.call("surface.close", serde_json::json!({"id":target}))?;
        }
        other => {
            return Err(CliError::Command(format!(
                "unsupported cmux tmux command: {other}"
            )))
        }
    }
    Ok(())
}

fn split_window(client: &mut SocketClient, args: &[String], leader: &str) -> Result<(), CliError> {
    let path = layout_state_path()?;
    let mut layout = load_layout_state(&path);
    let stacking =
        args.iter().any(|arg| arg == "-h") && layout.main_vertical && layout.last_split.is_some();
    let direction = if args.iter().any(|arg| arg == "-v") || stacking {
        "vertical"
    } else {
        "horizontal"
    };
    let anchor = layout
        .last_split
        .as_deref()
        .filter(|_| stacking)
        .unwrap_or(leader);
    let result = client.call(
        "surface.split",
        serde_json::json!({"id":anchor,"direction":direction}),
    )?;
    let id = result
        .get("uuid")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| CliError::Protocol("surface.split returned no identity".into()))?;
    layout.last_split = Some(id.into());
    save_layout_state(&path, &layout)?;
    let body = positional_command(args);
    if !body.is_empty() {
        let body = if let Some(cwd) = option_value(args, "-c") {
            format!("cd -- {} && {body}", shell_quote(cwd))
        } else {
            body
        };
        client.call(
            "surface.send_text",
            serde_json::json!({"id":id,"text":format!("{body}\r")}),
        )?;
    }
    if args.iter().any(|arg| arg == "-P") {
        println!(
            "{}",
            render(option_value(args, "-F").unwrap_or("#{pane_id}"), id)
        );
    }
    if args.iter().any(|arg| arg == "-d") {
        client.call("surface.focus", serde_json::json!({"id":leader}))?;
    }
    Ok(())
}

fn install_shim() -> Result<PathBuf, CliError> {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("cmux")
        .join("claude-teams-bin");
    cmux_platform::filesystem::create_private_directory(&root)
        .map_err(|error| CliError::Command(format!("cannot create team shim: {error}")))?;
    let path = root.join("tmux");
    cmux_platform::filesystem::atomic_write(
        &path,
        b"#!/bin/sh\nexec \"${CMUX_TEAMS_CMUX_BIN:?}\" __tmux-compat \"$@\"\n",
    )
    .map_err(|error| CliError::Command(format!("cannot write team shim: {error}")))?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| CliError::Command(format!("cannot protect team shim: {error}")))?;
    Ok(root)
}

#[derive(Default, Deserialize, Serialize)]
struct TeamLayoutState {
    main_vertical: bool,
    last_split: Option<String>,
}

fn layout_state_path() -> Result<PathBuf, CliError> {
    std::env::var_os("CMUX_TEAMS_LAYOUT_STATE")
        .map(PathBuf::from)
        .ok_or_else(|| CliError::Command("tmux shim has no layout state".into()))
}

fn load_layout_state(path: &std::path::Path) -> TeamLayoutState {
    let mut state: TeamLayoutState = cmux_platform::filesystem::read_text_bounded(path, 4096)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    if state
        .last_split
        .as_deref()
        .is_some_and(|id| uuid::Uuid::parse_str(id).is_err())
    {
        state.last_split = None;
    }
    state
}

fn save_layout_state(path: &std::path::Path, state: &TeamLayoutState) -> Result<(), CliError> {
    let encoded = serde_json::to_vec(state)
        .map_err(|error| CliError::Command(format!("cannot encode team layout: {error}")))?;
    cmux_platform::filesystem::atomic_write(path, &encoded)
        .map_err(|error| CliError::Command(format!("cannot save team layout: {error}")))
}

fn find_executable(name: &str) -> Result<PathBuf, ()> {
    use std::os::unix::fs::PermissionsExt;
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|directory| directory.join(name))
        .find(|candidate| {
            candidate.metadata().is_ok_and(|metadata| {
                metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
            })
        })
        .ok_or(())
}

fn exact_option(args: &[String], wanted: &str) -> bool {
    let value_options = [
        "--add-dir",
        "--agents",
        "--allowedTools",
        "--append-system-prompt",
        "--append-system-prompt-file",
        "--betas",
        "--debug-file",
        "--disallowedTools",
        "--fallback-model",
        "--input-format",
        "--json-schema",
        "--max-budget-usd",
        "--mcp-config",
        "--model",
        "--output-format",
        "--permission-mode",
        "--permission-prompt-tool",
        "--plugin-dir",
        "--resume",
        "--session-id",
        "--settings",
        "--system-prompt",
        "--system-prompt-file",
        "--teammate-mode",
        "--tools",
    ];
    let mut skip_value = false;
    for arg in args {
        if arg == "--" {
            break;
        }
        if skip_value {
            skip_value = false;
            continue;
        }
        if arg == wanted {
            return true;
        }
        skip_value = value_options.contains(&arg.as_str());
    }
    false
}

fn pane_token(surface: &str) -> String {
    format!("%{surface}")
}
fn target_surface(value: &str) -> &str {
    value.strip_prefix('%').unwrap_or(value)
}
fn render(format: &str, surface: &str) -> String {
    format
        .replace("#{pane_id}", &pane_token(surface))
        .replace("#{pane_active}", "1")
        .replace("#{pane_title}", "cmux agent")
        .replace("#{session_name}", "cmux")
        .replace("#{window_index}", "1")
}
fn option_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
}
fn positional_command(args: &[String]) -> String {
    let value_flags = ["-c", "-F", "-l", "-t"];
    let mut skip = false;
    args.iter()
        .filter_map(|arg| {
            if skip {
                skip = false;
                return None;
            }
            if value_flags.contains(&arg.as_str()) {
                skip = true;
                return None;
            }
            (!arg.starts_with('-')).then_some(arg.as_str())
        })
        .collect::<Vec<_>>()
        .join(" ")
}
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn team_argument_helpers_preserve_boundaries() {
        assert!(exact_option(
            &["--dangerously-skip-permissions".into()],
            "--dangerously-skip-permissions"
        ));
        assert!(!exact_option(
            &["--".into(), "--dangerously-skip-permissions".into()],
            "--dangerously-skip-permissions"
        ));
        assert!(!exact_option(
            &[
                "--append-system-prompt".into(),
                "--dangerously-skip-permissions".into()
            ],
            "--dangerously-skip-permissions"
        ));
        assert_eq!(shell_quote("a'b"), "'a'\"'\"'b'");
        assert_eq!(
            render(
                "#{pane_id}:#{pane_active}",
                "12345678-0000-4000-8000-000000000000"
            ),
            "%12345678-0000-4000-8000-000000000000:1"
        );
    }
}
