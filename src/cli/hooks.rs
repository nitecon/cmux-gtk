//! Native agent hook installation and bounded session payload ingestion.
use super::{
    args::{ClaudeHookEvent, CodexHookEvent, JsonHookEvent, RovoHookEvent},
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
    prompt_event: Option<&'static str>,
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
            prompt_event: Some("UserPromptSubmit"),
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
            prompt_event: Some("BeforeAgent"),
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
            prompt_event: None,
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
            prompt_event: None,
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
            prompt_event: None,
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
            prompt_event: None,
            stop_event: "Stop",
            notification_event: None,
            end_event: Some("SessionEnd"),
        },
        "opencode" => JsonProvider {
            name: "opencode",
            display: "OpenCode",
            binary: "opencode",
            directory: ".config/opencode",
            directory_env: Some("OPENCODE_CONFIG_DIR"),
            environment: &["OPENCODE_CONFIG_DIR"],
            file: "plugins/cmux-session.js",
            resume_prefix: &["--session"],
            start_event: "SessionStart",
            prompt_event: None,
            stop_event: "Stop",
            notification_event: None,
            end_event: Some("SessionEnd"),
        },
        "cursor" => JsonProvider {
            name: "cursor",
            display: "Cursor",
            binary: "cursor-agent",
            directory: ".cursor",
            directory_env: None,
            environment: &[],
            file: "hooks.json",
            resume_prefix: &["--resume"],
            start_event: "beforeSubmitPrompt",
            prompt_event: Some("beforeSubmitPrompt"),
            stop_event: "stop",
            notification_event: None,
            end_event: None,
        },
        "pi" => JsonProvider {
            name: "pi",
            display: "Pi",
            binary: "pi",
            directory: ".pi/agent",
            directory_env: Some("PI_CODING_AGENT_DIR"),
            environment: &["PI_CODING_AGENT_DIR"],
            file: "extensions/cmux-session.ts",
            resume_prefix: &["--session"],
            start_event: "SessionStart",
            prompt_event: Some("UserPromptSubmit"),
            stop_event: "Stop",
            notification_event: None,
            end_event: Some("SessionEnd"),
        },
        "omp" => JsonProvider {
            name: "omp",
            display: "OMP",
            binary: "omp",
            directory: ".omp/agent",
            directory_env: Some("PI_CODING_AGENT_DIR"),
            environment: &["PI_CODING_AGENT_DIR", "PI_CONFIG_DIR"],
            file: "extensions/cmux-omp-session.ts",
            resume_prefix: &["--session"],
            start_event: "SessionStart",
            prompt_event: Some("UserPromptSubmit"),
            stop_event: "Stop",
            notification_event: None,
            end_event: None,
        },
        "campfire" => JsonProvider {
            name: "campfire",
            display: "Campfire",
            binary: "campfire",
            directory: ".campfire/agent",
            directory_env: Some("CAMPFIRE_CODING_AGENT_DIR"),
            environment: &["CAMPFIRE_CODING_AGENT_DIR"],
            file: "extensions/cmux-campfire-session.ts",
            resume_prefix: &["--session"],
            start_event: "SessionStart",
            prompt_event: Some("UserPromptSubmit"),
            stop_event: "Stop",
            notification_event: Some("Notification"),
            end_event: None,
        },
        "amp" => JsonProvider {
            name: "amp",
            display: "Amp",
            binary: "amp",
            directory: ".config/amp",
            directory_env: None,
            environment: &[],
            file: "plugins/cmux-session.ts",
            resume_prefix: &["threads", "continue"],
            start_event: "SessionStart",
            prompt_event: Some("UserPromptSubmit"),
            stop_event: "Stop",
            notification_event: None,
            end_event: None,
        },
        "rovodev" => JsonProvider {
            name: "rovodev",
            display: "Rovo Dev",
            binary: "acli",
            directory: ".rovodev",
            directory_env: None,
            environment: &["CMUX_ROVODEV_SESSIONS_DIR"],
            file: "config.yml",
            resume_prefix: &["rovodev", "run", "--restore"],
            start_event: "on_tool_permission",
            prompt_event: Some("on_tool_permission"),
            stop_event: "on_complete",
            notification_event: None,
            end_event: None,
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
        ("UserPromptSubmit", "prompt-submit"),
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
        Some("opencode") => setup_opencode(true),
        Some("cursor") => setup_cursor(true),
        Some("pi") => setup_pi(true),
        Some("omp") => setup_pi_style_extension("omp", true),
        Some("campfire") => setup_pi_style_extension("campfire", true),
        Some("amp") => setup_amp(true),
        Some("rovodev" | "rovo") => setup_rovodev(true),
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
            setup_opencode(false)?;
            setup_cursor(false)?;
            setup_pi(false)?;
            setup_pi_style_extension("omp", false)?;
            setup_pi_style_extension("campfire", false)?;
            setup_amp(false)?;
            setup_rovodev(false)?;
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
        ("UserPromptSubmit", "prompt-submit"),
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
    if let Some(event) = provider.prompt_event {
        events.push((event, "prompt-submit"));
    }
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

const OPENCODE_PLUGIN: &str = r#"// cmux-opencode-session-plugin-marker v1
import { spawnSync } from "node:child_process";

const cmux = __CMUX_BIN__;
function first(...values) {
  return values.find((value) => typeof value === "string" && value.trim())?.trim();
}
function send(command, context, event) {
  if (process.env.CMUX_OPENCODE_HOOKS_DISABLED === "1" || !process.env.CMUX_SURFACE_ID) return;
  const properties = event?.properties || {};
  const info = properties.info || {};
  const sessionId = first(info.id, properties.sessionID, properties.sessionId,
    properties.session_id, properties.session?.id, event?.sessionID, event?.sessionId);
  if (!sessionId) return;
  const payload = JSON.stringify({
    hook_event_name: command === "session-start" ? "SessionStart"
      : command === "session-end" ? "SessionEnd" : "Stop",
    session_id: sessionId,
    cwd: first(info.directory, properties.cwd, properties.directory, context?.directory, process.cwd()),
  });
  spawnSync(cmux, ["hooks", "opencode", command], {
    input: payload, encoding: "utf8", stdio: ["pipe", "ignore", "ignore"],
    timeout: 5000, env: process.env,
  });
}

const CMUXSessionRestore = async (context) => ({
  event: async ({ event }) => {
    const properties = event?.properties || {};
    switch (event?.type) {
      case "session.created": send("session-start", context, event); break;
      case "session.updated":
        send(properties.info?.time?.archived ? "session-end" : "session-start", context, event);
        break;
      case "session.status":
        if (properties.status?.type === "idle") send("stop", context, event);
        break;
      case "session.idle": send("stop", context, event); break;
      case "session.deleted": send("session-end", context, event); break;
    }
  },
});
export { CMUXSessionRestore };
export default CMUXSessionRestore;
"#;

fn setup_opencode(explicit: bool) -> Result<(), CliError> {
    let provider = json_provider("opencode").expect("static provider");
    if cmux_platform::paths::find_command_on_path(provider.binary).is_none() {
        if explicit {
            return Err(CliError::Command(
                "install OpenCode before installing its hooks".into(),
            ));
        }
        println!("Skipped OpenCode: executable not found on PATH");
        return Ok(());
    }
    let directory = std::env::var_os("OPENCODE_CONFIG_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config/opencode"))
        })
        .ok_or_else(|| CliError::Command("cannot resolve OpenCode config directory".into()))?;
    let config_path = directory.join("opencode.json");
    let plugin_path = directory.join(provider.file);
    if config_path.is_symlink() || plugin_path.is_symlink() {
        return Err(CliError::Command(
            "OpenCode hook configuration is a symlink; configure its target explicitly".into(),
        ));
    }
    let mut config = match cmux_platform::filesystem::read_text_bounded(&config_path, 1024 * 1024) {
        Ok(text) => serde_json::from_str::<Value>(&text)
            .map_err(|_| CliError::Command("OpenCode config contains invalid JSON".into()))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(error) => return Err(CliError::Command(format!("read OpenCode config: {error}"))),
    };
    let object = config
        .as_object_mut()
        .ok_or_else(|| CliError::Command("OpenCode config must be an object".into()))?;
    let plugins = object
        .entry("plugin")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| CliError::Command("OpenCode plugin must be an array".into()))?;
    let registration = "./plugins/cmux-session.js";
    if !plugins
        .iter()
        .any(|value| value.as_str() == Some(registration))
    {
        plugins.push(json!(registration));
    }
    let config_bytes =
        serde_json::to_vec_pretty(&config).map_err(|error| CliError::Command(error.to_string()))?;
    if config_bytes.len() > 1024 * 1024 {
        return Err(CliError::Command(
            "merged OpenCode config exceeds one MiB".into(),
        ));
    }
    let cmux = std::env::current_exe().map_err(|error| CliError::Command(error.to_string()))?;
    let encoded_binary = serde_json::to_string(&cmux.to_string_lossy())
        .map_err(|error| CliError::Command(error.to_string()))?;
    let plugin = OPENCODE_PLUGIN.replace("__CMUX_BIN__", &encoded_binary);
    cmux_platform::filesystem::create_private_directory(
        plugin_path.parent().expect("plugin path has parent"),
    )
    .map_err(|error| CliError::Command(format!("create OpenCode plugin directory: {error}")))?;
    cmux_platform::filesystem::atomic_write(&plugin_path, plugin.as_bytes())
        .and_then(|_| cmux_platform::filesystem::sync_file_and_parent(&plugin_path))
        .map_err(|error| CliError::Command(format!("save OpenCode plugin: {error}")))?;
    cmux_platform::filesystem::atomic_write(&config_path, &config_bytes)
        .and_then(|_| cmux_platform::filesystem::sync_file_and_parent(&config_path))
        .map_err(|error| CliError::Command(format!("save OpenCode config: {error}")))?;
    println!("Installed OpenCode hooks in {}", plugin_path.display());
    Ok(())
}

fn setup_cursor(explicit: bool) -> Result<(), CliError> {
    let provider = json_provider("cursor").expect("static provider");
    if cmux_platform::paths::find_command_on_path(provider.binary).is_none() {
        if explicit {
            return Err(CliError::Command(
                "install Cursor Agent before installing its hooks".into(),
            ));
        }
        println!("Skipped Cursor: executable not found on PATH");
        return Ok(());
    }
    let directory = std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(provider.directory))
        .ok_or_else(|| CliError::Command("cannot resolve Cursor config directory".into()))?;
    cmux_platform::filesystem::create_private_directory(&directory)
        .map_err(|error| CliError::Command(format!("create Cursor config directory: {error}")))?;
    let path = directory.join(provider.file);
    if path.is_symlink() {
        return Err(CliError::Command(
            "Cursor hooks is a symlink; configure its target explicitly".into(),
        ));
    }
    let mut settings = match cmux_platform::filesystem::read_text_bounded(&path, 1024 * 1024) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|_| CliError::Command("Cursor hooks contains invalid JSON".into()))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(error) => return Err(CliError::Command(format!("read Cursor hooks: {error}"))),
    };
    let binary = std::env::current_exe().map_err(|error| CliError::Command(error.to_string()))?;
    let object = settings
        .as_object_mut()
        .ok_or_else(|| CliError::Command("Cursor hooks must be an object".into()))?;
    let hooks = object
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| CliError::Command("Cursor hooks.hooks must be an object".into()))?;
    for (event, command) in [
        (provider.start_event, "prompt-submit"),
        (provider.stop_event, "stop"),
    ] {
        let entries = hooks
            .entry(event)
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or_else(|| CliError::Command(format!("Cursor {event} hooks must be an array")))?;
        entries.retain(|entry| {
            !entry
                .get("command")
                .and_then(Value::as_str)
                .is_some_and(|text| text.contains(" hooks cursor "))
        });
        entries.push(json!({"command": format!("{} hooks cursor {command}", shell_argument(&binary.to_string_lossy()))}));
    }
    object.insert("version".into(), json!(1));
    let encoded = serde_json::to_vec_pretty(&settings)
        .map_err(|error| CliError::Command(error.to_string()))?;
    if encoded.len() > 1024 * 1024 {
        return Err(CliError::Command(
            "merged Cursor hooks exceeds one MiB".into(),
        ));
    }
    cmux_platform::filesystem::atomic_write(&path, &encoded)
        .and_then(|_| cmux_platform::filesystem::sync_file_and_parent(&path))
        .map_err(|error| CliError::Command(format!("save Cursor hooks: {error}")))?;
    println!("Installed Cursor hooks in {}", path.display());
    Ok(())
}

const PI_STYLE_EXTENSION_TEMPLATE: &str = r#"// __MARKER__ v1
// Generated by cmux. Bridges a native extension lifecycle into the owning surface.
import { spawn } from "node:child_process";

const cmux = __CMUX_BINARY__;

function text(value) {
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}

function send(subcommand, hookEventName, event, context) {
  if (process.env["__DISABLE_ENV__"] === "1" || !process.env.CMUX_SURFACE_ID) return;
  __ROLE_GUARD__
  const sessionId = text(context?.sessionManager?.getSessionId?.());
  const cwd = text(context?.cwd) || process.cwd();
  if (!sessionId) return;
  const message = text(event?.message) || text(event?.reason);
  const payload = { session_id: sessionId, cwd, hook_event_name: hookEventName };
  if (message) payload.message = message;
  try {
    const child = spawn(cmux, ["hooks", "__PROVIDER__", subcommand], {
      env: process.env, detached: true, stdio: ["pipe", "ignore", "ignore"],
    });
    const timer = setTimeout(() => child.kill(), 5000);
    timer.unref?.();
    child.on("error", () => clearTimeout(timer));
    child.on("exit", () => clearTimeout(timer));
    child.stdin.on("error", () => {});
    child.stdin.end(JSON.stringify(payload));
    child.unref();
  } catch (_) {}
}

export default function __EXPORT_NAME__(pi) {
  __STATE_DECLARATION__
  pi.on("session_start", (event, context) => { __START_SETUP__ send("session-start", "SessionStart", event, context); });
  pi.on("before_agent_start", (event, context) => { __EVENT_GUARD__ send("prompt-submit", "UserPromptSubmit", event, context); });
  pi.on("agent_end", (event, context) => { __EVENT_GUARD__ send("stop", "Stop", event, context); });
  __SHUTDOWN_HANDLER__
  __OBSERVER_REGISTRATION__
}
"#;

fn setup_pi(explicit: bool) -> Result<(), CliError> {
    setup_pi_style_extension("pi", explicit)
}

fn setup_pi_style_extension(name: &str, explicit: bool) -> Result<(), CliError> {
    let provider = json_provider(name).expect("static extension provider");
    if cmux_platform::paths::find_command_on_path(provider.binary).is_none() {
        if explicit {
            return Err(CliError::Command(format!(
                "install {} before installing its extension",
                provider.display
            )));
        }
        println!("Skipped {}: executable not found on PATH", provider.display);
        return Ok(());
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| CliError::Command("cannot resolve home directory".into()))?;
    let agent_directory = match name {
        "pi" => std::env::var_os("PI_CODING_AGENT_DIR")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".pi/agent")),
        "omp" => std::env::var_os("PI_CODING_AGENT_DIR")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let configured = std::env::var_os("PI_CONFIG_DIR")
                    .filter(|value| !value.is_empty())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from(".omp"));
                if configured.is_absolute() {
                    configured.join("agent")
                } else {
                    home.join(configured).join("agent")
                }
            }),
        "campfire" => std::env::var_os("CAMPFIRE_CODING_AGENT_DIR")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".campfire/agent")),
        _ => unreachable!("validated extension provider"),
    };
    let directory = agent_directory.join("extensions");
    cmux_platform::filesystem::create_private_directory(&directory).map_err(|error| {
        CliError::Command(format!(
            "create {} extensions directory: {error}",
            provider.display
        ))
    })?;
    let filename = Path::new(provider.file)
        .file_name()
        .expect("static extension provider filename");
    let path = directory.join(filename);
    if path.is_symlink() {
        return Err(CliError::Command(format!(
            "{} extension is a symlink; configure its target explicitly",
            provider.display
        )));
    }
    match cmux_platform::filesystem::read_text_bounded(&path, 1024 * 1024) {
        Ok(existing)
            if !existing.is_empty()
                && !existing.contains(&format!("cmux-{name}-session-extension-marker")) =>
        {
            return Err(CliError::Command(format!(
                "{} extension exists without the cmux ownership marker",
                provider.display
            )));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(CliError::Command(format!(
                "read {} extension: {error}",
                provider.display
            )))
        }
    }
    let binary = std::env::current_exe().map_err(|error| CliError::Command(error.to_string()))?;
    let encoded_binary = serde_json::to_string(&binary.to_string_lossy())
        .map_err(|error| CliError::Command(error.to_string()))?;
    let (
        export_name,
        disable_env,
        role_guard,
        shutdown_handler,
        state_declaration,
        start_setup,
        event_guard,
        observer_registration,
    ) = match name {
        "pi" => (
            "cmuxPiSessionExtension",
            "CMUX_PI_HOOKS_DISABLED",
            "",
            "pi.on(\"session_shutdown\", (event, context) => send(\"session-end\", \"SessionEnd\", event, context));",
            "",
            "",
            "",
            "",
        ),
        "omp" => (
            "cmuxOmpSessionExtension",
            "CMUX_OMP_HOOKS_DISABLED",
            "",
            "",
            "let ownerSession; const owns = context => text(context?.sessionManager?.getSessionId?.()) === ownerSession;",
            "const id = text(context?.sessionManager?.getSessionId?.()); if (!ownerSession) ownerSession = id; if (!id || id !== ownerSession) return;",
            "if (!owns(context)) return;",
            r#"const adopt = (event, context) => {
    const id = text(context?.sessionManager?.getSessionId?.());
    if (!id || id === ownerSession) return;
    ownerSession = id;
    send("session-start", "SessionStart", event, context);
  };
  pi.on("session_switch", adopt);
  pi.on("session_branch", adopt);"#,
        ),
        "campfire" => (
            "cmuxCampfireSessionExtension",
            "CMUX_CAMPFIRE_HOOKS_DISABLED",
            "if (process.env.CAMPFIRE_SESSION_ROLE !== \"host\") return;",
            "",
            "let activeContext;",
            "activeContext = context;",
            "",
            r#"const bridge = globalThis[Symbol.for("campfire.observer.v1")];
  if (bridge?.listeners instanceof Set) {
    let observerWindow = Date.now();
    let observerDeliveries = 0;
    const listener = (event) => {
      if (!activeContext || !["join.requested", "permission.asked", "relay.error"].includes(event?.type)) return;
      const now = Date.now();
      if (now - observerWindow >= 1000) { observerWindow = now; observerDeliveries = 0; }
      if (observerDeliveries >= 8) return;
      observerDeliveries += 1;
      const detail = [text(event?.displayName), text(event?.capability), text(event?.reason)].filter(Boolean).join(" · ");
      send("notification", "Notification", { message: `${event.type}${detail ? `: ${detail}` : ""}`.slice(0, 8192) }, activeContext);
    };
    bridge.listeners.add(listener);
    pi.on("session_end", () => { activeContext = undefined; bridge.listeners.delete(listener); });
  }"#,
        ),
        _ => unreachable!("validated extension provider"),
    };
    let source = PI_STYLE_EXTENSION_TEMPLATE
        .replace("__CMUX_BINARY__", &encoded_binary)
        .replace(
            "__MARKER__",
            &format!("cmux-{name}-session-extension-marker"),
        )
        .replace("__DISABLE_ENV__", disable_env)
        .replace("__ROLE_GUARD__", role_guard)
        .replace("__PROVIDER__", name)
        .replace("__EXPORT_NAME__", export_name)
        .replace("__SHUTDOWN_HANDLER__", shutdown_handler)
        .replace("__STATE_DECLARATION__", state_declaration)
        .replace("__START_SETUP__", start_setup)
        .replace("__EVENT_GUARD__", event_guard)
        .replace("__OBSERVER_REGISTRATION__", observer_registration);
    cmux_platform::filesystem::atomic_write(&path, source.as_bytes())
        .and_then(|_| cmux_platform::filesystem::sync_file_and_parent(&path))
        .map_err(|error| {
            CliError::Command(format!("save {} extension: {error}", provider.display))
        })?;
    println!(
        "Installed {} lifecycle extension in {}",
        provider.display,
        path.display()
    );
    Ok(())
}

const AMP_EXTENSION_TEMPLATE: &str = r#"// cmux-amp-session-extension-marker v1
// Generated by cmux. Bridges Amp's native plugin lifecycle into the owning surface.
// @i-know-the-amp-plugin-api-is-wip-and-very-experimental-right-now
import { spawn } from "node:child_process";

const cmux = __CMUX_BINARY__;

function text(value) {
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}

export default function cmuxAmpSessionPlugin(amp) {
  const rootThread = amp?.thread;
  function send(subcommand, hookEventName, event, context) {
    if (process.env.CMUX_AMP_HOOKS_DISABLED === "1" || !process.env.CMUX_SURFACE_ID) return;
    const sessionId = text(event?.thread?.id) || text(context?.thread?.id) || text(rootThread?.id);
    const cwd = text(process.env.CMUX_AGENT_LAUNCH_CWD) || process.cwd();
    if (!sessionId) return;
    const message = text(event?.message) || text(event?.status);
    const payload = { session_id: sessionId, cwd, hook_event_name: hookEventName };
    if (message) payload.message = message;
    try {
      const child = spawn(cmux, ["hooks", "amp", subcommand], {
        env: process.env, detached: true, stdio: ["pipe", "ignore", "ignore"],
      });
      const timer = setTimeout(() => child.kill(), 5000);
      timer.unref?.();
      child.on("error", () => clearTimeout(timer));
      child.on("exit", () => clearTimeout(timer));
      child.stdin.on("error", () => {});
      child.stdin.end(JSON.stringify(payload));
      child.unref();
    } catch (_) {}
  }
  amp.on("session.start", (event, context) => send("session-start", "SessionStart", event, context));
  amp.on("agent.start", (event, context) => send("prompt-submit", "UserPromptSubmit", event, context));
  amp.on("agent.end", (event, context) => send("stop", "Stop", event, context));
}
"#;

fn setup_amp(explicit: bool) -> Result<(), CliError> {
    let provider = json_provider("amp").expect("static provider");
    if cmux_platform::paths::find_command_on_path(provider.binary).is_none() {
        if explicit {
            return Err(CliError::Command(
                "install Amp before installing its plugin".into(),
            ));
        }
        println!("Skipped Amp: executable not found on PATH");
        return Ok(());
    }
    let directory = std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".config/amp/plugins"))
        .ok_or_else(|| CliError::Command("cannot resolve Amp plugin directory".into()))?;
    cmux_platform::filesystem::create_private_directory(&directory)
        .map_err(|error| CliError::Command(format!("create Amp plugin directory: {error}")))?;
    let path = directory.join("cmux-session.ts");
    if path.is_symlink() {
        return Err(CliError::Command(
            "Amp plugin is a symlink; configure its target explicitly".into(),
        ));
    }
    match cmux_platform::filesystem::read_text_bounded(&path, 1024 * 1024) {
        Ok(existing)
            if !existing.is_empty() && !existing.contains("cmux-amp-session-extension-marker") =>
        {
            return Err(CliError::Command(
                "Amp plugin exists without the cmux ownership marker".into(),
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(CliError::Command(format!("read Amp plugin: {error}"))),
    }
    let binary = std::env::current_exe().map_err(|error| CliError::Command(error.to_string()))?;
    let encoded_binary = serde_json::to_string(&binary.to_string_lossy())
        .map_err(|error| CliError::Command(error.to_string()))?;
    let source = AMP_EXTENSION_TEMPLATE.replace("__CMUX_BINARY__", &encoded_binary);
    cmux_platform::filesystem::atomic_write(&path, source.as_bytes())
        .and_then(|_| cmux_platform::filesystem::sync_file_and_parent(&path))
        .map_err(|error| CliError::Command(format!("save Amp plugin: {error}")))?;
    println!("Installed Amp lifecycle plugin in {}", path.display());
    Ok(())
}

const ROVO_BEGIN: &str = "# cmux hooks rovodev begin";
const ROVO_END: &str = "# cmux hooks rovodev end";

fn yaml_double_quoted(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    )
}

fn rovo_event_lines(indent: &str, binary: &Path) -> Vec<String> {
    let prefix = shell_argument(&binary.to_string_lossy());
    [
        ("on_complete", "stop"),
        ("on_error", "stop"),
        ("on_tool_permission", "prompt-submit"),
    ]
    .into_iter()
    .flat_map(|(event, command)| {
        let hook = yaml_double_quoted(&format!("{prefix} hooks rovodev {command}"));
        [
            format!("{indent}- name: {event}"),
            format!("{indent}  commands:"),
            format!("{indent}    - command: {hook}"),
        ]
    })
    .collect()
}

fn merge_rovodev_hooks(existing: &str, binary: &Path) -> String {
    let normalized = existing.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines: Vec<String> = normalized.split('\n').map(ToOwned::to_owned).collect();
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    let mut cursor = 0;
    while cursor < lines.len() {
        if lines[cursor].trim() != ROVO_BEGIN {
            cursor += 1;
            continue;
        }
        if let Some(relative) = lines[cursor + 1..]
            .iter()
            .position(|line| line.trim() == ROVO_END)
        {
            lines.drain(cursor..=cursor + 1 + relative);
        } else {
            cursor += 1;
        }
    }
    let root = lines
        .iter()
        .position(|line| !line.starts_with([' ', '\t']) && line.trim() == "eventHooks:");
    if let Some(root) = root {
        let root_end = ((root + 1)..lines.len())
            .find(|index| {
                !lines[*index].trim().is_empty() && !lines[*index].starts_with([' ', '\t'])
            })
            .unwrap_or(lines.len());
        let events = ((root + 1)..root_end)
            .find(|index| lines[*index].starts_with("  ") && lines[*index].trim() == "events:");
        if let Some(events) = events {
            let mut block = vec![format!("    {ROVO_BEGIN}")];
            block.extend(rovo_event_lines("    ", binary));
            block.push(format!("    {ROVO_END}"));
            lines.splice(events + 1..events + 1, block);
        } else {
            let mut block = vec![format!("  {ROVO_BEGIN}"), "  events:".into()];
            block.extend(rovo_event_lines("    ", binary));
            block.push(format!("  {ROVO_END}"));
            lines.splice(root + 1..root + 1, block);
        }
    } else {
        if lines.last().is_some_and(|line| !line.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push(ROVO_BEGIN.into());
        lines.push("eventHooks:".into());
        lines.push("  events:".into());
        lines.extend(rovo_event_lines("    ", binary));
        lines.push(ROVO_END.into());
    }
    lines.join("\n") + "\n"
}

fn setup_rovodev(explicit: bool) -> Result<(), CliError> {
    let provider = json_provider("rovodev").expect("static provider");
    if cmux_platform::paths::find_command_on_path(provider.binary).is_none() {
        if explicit {
            return Err(CliError::Command(
                "install Rovo Dev before installing its hooks".into(),
            ));
        }
        println!("Skipped Rovo Dev: executable not found on PATH");
        return Ok(());
    }
    let directory = std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".rovodev"))
        .ok_or_else(|| CliError::Command("cannot resolve Rovo Dev config directory".into()))?;
    cmux_platform::filesystem::create_private_directory(&directory)
        .map_err(|error| CliError::Command(format!("create Rovo Dev config directory: {error}")))?;
    let path = directory.join("config.yml");
    if path.is_symlink() {
        return Err(CliError::Command(
            "Rovo Dev config is a symlink; configure its target explicitly".into(),
        ));
    }
    let existing = match cmux_platform::filesystem::read_text_bounded(&path, 1024 * 1024) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(CliError::Command(format!("read Rovo Dev config: {error}"))),
    };
    let binary = std::env::current_exe().map_err(|error| CliError::Command(error.to_string()))?;
    let merged = merge_rovodev_hooks(&existing, &binary);
    if merged.len() > 1024 * 1024 {
        return Err(CliError::Command(
            "merged Rovo Dev config exceeds one MiB".into(),
        ));
    }
    cmux_platform::filesystem::atomic_write(&path, merged.as_bytes())
        .and_then(|_| cmux_platform::filesystem::sync_file_and_parent(&path))
        .map_err(|error| CliError::Command(format!("save Rovo Dev config: {error}")))?;
    println!("Installed Rovo Dev lifecycle hooks in {}", path.display());
    Ok(())
}

/// Consume a bounded native payload and persist its exact session identity without granting automatic trust.
/// Session-end clears only the matching checkpoint; commands, prompts and payload bodies are not logged.
pub fn claude_event(client: &mut SocketClient, event: ClaudeHookEvent) -> Result<(), CliError> {
    let expected = match event {
        ClaudeHookEvent::SessionStart => "SessionStart",
        ClaudeHookEvent::PromptSubmit => "UserPromptSubmit",
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
            record_turn_baseline(&payload, &surface, &id);
        }
        ClaudeHookEvent::PromptSubmit => record_turn_baseline(&payload, &surface, &id),
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
        CodexHookEvent::PromptSubmit => "UserPromptSubmit",
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
            record_turn_baseline(&payload, &surface, &id);
        }
        CodexHookEvent::PromptSubmit => record_turn_baseline(&payload, &surface, &id),
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
        JsonHookEvent::PromptSubmit => provider
            .prompt_event
            .ok_or_else(|| CliError::Command("provider has no prompt-submit hook".into()))?,
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
        JsonHookEvent::SessionStart => {
            set_agent_resume(
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
            )?;
            if provider.name != "opencode" {
                record_turn_baseline(&payload, &surface, &id);
            }
        }
        JsonHookEvent::PromptSubmit => {
            if provider.name == "cursor" {
                set_agent_resume(
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
                )?;
            }
            record_turn_baseline(&payload, &surface, &id);
        }
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

fn rovo_yaml_scalar(raw: &str) -> Option<String> {
    let value = raw.trim();
    if let Some(value) = value.strip_prefix('\'') {
        let mut result = String::new();
        let mut chars = value.chars().peekable();
        while let Some(character) = chars.next() {
            if character == '\'' {
                if chars.peek() == Some(&'\'') {
                    chars.next();
                    result.push('\'');
                } else {
                    return Some(result);
                }
            } else {
                result.push(character);
            }
        }
        return None;
    }
    if let Some(value) = value.strip_prefix('"') {
        return value.split('"').next().map(ToOwned::to_owned);
    }
    Some(value.split(" #").next().unwrap_or(value).trim().to_owned())
}

fn rovo_sessions_root() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("CMUX_ROVODEV_SESSIONS_DIR") {
        return Some(PathBuf::from(path));
    }
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let config_path = home.join(".rovodev/config.yml");
    if let Ok(config) = cmux_platform::filesystem::read_text_bounded(&config_path, 1024 * 1024) {
        let mut in_sessions = false;
        for line in config.replace("\r\n", "\n").replace('\r', "\n").lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
            if indent == 0 {
                in_sessions = trimmed == "sessions:";
                continue;
            }
            if in_sessions && indent == 2 {
                if let Some(raw) = trimmed.strip_prefix("persistenceDir:") {
                    if let Some(value) = rovo_yaml_scalar(raw).filter(|value| !value.is_empty()) {
                        if value == "~" {
                            return Some(home);
                        }
                        if let Some(relative) = value.strip_prefix("~/") {
                            return Some(home.join(relative));
                        }
                        return Some(PathBuf::from(value));
                    }
                }
            }
        }
    }
    Some(home.join(".rovodev/sessions"))
}

fn normalized_existing_path(path: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(path).ok().or_else(|| {
        path.is_absolute()
            .then(|| path.components().collect::<PathBuf>())
    })
}

fn infer_rovodev_session(cwd: &Path) -> Result<Option<String>, CliError> {
    let Some(root) = rovo_sessions_root() else {
        return Ok(None);
    };
    let Some(cwd) = normalized_existing_path(cwd) else {
        return Ok(None);
    };
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(CliError::Command(format!(
                "read Rovo Dev sessions: {error}"
            )))
        }
    };
    let mut candidates = Vec::new();
    for (index, entry) in entries.enumerate() {
        if index >= 256 {
            return Err(CliError::Command(
                "Rovo Dev session directory exceeds 256 entries".into(),
            ));
        }
        let entry = entry.map_err(|error| CliError::Command(error.to_string()))?;
        if !entry
            .file_type()
            .map_err(|error| CliError::Command(error.to_string()))?
            .is_dir()
        {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        if id.is_empty()
            || id.len() > 1024
            || id.starts_with('-')
            || id.chars().any(char::is_control)
        {
            continue;
        }
        let metadata_path = entry.path().join("metadata.json");
        let metadata_text =
            match cmux_platform::filesystem::read_text_bounded(&metadata_path, 65536) {
                Ok(text) => text,
                Err(_) => continue,
            };
        let metadata: Value = match serde_json::from_str(&metadata_text) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let Some(workspace) = hook_string(
            &metadata,
            &[
                "workspace_path",
                "workspacePath",
                "working_directory",
                "cwd",
            ],
        )
        .map(PathBuf::from)
        .and_then(|path| normalized_existing_path(&path)) else {
            continue;
        };
        if workspace != cwd {
            continue;
        }
        let modified = [metadata_path, entry.path().join("session_context.json")]
            .into_iter()
            .filter_map(|path| std::fs::metadata(path).ok()?.modified().ok())
            .max()
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        candidates.push((modified, id));
    }
    candidates.sort_by(|left, right| right.cmp(left));
    Ok(candidates.into_iter().next().map(|(_, id)| id))
}

/// Rovo's lifecycle payload omits its durable session ID. Resolve the newest bounded metadata
/// candidate for the exact workspace before recording or notifying; unrelated sessions are ignored.
pub fn rovodev_event(client: &mut SocketClient, event: RovoHookEvent) -> Result<(), CliError> {
    let expected: &[&str] = match event {
        RovoHookEvent::PromptSubmit => &["on_tool_permission"],
        RovoHookEvent::Stop => &["on_complete", "on_error"],
    };
    let (payload, supplied_id, surface) = read_hook_payload_optional(expected)?;
    let cwd = hook_string(&payload, &["cwd", "working_directory", "workingDirectory"])
        .filter(|cwd| Path::new(cwd).is_absolute());
    let inferred_id = match (supplied_id, cwd) {
        (Some(id), _) => Some(id),
        (None, Some(cwd)) => infer_rovodev_session(Path::new(cwd))?,
        (None, None) => None,
    };
    let Some(id) = inferred_id else {
        return Ok(());
    };
    set_agent_resume(
        client,
        &payload,
        &surface,
        &id,
        ResumeCommand {
            kind: "rovodev",
            executable: "acli",
            prefix: &["rovodev", "run", "--restore"],
            environment: &["CMUX_ROVODEV_SESSIONS_DIR"],
        },
    )?;
    if matches!(event, RovoHookEvent::PromptSubmit) {
        record_turn_baseline(&payload, &surface, &id);
    }
    if matches!(event, RovoHookEvent::Stop) {
        let body = hook_string(&payload, &["message", "summary", "error"])
            .unwrap_or("Rovo Dev has finished responding.");
        create_agent_notification(
            client,
            &surface,
            "Rovo Dev response ready".into(),
            String::new(),
            bounded_notification_text(body, 8192)?,
        )?;
    }
    Ok(())
}

fn read_hook_payload(expected: &str) -> Result<(Value, String, String), CliError> {
    let (payload, id, surface) = read_hook_payload_optional(&[expected])?;
    let id = id.ok_or_else(|| CliError::Command("hook has no valid native session_id".into()))?;
    Ok((payload, id, surface))
}

fn read_hook_payload_optional(
    expected: &[&str],
) -> Result<(Value, Option<String>, String), CliError> {
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
    if !event_name.is_some_and(|event| expected.contains(&event)) {
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
    .map(ToOwned::to_owned);
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

/// Best-effort Git snapshot for the next `cmux diff --last-turn`; hook delivery remains usable
/// outside repositories and when a repository exceeds the baseline resource budget.
fn record_turn_baseline(payload: &Value, surface: &str, session: &str) {
    let Some(cwd) = hook_string(payload, &["cwd", "working_directory", "workingDirectory"])
        .filter(|cwd| Path::new(cwd).is_absolute())
    else {
        return;
    };
    let _ = super::diff::record_baseline(Path::new(cwd), surface, session);
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
