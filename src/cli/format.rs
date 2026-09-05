//! Human-readable output formatters for cmux CLI responses.
//!
//! Handles color support (D-07), list formatting with active markers (D-08),
//! and mutation success messages (D-09).

use serde_json::Value;
use std::io::IsTerminal;

/// Determine whether to use color output based on the --color flag value.
pub fn use_color(color_flag: &str) -> bool {
    match color_flag {
        "always" => true,
        "never" => false,
        _ => std::io::stdout().is_terminal(),
    }
}

/// Highlight active or successful output only when terminal color is enabled.
fn green(s: &str, color: bool) -> String {
    if color {
        format!("\x1b[1;32m{}\x1b[0m", s)
    } else {
        s.to_string()
    }
}

/// De-emphasize supplementary output while preserving plain-text mode.
fn dim(s: &str, color: bool) -> String {
    if color {
        format!("\x1b[2m{}\x1b[0m", s)
    } else {
        s.to_string()
    }
}

/// Format a workspace list response with active marker.
pub fn format_workspace_list(result: &Value, color: bool) -> String {
    let workspaces = match result.get("workspaces").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => return format_fallback(result),
    };
    if workspaces.is_empty() {
        return "No workspaces".to_string();
    }
    let mut lines = Vec::new();
    for (i, ws) in workspaces.iter().enumerate() {
        let selected = ws
            .get("selected")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let title = ws
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("untitled");
        let pane_count = match ws.get("pane_count").and_then(Value::as_u64) {
            Some(1) => "1 pane".to_owned(),
            Some(count) => format!("{count} panes"),
            None => "pane count unavailable".to_owned(),
        };
        let marker = if selected { "*" } else { " " };
        let line = format!("{} {}: {} ({})", marker, i + 1, title, pane_count);
        if selected && color {
            lines.push(green(&line, true));
        } else {
            lines.push(line);
        }
    }
    lines.join("\n")
}

/// Format terminal/browser surface identities with the active-tab marker.
pub fn format_surface_list(result: &Value, color: bool) -> String {
    format_identity_list(result, "surfaces", "No surfaces", color)
}

/// Format session-local pane identities with the focused-pane marker.
pub fn format_pane_list(result: &Value, color: bool) -> String {
    format_identity_list(result, "panes", "No panes", color)
}

/// Render ordered identity records using current protocol fields and their legacy aliases.
/// Preserve full IDs so output can be passed back to focus/close commands without guessing.
fn format_identity_list(result: &Value, field: &str, empty: &str, color: bool) -> String {
    let Some(records) = result.get(field).and_then(Value::as_array) else {
        return format_fallback(result);
    };
    if records.is_empty() {
        return empty.to_owned();
    }
    records
        .iter()
        .map(|record| {
            let focused = record
                .get("active")
                .and_then(Value::as_bool)
                .or_else(|| record.get("focused").and_then(Value::as_bool))
                .unwrap_or(false);
            let id = record
                .get("id")
                .and_then(Value::as_str)
                .or_else(|| record.get("uuid").and_then(Value::as_str))
                .unwrap_or("unknown");
            let title = record.get("title").and_then(Value::as_str).unwrap_or("");
            let marker = if focused { "*" } else { " " };
            let line = if title.is_empty() {
                format!("{marker} {id}")
            } else {
                format!("{marker} {id} ({title})")
            };
            green(&line, focused && color)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Format a window list response.
pub fn format_window_list(result: &Value, color: bool) -> String {
    let windows = match result.get("windows").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => return format_fallback(result),
    };
    if windows.is_empty() {
        return "No windows".to_string();
    }
    let mut lines = Vec::new();
    for (i, win) in windows.iter().enumerate() {
        let focused = win
            .get("focused")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let title = win.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let marker = if focused { "*" } else { " " };
        let line = format!("{} {}: {}", marker, i + 1, title);
        if focused && color {
            lines.push(green(&line, true));
        } else {
            lines.push(line);
        }
    }
    lines.join("\n")
}

/// Format identify response.
fn format_identify(result: &Value) -> String {
    let version = result
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let platform = result
        .get("platform")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let pid = result.get("pid").and_then(|v| v.as_u64());
    match pid {
        Some(p) => format!("cmux {} ({}) pid {}", version, platform, p),
        None => format!("cmux {} ({})", version, platform),
    }
}

/// Format capabilities response.
fn format_capabilities(result: &Value, color: bool) -> String {
    let methods = match result.get("methods").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => return format_fallback(result),
    };
    let mut lines = vec![format!("{} methods available:", methods.len())];
    for m in methods {
        let name = m.as_str().unwrap_or("?");
        lines.push(format!("  {}", dim(name, color)));
    }
    lines.join("\n")
}

/// Format notification list response.
fn format_notification_list(result: &Value, color: bool) -> String {
    let notifications = match result.get("notifications").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => return format_fallback(result),
    };
    if notifications.is_empty() {
        return "No notifications".to_string();
    }
    let mut lines = Vec::new();
    for n in notifications {
        let id = n.get("id").and_then(|v| v.as_str()).unwrap_or("unknown");
        let id_short = &id[..id.len().min(8)];
        let attention = n
            .get("attention")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let marker = if attention { "!" } else { " " };
        let line = format!("{} {}", marker, id_short);
        if attention && color {
            lines.push(green(&line, true));
        } else {
            lines.push(line);
        }
    }
    lines.join("\n")
}

/// Format a mutation command result with a success message.
pub fn format_mutation(command_name: &str, result: &Value) -> String {
    let id = result.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let title = result
        .get("title")
        .or_else(|| result.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    match command_name {
        "workspace.create" => {
            if title.is_empty() {
                format!("Created workspace: {}", id)
            } else {
                format!("Created workspace: {} ({})", title, id)
            }
        }
        "workspace.close" => format!("Closed workspace: {}", id),
        "workspace.rename" => {
            let name = result.get("name").and_then(|v| v.as_str()).unwrap_or(title);
            format!("Renamed workspace {} to: {}", id, name)
        }
        "surface.split" => format!("Split created: {}", id),
        "surface.close" => format!("Closed surface: {}", id),
        _ => String::new(),
    }
}

/// Format a command response for human-readable output.
///
/// If `json_mode` is true, returns raw JSON (D-06).
/// Otherwise, picks the appropriate formatter based on the method name.
pub fn format_response(method: &str, result: &Value, json_mode: bool, color: bool) -> String {
    if json_mode {
        return serde_json::to_string_pretty(result).unwrap_or_default();
    }

    match method {
        "workspace.list" => format_workspace_list(result, color),
        "workspace.current" => {
            let title = result
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let id = result.get("id").and_then(|v| v.as_str()).unwrap_or("");
            format!("{} ({})", title, id)
        }
        "surface.list" => format_surface_list(result, color),
        "pane.list" => format_pane_list(result, color),
        "window.list" => format_window_list(result, color),
        "window.current" => {
            let title = result
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            title.to_string()
        }
        "system.ping" => result
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("pong")
            .to_string(),
        "system.identify" => format_identify(result),
        "system.capabilities" => format_capabilities(result, color),
        "notification.list" => format_notification_list(result, color),
        "debug.layout" => serde_json::to_string_pretty(result).unwrap_or_default(),

        // Mutation commands: show success message
        "workspace.create" | "workspace.close" | "workspace.rename" | "surface.split"
        | "surface.close" => {
            let msg = format_mutation(method, result);
            if msg.is_empty() {
                format_fallback(result)
            } else {
                msg
            }
        }

        // Browser list: human-readable table
        "browser.list" => format_browser_list(result, color),

        // Default: pretty-print JSON for uncommon commands
        _ => format_fallback(result),
    }
}

/// Format a browser surface list response.
pub fn format_browser_list(result: &Value, _color: bool) -> String {
    let surfaces = match result.get("surfaces").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => return format_fallback(result),
    };
    if surfaces.is_empty() {
        return "No browser surfaces".to_string();
    }
    let mut lines = Vec::new();
    lines.push(format!(
        "{:<12} {:<38} {:<50} {}",
        "REF", "UUID", "URL", "STATUS"
    ));
    for s in surfaces {
        let ref_str = s.get("ref").and_then(|v| v.as_str()).unwrap_or("-");
        let uuid = s.get("uuid").and_then(|v| v.as_str()).unwrap_or("-");
        let url = s.get("url").and_then(|v| v.as_str()).unwrap_or("-");
        let status = s
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        lines.push(format!(
            "{:<12} {:<38} {:<50} {}",
            ref_str, uuid, url, status
        ));
    }
    lines.join("\n")
}

/// Fallback: pretty-print JSON.
fn format_fallback(result: &Value) -> String {
    serde_json::to_string_pretty(result).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Missing layout metadata must not be rendered as a known zero-pane workspace.
    #[test]
    fn workspace_count_availability() {
        let result = json!({"workspaces": [
            {"title": "known", "pane_count": 2, "selected": true},
            {"title": "unknown", "pane_count": null}
        ]});
        assert_eq!(
            format_workspace_list(&result, false),
            "* 1: known (2 panes)\n  2: unknown (pane count unavailable)"
        );
    }

    /// Current surface records must produce reusable UUIDs and truthful selection markers.
    #[test]
    fn surface_protocol_identity_output() {
        let id = "20000000-0000-4000-8000-000000000002";
        let result = json!({"surfaces": [{"uuid": id, "active": true}]});
        assert_eq!(
            format_response("surface.list", &result, false, false),
            format!("* {id}")
        );
        assert_eq!(
            format_surface_list(&result, true),
            format!("\x1b[1;32m* {id}\x1b[0m")
        );
        assert_eq!(
            serde_json::from_str::<Value>(&format_response("surface.list", &result, true, true))
                .unwrap(),
            result
        );
    }

    /// Pane references and non-ASCII legacy IDs retain their full identities without byte slicing.
    #[test]
    fn pane_protocol_and_legacy_fields() {
        let result = json!({"panes": [
            {"id": "pane:100001", "active": false, "focused": true},
            {"id": "ééééééééé", "focused": true, "title": "terminal"}
        ]});
        assert_eq!(
            format_pane_list(&result, false),
            "  pane:100001\n* ééééééééé (terminal)"
        );
        assert_eq!(
            format_surface_list(&json!({"surfaces": []}), false),
            "No surfaces"
        );
        assert_eq!(format_pane_list(&json!({"panes": []}), false), "No panes");
    }
}
