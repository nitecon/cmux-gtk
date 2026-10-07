//! Gateway v1.19 lifecycle protocol; bounded native Bearer transport never writes task comments.
use super::model::*;
use futures_util::SinkExt;
use serde_json::{json, Value};
use std::time::Duration;
use tokio_tungstenite::{tungstenite, MaybeTlsStream, WebSocketStream};

pub type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// REST and WebSocket share the validated base address and private bearer credential.
pub struct Client {
    http: reqwest::Client,
    base: url::Url,
    key: String,
}

impl Client {
    /// Reject redirects and use bounded request deadlines so credentials stay on the configured host.
    pub fn new(config: &Config, key: &str) -> Result<Self, String> {
        if key.is_empty() {
            return Err("Enter a gateway API key".into());
        }
        let mut base = endpoint(&config.url)?;
        let scheme = if matches!(base.scheme(), "wss" | "https") {
            "https"
        } else {
            "http"
        };
        base.set_scheme(scheme)
            .map_err(|_| "Invalid gateway scheme")?;
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| "Cannot initialize gateway client")?;
        Ok(Self {
            http,
            base,
            key: key.into(),
        })
    }

    /// Read bounded JSON without retaining raw network errors or API-key text.
    async fn get(&self, segments: &[&str]) -> Result<Value, String> {
        let mut url = self.base.clone();
        url.path_segments_mut()
            .map_err(|_| "Invalid gateway address")?
            .clear()
            .extend(segments);
        let mut response = self
            .http
            .get(url)
            .bearer_auth(&self.key)
            .send()
            .await
            .map_err(|_| "Gateway metadata request failed")?;
        if !response.status().is_success() {
            return Err(format!(
                "Gateway metadata returned HTTP {}",
                response.status().as_u16()
            ));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "Gateway metadata read failed")?
        {
            if bytes.len() + chunk.len() > 1024 * 1024 {
                return Err("Gateway metadata exceeds one MiB".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let mut value =
            serde_json::from_slice(&bytes).map_err(|_| "Invalid gateway metadata JSON")?;
        redact(&mut value, &self.key);
        Ok(value)
    }

    /// Discover canonical repository identities; projects without repository metadata cannot match.
    pub async fn projects(&self) -> Result<Vec<Project>, String> {
        let value = self.get(&["v1", "projects"]).await?;
        let entries = value
            .as_array()
            .filter(|p| p.len() <= 4096)
            .ok_or("Invalid gateway project list")?;
        let mut result = Vec::new();
        let mut ids = std::collections::HashSet::new();
        for entry in entries {
            let ident = entry["ident"].as_str().ok_or("Missing project identity")?;
            identity(ident)?;
            if !ids.insert(ident) {
                return Err("Duplicate gateway project identity".into());
            }
            let remotes = entry["canonical_remote"]
                .as_str()
                .and_then(repository)
                .into_iter()
                .collect();
            result.push(Project {
                ident: ident.into(),
                upstream_urls: remotes,
            });
        }
        Ok(result)
    }

    /// Own one authenticated upgrade with the protocol's frame/message bounds and handshake deadline.
    pub async fn connect(&self, journal: &Journal) -> Result<Socket, String> {
        use tungstenite::client::IntoClientRequest;
        let mut url = self.base.clone();
        url.set_scheme(if self.base.scheme() == "https" {
            "wss"
        } else {
            "ws"
        })
        .map_err(|_| "Invalid stream scheme")?;
        url.set_path("/v1/tasks/stream");
        let mut request = url
            .as_str()
            .into_client_request()
            .map_err(|_| "Invalid gateway upgrade")?;
        request.headers_mut().insert(
            "Authorization",
            format!("Bearer {}", self.key)
                .parse()
                .map_err(|_| "Invalid gateway API key")?,
        );
        let limits = tungstenite::protocol::WebSocketConfig {
            max_message_size: Some(65536),
            max_frame_size: Some(65536),
            ..Default::default()
        };
        let (mut socket, _) = tokio::time::timeout(
            Duration::from_secs(10),
            tokio_tungstenite::connect_async_with_config(request, Some(limits), false),
        )
        .await
        .map_err(|_| "Gateway upgrade timed out")?
        .map_err(|_| "Gateway upgrade rejected or unavailable")?;
        send(
            &mut socket,
            json!({"type":"subscribe", "protocol_version":1,
            "consumer_id":journal.instance_id,"after_event_id":journal.cursor}),
        )
        .await?;
        Ok(socket)
    }

    /// Recover shortened task context before routing; oversized context is explicitly reported as failed.
    pub async fn message(&self, event: &Value) -> Result<Option<Message>, String> {
        let id = event_id(event)?;
        let project = event["project_ident"]
            .as_str()
            .ok_or("Missing event project")?;
        let task_id = event["task"]["id"].as_str().ok_or("Missing event task")?;
        identity(project)?;
        identity(task_id)?;
        let kind = match event["kind"].as_str() {
            Some("task_created") => Kind::Created,
            Some("task_commented") => Kind::Commented,
            Some("task_completed") => Kind::Completed,
            _ => return Err("Unsupported task lifecycle event".into()),
        };
        if kind == Kind::Created
            && event["task"]["kind"] == "delegated"
            && event["task"]["delegated_to_task_id"].is_string()
        {
            return Ok(None);
        }
        let full;
        let task = if event["truncated"] == true {
            full = self
                .get(&["v1", "projects", project, "tasks", task_id])
                .await?;
            if full["id"].as_str() != Some(task_id) {
                return Err("Task detail identity changed".into());
            }
            &full
        } else {
            &event["task"]
        };
        let comment = if kind == Kind::Commented && event["truncated"] == true {
            task["comments"]
                .as_array()
                .and_then(|comments| comments.iter().find(|c| c["id"] == event["comment"]["id"]))
                .ok_or("Full event comment is unavailable")?
        } else {
            &event["comment"]
        };
        let mut message = normalize(id, project, task_id, kind, task, comment)?;
        if let Some(origin) = event.get("origin").filter(|value| !value.is_null()) {
            let origin: Origin =
                serde_json::from_value(origin.clone()).map_err(|_| "Invalid event agent origin")?;
            origin.validate()?;
            if let Some(message) = &mut message {
                message.origin = Some(origin);
            }
        }
        Ok(message)
    }
}

/// Send native JSON with a five-second deadline; callers retain sole socket ownership.
pub async fn send(socket: &mut Socket, value: Value) -> Result<(), String> {
    tokio::time::timeout(
        Duration::from_secs(5),
        socket.send(tungstenite::Message::Text(value.to_string())),
    )
    .await
    .map_err(|_| "Gateway write timed out")?
    .map_err(|_| "Gateway stream write failed".to_owned())
}

/// Lifecycle IDs are positive signed integers; no synthetic identifiers enter the stream journal.
pub fn event_id(event: &Value) -> Result<i64, String> {
    event["id"]
        .as_i64()
        .filter(|id| *id > 0)
        .ok_or_else(|| "Invalid lifecycle event ID".into())
}

/// Redact secret text recursively before journaling; identifiers and bodies use the same rule.
pub fn redact(value: &mut Value, key: &str) {
    if key.is_empty() {
        return;
    }
    match value {
        Value::String(s) => *s = s.replace(key, "[redacted]"),
        Value::Array(a) => a.iter_mut().for_each(|v| redact(v, key)),
        Value::Object(o) => o.values_mut().for_each(|v| redact(v, key)),
        _ => (),
    }
}

/// Render assignments, attributed comments and completion notifications as bounded plain text.
fn normalize(
    id: i64,
    project: &str,
    task_id: &str,
    kind: Kind,
    task: &Value,
    comment: &Value,
) -> Result<Option<Message>, String> {
    let title = task["title"].as_str().ok_or("Missing task title")?;
    let mut text = format!("Task: {title}\n");
    let mut author = None;
    match kind {
        Kind::Created => {
            for (label, field) in [("Description", "description"), ("Specification", "details")] {
                if let Some(value) = task[field].as_str().filter(|s| !s.is_empty()) {
                    text.push_str(&format!("{label}:\n{value}\n"));
                }
            }
        }
        Kind::Commented => {
            let name = comment["author"].as_str().ok_or("Missing comment author")?;
            let role = comment["author_type"]
                .as_str()
                .ok_or("Missing comment author type")?;
            let body = comment["content"]
                .as_str()
                .ok_or("Missing comment content")?;
            author = Some(name.into());
            text.push_str(&format!("Comment from {name} ({role}):\n{body}"));
        }
        Kind::Completed => {
            text.push_str("Completion notification: this task is already done. This event does not assign new work.\n");
            if let Some(body) = comment["content"].as_str() {
                text.push_str(&format!(
                    "Latest task comment (not a guaranteed completion summary):\n{body}"
                ));
            }
        }
    }
    let message = Message {
        event_id: id.to_string(),
        project_ident: project.into(),
        task_id: task_id.into(),
        kind,
        text,
        author_id: author,
        source_instance: None,
        origin: None,
    };
    message.validate().map_err(|_| {
        "Task content is unsafe or exceeds 48 KiB; fetch the full task through agent-tools"
    })?;
    Ok(Some(message))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Preserve all comment author roles, exact delimiters and notification semantics without control bytes.
    #[test]
    fn lifecycle_content_and_redaction() {
        let task = json!({"title":"Inspect", "description":"Context", "details":"Specification"});
        for role in ["user", "agent", "system"] {
            let comment =
                json!({"author":"writer","author_type":role,"content":"Ordinary comment"});
            let message = normalize(1, "project", "task", Kind::Commented, &task, &comment)
                .unwrap()
                .unwrap();
            assert!(message.text.contains(role));
            assert!(message.terminal_text().starts_with(START));
            assert!(message.terminal_text().ends_with(STOP));
        }
        let done = normalize(2, "project", "task", Kind::Completed, &task, &Value::Null)
            .unwrap()
            .unwrap();
        assert!(done.text.contains("already done"));
        assert!(normalize(
            3,
            "project",
            "task",
            Kind::Created,
            &json!({"title":"\u{001b}"}),
            &Value::Null
        )
        .is_err());
        let mut value = json!({"content":"private-secret", "nested":["secret"]});
        redact(&mut value, "secret");
        assert!(!value.to_string().contains("secret"));
    }
}
