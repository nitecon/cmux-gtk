//! Typed action intent; parsing validates data and never launches commands.
#[path = "project_layout.rs"]
pub(crate) mod project_layout;
#[allow(unused_imports)] // The standalone CLI resolves config but does not read session files.
pub(crate) use project_layout::environment as parse_environment;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Shared Linux project/restore nesting limit; deeper nested GtkPaned trees block responsiveness.
pub const MAX_LAYOUT_DEPTH: usize = 16;

/// Terminal command targets use upstream names and default to a fresh sibling tab.
#[derive(Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Target {
    CurrentTerminal,
    #[default]
    NewTabInCurrentPane,
}

/// Canonical upstream builtin identifiers; recognition does not imply this platform implements them.
#[derive(Debug, Serialize, PartialEq)]
pub enum Builtin {
    #[serde(rename = "cmux.newWorkspace")]
    NewWorkspace,
    #[serde(rename = "cmux.newAgentChat")]
    NewAgentChat,
    #[serde(rename = "cmux.cloudvm")]
    CloudVm,
    #[serde(rename = "cmux.mobileconnect")]
    MobileConnect,
    #[serde(rename = "cmux.newTerminal")]
    NewTerminal,
    #[serde(rename = "cmux.newBrowser")]
    NewBrowser,
    #[serde(rename = "cmux.newSimulator")]
    NewSimulator,
    #[serde(rename = "cmux.splitRight")]
    SplitRight,
    #[serde(rename = "cmux.splitDown")]
    SplitDown,
}

impl Builtin {
    /// Normalize upstream spelling aliases; unknown names never become executable string dispatch.
    fn parse(name: &str) -> Result<Self, String> {
        Ok(match name.trim() {
            "cmux.newWorkspace" | "newWorkspace" => Self::NewWorkspace,
            "cmux.newAgentChat" | "cmux.agentChat" | "newAgentChat" | "new-agent-chat"
            | "agentChat" => Self::NewAgentChat,
            "cmux.cloudvm" | "cmux.cloudVM" | "cloudVM" | "cloudvm" | "cmux.newCloudVM"
            | "cmux.newCloudVm" | "newCloudVM" | "newCloudVm" | "cmux.startCloudVM"
            | "cmux.startCloudVm" | "startCloudVM" | "startCloudVm" => Self::CloudVm,
            "cmux.mobileconnect" | "cmux.mobileConnect" | "mobileConnect" | "mobileconnect"
            | "cmux.connectPhone" | "connectPhone" => Self::MobileConnect,
            "cmux.newTerminal" | "newTerminal" => Self::NewTerminal,
            "cmux.newBrowser" | "newBrowser" => Self::NewBrowser,
            "cmux.newSimulator" | "newSimulator" | "new-simulator" | "simulator" => {
                Self::NewSimulator
            }
            "cmux.splitRight" | "splitRight" => Self::SplitRight,
            "cmux.splitDown" | "splitDown" => Self::SplitDown,
            _ => return Err("unknown builtin action".into()),
        })
    }
}

/// Recognized action families; workspace layout validation remains a separate implementation gate.
#[derive(Debug, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Intent {
    Metadata,
    Command {
        command: String,
    },
    Agent {
        agent: String,
        args: Option<String>,
    },
    Builtin {
        builtin: Builtin,
    },
    WorkspaceCommand {
        name: String,
    },
    Workspace {
        workspace: Box<project_layout::Workspace>,
    },
}

/// Match upstream agent aliases and shell-argument semantics after schema validation.
/// Args are reviewed shell text, not an argv list; executable names have already been bounded.
#[allow(dead_code)] // Shared with the discovery-only CLI binary, which never executes actions.
pub fn agent_command(agent: &str, args: Option<&str>) -> String {
    let executable = match agent {
        "claudeCode" | "claude-code" => "claude",
        "openCode" | "open-code" => "opencode",
        value => value,
    };
    let args = args.unwrap_or_default().trim();
    if args.is_empty() {
        executable.to_string()
    } else {
        format!("{executable} {args}")
    }
}

/// Require bounded, NUL-free string values; shell commands retain their literal contents.
fn string(
    value: &Value,
    key: &str,
    required: bool,
    limit: usize,
) -> Result<Option<String>, String> {
    match value.get(key).filter(|value| !value.is_null()) {
        None if !required => Ok(None),
        Some(Value::String(text))
            if text.len() <= limit
                && !text.contains('\0')
                && (!required || !text.trim().is_empty()) =>
        {
            Ok(Some(text.clone()))
        }
        _ => Err(format!("invalid or missing action {key}")),
    }
}

/// Named commands have their own upstream schema: exactly one workspace or shell command.
/// Ignore action-only fields rather than letting them change the named command's family.
pub fn parse_named(value: &Value) -> Result<(Intent, Target), String> {
    let workspace = value.get("workspace").filter(|value| !value.is_null());
    let command = value.get("command").filter(|value| !value.is_null());
    let (kind, payload) = match (workspace, command) {
        (Some(workspace), None) => ("workspace", workspace),
        (None, Some(command)) => ("command", command),
        _ => return Err("named command must define exactly one workspace or command".into()),
    };
    if let Some(restart) = value.get("restart").filter(|value| !value.is_null()) {
        if !matches!(
            restart.as_str(),
            Some("new" | "recreate" | "ignore" | "confirm")
        ) {
            return Err("invalid named command restart behavior".into());
        }
    }
    let mut selected = serde_json::json!({"type":kind});
    selected[kind] = payload.clone();
    for key in ["description", "keywords", "confirm"] {
        if let Some(value) = value.get(key) {
            selected[key] = value.clone();
        }
    }
    parse(&selected)
}

/// Infer upstream action families, reject invalid targets and preserve presentation metadata separately.
pub fn parse(value: &Value) -> Result<(Intent, Target), String> {
    for key in ["title", "subtitle", "description", "tooltip"] {
        string(value, key, false, 4096)?;
    }
    for key in ["palette", "confirm", "newWorkspaceMenu"] {
        if value
            .get(key)
            .is_some_and(|value| !value.is_null() && !value.is_boolean())
        {
            return Err(format!("action {key} must be boolean"));
        }
    }
    if let Some(keywords) = value.get("keywords").filter(|value| !value.is_null()) {
        let words = keywords
            .as_array()
            .ok_or("action keywords must be an array")?;
        if words.len() > 64
            || words.iter().any(|word| {
                word.as_str()
                    .is_none_or(|word| word.len() > 256 || word.contains('\0'))
            })
        {
            return Err("invalid action keywords".into());
        }
    }
    let target = match value.get("target").filter(|value| !value.is_null()) {
        Some(target) => {
            serde_json::from_value(target.clone()).map_err(|_| "invalid action target")?
        }
        None => Target::default(),
    };
    let explicit = string(value, "type", false, 64)?;
    let inferred = [
        ("agent", "agent"),
        ("builtin", "builtin"),
        ("workspace", "workspace"),
        ("command", "command"),
    ]
    .into_iter()
    .find(|(key, _)| value.get(*key).is_some())
    .map(|(_, kind)| kind);
    let required = |key| string(value, key, true, 16384).map(|value| value.unwrap());
    let intent = match explicit.as_deref().map(str::trim).or(inferred) {
        None => Intent::Metadata,
        Some("command") => Intent::Command {
            command: required("command")?,
        },
        Some("agent") => {
            let agent = required("agent")?.trim().to_string();
            if !agent
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
            {
                return Err("agent must be a CLI name".into());
            }
            Intent::Agent {
                agent,
                args: string(value, "args", false, 16384)?,
            }
        }
        Some("builtin") => Intent::Builtin {
            builtin: Builtin::parse(&required("builtin")?)?,
        },
        Some("workspaceCommand") => {
            let key = ["commandName", "name", "command"]
                .into_iter()
                .find(|key| value.get(*key).is_some_and(|value| !value.is_null()))
                .ok_or("workspaceCommand requires commandName")?;
            Intent::WorkspaceCommand {
                name: required(key)?,
            }
        }
        Some("workspace") => {
            let workspace = value
                .get("workspace")
                .filter(|value| value.is_object())
                .ok_or("workspace action requires an object")?;
            Intent::Workspace {
                workspace: Box::new(project_layout::parse(workspace)?),
            }
        }
        _ => return Err("unknown action type".into()),
    };
    Ok((intent, target))
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Explicit type wins inference; command text is literal and target defaults match upstream.
    #[test]
    fn command_semantics_and_inference() {
        let (intent, target) = parse(&serde_json::json!({"command":"  echo '$HOME'\n"})).unwrap();
        assert_eq!(
            intent,
            Intent::Command {
                command: "  echo '$HOME'\n".into()
            }
        );
        assert_eq!(target, Target::NewTabInCurrentPane);
        let (intent,target)=parse(&serde_json::json!({"type":"command","command":"pwd","agent":"claude","target":"currentTerminal"})).unwrap();
        assert_eq!(
            intent,
            Intent::Command {
                command: "pwd".into()
            }
        );
        assert_eq!(target, Target::CurrentTerminal);
    }
    /// Named schema cannot be redirected by action-only fields or contain ambiguous launch data.
    #[test]
    fn named_commands_validate_their_own_schema() {
        let (intent, _) = parse_named(
            &serde_json::json!({"command":"pwd","agent":"other","type":"agent","restart":"ignore"}),
        )
        .unwrap();
        assert_eq!(
            intent,
            Intent::Command {
                command: "pwd".into()
            }
        );
        for value in [
            serde_json::json!({}),
            serde_json::json!({"command":"pwd","workspace":{}}),
            serde_json::json!({"command":"  "}),
            serde_json::json!({"command":"pwd","restart":"unknown"}),
            serde_json::json!({"command":"pwd","confirm":"yes"}),
        ] {
            assert!(parse_named(&value).is_err());
        }
    }

    /// Provider aliases resolve while quoted arguments and shell expansion remain intact.
    #[test]
    fn agent_launch_matches_upstream_shell_semantics() {
        assert_eq!(
            agent_command("claudeCode", Some("  --resume 'two words'  ")),
            "claude --resume 'two words'"
        );
        assert_eq!(agent_command("open-code", None), "opencode");
        assert_eq!(agent_command("custom-agent", Some("  ")), "custom-agent");
        assert_eq!(
            agent_command("codex", Some("--model $MODEL")),
            "codex --model $MODEL"
        );
    }

    /// Aliases normalize to stable identities, including recognized features awaiting platform adaptation.
    #[test]
    fn builtin_aliases_are_canonical_and_unknowns_fail() {
        for (alias, canonical) in [
            ("newTerminal", "cmux.newTerminal"),
            ("cmux.startCloudVm", "cmux.cloudvm"),
            ("connectPhone", "cmux.mobileconnect"),
            ("simulator", "cmux.newSimulator"),
        ] {
            let (intent, _) = parse(&serde_json::json!({"builtin":alias})).unwrap();
            assert_eq!(serde_json::to_value(intent).unwrap()["builtin"], canonical);
        }
        assert!(parse(&serde_json::json!({"builtin":"cmux.typo"})).is_err());
    }

    /// Malformed executable fields cannot be mistaken for metadata-only definitions.
    #[test]
    fn rejects_malformed_actions() {
        for value in [
            serde_json::json!({"command":" "}),
            serde_json::json!({"command":5}),
            serde_json::json!({"agent":"x; echo bad"}),
            serde_json::json!({"type":"unknown"}),
            serde_json::json!({"target":"elsewhere"}),
            serde_json::json!({"confirm":"true"}),
        ] {
            assert!(parse(&value).is_err());
        }
    }
}
