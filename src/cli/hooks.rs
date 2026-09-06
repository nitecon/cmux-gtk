//! Native agent hook installation and bounded session payload ingestion.
use super::{
    args::{ClaudeHookEvent, CodexHookEvent, JsonHookEvent},
    socket_client::SocketClient,
    CliError,
};
use serde_json::{json, Value};
use std::io::Read;
use std::path::{Path, PathBuf};

const MARKER: &str = "# cmux-gtk-claude-session-v1";

struct JsonProvider {
    name: &'static str,
    display: &'static str,
    binary: &'static str,
    directory: &'static str,
    directory_env: Option<&'static str>,
    environment: &'static [&'static str],
    file: &'static str,
    resume_prefix: &'static [&'static str],
    start_event: &'static str,
    stop_event: &'static str,
    notification_event: Option<&'static str>,
    end_event: Option<&'static str>,
}

struct ResumeCommand<'a> {
    kind: &'a str,
    executable: &'a str,
    prefix: &'a [&'a str],
    environment: &'a [&'a str],
}

fn json_provider(name: &str) -> Option<JsonProvider> {
    Some(match name {
        "grok" => JsonProvider {
            name: "grok",
            display: "Grok",
            binary: "grok",
            directory: ".grok/hooks",
            directory_env: Some("GROK_HOME"),
            environment: &["GROK_HOME"],
            file: "cmux-session.json",
            resume_prefix: &["-r"],
            start_event: "SessionStart",
            stop_event: "Stop",
            notification_event: Some("Notification"),
            // Grok emits SessionEnd at a turn boundary, so it must not clear durable identity.
            end_event: None,
        },
        "gemini" => JsonProvider {
            name: "gemini",
            display: "Gemini",
            binary: "gemini",
            directory: ".gemini",
            directory_env: None,
            environment: &[],
            file: "settings.json",
            resume_prefix: &["--resume"],
            start_event: "SessionStart",
            stop_event: "AfterAgent",
            notification_event: None,
            end_event: Some("SessionEnd"),
        },
        "copilot" => JsonProvider {
            name: "copilot",
            display: "Copilot",
            binary: "copilot",
            directory: ".copilot",
            directory_env: Some("COPILOT_HOME"),
            environment: &["COPILOT_HOME"],
            file: "config.json",
            resume_prefix: &["--resume"],
            start_event: "SessionStart",
            stop_event: "Stop",
            notification_event: Some("Notification"),
            end_event: Some("SessionEnd"),
        },
        "codebuddy" => JsonProvider {
            name: "codebuddy",
            display: "CodeBuddy",
            binary: "codebuddy",
            directory: ".codebuddy",
            directory_env: Some("CODEBUDDY_CONFIG_DIR"),
            environment: &["CODEBUDDY_CONFIG_DIR"],
            file: "settings.json",
            resume_prefix: &["--resume"],
            start_event: "SessionStart",
            stop_event: "Stop",
            notification_event: Some("Notification"),
            end_event: Some("SessionEnd"),
        },
        "factory" => JsonProvider {
            name: "factory",
            display: "Factory",
            binary: "droid",
            directory: ".factory",
            directory_env: None,
            environment: &[],
            file: "settings.json",
            resume_prefix: &["--resume"],
            start_event: "SessionStart",
            stop_event: "Stop",
            notification_event: Some("Notification"),
            end_event: Some("SessionEnd"),
        },
        "qoder" => JsonProvider {
            name: "qoder",
            display: "Qoder",
            binary: "qodercli",
            directory: ".qoder",
            directory_env: Some("QODER_CONFIG_DIR"),
            environment: &["QODER_CONFIG_DIR"],
            file: "settings.json",
            resume_prefix: &["--resume"],
            start_event: "SessionStart",
            stop_event: "Stop",
            notification_event: None,
            end_event: Some("SessionEnd"),
        },
        _ => return None,
    })
}

/// Quote one literal argument for a POSIX hook command, including embedded apostrophes.
fn shell_argument(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Merge only cmux-owned handlers into supported session events, retaining user settings and hooks.
fn merge_claude_hooks(settings: &mut Value, binary: &Path) -> Result<(), CliError> {
    let object = settings
        .as_object_mut()
        .ok_or_else(|| CliError::Command("agent settings must be an object".into()))?;
    let hooks = object
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| CliError::Command("agent hooks must be an object".into()))?;
    for (event, command) in [
        ("SessionStart", "session-start"),
        ("SessionEnd", "session-end"),
        ("Stop", "stop"),
        ("Notification", "notification"),
    ] {
        let entries = hooks
            .entry(event)
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or_else(|| CliError::Command(format!("{event} hooks must be an array")))?;
        entries.retain_mut(|entry| {
            if let Some(handlers) = entry.get_mut("hooks").and_then(Value::as_array_mut) {
                let was_empty = handlers.is_empty();
                handlers.retain(|handler| {
                    !handler
                        .get("command")
                        .and_then(Value::as_str)
                        .is_some_and(|text| text.ends_with(MARKER))
                });
                return was_empty || !handlers.is_empty();
            }
            true
        });
        entries.push(json!({"hooks": [{"type": "command", "command": format!("{} hooks claude {} {}", shell_argument(&binary.to_string_lossy()), command, MARKER), "timeout": 10}]}));
    }
    Ok(())
}

/// Install one provider or every detected provider without contacting GTK.
pub fn setup(agent: Option<&str>) -> Result<(), CliError> {
    match agent {
        Some("claude") => setup_claude(true),
        Some("codex") => setup_codex(true),
        Some(name) if json_provider(name).is_some() => setup_json_provider(name, true),
        Some(other) => Err(CliError::Command(format!(
            "unsupported hook provider: {other}"
        ))),
        None => {
            setup_claude(false)?;
            setup_codex(false)?;
            for provider in ["grok", "gemini", "copilot", "codebuddy", "factory", "qoder"] {
                setup_json_provider(provider, false)?;
            }
            Ok(())
        }
    }
}

/// Install detected Claude session hooks without contacting GTK or modifying unsupported provider settings.
/// Read at most one MiB, merge idempotently, and atomically replace/sync the caller-selected config.
/// Refuse symlinks so an existing configuration link is never silently replaced.
fn setup_claude(explicit: bool) -> Result<(), CliError> {
    if cmux_platform::paths::find_command_on_path("claude").is_none() {
        if explicit {
            return Err(CliError::Command(
                "install the Claude CLI before installing its hooks".into(),
            ));
        }
        println!("Skipped Claude: executable not found on PATH");
        return Ok(());
    }
    let directory = std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".claude")))
        .ok_or_else(|| CliError::Command("cannot resolve Claude configuration directory".into()))?;
    cmux_platform::filesystem::create_private_directory(&directory)
        .map_err(|error| CliError::Command(format!("create Claude config directory: {error}")))?;
    let path = directory.join("settings.json");
    if path.is_symlink() {
        return Err(CliError::Command(
            "Claude settings is a symlink; configure hooks in its target explicitly".into(),
        ));
    }
    let mut settings = match cmux_platform::filesystem::read_text_bounded(&path, 1024 * 1024) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|_| CliError::Command("Claude settings contains invalid JSON".into()))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(error) => return Err(CliError::Command(format!("read Claude settings: {error}"))),
    };
    let binary = std::env::current_exe().map_err(|error| CliError::Command(error.to_string()))?;
    merge_claude_hooks(&mut settings, &binary)?;
    let encoded = serde_json::to_vec_pretty(&settings)
        .map_err(|error| CliError::Command(error.to_string()))?;
    if encoded.len() > 1024 * 1024 {
        return Err(CliError::Command(
            "merged Claude settings exceeds one MiB".into(),
        ));
    }
    cmux_platform::filesystem::atomic_write(&path, &encoded)
        .and_then(|_| cmux_platform::filesystem::sync_file_and_parent(&path))
        .map_err(|error| CliError::Command(format!("save Claude hooks: {error}")))?;
    println!(
        "Installed Claude session and notification hooks in {}",
        path.display()
    );
    Ok(())
}

/// Merge Codex's documented nested lifecycle-hook schema into its user hook file.
fn setup_codex(explicit: bool) -> Result<(), CliError> {
    if cmux_platform::paths::find_command_on_path("codex").is_none() {
        if explicit {
            return Err(CliError::Command(
                "install the Codex CLI before installing its hooks".into(),
            ));
        }
        println!("Skipped Codex: executable not found on PATH");
        return Ok(());
    }
    let directory = std::env::var_os("CODEX_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".codex")))
        .ok_or_else(|| CliError::Command("cannot resolve Codex configuration directory".into()))?;
    cmux_platform::filesystem::create_private_directory(&directory)
        .map_err(|error| CliError::Command(format!("create Codex config directory: {error}")))?;
    let path = directory.join("hooks.json");
    if path.is_symlink() {
        return Err(CliError::Command(
            "Codex hooks file is a symlink; configure hooks in its target explicitly".into(),
        ));
    }
    let mut settings = match cmux_platform::filesystem::read_text_bounded(&path, 1024 * 1024) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|_| CliError::Command("Codex hooks contains invalid JSON".into()))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(error) => return Err(CliError::Command(format!("read Codex hooks: {error}"))),
    };
    let binary = std::env::current_exe().map_err(|error| CliError::Command(error.to_string()))?;
    merge_codex_hooks(&mut settings, &binary)?;
    let encoded = serde_json::to_vec_pretty(&settings)
        .map_err(|error| CliError::Command(error.to_string()))?;
    if encoded.len() > 1024 * 1024 {
        return Err(CliError::Command(
            "merged Codex hooks exceeds one MiB".into(),
        ));
    }
    cmux_platform::filesystem::atomic_write(&path, &encoded)
        .and_then(|_| cmux_platform::filesystem::sync_file_and_parent(&path))
        .map_err(|error| CliError::Command(format!("save Codex hooks: {error}")))?;
    println!(
        "Installed Codex session and notification hooks in {}",
        path.display()
    );
    Ok(())
}

fn merge_codex_hooks(settings: &mut Value, binary: &Path) -> Result<(), CliError> {
    let object = settings
        .as_object_mut()
        .ok_or_else(|| CliError::Command("Codex hooks must be an object".into()))?;
    let hooks = object
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| CliError::Command("Codex hooks.hooks must be an object".into()))?;
    for (event, command) in [
        ("SessionStart", "session-start"),
        ("SessionEnd", "session-end"),
        ("Stop", "stop"),
    ] {
        let entries = hooks
            .entry(event)
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or_else(|| CliError::Command(format!("{event} hooks must be an array")))?;
        entries.retain_mut(|entry| {
            if let Some(handlers) = entry.get_mut("hooks").and_then(Value::as_array_mut) {
                handlers.retain(|handler| {
                    !handler
                        .get("command")
                        .and_then(Value::as_str)
                        .is_some_and(|text| text.contains(" hooks codex "))
                });
                return !handlers.is_empty();
            }
            true
        });
        entries.push(json!({"hooks": [{"type": "command", "command": format!("{} hooks codex {command}", shell_argument(&binary.to_string_lossy())), "timeout": 5}]}));
    }
    Ok(())
}

fn setup_json_provider(name: &str, explicit: bool) -> Result<(), CliError> {
    let provider = json_provider(name)
        .ok_or_else(|| CliError::Command(format!("unsupported hook provider: {name}")))?;
    if cmux_platform::paths::find_command_on_path(provider.binary).is_none() {
        if explicit {
            return Err(CliError::Command(format!(
                "install the {} CLI before installing its hooks",
                provider.display
            )));
        }
        println!("Skipped {}: executable not found on PATH", provider.display);
        return Ok(());
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| CliError::Command("cannot resolve home directory".into()))?;
    let mut directory = provider
        .directory_env
        .and_then(std::env::var_os)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(provider.directory));
    if provider.name == "grok" && std::env::var_os("GROK_HOME").is_some() {
        directory.push("hooks");
    }
    cmux_platform::filesystem::create_private_directory(&directory).map_err(|error| {
        CliError::Command(format!(
            "create {} config directory: {error}",
            provider.display
        ))
    })?;
    let path = directory.join(provider.file);
    if path.is_symlink() {
        return Err(CliError::Command(format!(
            "{} hook configuration is a symlink; configure its target explicitly",
            provider.display
        )));
    }
    let mut settings = match cmux_platform::filesystem::read_text_bounded(&path, 1024 * 1024) {
        Ok(text) => serde_json::from_str(&text).map_err(|_| {
            CliError::Command(format!("{} hooks contains invalid JSON", provider.display))
        })?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(error) => {
            return Err(CliError::Command(format!(
                "read {} hooks: {error}",
                provider.display
            )))
        }
    };
    let binary = std::env::current_exe().map_err(|error| CliError::Command(error.to_string()))?;
    merge_json_provider_hooks(&mut settings, &binary, &provider)?;
    let encoded = serde_json::to_vec_pretty(&settings)
        .map_err(|error| CliError::Command(error.to_string()))?;
    if encoded.len() > 1024 * 1024 {
        return Err(CliError::Command(format!(
            "merged {} hooks exceeds one MiB",
            provider.display
        )));
    }
    cmux_platform::filesystem::atomic_write(&path, &encoded)
        .and_then(|_| cmux_platform::filesystem::sync_file_and_parent(&path))
        .map_err(|error| CliError::Command(format!("save {} hooks: {error}", provider.display)))?;
    println!(
        "Installed {} session and notification hooks in {}",
        provider.display,
        path.display()
    );
    Ok(())
}

fn merge_json_provider_hooks(
    settings: &mut Value,
    binary: &Path,
    provider: &JsonProvider,
) -> Result<(), CliError> {
    let object = settings.as_object_mut().ok_or_else(|| {
        CliError::Command(format!("{} settings must be an object", provider.display))
    })?;
    let hooks = object
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| {
            CliError::Command(format!("{} hooks must be an object", provider.display))
        })?;
    let mut events = vec![
        (provider.start_event, "session-start"),
        (provider.stop_event, "stop"),
    ];
    if let Some(event) = provider.notification_event {
        events.push((event, "notification"));
    }
    if let Some(event) = provider.end_event {
        events.push((event, "session-end"));
    }
    let marker = format!(" hooks {} ", provider.name);
    for (event, command) in events {
        let entries = hooks
            .entry(event)
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or_else(|| CliError::Command(format!("{event} hooks must be an array")))?;
        entries.retain_mut(|entry| {
            if let Some(handlers) = entry.get_mut("hooks").and_then(Value::as_array_mut) {
                handlers.retain(|handler| {
                    !handler
                        .get("command")
                        .and_then(Value::as_str)
                        .is_some_and(|text| text.contains(&marker))
                });
                return !handlers.is_empty();
            }
            true
        });
        entries.push(json!({"hooks": [{"type": "command", "command": format!("{} hooks {} {command}", shell_argument(&binary.to_string_lossy()), provider.name), "timeout": 10}]}));
    }
    Ok(())
}

/// Consume a bounded native payload and persist its exact session identity without granting automatic trust.
/// Session-end clears only the matching checkpoint; commands, prompts and payload bodies are not logged.
pub fn claude_event(client: &mut SocketClient, event: ClaudeHookEvent) -> Result<(), CliError> {
    let expected = match event {
        ClaudeHookEvent::SessionStart => "SessionStart",
        ClaudeHookEvent::SessionEnd => "SessionEnd",
        ClaudeHookEvent::Stop => "Stop",
        ClaudeHookEvent::Notification => "Notification",
    };
    let (payload, id, surface) = read_hook_payload(expected)?;
    match event {
        ClaudeHookEvent::SessionStart => {
            set_agent_resume(
                client,
                &payload,
                &surface,
                &id,
                ResumeCommand {
                    kind: "claude",
                    executable: "claude",
                    prefix: &["--resume"],
                    environment: &["CLAUDE_CONFIG_DIR", "CLAUDE_SECURESTORAGE_CONFIG_DIR"],
                },
            )?;
        }
        ClaudeHookEvent::SessionEnd => clear_agent_resume(client, &surface, &id)?,
        ClaudeHookEvent::Stop | ClaudeHookEvent::Notification => {
            let (title_key, title_default, body_key, body_default) =
                if matches!(event, ClaudeHookEvent::Stop) {
                    (
                        "title",
                        "Claude response ready",
                        "last_assistant_message",
                        "Claude has finished responding.",
                    )
                } else {
                    (
                        "title",
                        "Claude needs attention",
                        "message",
                        "Claude needs your attention.",
                    )
                };
            let title = notification_text(&payload, title_key, title_default, 512)?;
            let body = notification_text(&payload, body_key, body_default, 8192)?;
            let subtitle = if matches!(event, ClaudeHookEvent::Notification) {
                notification_text(&payload, "notification_type", "", 1024)?
            } else {
                String::new()
            };
            create_agent_notification(client, &surface, title, subtitle, body)?;
        }
    }
    Ok(())
}

/// Consume Codex's documented lifecycle payload and bind its native session identity.
pub fn codex_event(client: &mut SocketClient, event: CodexHookEvent) -> Result<(), CliError> {
    let expected = match event {
        CodexHookEvent::SessionStart => "SessionStart",
        CodexHookEvent::SessionEnd => "SessionEnd",
        CodexHookEvent::Stop => "Stop",
    };
    let (payload, id, surface) = read_hook_payload(expected)?;
    match event {
        CodexHookEvent::SessionStart => {
            set_agent_resume(
                client,
                &payload,
                &surface,
                &id,
                ResumeCommand {
                    kind: "codex",
                    executable: "codex",
                    prefix: &["resume"],
                    environment: &["CODEX_HOME"],
                },
            )?;
        }
        CodexHookEvent::SessionEnd => clear_agent_resume(client, &surface, &id)?,
        CodexHookEvent::Stop => {
            let title = notification_text(&payload, "title", "Codex response ready", 512)?;
            let body = ["last_assistant_message", "message", "summary"]
                .iter()
                .find_map(|key| payload.get(*key).and_then(Value::as_str))
                .unwrap_or("Codex has finished responding.");
            let body = bounded_notification_text(body, 8192)?;
            create_agent_notification(client, &surface, title, String::new(), body)?;
        }
    }
    Ok(())
}

/// Handle the common nested JSON lifecycle contract used by several terminal agents.
pub fn json_provider_event(
    client: &mut SocketClient,
    provider_name: &str,
    event: JsonHookEvent,
) -> Result<(), CliError> {
    let provider = json_provider(provider_name)
        .ok_or_else(|| CliError::Command(format!("unsupported hook provider: {provider_name}")))?;
    let expected = match event {
        JsonHookEvent::SessionStart => provider.start_event,
        JsonHookEvent::SessionEnd => provider.end_event.ok_or_else(|| {
            CliError::Command("provider has no destructive session-end hook".into())
        })?,
        JsonHookEvent::Stop => provider.stop_event,
        JsonHookEvent::Notification => provider.notification_event.ok_or_else(|| {
            CliError::Command("provider has no separate notification hook".into())
        })?,
    };
    let (payload, id, surface) = read_hook_payload(expected)?;
    match event {
        JsonHookEvent::SessionStart => set_agent_resume(
            client,
            &payload,
            &surface,
            &id,
            ResumeCommand {
                kind: provider.name,
                executable: provider.binary,
                prefix: provider.resume_prefix,
                environment: provider.environment,
            },
        )?,
        JsonHookEvent::SessionEnd => clear_agent_resume(client, &surface, &id)?,
        JsonHookEvent::Stop | JsonHookEvent::Notification => {
            let attention = matches!(event, JsonHookEvent::Notification);
            let default_title = if attention {
                format!("{} needs attention", provider.display)
            } else {
                format!("{} response ready", provider.display)
            };
            let title = hook_string(&payload, &["title"])
                .map(|text| bounded_notification_text(text, 512))
                .transpose()?
                .unwrap_or(default_title);
            let body = hook_string(
                &payload,
                &[
                    "last_assistant_message",
                    "message",
                    "summary",
                    "body",
                    "text",
                ],
            )
            .unwrap_or(if attention {
                "The agent needs your attention."
            } else {
                "The agent has finished responding."
            });
            let subtitle = hook_string(&payload, &["notification_type", "notificationType"])
                .map(|text| bounded_notification_text(text, 1024))
                .transpose()?
                .unwrap_or_default();
            create_agent_notification(
                client,
                &surface,
                title,
                subtitle,
                bounded_notification_text(body, 8192)?,
            )?;
        }
    }
    Ok(())
}

fn read_hook_payload(expected: &str) -> Result<(Value, String, String), CliError> {
    let mut input = Vec::new();
    std::io::stdin()
        .take(65537)
        .read_to_end(&mut input)
        .map_err(|error| CliError::Command(error.to_string()))?;
    if input.len() > 65536 {
        return Err(CliError::Command("hook payload exceeds 65536 bytes".into()));
    }
    let payload: Value = serde_json::from_slice(&input)
        .map_err(|_| CliError::Command("invalid hook JSON".into()))?;
    let event_name = hook_string(
        &payload,
        &[
            "hook_event_name",
            "hookEventName",
            "event_name",
            "eventName",
            "event",
        ],
    );
    if event_name != Some(expected) {
        return Err(CliError::Command(
            "hook event does not match its payload".into(),
        ));
    }
    let id = hook_string(
        &payload,
        &[
            "session_id",
            "sessionId",
            "conversation_id",
            "conversationId",
        ],
    )
    .or_else(|| payload.get("session")?.get("id")?.as_str())
    .filter(|id| {
        !id.is_empty()
            && id.len() <= 1024
            && !id.starts_with('-')
            && !id.chars().any(char::is_control)
    })
    .ok_or_else(|| CliError::Command("hook has no valid native session_id".into()))?
    .to_owned();
    let surface = std::env::var("CMUX_SURFACE_ID")
        .ok()
        .filter(|id| uuid::Uuid::parse_str(id).is_ok())
        .ok_or_else(|| CliError::Command("hook requires its originating CMUX_SURFACE_ID".into()))?;
    Ok((payload, id, surface))
}

/// Read one known scalar from the top-level payload or a documented provider envelope.
fn hook_string<'a>(payload: &'a Value, keys: &[&str]) -> Option<&'a str> {
    let objects = std::iter::once(payload).chain(
        ["notification", "data", "session", "context"]
            .iter()
            .filter_map(|key| payload.get(*key)),
    );
    for object in objects {
        for key in keys {
            if let Some(value) = object.get(*key).and_then(Value::as_str) {
                return Some(value);
            }
        }
    }
    None
}

fn set_agent_resume(
    client: &mut SocketClient,
    payload: &Value,
    surface: &str,
    id: &str,
    resume: ResumeCommand<'_>,
) -> Result<(), CliError> {
    let cwd = hook_string(payload, &["cwd", "working_directory", "workingDirectory"])
        .filter(|cwd| Path::new(cwd).is_absolute())
        .ok_or_else(|| CliError::Command("hook requires an absolute working directory".into()))?;
    let binary =
        cmux_platform::paths::find_command_on_path(resume.executable).ok_or_else(|| {
            CliError::Command(format!(
                "{} executable not found on PATH",
                resume.executable
            ))
        })?;
    let binary =
        std::path::absolute(binary).map_err(|error| CliError::Command(error.to_string()))?;
    let mut environment = serde_json::Map::new();
    for key in resume.environment {
        if let Ok(value) = std::env::var(key) {
            environment.insert((*key).into(), json!(value));
        }
    }
    let mut command = vec![shell_argument(&binary.to_string_lossy())];
    command.extend(
        resume
            .prefix
            .iter()
            .map(|argument| shell_argument(argument)),
    );
    command.push(shell_argument(id));
    client.call(
        "surface.resume.set",
        json!({"surface_id": surface, "kind": resume.kind,
            "command": command.join(" "),
            "checkpoint_id": id, "cwd": cwd, "environment": environment}),
    )?;
    Ok(())
}

fn clear_agent_resume(client: &mut SocketClient, surface: &str, id: &str) -> Result<(), CliError> {
    client.call(
        "surface.resume.clear",
        json!({"surface_id": surface, "checkpoint_id": id}),
    )?;
    Ok(())
}

fn create_agent_notification(
    client: &mut SocketClient,
    surface: &str,
    title: String,
    subtitle: String,
    body: String,
) -> Result<(), CliError> {
    client.call(
        "notification.create_for_surface",
        json!({"surface_id":surface,"title":title,"subtitle":subtitle,"body":body}),
    )?;
    Ok(())
}

fn bounded_notification_text(text: &str, limit: usize) -> Result<String, CliError> {
    if text.contains('\0') {
        return Err(CliError::Command(
            "notification text must not contain NUL".into(),
        ));
    }
    if text.len() <= limit {
        return Ok(text.into());
    }
    let mut boundary = limit - 3;
    while !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    Ok(format!("{}...", &text[..boundary]))
}

/// Fit native hook text into inbox limits at a UTF-8 boundary, with explicit visible truncation.
fn notification_text(
    payload: &Value,
    key: &str,
    default: &str,
    limit: usize,
) -> Result<String, CliError> {
    let text = match payload.get(key) {
        None | Some(Value::Null) => default,
        Some(Value::String(text)) => text,
        _ => return Err(CliError::Command(format!("{key} must be a string"))),
    };
    bounded_notification_text(text, limit)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reinstallation preserves arbitrary user settings and companion handlers without duplicating ours.
    #[test]
    fn merge_preserves_user_hooks() {
        let mut settings = json!({"model": "user-choice", "hooks": {"SessionStart": [{"matcher": "startup", "hooks": [{"type": "command", "command": "user-hook"}]}]}});
        merge_claude_hooks(&mut settings, Path::new("/a path/cmux")).unwrap();
        let once = settings.clone();
        merge_claude_hooks(&mut settings, Path::new("/a path/cmux")).unwrap();
        assert_eq!(settings, once);
        assert_eq!(settings["model"], "user-choice");
        assert_eq!(
            settings["hooks"]["SessionStart"][0]["hooks"][0]["command"],
            "user-hook"
        );
    }
}
