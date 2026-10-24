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

    /// Exchange bounded JSON without redirects, raw network errors or API-key text.
    async fn request(
        &self,
        method: reqwest::Method,
        segments: &[&str],
        body: Option<Value>,
    ) -> Result<Value, String> {
        let mut url = self.base.clone();
        url.path_segments_mut()
            .map_err(|_| "Invalid gateway address")?
            .clear()
            .extend(segments);
        let mut request = self.http.request(method, url).bearer_auth(&self.key);
        if let Some(body) = body {
            request = request
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body.to_string());
        }
        let mut response = request
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
        let value = self
            .request(reqwest::Method::GET, &["v1", "projects"], None)
            .await?;
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

    /// Let the gateway derive project identity from a verified Git remote and heal empty mappings.
    /// Existing mappings are preserved by the registration API; callers must reread and match them.
    pub async fn register_repository(&self, remote: &str) -> Result<(), String> {
        let remote_url = format!("https://{remote}");
        if repository(&remote_url).as_deref() != Some(remote) {
            return Err("Invalid local repository identity".into());
        }
        let value = self
            .request(
                reqwest::Method::POST,
                &["v1", "projects"],
                Some(json!({"ident":remote, "repo_url":remote_url})),
            )
            .await?;
        identity(
            value["ident"]
                .as_str()
                .ok_or("Missing registered project identity")?,
        )
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

    /// Normalize a compact task reference without fetching descriptions, specifications or comment bodies.
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
            Some("task_unclaimed") => Kind::Unclaimed,
            _ => return Err("Unsupported task lifecycle event".into()),
        };
        if kind == Kind::Created
            && event["task"]["kind"] == "delegated"
            && event["task"]["delegated_to_task_id"].is_string()
        {
            return Ok(None);
        }
        let mut message = normalize(
            id,
            project,
            task_id,
            kind,
            &event["comment"],
            event["created_at"].as_i64().unwrap_or(0),
        )?;
        if let Some(origin) = event.get("origin").filter(|value| !value.is_null()) {
            let origin: Origin =
                serde_json::from_value(origin.clone()).map_err(|_| "Invalid event agent origin")?;
            origin.validate()?;
            if let Some(message) = &mut message {
                message.origin = Some(origin);
            }
        }
        if let Some(message) = &mut message {
            message.goal = match event["task"].get("goal") {
                None | Some(Value::Null) => None,
                Some(Value::String(value)) => normalize_goal(Some(value))?,
                _ => return Err("Invalid event task goal".into()),
            };
            message.validate()?;
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

/// Keep comment attribution for routing while excluding all task and comment content from input.
fn normalize(
    id: i64,
    project: &str,
    task_id: &str,
    kind: Kind,
    comment: &Value,
    created_at: i64,
) -> Result<Option<Message>, String> {
    let message = Message {
        event_id: id.to_string(),
        project_ident: project.into(),
        task_id: task_id.into(),
        kind,
        created_at,
        text: task_id.into(),
        author_id: comment["author"].as_str().map(str::to_owned),
        source_instance: None,
        origin: None,
        goal: None,
    };
    message.validate()?;
    Ok(Some(message))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decode additive goal metadata without letting malformed types fall back to legacy broadcast.
    #[tokio::test]
    async fn event_goal_is_optional_but_invalid_context_fails_closed() {
        let client = Client::new(
            &Config {
                url: "http://localhost:1234".into(),
                enabled: true,
                injection_approved: true,
            },
            "test-key",
        )
        .unwrap();
        let mut event = json!({"id":1,"project_ident":"project","task":{"id":"task"},"kind":"task_created","created_at":now_millis()});
        assert!(client
            .message(&event)
            .await
            .unwrap()
            .unwrap()
            .goal
            .is_none());
        event["task"]["goal"] = Value::Null;
        assert!(client
            .message(&event)
            .await
            .unwrap()
            .unwrap()
            .goal
            .is_none());
        event["task"]["goal"] = json!(" alpha ");
        assert_eq!(
            client
                .message(&event)
                .await
                .unwrap()
                .unwrap()
                .goal
                .as_deref(),
            Some("alpha")
        );
        for invalid in [
            json!(false),
            json!(["alpha"]),
            json!("bad\ngoal"),
            json!("x".repeat(129)),
        ] {
            event["task"]["goal"] = invalid;
            assert!(client.message(&event).await.is_err());
        }
    }

    /// Task notifications expose only kind and ID, regardless of author role or event body size.
    #[test]
    fn compact_lifecycle_content_and_redaction() {
        for role in ["user", "agent", "system"] {
            let comment = json!({"author":"writer","author_type":role,"content":"Private comment"});
            let message = normalize(
                1,
                "project",
                "task",
                Kind::Commented,
                &comment,
                now_millis(),
            )
            .unwrap()
            .unwrap();
            assert_eq!(
                message.terminal_text(),
                "<Gateway message>New comment on task: task</Gateway message>"
            );
            assert_eq!(message.author_id.as_deref(), Some("writer"));
            assert!(!message.text.contains("Private"));
        }
        for (kind, label) in [
            (Kind::Created, "New delegated task received"),
            (Kind::Completed, "Task completed"),
            (Kind::Unclaimed, "Task became unclaimed"),
        ] {
            let message = normalize(2, "project", "task", kind, &Value::Null, now_millis())
                .unwrap()
                .unwrap();
            assert_eq!(
                message.terminal_text(),
                format!("{START}{label}: task{STOP}")
            );
        }
        assert!(normalize(
            3,
            "project",
            "task<spoof>",
            Kind::Created,
            &Value::Null,
            now_millis()
        )
        .is_err());
        let mut value = json!({"content":"private-secret", "nested":["secret"]});
        redact(&mut value, "secret");
        assert!(!value.to_string().contains("secret"));
    }

    /// Unclaimed snapshots accept optional audit comments and preserve mutation provenance over ownership.
    #[tokio::test]
    async fn unclaimed_event_keeps_compact_content_and_origin() {
        let client = Client::new(
            &Config {
                url: "http://localhost".into(),
                ..Default::default()
            },
            "test-key",
        )
        .unwrap();
        let origin = json!({
            "session_id": uuid::Uuid::new_v4().to_string(),
            "instance_id": uuid::Uuid::new_v4().to_string(),
            "provider": "codex", "os": "linux"
        });
        let mut event = json!({
            "id": 42, "project_ident": "project", "kind": "task_unclaimed",
            "created_at": now_millis(),
            "task": {"id": "task", "status": "todo", "owner_agent_id": null,
                "owner_origin": null, "description": "Private task content"}
        });
        for comment in [
            Value::Null,
            json!({"author": "system", "content": "Private audit"}),
        ] {
            event["comment"] = comment;
            let message = client.message(&event).await.unwrap().unwrap();
            assert_eq!(message.kind, Kind::Unclaimed);
            assert_eq!(message.event_id, "42");
            assert_eq!(message.text, "task");
            assert_eq!(message.origin, None);
            assert_eq!(
                message.terminal_text(),
                "<Gateway message>Task became unclaimed: task</Gateway message>"
            );
        }
        event["origin"] = origin.clone();
        let message = client.message(&event).await.unwrap().unwrap();
        assert_eq!(serde_json::to_value(message.origin.unwrap()).unwrap(), origin);
        // Ordinary creation and reassignment audits retain the caller as the echo-suppression origin.
        event["task"]["owner_origin"] = json!({
            "session_id": uuid::Uuid::new_v4().to_string(),
            "instance_id": uuid::Uuid::new_v4().to_string(),
            "provider": "claude", "os": "windows"
        });
        for kind in ["task_created", "task_commented"] {
            event["kind"] = json!(kind);
            let message = client.message(&event).await.unwrap().unwrap();
            assert_eq!(serde_json::to_value(message.origin.unwrap()).unwrap(), origin);
        }
    }

    /// Five minutes is inclusive; older and legacy undated events cannot be delivered.
    #[test]
    fn event_age_boundary() {
        let now = 1_800_000_000_000;
        assert!(fresh_at(now - MAX_EVENT_AGE_MS, now));
        assert!(!fresh_at(now - MAX_EVENT_AGE_MS - 1, now));
        assert!(!fresh_at(0, now));
    }
}
