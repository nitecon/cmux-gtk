//! Bounded gateway configuration, exact session identities and durable delivery state.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const MAX_SESSIONS: usize = 64;

/// Explicit project association; workspace UUIDs survive application restarts.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mapping {
    pub workspace_id: String,
    pub project_ident: String,
}

/// Non-secret preferences kept separately from terminal session snapshots.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Config {
    pub enabled: bool,
    pub url: String,
    pub mappings: Vec<Mapping>,
}

/// A provider hook-confirmed native agent; readiness never comes from terminal text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub workspace_id: String,
    pub surface_id: String,
    pub session_id: String,
    pub project_ident: String,
    pub client: String,
    pub model: Option<String>,
    pub cwd: String,
    pub state: String,
}

impl Session {
    /// Compare immutable routing identities, excluding transient readiness.
    pub fn matches(&self, assignment: &Assignment) -> bool {
        self.workspace_id == assignment.workspace_id
            && self.surface_id == assignment.surface_id
            && self.session_id == assignment.session_id
            && self.project_ident == assignment.project_ident
    }
}

/// Validated structured instruction addressed to exactly one registered native session.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Assignment {
    pub run_id: String,
    pub task_id: String,
    pub project_ident: String,
    pub session_key: String,
    pub workspace_id: String,
    pub surface_id: String,
    pub session_id: String,
    pub action: String,
    pub prompt: String,
}

impl Assignment {
    /// Reject malformed identities and unsafe terminal control bytes before journal admission.
    pub fn validate(&self) -> Result<(), String> {
        for id in [
            &self.run_id,
            &self.task_id,
            &self.session_key,
            &self.workspace_id,
            &self.surface_id,
        ] {
            uuid::Uuid::parse_str(id).map_err(|_| "Invalid assignment UUID")?;
        }
        identity(&self.project_ident)?;
        identity(&self.session_id)?;
        if self.action != "execute"
            || self.prompt.len() > 48 * 1024
            || self
                .prompt
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
        {
            return Err("Invalid assignment prompt or action".into());
        }
        Ok(())
    }

    /// Include native reporting instructions without asking the agent to launch another process.
    pub fn terminal_prompt(&self) -> String {
        format!("{}\nNew delegated task {} is in your queue. Fetch its current details and claim it with agent-tools before changing code. Report progress/questions with cmux gateway report --run {} --state running (or waiting-input) --message '...'. When work ends, report --state finished (or failed) --message '...' --summary 'Work; validation; blockers; references'. Task completion still requires agent-tools tasks done.\n", self.prompt, self.task_id, self.run_id)
    }
}

/// A report stays queued durably until the server records its sequence.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub sequence: i64,
    pub state: String,
    pub message: String,
    pub summary: Option<String>,
}

/// One run's acceptance and submission markers fence duplicate or uncertain terminal input.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Run {
    pub assignment: Assignment,
    pub session: Session,
    pub phase: String,
    pub status: String,
    #[serde(default)]
    pub ended: bool,
    pub last_sequence: i64,
    pub reports: Vec<Report>,
}

impl Run {
    /// Execution completion is authoritative server state, distinct from an agent turn ending.
    pub fn terminal(&self) -> bool {
        self.ended || terminal_status(&self.status)
    }

    /// Allocate a persisted signed-64-bit sequence and bound pending progress before sending.
    pub fn report(
        &mut self,
        state: &str,
        message: &str,
        summary: Option<&str>,
    ) -> Result<(), String> {
        if !matches!(state, "running" | "waiting_input" | "finished" | "failed") || self.terminal()
        {
            return Err("Run cannot accept this report".into());
        }
        if self.phase != "submitted" && self.phase != "uncertain" {
            return Err("Accept and submit the assignment before reporting".into());
        }
        if self.reports.len() >= 16 {
            return Err("Pending gateway report queue is full".into());
        }
        self.last_sequence = self
            .last_sequence
            .checked_add(1)
            .ok_or("Report sequence exhausted")?;
        self.reports.push(Report {
            sequence: self.last_sequence,
            state: state.into(),
            message: bounded(message, 4096),
            summary: summary.map(|s| bounded(s, 16384)),
        });
        Ok(())
    }

    /// Encode only the first pending report; later reports wait for durable acknowledgment.
    pub fn pending_report(&self) -> Option<Value> {
        let report = self.reports.first()?;
        Some(json!({"type":"report", "run_id":self.assignment.run_id,
            "session_key":self.assignment.session_key, "sequence":report.sequence,
            "state":report.state, "message":report.message, "summary":report.summary}))
    }
}

/// Credential-free disk state; prompts/reports use private, bounded application storage.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Journal {
    pub instance_id: String,
    pub config: Config,
    pub runs: Vec<Run>,
}

impl Default for Journal {
    /// Generate a stable identity only when there is no previous application journal.
    fn default() -> Self {
        Self {
            instance_id: uuid::Uuid::new_v4().to_string(),
            config: Config::default(),
            runs: Vec::new(),
        }
    }
}

/// Credential-free UI/socket snapshot; terminal prompt text remains inspectable by the user.
#[derive(Clone, Debug, Default, Serialize)]
pub struct View {
    pub connection: String,
    pub config: Config,
    pub runs: Vec<Run>,
    pub sessions: Vec<Session>,
}

/// Recognize only finalized execution statuses; needs_attention without finished_at remains active.
pub fn terminal_status(status: &str) -> bool {
    matches!(status, "completed" | "failed" | "cancelled" | "interrupted")
}

/// Validate a human-readable protocol identity without exposing its contents in errors.
pub fn identity(value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
        return Err("Invalid gateway identity".into());
    }
    Ok(())
}

/// Truncate on UTF-8 boundaries with a visible suffix inside the requested byte limit.
pub fn bounded(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_owned();
    }
    let mut end = limit.saturating_sub("…".len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

/// Require TLS outside exact loopback hosts and reject URL credentials, query and fragments.
pub fn endpoint(value: &str) -> Result<url::Url, String> {
    let mut url = url::Url::parse(value).map_err(|_| "Invalid gateway URL")?;
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if !(matches!(url.scheme(), "https" | "wss")
        || loopback && matches!(url.scheme(), "http" | "ws"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "/" | "" | "/v1/execution/connect")
    {
        return Err("Use a gateway base HTTPS URL (HTTP is allowed only on loopback)".into());
    }
    let scheme = if matches!(url.scheme(), "https" | "wss") {
        "wss"
    } else {
        "ws"
    };
    url.set_scheme(scheme)
        .map_err(|_| "Invalid gateway URL scheme")?;
    url.set_path("/v1/execution/connect");
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exercise public URL policy and UTF-8 limits rather than implementation details.
    #[test]
    fn endpoint_and_text_bounds() {
        assert!(endpoint("http://example.com").is_err());
        assert!(endpoint("https://key@example.com").is_err());
        assert!(endpoint("https://example.com?token=secret").is_err());
        assert!(endpoint("http://127.0.0.1:8080").is_ok());
        assert_eq!(
            endpoint("https://gateway.example").unwrap().path(),
            "/v1/execution/connect"
        );
        assert_eq!(bounded("ééééé", 7), "éé…");
    }

    /// Native identity and execution completion stay separate from turn/attention status.
    #[test]
    fn exact_identity_and_durable_report_limits() {
        let id = uuid::Uuid::new_v4().to_string();
        let session = Session {
            workspace_id: id.clone(),
            surface_id: id.clone(),
            session_id: "native".into(),
            project_ident: "project".into(),
            client: "codex".into(),
            model: None,
            cwd: "/repo".into(),
            state: "idle".into(),
        };
        let assignment = Assignment {
            run_id: id.clone(),
            task_id: id.clone(),
            session_key: id.clone(),
            workspace_id: id.clone(),
            surface_id: id,
            session_id: "native".into(),
            project_ident: "project".into(),
            action: "execute".into(),
            prompt: "Task".into(),
        };
        assert!(session.matches(&assignment));
        let mut stale = assignment.clone();
        stale.session_id = "other-native".into();
        assert!(!session.matches(&stale));
        let mut run = Run {
            assignment,
            session,
            phase: "offered".into(),
            status: "assigned".into(),
            ended: false,
            last_sequence: 0,
            reports: Vec::new(),
        };
        assert!(run.report("running", "Premature", None).is_err());
        run.phase = "submitted".into();
        run.status = "needs_attention".into();
        assert!(!run.terminal());
        for index in 1..=16 {
            run.report("waiting_input", "Question", None).unwrap();
            assert_eq!(run.last_sequence, index);
        }
        assert!(run.report("running", "Overflow", None).is_err());
        assert_eq!(run.last_sequence, 16);
        run.ended = true;
        assert!(run.terminal());
    }
}
