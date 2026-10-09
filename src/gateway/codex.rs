//! Explicitly owned Codex backend, exact conversation and provider-native queue transport.
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::process::{Child, Command};
use tokio_tungstenite::{
    tungstenite::{client::IntoClientRequest, Message},
    MaybeTlsStream, WebSocketStream,
};

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// The desktop owns this child. Credentials remain in memory and never enter journal/status.
pub(super) struct Backend {
    child: Child,
    pub launcher: cmux_platform::listeners::ProcessIdentity,
    pub terminal: super::model::Terminal,
    pub thread_id: String,
    pub actor_origin: Option<super::model::Origin>,
    pub tui: Option<cmux_platform::process::Identity>,
    endpoint: String,
    token: String,
}

impl Backend {
    /// Start one backend for one pane, without detached jobs, hooks or provider configuration edits.
    pub async fn start(
        terminal: super::model::Terminal,
        launcher: cmux_platform::listeners::ProcessIdentity,
        executable: &Path,
    ) -> Result<Self, String> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .map_err(|_| "Cannot allocate Codex endpoint")?;
        let endpoint = format!(
            "ws://{}",
            listener
                .local_addr()
                .map_err(|_| "Cannot allocate Codex endpoint")?
        );
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let digest = format!("{:x}", Sha256::digest(token.as_bytes()));
        drop(listener);
        let mut command = Command::new(executable);
        command
            .args([
                "app-server",
                "--listen",
                &endpoint,
                "--ws-auth",
                "capability-token",
                "--ws-token-sha256",
                &digest,
            ])
            .current_dir(&terminal.directory)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW, deliberately without breakaway/detachment.
        let child = command
            .spawn()
            .map_err(|error| format!("Cannot start Codex backend: {error}"))?;
        let mut backend = Self {
            child,
            launcher,
            terminal,
            thread_id: String::new(),
            actor_origin: None,
            tui: None,
            endpoint,
            token,
        };
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        let mut socket = loop {
            if backend
                .child
                .try_wait()
                .map_err(|_| "Cannot inspect Codex backend")?
                .is_some()
            {
                return Err("Codex backend exited during startup; Codex 0.161 or newer with WebSocket authentication and queue APIs is required".into());
            }
            match tokio::time::timeout(
                Duration::from_millis(500),
                connect(&backend.endpoint, &backend.token),
            )
            .await
            {
                Ok(Ok(socket)) => break socket,
                _ if tokio::time::Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(100)).await
                }
                _ => return Err("Codex backend startup timed out".into()),
            }
        };
        initialize(&mut socket).await?;
        let started = call(
            &mut socket,
            2,
            "thread/start",
            json!({"cwd":backend.terminal.directory}),
        )
        .await?;
        let id = started["thread"]["id"]
            .as_str()
            .ok_or("Codex did not return a conversation ID")?;
        backend.thread_id = uuid::Uuid::parse_str(id)
            .map_err(|_| "Invalid Codex conversation ID")?
            .to_string();
        Ok(backend)
    }

    /// Inspect only descendants of the known launcher, independent of the terminal's screen or foreground group leader.
    pub fn process(&self) -> Option<cmux_platform::process::Identity> {
        let root = cmux_platform::listeners::identity(self.launcher.pid).ok()?;
        if root != self.launcher {
            return None;
        }
        let tree = cmux_platform::listeners::process_tree(root).ok()?;
        if let Some(tui) = &self.tui {
            return tree
                .iter()
                .any(|p| u64::from(p.pid) == tui.pid && p.start_ticks == tui.start_ticks)
                .then(|| tui.clone());
        }
        let mut candidates = tree
            .into_iter()
            .filter_map(|p| cmux_platform::process::agent_identity(u64::from(p.pid)))
            .filter(|p| p.client == "codex");
        let first = candidates.next()?;
        if candidates.any(|p| p != first) {
            return None;
        }
        Some(first)
    }

    /// Correlate ordinary SDK registration only with this exact known conversation and owned backend child.
    pub fn accepts_actor(&self, actor: &super::actor::Actor, peer_pid: u32) -> bool {
        if actor.origin.provider != "codex" || actor.provider_session_id != self.thread_id {
            return false;
        }
        self.child
            .id()
            .and_then(|pid| cmux_platform::listeners::identity(pid).ok())
            .and_then(|root| cmux_platform::listeners::process_tree(root).ok())
            .is_some_and(|tree| tree.iter().any(|process| process.pid == peer_pid))
    }

    /// Publish only known conversation/process metadata; endpoint and capability are deliberately excluded.
    pub fn context(&self) -> Value {
        json!({"surface_id":self.terminal.surface_id,"workspace_id":self.terminal.workspace_id,
            "thread_id":self.thread_id,"launcher_pid":self.launcher.pid,"launcher_start_ticks":self.launcher.start_ticks,"process_pid":self.tui.as_ref().map(|p|p.pid),
            "process_start_ticks":self.tui.as_ref().map(|p|p.start_ticks),"origin":self.actor_origin})
    }

    /// Return credentials only to the authenticated launcher; never publish them in snapshots.
    pub fn launch_context(&self) -> Value {
        json!({"thread_id":self.thread_id,"endpoint":self.endpoint,"token":self.token})
    }

    /// Retirement follows launcher generation, pane lifetime and backend exit, not screen content.
    pub fn alive(&mut self, terminals: &[super::model::Terminal]) -> bool {
        cmux_platform::listeners::identity(self.launcher.pid)
            .ok()
            .as_ref()
            == Some(&self.launcher)
            && self.tui.as_ref().is_none_or(|tui| {
                cmux_platform::listeners::identity(tui.pid as u32)
                    .is_ok_and(|identity| identity.start_ticks == tui.start_ticks)
            })
            && self.child.try_wait().is_ok_and(|status| status.is_none())
            && terminals.iter().any(|t| {
                t.surface_id == self.terminal.surface_id
                    && t.workspace_id == self.terminal.workspace_id
                    && t.directory == self.terminal.directory
            })
    }

    /// Queue to this exact loaded thread. Lost responses stay uncertain and are never replayed.
    pub async fn queue(
        &self,
        message: &super::model::Message,
        recipient: &str,
    ) -> Result<(), String> {
        message.validate()?;
        let mut socket = connect(&self.endpoint, &self.token).await?;
        initialize(&mut socket).await?;
        let loaded = call(&mut socket, 2, "thread/loaded/list", json!({})).await?;
        let ids = loaded["data"]
            .as_array()
            .ok_or("Invalid Codex loaded-thread response")?;
        if ids.len() != 1
            || ids[0].as_str() != Some(&self.thread_id)
            || !loaded["nextCursor"].is_null()
        {
            return Err("Managed Codex conversation changed; restart with cmux codex".into());
        }
        let client_id = format!("cmux/{}/{recipient}", message.event_id);
        let result = call(
            &mut socket,
            3,
            "thread/queue/add",
            json!({"threadId":self.thread_id,"clientUserMessageId":client_id,
            "input":[{"type":"text","text":message.terminal_text()}]}),
        )
        .await?;
        if result["queuedSubmission"]["clientUserMessageId"] != client_id
            || result["queuedSubmission"]["id"]
                .as_str()
                .is_none_or(str::is_empty)
        {
            return Err("Codex queue returned an invalid receipt".into());
        }
        Ok(())
    }
}

/// Connect only to the desktop-owned loopback endpoint with a per-launch bearer capability.
async fn connect(endpoint: &str, token: &str) -> Result<Socket, String> {
    let mut request = endpoint
        .into_client_request()
        .map_err(|_| "Invalid Codex endpoint")?;
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {token}")
            .parse()
            .map_err(|_| "Invalid Codex capability")?,
    );
    let config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig {
        max_message_size: Some(4 * 1024 * 1024),
        max_frame_size: Some(4 * 1024 * 1024),
        ..Default::default()
    };
    tokio_tungstenite::connect_async_with_config(request, Some(config), false)
        .await
        .map(|(socket, _)| socket)
        .map_err(|_| "Codex backend connection failed".to_owned())
}

/// Experimental queue endpoints use the official app-server initialization handshake.
async fn initialize(socket: &mut Socket) -> Result<(), String> {
    call(socket, 1, "initialize", json!({"clientInfo":{"name":"cmux","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}})).await?;
    socket
        .send(Message::Text(json!({"method":"initialized"}).to_string()))
        .await
        .map_err(|_| "Codex initialization failed".to_owned())
}

/// A bounded request accepts only its own response; notifications never count as acceptance.
async fn call(socket: &mut Socket, id: u64, method: &str, params: Value) -> Result<Value, String> {
    tokio::time::timeout(Duration::from_secs(10), async {
        socket
            .send(Message::Text(
                json!({"id":id,"method":method,"params":params}).to_string(),
            ))
            .await
            .map_err(|_| "Codex request write failed")?;
        while let Some(frame) = socket.next().await {
            match frame.map_err(|_| "Codex response lost")? {
                Message::Text(text) => {
                    let value: Value =
                        serde_json::from_str(&text).map_err(|_| "Invalid Codex response")?;
                    if value["id"] != id || value.get("method").is_some() {
                        continue;
                    }
                    if value.get("error").is_some() {
                        return Err(
                            "Codex rejected the request; inspect provider configuration/version"
                                .into(),
                        );
                    }
                    return value
                        .get("result")
                        .cloned()
                        .ok_or_else(|| "Missing Codex result".into());
                }
                Message::Ping(data) => socket
                    .send(Message::Pong(data))
                    .await
                    .map_err(|_| "Codex connection lost")?,
                Message::Close(_) => break,
                _ => (),
            }
        }
        Err("Codex response lost".into())
    })
    .await
    .unwrap_or_else(|_| Err("Codex request timed out".into()))
}
