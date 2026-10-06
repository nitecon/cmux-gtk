//! Transport-independent lifecycle messages and bounded, credential-free delivery state.
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub const MAX_PENDING: usize = 64;
pub const MAX_RECEIPTS: usize = 2048;
pub const START: &str = "<Start Agent Gateway Message Injection>";
pub const STOP: &str = "</Stop AgentGateway Message injection>";

/// Global preferences; injection approval is separate from enabling the connection.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub enabled: bool,
    pub url: String,
    pub injection_approved: bool,
}

/// Gateway project metadata, normalized by the future stream adapter, never inferred from folder names.
#[derive(Clone, Debug)]
pub struct Project {
    pub ident: String,
    pub upstream_urls: Vec<String>,
}

/// Canonical task lifecycle; these names define an internal model, not a WebSocket schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Created,
    Commented,
    Completed,
}

/// An event from the authenticated stream, including author attribution for comment loop suppression.
#[derive(Clone, Debug)]
pub struct Message {
    pub event_id: String,
    pub project_ident: String,
    pub task_id: String,
    pub kind: Kind,
    pub text: String,
    pub author_id: Option<String>,
    pub source_instance: Option<String>,
}

impl Message {
    /// Reject terminal controls, delimiter spoofing and oversized content before queue admission.
    pub fn validate(&self) -> Result<(), String> {
        for id in [&self.event_id, &self.project_ident, &self.task_id] {
            identity(id)?;
        }
        for id in [&self.author_id, &self.source_instance]
            .into_iter()
            .flatten()
        {
            identity(id)?;
        }
        if self.text.trim().is_empty()
            || self.text.len() > 48 * 1024
            || self
                .text
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
            || self.text.contains(START)
            || self.text.contains(STOP)
        {
            return Err("Invalid gateway message text".into());
        }
        if self.kind == Kind::Commented && self.author_id.is_none() {
            return Err("Task comments require author attribution".into());
        }
        Ok(())
    }

    /// Wrap literal input with the user's visible delimiters; task state remains owned by agent-tools.
    pub fn terminal_text(&self) -> String {
        let kind = match self.kind {
            Kind::Created => "New task",
            Kind::Commented => "Task commented",
            Kind::Completed => "Task completed",
        };
        format!("{START}\n{kind}: {}\n{}\n{STOP}", self.task_id, self.text)
    }
}

/// Unknown, busy and unfinished prompts never authorize input; an observer must positively confirm readiness.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InputState {
    #[default]
    Unknown,
    Busy,
    Unfinished,
    EmptyReady,
}

/// A recent hookless observation, bound to the process and all subsequent terminal input.
#[derive(Clone, Debug)]
pub struct Observation {
    pub process: cmux_platform::process::Identity,
    pub input_revision: u64,
    pub input: InputState,
    pub actor_id: Option<String>,
    pub observed_at: std::time::Instant,
}

/// GTK copies only owned metadata; native pointers never cross into a worker.
#[derive(Clone, Debug)]
pub struct Terminal {
    pub workspace_id: String,
    pub surface_id: String,
    pub directory: std::path::PathBuf,
    pub foreground_pid: u64,
    pub input_revision: u64,
    pub input_pending: bool,
    pub observation: Option<Observation>,
}

/// A verified active Claude/Codex process and upstream repository, independent of resume hooks.
#[derive(Clone, Debug)]
pub struct Session {
    pub terminal: Terminal,
    pub process: cmux_platform::process::Identity,
    pub repository: String,
}

impl Session {
    /// Require a fresh positive observation for this process and unchanged input revision.
    pub fn ready(&self) -> bool {
        !self.terminal.input_pending
            && self.terminal.observation.as_ref().is_some_and(|o| {
                o.process == self.process
                    && o.input_revision == self.terminal.input_revision
                    && o.input == InputState::EmptyReady
                    && o.observed_at.elapsed() < std::time::Duration::from_secs(1)
            })
    }

    /// Pin queued messages to one workspace, surface, process generation and repository.
    pub fn same_target(&self, other: &Self) -> bool {
        self.terminal.workspace_id == other.terminal.workspace_id
            && self.terminal.surface_id == other.terminal.surface_id
            && self.terminal.directory == other.terminal.directory
            && self.process == other.process
            && self.repository == other.repository
    }
}

/// Durable at-most-once fence; uncertain input is never automatically replayed after a crash.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Receipt {
    pub event_id: String,
    pub outcome: String,
}

/// V2 replaces assignments and reports; old configuration loads with injection approval disabled.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Journal {
    pub instance_id: String,
    pub config: Config,
    #[serde(default)]
    pub receipts: VecDeque<Receipt>,
}

impl Default for Journal {
    /// Allocate a stable instance ID only when no journal exists.
    fn default() -> Self {
        Self {
            instance_id: uuid::Uuid::new_v4().to_string(),
            config: Config::default(),
            receipts: VecDeque::new(),
        }
    }
}

/// Final GTK outcome; a busy/input race defers without claiming a message was injected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryOutcome {
    Injected,
    Deferred,
}

/// Credential-free preferences/status snapshot, with no event bodies or execution reports.
#[derive(Clone, Debug, Default, Serialize)]
pub struct View {
    pub connection: String,
    pub config: Config,
    pub pending: usize,
    pub receipts: VecDeque<Receipt>,
}

/// Validate bounded nonempty identifiers without exposing their contents in errors.
pub fn identity(value: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > 512
        || value.chars().any(char::is_control)
        || value.contains(START)
        || value.contains(STOP)
    {
        return Err("Invalid gateway identity".into());
    }
    Ok(())
}

/// Validate a base address without assuming the pending WebSocket endpoint or subscription protocol.
pub fn endpoint(value: &str) -> Result<url::Url, String> {
    let url = url::Url::parse(value).map_err(|_| "Invalid gateway URL")?;
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if !(matches!(url.scheme(), "https" | "wss")
        || loopback && matches!(url.scheme(), "http" | "ws"))
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "/" | "")
    {
        return Err("Use a gateway base HTTPS URL (HTTP is allowed only on loopback)".into());
    }
    Ok(url)
}

/// Match SSH/scp and HTTPS Git remotes by host and case-sensitive repository path, preserving custom ports.
pub fn repository(value: &str) -> Option<String> {
    if value.len() > 4096 || value.chars().any(char::is_control) {
        return None;
    }
    let url = if value.contains("://") {
        url::Url::parse(value).ok()?
    } else {
        let (host, path) = value.split_once(':')?;
        if host.contains('/') {
            return None;
        }
        url::Url::parse(&format!("ssh://{host}/{path}")).ok()?
    };
    if !matches!(url.scheme(), "https" | "http" | "ssh" | "git")
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let host = url.host_str()?.to_ascii_lowercase();
    let path = url.path().trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    if path.is_empty() {
        return None;
    }
    let port = url
        .port()
        .filter(|p| !(url.scheme() == "ssh" && *p == 22 || url.scheme() == "git" && *p == 9418));
    Some(match port {
        Some(port) => format!("{host}:{port}/{path}"),
        None => format!("{host}/{path}"),
    })
}
