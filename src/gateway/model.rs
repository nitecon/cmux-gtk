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

/// Gateway project metadata, normalized by the lifecycle stream adapter, never inferred from folder names.
#[derive(Clone, Debug)]
pub struct Project {
    pub ident: String,
    pub upstream_urls: Vec<String>,
}

/// Canonical task lifecycle; these names define an internal model, not a WebSocket schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Created,
    Commented,
    Completed,
}

/// Mutation provenance identifies an exact agent; provider and OS describe it without selecting work.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Origin {
    pub session_id: String,
    pub instance_id: String,
    pub provider: String,
    pub os: String,
}

impl Origin {
    /// Validate the additive gateway context without treating its identifiers as authentication.
    pub fn validate(&self) -> Result<(), String> {
        uuid::Uuid::parse_str(&self.session_id).map_err(|_| "Invalid agent session identity")?;
        uuid::Uuid::parse_str(&self.instance_id).map_err(|_| "Invalid agent instance identity")?;
        if !matches!(self.provider.as_str(), "codex" | "claude")
            || !matches!(self.os.as_str(), "linux" | "windows" | "macos")
        {
            return Err("Invalid agent provider or OS".into());
        }
        Ok(())
    }
}

/// An event from the authenticated stream, including visible comment author attribution.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub event_id: String,
    pub project_ident: String,
    pub task_id: String,
    pub kind: Kind,
    pub text: String,
    pub author_id: Option<String>,
    pub source_instance: Option<String>,
    #[serde(default)]
    pub origin: Option<Origin>,
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
        if let Some(origin) = &self.origin {
            origin.validate()?;
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
        let origin = self
            .origin
            .as_ref()
            .map(|o| {
                format!(
                    "Origin: {} on {} · session {}\n",
                    o.provider, o.os, o.session_id
                )
            })
            .unwrap_or_default();
        format!("{START}\nGateway event {} · {} · {kind}: {}\n{origin}{}\nDo not post task comments solely to acknowledge this injected message.\n{STOP}", self.event_id, self.project_ident, self.task_id, self.text)
    }
}

/// GTK copies only owned metadata; native pointers never cross into a worker.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Terminal {
    pub workspace_id: String,
    pub surface_id: String,
    pub directory: std::path::PathBuf,
    /// POSIX foreground PID, or the ConPTY root used for conservative Windows agent discovery.
    pub foreground_pid: u64,
    /// Human drafts stay in CMUX; the GTK-owned executor is attached to this process.
    #[serde(default)]
    pub composer_active: bool,
}

/// A verified running application process and upstream repository, independent of resume hooks.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub terminal: Terminal,
    #[serde(with = "ProcessIdentity")]
    pub process: cmux_platform::process::Identity,
    pub repository: String,
    #[serde(default)]
    pub session_id: String,
    /// Logical actor provenance is independent of this terminal's durable delivery fence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_origin: Option<Origin>,
}

impl Session {
    /// Bind a globally namespaced UUID to a verified process generation and its exact terminal.
    pub fn identified(
        terminal: Terminal,
        process: cmux_platform::process::Identity,
        repository: String,
        instance_id: &str,
    ) -> Self {
        // Linux start ticks repeat across boots; missing native identity gets a fresh runtime namespace.
        static BOOT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        let boot = BOOT.get_or_init(|| {
            cmux_platform::process::boot_identity()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
        });
        let name = format!(
            "cmux/{instance_id}/{boot}/{}/{}/{}/{}/{}",
            terminal.workspace_id,
            terminal.surface_id,
            process.pid,
            process.start_ticks,
            process.client
        );
        Self {
            terminal,
            process,
            repository,
            session_id: uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, name.as_bytes()).to_string(),
            actor_origin: None,
        }
    }

    /// Expose current local context without credentials, screen contents or readiness authority.
    pub fn context(&self, instance_id: &str) -> serde_json::Value {
        serde_json::json!({
            "session_id": self.actor_origin.as_ref().map(|o|o.session_id.as_str()).unwrap_or(&self.session_id),
            "instance_id": self.actor_origin.as_ref().map(|o|o.instance_id.as_str()).unwrap_or(instance_id),
            "recipient_session_id": self.session_id,
            "provider": self.process.client,
            "delivery_transport": "cmux_input_queue",
            "composer_active": self.terminal.composer_active,
            "os": std::env::consts::OS,
            "surface_id": self.terminal.surface_id,
            "workspace_id": self.terminal.workspace_id,
            "repository": self.repository,
            "directory": self.terminal.directory,
        })
    }

    /// Suppress logical actor echoes while accepting exact provenance issued by older CMUX versions.
    pub fn is_origin(&self, origin: &Origin, legacy_instance: &str) -> bool {
        self.actor_origin.as_ref().is_some_and(|own| {
            own.session_id == origin.session_id && own.instance_id == origin.instance_id
        }) || self.session_id == origin.session_id && legacy_instance == origin.instance_id
    }

    /// Complete messages can run independently of downstream UI and unfinished local human text.
    pub fn ready(&self) -> bool {
        self.terminal.composer_active
    }

    /// Pin queued messages to one workspace, surface, process generation and repository.
    pub fn same_target(&self, other: &Self) -> bool {
        self.same_attachment(other)
            && self.repository == other.repository
            && self
                .actor_origin
                .as_ref()
                .is_none_or(|origin| other.actor_origin.as_ref() == Some(origin))
    }

    /// Native terminal membership, independent of Git context and client-owned logical identity.
    pub fn same_attachment(&self, other: &Self) -> bool {
        self.terminal.workspace_id == other.terminal.workspace_id
            && self.terminal.surface_id == other.terminal.surface_id
            && self.terminal.directory == other.terminal.directory
            && self.terminal.foreground_pid == other.terminal.foreground_pid
            && self.process == other.process
            && self.session_id == other.session_id
    }
}

/// Persistable Linux identity without adding application serialization dependencies to platform services.
#[derive(Serialize, Deserialize)]
#[serde(remote = "cmux_platform::process::Identity")]
struct ProcessIdentity {
    pid: u64,
    start_ticks: u64,
    client: String,
}

/// Durable event and delivery state; terminal outcomes retain routing/reason but release message bodies.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Receipt {
    pub event_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipient_session_id: Option<String>,
    pub outcome: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub payload: Option<Message>,
    #[serde(default)]
    pub target: Option<Session>,
    #[serde(default)]
    pub confirmed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<Session>,
}

impl Receipt {
    /// Distinguish each recipient fence while retaining one event-level transport cursor.
    pub fn recipient(&self) -> Option<&str> {
        self.recipient_session_id
            .as_deref()
            .or_else(|| self.target.as_ref().map(|s| s.session_id.as_str()))
    }

    /// Recognize finalized delivery, independent of canonical task status.
    pub fn terminal(&self) -> bool {
        matches!(
            self.outcome.as_str(),
            "injected" | "skipped" | "failed" | "uncertain"
        )
    }
}

/// Aggregate transport outcomes only after every pinned recipient has reached a terminal state.
pub fn receipt_status(receipts: &[&Receipt]) -> &'static str {
    for (state, aggregate) in [
        ("received", "received"),
        ("queued", "queued"),
        ("submitting", "queued"),
        ("uncertain", "uncertain"),
        ("failed", "failed"),
        ("injected", "injected"),
    ] {
        if receipts.iter().any(|r| r.outcome == state) {
            return aggregate;
        }
    }
    "skipped"
}

/// V2 replaces assignments and reports; old configuration loads with injection approval disabled.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Journal {
    pub instance_id: String,
    pub config: Config,
    #[serde(default)]
    pub receipts: VecDeque<Receipt>,
    #[serde(default)]
    pub cursor: Option<i64>,
}

impl Default for Journal {
    /// Allocate a stable instance ID only when no journal exists.
    fn default() -> Self {
        Self {
            instance_id: uuid::Uuid::new_v4().to_string(),
            config: Config::default(),
            receipts: VecDeque::new(),
            cursor: None,
        }
    }
}

/// Executor submission outcome; unavailable queue admission defers without claiming a message was sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryOutcome {
    Injected,
    Deferred,
    Uncertain,
}

/// Credential-free preferences/status snapshot, with no event bodies or execution reports.
#[derive(Clone, Debug, Default, Serialize)]
pub struct View {
    pub instance_id: String,
    pub connection: String,
    pub config: Config,
    pub pending: usize,
    pub projects: usize,
    pub agents: usize,
    pub receipts: VecDeque<Receipt>,
    /// Verified local process metadata used by GTK to attach each input composer, even with gateway disabled.
    #[serde(skip)]
    pub sessions: Vec<Session>,
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

/// Validate a base address for the published lifecycle WebSocket and REST metadata.
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
    } else if !value.contains(':')
        && value
            .split_once('/')
            .is_some_and(|(host, _)| host.contains('.'))
    {
        url::Url::parse(&format!("https://{value}")).ok()?
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
