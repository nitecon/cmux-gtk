//! cmux CLI — clap-based argument parser and command dispatch.
//!
//! This module is entirely independent of GTK4 and the GUI app.
//! It connects to the cmux-app via Unix socket JSON-RPC.

#[path = "../bounded_json.rs"]
mod bounded_json;
pub mod discovery;
pub mod format;
pub mod socket_client;
#[path = "../updater.rs"]
#[allow(dead_code)]
mod updater;

pub use socket_client::CliError;

mod args;
pub use args::{BrowserCommand, Cli, Commands};
use std::time::Duration;

/// Run the CLI with the parsed arguments.
pub fn run(cli: Cli) -> Result<(), CliError> {
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

    // Use longer timeout for browser wait commands
    let timeout = match &cli.command {
        Commands::Browser(BrowserCommand::Wait { timeout_ms, .. }) => {
            Duration::from_millis(timeout_ms + 5000)
        }
        _ => Duration::from_secs(5),
    };

    let mut client = socket_client::SocketClient::connect(&socket_path, timeout)?;

    if cli.verbose {
        eprintln!("Connected to {}", socket_path);
    }

    let use_color = format::use_color(&cli.color);

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
        println!("{}", output);
    }

    Ok(())
}

/// Map a BrowserCommand variant to its JSON-RPC method and params.
fn browser_command_to_rpc(cmd: &BrowserCommand) -> (&'static str, serde_json::Value) {
    use serde_json::json;
    match cmd {
        BrowserCommand::Open { url, workspace } => {
            let url = if !url.contains("://") {
                format!("https://{}", url)
            } else {
                url.clone()
            };
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
        BrowserCommand::Goto { surface, url } => {
            ("browser.goto", json!({"surface_ref": surface, "url": url}))
        }
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

        Commands::ListNotifications => ("notification.list", json!({})),
        Commands::ClearNotification { id } => ("notification.clear", json!({"id": id})),

        Commands::Browser(cmd) => browser_command_to_rpc(cmd),
    }
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
