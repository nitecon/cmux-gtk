//! cmux CLI — clap-based argument parser and command dispatch.
//!
//! This module is entirely independent of GTK4 and the GUI app.
//! It connects to the cmux-app via Unix socket JSON-RPC.

#[path = "../bounded_json.rs"]
mod bounded_json;
#[path = "../browser_address.rs"]
mod browser_address;
use cmux_platform::discovery;
pub mod format;
mod hooks;
#[path = "../resume.rs"]
#[allow(dead_code)]
mod resume;
pub mod socket_client;
#[path = "../updater.rs"]
#[allow(dead_code)]
mod updater;

pub use socket_client::CliError;

mod args;
pub use args::{BrowserCommand, Cli, Commands};
use std::io::Write;
use std::time::Duration;

/// Replace this CLI in its owning local terminal with an explicitly requested saved command.
/// Validate identity/checkpoint/location before exec; never inject text into a foreground application.
fn restore_terminal(
    mut client: socket_client::SocketClient,
    surface: Option<&str>,
    checkpoint: Option<&str>,
    automatic: bool,
) -> Result<(), CliError> {
    use std::io::IsTerminal;
    let current = std::env::var("CMUX_SURFACE_ID").ok();
    let surface = surface
        .ok_or_else(|| CliError::Command("restore requires a cmux terminal surface".into()))?;
    if current.as_deref() != Some(surface) || !std::io::stdin().is_terminal() {
        return Err(CliError::Command(
            "run restore inside the target cmux terminal".into(),
        ));
    }
    let response = client.call(
        "surface.resume.show",
        serde_json::json!({"surface_id": surface}),
    )?;
    if response
        .get("execution_location")
        .and_then(|value| value.as_str())
        != Some("local")
    {
        return Err(CliError::Command(
            "remote resume must run through its remote workspace transport".into(),
        ));
    }
    if automatic
        && response
            .get("auto_resume")
            .and_then(|value| value.as_bool())
            != Some(true)
    {
        return Err(CliError::Command(
            "automatic resume requires a current approval in Preferences".into(),
        ));
    }
    let mut binding: resume::ResumeBinding =
        serde_json::from_value(response.get("resume_binding").cloned().unwrap_or_default())
            .map_err(|_| CliError::Command("terminal has no usable resume binding".into()))?;
    binding
        .validate()
        .map_err(|error| CliError::Command(error.into()))?;
    binding.sanitize_environment();
    if checkpoint.is_some() && checkpoint != binding.checkpoint_id.as_deref() {
        return Err(CliError::Command("checkpoint mismatch".into()));
    }
    let mut command = std::process::Command::new("/bin/sh");
    command
        .arg("-c")
        .arg(&binding.command)
        .envs(&binding.environment);
    if let Some(cwd) = binding.cwd.as_ref().filter(|cwd| !cwd.is_empty()) {
        command.current_dir(cwd);
    }
    drop(client);
    Err(CliError::Command(format!(
        "resume launch failed: {}",
        cmux_platform::process::replace_current(&mut command)
    )))
}

/// Run the CLI with the parsed arguments.
pub fn run(cli: Cli) -> Result<(), CliError> {
    if let Commands::Hooks {
        command: args::HookCommands::Setup { agent },
    } = &cli.command
    {
        return hooks::setup(agent.as_deref());
    }
    // Global agent settings also run outside cmux; only implicit, context-free hooks are skipped.
    // An explicit socket still gets ordinary connection errors instead of silent success.
    if matches!(
        &cli.command,
        Commands::Hooks {
            command: args::HookCommands::Claude { .. }
        }
    ) && std::env::var_os("CMUX_SURFACE_ID").is_none()
        && cli.socket.is_none()
    {
        return Ok(());
    }
    if matches!(cli.command, Commands::Update) {
        updater::manual_update().map_err(|e| CliError::Command(format!("{e:#}")))?;
        return Ok(());
    }

    // Resolve socket path: --socket flag > discovery > error
    let socket_path = if let Some(ref path) = cli.socket {
        path.clone()
    } else {
        discovery::discover_socket().ok_or_else(|| {
            CliError::Connection("no cmux socket found (is cmux-app running?)".into())
        })?
    };

    // Keep the CLI alive through the daemon operation and both response boundaries.
    let timeout = match &cli.command {
        Commands::Browser(BrowserCommand::Wait { timeout_ms, .. }) => {
            let (_, client) = crate::browser_timeout::wait_budgets(*timeout_ms);
            client
        }
        Commands::Browser(BrowserCommand::Open { .. }) => Duration::from_secs(30),
        _ => Duration::from_secs(5),
    };

    let mut client = socket_client::SocketClient::connect(&socket_path, timeout)?;
    if let Commands::Hooks {
        command: args::HookCommands::Claude { event },
    } = &cli.command
    {
        return hooks::claude_event(&mut client, *event);
    }

    if let Commands::Restore {
        surface,
        checkpoint,
        automatic,
    } = &cli.command
    {
        return restore_terminal(
            client,
            surface.as_deref(),
            checkpoint.as_deref(),
            *automatic,
        );
    }

    if cli.verbose {
        eprintln!("Connected to {}", socket_path);
    }

    let use_color = format::use_color(cli.color.as_deref().unwrap_or("auto"));

    let started = std::time::Instant::now();
    // Handle Raw command separately (dynamic method name)
    let (method_name, result) = if let Commands::Raw {
        ref method,
        ref params,
    } = cli.command
    {
        let params_val: serde_json::Value = serde_json::from_str(params)
            .map_err(|e| CliError::Protocol(format!("invalid JSON params: {}", e)))?;
        let result = client.call(method, params_val);
        (method.clone(), result)
    } else {
        let (method, params) = command_to_rpc(&cli.command);
        let result = client.call(method, params);
        (method.to_string(), result)
    };

    if cli.verbose {
        if let Some(trace_id) = client.last_trace_id() {
            eprintln!(
                "trace_id={trace_id} round_trip_us={}",
                started.elapsed().as_micros()
            );
        }
    }

    let result = result?;

    // Browser commands default to JSON; everything else defaults to human-readable
    let json_mode = match &cli.command {
        Commands::Browser(_) => !cli.no_json,
        _ => cli.json,
    };

    // Output formatted result
    let output = format::format_response(&method_name, &result, json_mode, use_color);
    if !output.is_empty() {
        // A downstream consumer may finish early (for example, head). Flush
        // explicitly so other output failures reach the normal CLI error path.
        let mut stdout = std::io::stdout().lock();
        if let Err(error) = writeln!(stdout, "{output}").and_then(|()| stdout.flush()) {
            if error.kind() != std::io::ErrorKind::BrokenPipe {
                return Err(CliError::Output(format!("cannot write stdout: {error}")));
            }
        }
    }

    Ok(())
}

/// Map a BrowserCommand variant to its JSON-RPC method and params.
fn browser_command_to_rpc(cmd: &BrowserCommand) -> (&'static str, serde_json::Value) {
    use serde_json::json;
    match cmd {
        BrowserCommand::Open { url, workspace } => {
            let url = browser_address::normalize(url);
            ("browser.open", json!({"url": url, "workspace": workspace}))
        }
        BrowserCommand::List => ("browser.list", json!({})),
        BrowserCommand::Close { surface } => ("browser.close", json!({"surface_ref": surface})),
        BrowserCommand::Snapshot {
            surface,
            interactive,
            compact,
            max_depth,
        } => (
            "browser.snapshot",
            json!({
                "surface_ref": surface,
                "interactive": interactive,
                "compact": compact,
                "max_depth": max_depth
            }),
        ),
        BrowserCommand::Click {
            surface,
            target,
            snapshot_after,
        } => (
            "browser.click",
            json!({
                "surface_ref": surface,
                "target": target,
                "snapshot_after": snapshot_after
            }),
        ),
        BrowserCommand::Fill {
            surface,
            target,
            text,
            snapshot_after,
        } => (
            "browser.fill",
            json!({
                "surface_ref": surface,
                "target": target,
                "text": text,
                "snapshot_after": snapshot_after
            }),
        ),
        BrowserCommand::BrowserType {
            surface,
            selector,
            text,
        } => (
            "browser.type",
            json!({
                "surface_ref": surface,
                "selector": selector,
                "text": text
            }),
        ),
        BrowserCommand::Press { surface, key } => {
            ("browser.press", json!({"surface_ref": surface, "key": key}))
        }
        BrowserCommand::Hover { surface, selector } => (
            "browser.hover",
            json!({"surface_ref": surface, "selector": selector}),
        ),
        BrowserCommand::Scroll {
            surface,
            direction,
            amount,
        } => (
            "browser.scroll",
            json!({
                "surface_ref": surface,
                "direction": direction,
                "amount": amount
            }),
        ),
        BrowserCommand::Select {
            surface,
            selector,
            value,
        } => (
            "browser.select",
            json!({
                "surface_ref": surface,
                "selector": selector,
                "value": value
            }),
        ),
        BrowserCommand::Eval {
            surface,
            expression,
        } => (
            "browser.eval",
            json!({"surface_ref": surface, "script": expression}),
        ),
        BrowserCommand::Wait {
            surface,
            selector,
            text,
            url_contains,
            load_state,
            function,
            timeout_ms,
        } => (
            "browser.wait",
            json!({
                "surface_ref": surface,
                "selector": selector,
                "text": text,
                "url_contains": url_contains,
                "load_state": load_state,
                "function": function,
                "timeout_ms": timeout_ms
            }),
        ),
        BrowserCommand::Goto { surface, url } => (
            "browser.goto",
            json!({"surface_ref": surface, "url": browser_address::normalize(url)}),
        ),
        BrowserCommand::Back { surface } => ("browser.back", json!({"surface_ref": surface})),
        BrowserCommand::Forward { surface } => ("browser.forward", json!({"surface_ref": surface})),
        BrowserCommand::Reload { surface } => ("browser.reload", json!({"surface_ref": surface})),
        BrowserCommand::GetUrl { surface } => ("browser.url", json!({"surface_ref": surface})),
        BrowserCommand::GetTitle { surface } => ("browser.title", json!({"surface_ref": surface})),
        BrowserCommand::GetText { surface, selector } => (
            "browser.gettext",
            json!({"surface_ref": surface, "selector": selector}),
        ),
        BrowserCommand::GetHtml { surface, selector } => (
            "browser.gethtml",
            json!({"surface_ref": surface, "selector": selector}),
        ),
        BrowserCommand::Screenshot { surface } => {
            ("browser.screenshot", json!({"surface_ref": surface}))
        }
        BrowserCommand::StreamEnable => ("browser.stream.enable", json!({})),
        BrowserCommand::StreamDisable => ("browser.stream.disable", json!({})),
    }
}

/// Convert a CLI command to a JSON-RPC method and params.
/// Raw is handled separately in run() — panics if called with Raw.
fn command_to_rpc(cmd: &Commands) -> (&'static str, serde_json::Value) {
    use args::{ResumeCommands, SurfaceCommands};
    use serde_json::{json, Value};
    match cmd {
        Commands::Update => unreachable!("update is handled before socket discovery"),
        Commands::Ping => ("system.ping", json!({})),
        Commands::Identify => ("system.identify", json!({})),
        Commands::Capabilities => ("system.capabilities", json!({})),
        Commands::Diagnostics => ("system.diagnostics", json!({})),
        Commands::ListWorkspaces => ("workspace.list", json!({})),
        Commands::CurrentWorkspace => ("workspace.current", json!({})),

        Commands::Raw { .. } => unreachable!("Raw handled separately"),

        Commands::NewWorkspace { name, cwd } => {
            let mut params = serde_json::Map::new();
            if let Some(name) = name {
                params.insert("name".into(), json!(name));
            }
            if let Some(cwd) = cwd {
                params.insert("working_directory".into(), json!(cwd));
            }
            ("workspace.create", Value::Object(params))
        }
        Commands::SelectWorkspace { id } => ("workspace.select", json!({"id": id})),
        Commands::CloseWorkspace { id } => ("workspace.close", json!({"id": id})),
        Commands::RenameWorkspace { id, name } => {
            ("workspace.rename", json!({"id": id, "name": name}))
        }
        Commands::NextWorkspace => ("workspace.next", json!({})),
        Commands::PrevWorkspace => ("workspace.previous", json!({})),
        Commands::LastWorkspace => ("workspace.last", json!({})),
        Commands::ReorderWorkspace { id, position } => {
            ("workspace.reorder", json!({"id": id, "position": position}))
        }
        Commands::ReorderWorkspaces { order, dry_run } => (
            "workspace.reorder_many",
            json!({"workspace_ids":order,"dry_run":dry_run}),
        ),

        Commands::Surface {
            command: SurfaceCommands::Resume { command },
        } => match command {
            ResumeCommands::Set {
                surface,
                shell,
                kind,
                checkpoint,
                cwd,
                name,
            } => (
                "surface.resume.set",
                json!({"surface_id": surface, "command": shell,
                    "kind": kind, "checkpoint_id": checkpoint, "cwd": cwd, "name": name}),
            ),
            ResumeCommands::Show { surface } => {
                ("surface.resume.show", json!({"surface_id": surface}))
            }
            ResumeCommands::Clear {
                surface,
                checkpoint,
            } => (
                "surface.resume.clear",
                json!({"surface_id": surface, "checkpoint_id": checkpoint}),
            ),
        },
        Commands::Hooks { .. } => unreachable!("hooks are handled before ordinary RPC dispatch"),
        Commands::Restore { .. } => unreachable!("restore executes in the caller terminal"),
        Commands::ListSurfaces => ("surface.list", json!({})),
        Commands::Split { direction, id } => {
            let mut p = serde_json::Map::new();
            p.insert("direction".into(), json!(direction));
            if let Some(ref id) = id {
                p.insert("id".into(), json!(id));
            }
            ("surface.split", Value::Object(p))
        }
        Commands::FocusSurface { id } => ("surface.focus", json!({"id": id})),
        Commands::CloseSurface { id } => ("surface.close", json!({"id": id})),
        Commands::SendText { text, id } => {
            let mut p = serde_json::Map::new();
            p.insert("text".into(), json!(text));
            if let Some(ref id) = id {
                p.insert("id".into(), json!(id));
            }
            ("surface.send_text", Value::Object(p))
        }
        Commands::SendKey { key, id } => {
            let mut p = serde_json::Map::new();
            p.insert("key".into(), json!(key));
            if let Some(ref id) = id {
                p.insert("id".into(), json!(id));
            }
            ("surface.send_key", Value::Object(p))
        }
        Commands::ReadText { id } => {
            let mut p = serde_json::Map::new();
            if let Some(ref id) = id {
                p.insert("id".into(), json!(id));
            }
            ("surface.read_text", Value::Object(p))
        }
        Commands::ReadScrollback { id } => ("surface.read_scrollback", json!({"id": id})),
        Commands::Health { id } => {
            let mut p = serde_json::Map::new();
            if let Some(ref id) = id {
                p.insert("id".into(), json!(id));
            }
            ("surface.health", Value::Object(p))
        }
        Commands::Refresh { id } => {
            let mut p = serde_json::Map::new();
            if let Some(ref id) = id {
                p.insert("id".into(), json!(id));
            }
            ("surface.refresh", Value::Object(p))
        }

        Commands::ListPanes => ("pane.list", json!({})),
        Commands::FocusPane { id } => {
            let mut p = serde_json::Map::new();
            if let Some(ref id) = id {
                p.insert("id".into(), json!(id));
            }
            ("pane.focus", Value::Object(p))
        }
        Commands::LastPane => ("pane.last", json!({})),

        Commands::ListWindows => ("window.list", json!({})),
        Commands::CurrentWindow => ("window.current", json!({})),

        Commands::Layout => ("debug.layout", json!({})),
        Commands::Type { text } => ("debug.type", json!({"text": text})),

        Commands::SetStatus {
            key,
            value,
            icon,
            color,
            priority,
            format,
            url,
            workspace,
        } => (
            "sidebar.set_status",
            json!({"key":key,"value":value,"icon":icon,"color":color,"priority":priority,"format":format,"url":url,"workspace_id":workspace}),
        ),
        Commands::ClearStatus { key, workspace } => (
            "sidebar.clear_status",
            json!({"key":key,"workspace_id":workspace}),
        ),
        Commands::ReportMetaBlock {
            key,
            markdown,
            priority,
            workspace,
        } => (
            "sidebar.report_meta_block",
            json!({"key":key,"markdown":markdown,"priority":priority,"workspace_id":workspace}),
        ),
        Commands::ClearMetaBlock { key, workspace } => (
            "sidebar.clear_meta_block",
            json!({"key":key,"workspace_id":workspace}),
        ),
        Commands::ListMetaBlocks { workspace } => {
            ("sidebar.metadata", json!({"workspace_id":workspace}))
        }
        Commands::ListStatus { workspace } => {
            ("sidebar.metadata", json!({"workspace_id":workspace}))
        }
        Commands::SetProgress {
            value,
            label,
            workspace,
        } => (
            "sidebar.set_progress",
            json!({"value":value,"label":label,"workspace_id":workspace}),
        ),
        Commands::ClearProgress { workspace } => {
            ("sidebar.clear_progress", json!({"workspace_id":workspace}))
        }
        Commands::ListNotifications => ("notification.list", json!({})),
        Commands::Notify {
            title,
            subtitle,
            body,
            workspace,
            surface,
        } => {
            if workspace.is_some() || surface.is_some() {
                (
                    "notification.create",
                    json!({"title":title,"subtitle":subtitle,"body":body,
                    "workspace_id":workspace,"surface_id":surface}),
                )
            } else {
                let mut params = notification_caller_params();
                params["title"] = json!(title);
                params["subtitle"] = json!(subtitle);
                params["body"] = json!(body);
                ("notification.create_for_caller", params)
            }
        }
        Commands::Notifications { command } => match command {
            args::NotificationCommands::List => ("notification.list", json!({})),
            args::NotificationCommands::Clear {
                caller,
                workspace,
                surface,
            } => {
                let params = if *caller {
                    let mut params = notification_caller_params();
                    params["caller"] = json!(true);
                    params
                } else {
                    json!({"workspace_id":workspace,"surface_id":surface})
                };
                ("notification.clear", params)
            }
            args::NotificationCommands::MarkRead {
                id,
                workspace,
                surface,
                all,
            } => (
                "notification.mark_read",
                json!({"id":id,"workspace_id":workspace,"surface_id":surface,"all":all}),
            ),
            args::NotificationCommands::Dismiss { id, all_read } => {
                ("notification.dismiss", json!({"id":id,"all_read":all_read}))
            }
            args::NotificationCommands::Open { id } => ("notification.open", json!({"id":id})),
            args::NotificationCommands::JumpToUnread => ("notification.jump_to_unread", json!({})),
        },
        Commands::ClearNotification { id } => ("notification.clear", json!({"id": id})),

        Commands::Browser(cmd) => browser_command_to_rpc(cmd),
    }
}

/// Preserve ambient caller identity separately from explicit command flags; pipes provide no TTY claim.
fn notification_caller_params() -> serde_json::Value {
    serde_json::json!({
        "preferred_workspace_id": std::env::var("CMUX_WORKSPACE_ID").ok().filter(|value| !value.is_empty()),
        "preferred_surface_id": std::env::var("CMUX_SURFACE_ID").ok().filter(|value| !value.is_empty()),
        "preferred_workspace_is_explicit": false,
        "caller_tty": cmux_platform::terminal::caller_tty(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    /// Preserve explicit workspace naming and directory arguments in the outgoing RPC parameters.
    #[test]
    fn new_workspace_cli_sends_name_and_directory() {
        let cli = Cli::try_parse_from([
            "cmux",
            "new-workspace",
            "--name",
            "Project Alpha",
            "--cwd",
            "/tmp/project-alpha",
        ])
        .expect("workspace arguments should parse");
        let (method, params) = command_to_rpc(&cli.command);
        assert_eq!(method, "workspace.create");
        assert_eq!(params["name"], "Project Alpha");
        assert_eq!(params["working_directory"], "/tmp/project-alpha");
    }
}
