//! One bounded outbound WebSocket owner; GTK never performs network or journal I/O.
use super::{model::*, storage};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{path::PathBuf, time::Duration};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_tungstenite::tungstenite::{
    client::IntoClientRequest, protocol::WebSocketConfig, Message,
};

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// User decisions and provider reports share an ordered, bounded queue of 32 operations.
pub enum Action {
    Configure {
        enabled: bool,
        url: String,
        key: Option<String>,
    },
    Bind {
        workspace: String,
        project: String,
    },
    Accept {
        run: String,
    },
    Report {
        run: String,
        state: String,
        message: String,
        summary: Option<String>,
    },
    Lifecycle {
        surface: String,
        session: String,
        event: String,
        message: String,
    },
}

/// Results return to the socket/UI caller after durable state is recorded.
pub struct Request {
    pub action: Action,
    pub reply: oneshot::Sender<Result<Value, String>>,
}

/// Only exact, acknowledged assignments may request GTK input; the reply fences submission.
pub enum Event {
    Offered(Assignment),
    Deliver {
        assignment: Assignment,
        reply: oneshot::Sender<Result<(), String>>,
    },
}

/// Own state and credential material exclusively on Tokio, separate from published snapshots.
struct Worker {
    journal: Journal,
    key: String,
    path: PathBuf,
    sessions: Vec<Session>,
    view: watch::Sender<View>,
    events: mpsc::Sender<Event>,
    connection: String,
}

impl Worker {
    /// Publish credential-free metadata without logging prompt or provider output.
    fn publish(&self) {
        self.view.send_replace(View {
            connection: self.connection.clone(),
            config: self.journal.config.clone(),
            runs: self.journal.runs.clone(),
            sessions: self.registered_sessions(),
        });
    }

    /// Persist on a blocking worker before advancing any externally observable state.
    async fn save(&self) -> Result<(), String> {
        let path = self.path.clone();
        let journal = self.journal.clone();
        tokio::task::spawn_blocking(move || storage::save(&path, &journal))
            .await
            .map_err(|_| "Gateway storage worker stopped")?
    }

    /// Build the complete mapped snapshot, retaining ended native identities for unfinished runs.
    fn registered_sessions(&self) -> Vec<Session> {
        let mut sessions: Vec<Session> = self
            .sessions
            .iter()
            .filter_map(|session| {
                let mapping = self
                    .journal
                    .config
                    .mappings
                    .iter()
                    .find(|m| m.workspace_id == session.workspace_id)?;
                let mut session = session.clone();
                session.project_ident = mapping.project_ident.clone();
                Some(session)
            })
            .collect();
        for run in self.journal.runs.iter().filter(|r| !r.terminal()) {
            if !sessions.iter().any(|s| s.matches(&run.assignment)) {
                let mut ended = run.session.clone();
                ended.state = "exited".into();
                sessions.push(ended);
            }
        }
        sessions
    }

    /// Encode versioned complete registration, refusing oversize snapshots rather than dropping sessions.
    fn registration(&self) -> Result<Value, String> {
        let sessions = self.registered_sessions();
        if sessions.len() > MAX_SESSIONS {
            return Err("Too many gateway sessions; reduce workspace mappings".into());
        }
        Ok(
            json!({"type":"register", "protocol_version":1, "instance_id":self.journal.instance_id,
            "sessions":sessions}),
        )
    }

    /// Validate local opt-in and exact live identity; human confirmation never authorizes a stale target.
    fn ready(&self, assignment: &Assignment) -> bool {
        self.journal.config.enabled
            && self
                .registered_sessions()
                .iter()
                .any(|s| s.matches(assignment) && s.state == "idle")
    }

    /// Apply one durable user/hook operation. Connection loss keeps reports queued, never input replayed.
    async fn request(
        &mut self,
        request: Request,
        connected: bool,
    ) -> Result<Option<Value>, String> {
        let previous = self.journal.clone();
        let result = self.apply(request.action, connected).await;
        let result = match result {
            Ok(outgoing) => match self.save().await {
                Ok(()) => Ok(outgoing),
                Err(error) => {
                    self.journal = previous;
                    Err(error)
                }
            },
            Err(error) => {
                self.journal = previous;
                Err(error)
            }
        };
        self.publish();
        match result {
            Ok(outgoing) => {
                let _ = request.reply.send(Ok(json!({"queued":true})));
                Ok(outgoing)
            }
            Err(error) => {
                let _ = request.reply.send(Err(error));
                Ok(None)
            }
        }
    }

    /// Validate configuration, routing and report bounds before changing the journal.
    async fn apply(&mut self, action: Action, connected: bool) -> Result<Option<Value>, String> {
        match action {
            Action::Configure { enabled, url, key } => {
                if enabled || !url.is_empty() {
                    endpoint(&url)?;
                }
                if url != self.journal.config.url && self.journal.runs.iter().any(|r| !r.terminal())
                {
                    return Err("Finish or reconcile active runs before changing gateways".into());
                }
                if let Some(key) = key {
                    storage::validate_key(&key)?;
                    let path = self.path.clone();
                    let saved_key = key.clone();
                    tokio::task::spawn_blocking(move || storage::save_key(&path, &saved_key))
                        .await
                        .map_err(|_| "Credential worker stopped")??;
                    self.key = key;
                }
                self.journal.config.enabled = enabled;
                self.journal.config.url = url;
            }
            Action::Bind { workspace, project } => {
                uuid::Uuid::parse_str(&workspace).map_err(|_| "Invalid workspace UUID")?;
                if !project.is_empty() {
                    identity(&project)?;
                }
                if self
                    .journal
                    .runs
                    .iter()
                    .any(|r| !r.terminal() && r.assignment.workspace_id == workspace)
                {
                    return Err("Cannot remap a workspace with an unfinished assignment".into());
                }
                self.journal
                    .config
                    .mappings
                    .retain(|m| m.workspace_id != workspace);
                if !project.is_empty() {
                    if self.journal.config.mappings.len() >= MAX_SESSIONS {
                        return Err("Gateway mapping limit reached".into());
                    }
                    self.journal.config.mappings.push(Mapping {
                        workspace_id: workspace,
                        project_ident: project,
                    });
                }
            }
            Action::Accept { run } => {
                if !connected {
                    return Err("Gateway disconnected; reconnect before sending".into());
                }
                let index = self
                    .journal
                    .runs
                    .iter()
                    .position(|r| r.assignment.run_id == run)
                    .ok_or("Unknown assignment")?;
                let record = &self.journal.runs[index];
                if record.phase != "offered" || record.terminal() {
                    return Err("Assignment already accepted or delivery uncertain; it will not be sent again".into());
                }
                if !self.ready(&record.assignment) {
                    return Err(
                        "Mapped native agent is busy, exited or changed; wait for its idle hook"
                            .into(),
                    );
                }
                let record = &mut self.journal.runs[index];
                record.phase = "accepting".into();
                return Ok(Some(
                    json!({"type":"accepted", "run_id":run, "session_key":record.assignment.session_key}),
                ));
            }
            Action::Report {
                run,
                state,
                message,
                summary,
            } => {
                let message = self.redact(&message, 4096);
                let summary = summary.map(|s| self.redact(&s, 16384));
                let record = self
                    .journal
                    .runs
                    .iter_mut()
                    .find(|r| r.assignment.run_id == run)
                    .ok_or("Unknown assignment")?;
                record.report(&state, &message, summary.as_deref())?;
                if connected {
                    return Ok(record.pending_report());
                }
            }
            Action::Lifecycle {
                surface,
                session,
                event,
                message,
            } => {
                let message = self.redact(&message, 4096);
                for run in self.journal.runs.iter_mut().filter(|r| {
                    !r.terminal()
                        && r.assignment.surface_id == surface
                        && r.assignment.session_id == session
                        && r.phase == "submitted"
                }) {
                    let (state, text) = match event.as_str() {
                        "prompt" => ("running", "Agent is responding"),
                        "attention" => ("waiting_input", "Agent needs input in the terminal"),
                        "stop" => (
                            "running",
                            "Agent turn ended; task outcome has not been reported",
                        ),
                        "exit" => (
                            "failed",
                            "Native agent session exited before reporting an outcome",
                        ),
                        _ => continue,
                    };
                    run.report(
                        state,
                        if message.is_empty() { text } else { &message },
                        None,
                    )?;
                }
                if connected {
                    return Ok(self
                        .journal
                        .runs
                        .iter()
                        .find(|r| {
                            r.assignment.surface_id == surface
                                && r.assignment.session_id == session
                                && !r.terminal()
                        })
                        .and_then(Run::pending_report));
                }
            }
        }
        Ok(None)
    }

    /// Remove the shared bearer before retaining any remote/user text.
    fn redact(&self, text: &str, limit: usize) -> String {
        bounded(
            &if self.key.is_empty() {
                text.to_owned()
            } else {
                text.replace(&self.key, "[redacted]")
            },
            limit,
        )
    }

    /// Reconcile server sequences and terminal state using bounded authenticated REST metadata.
    async fn reconcile(&mut self, registered: &Value) -> Result<(), String> {
        if registered["protocol_version"] != 1 {
            return Err("Unsupported gateway protocol".into());
        }
        let sessions = registered["sessions"]
            .as_array()
            .ok_or("Invalid registered snapshot")?;
        let mut projects = std::collections::BTreeSet::new();
        for session in sessions {
            if let Some(run) = session["run_id"].as_str() {
                let record = self
                    .journal
                    .runs
                    .iter()
                    .find(|r| r.assignment.run_id == run)
                    .ok_or("Unrecognized active run; manual reconciliation required")?;
                if session["session_key"] != record.assignment.session_key
                    || session["surface_id"] != record.assignment.surface_id
                    || session["session_id"] != record.assignment.session_id
                {
                    return Err("Gateway reconnect session identity mismatch".into());
                }
                projects.insert(record.assignment.project_ident.clone());
            }
        }
        projects.extend(
            self.journal
                .runs
                .iter()
                .filter(|r| !r.terminal())
                .map(|r| r.assignment.project_ident.clone()),
        );
        for project in projects {
            let mut url = endpoint(&self.journal.config.url)?;
            let scheme = if url.scheme() == "wss" {
                "https"
            } else {
                "http"
            };
            url.set_scheme(scheme).map_err(|_| "Invalid REST scheme")?;
            url.set_path("/v1/projects/");
            url.path_segments_mut()
                .map_err(|_| "Invalid REST URL")?
                .pop_if_empty()
                .push(&project)
                .push("execution")
                .push("runs");
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|_| "Cannot create reconciliation client")?;
            let mut response = client
                .get(url)
                .bearer_auth(&self.key)
                .send()
                .await
                .map_err(|_| "Gateway reconciliation request failed")?;
            if !response.status().is_success() {
                return Err("Gateway reconciliation rejected; delivery paused".into());
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| "Gateway reconciliation body failed")?
            {
                if bytes.len() + chunk.len() > 1024 * 1024 {
                    return Err("Gateway reconciliation exceeds limit".into());
                }
                bytes.extend_from_slice(&chunk);
            }
            let records: Vec<Value> =
                serde_json::from_slice(&bytes).map_err(|_| "Invalid reconciliation metadata")?;
            for run in self
                .journal
                .runs
                .iter_mut()
                .filter(|r| !r.terminal() && r.assignment.project_ident == project)
            {
                let record = records
                    .iter()
                    .find(|r| r["id"] == run.assignment.run_id)
                    .ok_or("Run absent from gateway history; manual reconciliation required")?;
                if record["session_key"] != run.assignment.session_key {
                    return Err("Reconciliation run ownership mismatch".into());
                }
                let sequence = record["last_sequence"]
                    .as_i64()
                    .filter(|s| *s >= 0)
                    .ok_or("Invalid server report sequence")?;
                run.last_sequence = run.last_sequence.max(sequence);
                run.reports.retain(|r| r.sequence > sequence);
                run.status = record["status"]
                    .as_str()
                    .ok_or("Invalid server run status")?
                    .into();
                run.ended = !record["finished_at"].is_null();
                if matches!(run.phase.as_str(), "accepting" | "submitting") {
                    run.phase = "uncertain".into();
                }
            }
        }
        self.save().await
    }

    /// Admit remote messages; every delivery marker is durable before GTK receives a prompt.
    async fn receive(&mut self, value: Value, socket: &mut Socket) -> Result<(), String> {
        match value["type"]
            .as_str()
            .ok_or("Invalid gateway message type")?
        {
            "registered" => {
                if self.connection != "Connected" {
                    self.reconcile(&value).await?;
                } else if value["protocol_version"] != 1 {
                    return Err("Unsupported gateway protocol".into());
                }
                self.connection = "Connected".into();
                for run in &self.journal.runs {
                    if !run.terminal() {
                        if let Some(report) = run.pending_report() {
                            send(socket, report).await?;
                        }
                    }
                }
            }
            "assignment" => {
                let mut assignment: Assignment =
                    serde_json::from_value(value).map_err(|_| "Invalid gateway assignment")?;
                assignment.validate()?;
                assignment.prompt = self.redact(&assignment.prompt, 48 * 1024);
                if self
                    .journal
                    .runs
                    .iter()
                    .any(|r| r.assignment.run_id == assignment.run_id)
                {
                    return Ok(());
                }
                let session = self
                    .registered_sessions()
                    .into_iter()
                    .find(|s| s.matches(&assignment) && s.state != "exited")
                    .ok_or("Assignment addressed to an unknown session")?;
                if self
                    .journal
                    .runs
                    .iter()
                    .any(|r| !r.terminal() && r.assignment.surface_id == assignment.surface_id)
                {
                    return Err("A surface already owns an unfinished assignment".into());
                }
                if self.journal.runs.iter().filter(|r| !r.terminal()).count() >= MAX_SESSIONS {
                    return Err("Too many unfinished assignments".into());
                }
                while self.journal.runs.len() >= 128 {
                    let Some(index) = self.journal.runs.iter().position(Run::terminal) else {
                        return Err("Assignment journal full".into());
                    };
                    self.journal.runs.remove(index);
                }
                if self.journal.runs.len() >= 128 {
                    return Err("Assignment journal full".into());
                }
                self.journal.runs.push(Run {
                    assignment: assignment.clone(),
                    session,
                    phase: "offered".into(),
                    status: "assigned".into(),
                    ended: false,
                    last_sequence: 0,
                    reports: Vec::new(),
                });
                self.save().await?;
                self.events
                    .send(Event::Offered(assignment))
                    .await
                    .map_err(|_| "GTK gateway channel closed")?;
            }
            "accepted" => {
                let id = value["run_id"].as_str().ok_or("Invalid acceptance")?;
                let status = value["status"]
                    .as_str()
                    .ok_or("Invalid acceptance status")?;
                let index = self
                    .journal
                    .runs
                    .iter()
                    .position(|r| r.assignment.run_id == id)
                    .ok_or("Unknown acceptance run")?;
                self.journal.runs[index].status = status.into();
                if self.journal.runs[index].terminal() {
                    self.save().await?;
                    return Ok(());
                }
                // Only the current connection's user-initiated acceptance may deliver. Duplicates are inert.
                if self.journal.runs[index].phase != "accepting" {
                    return Ok(());
                }
                if status != "running" || !self.ready(&self.journal.runs[index].assignment) {
                    self.journal.runs[index].phase = "uncertain".into();
                    self.save().await?;
                    return Err(
                        "Agent became busy or gateway requires reconciliation; no prompt sent"
                            .into(),
                    );
                }
                self.journal.runs[index].phase = "submitting".into();
                self.save().await?;
                let assignment = self.journal.runs[index].assignment.clone();
                let (reply, result) = oneshot::channel();
                tokio::time::timeout(
                    Duration::from_secs(5),
                    self.events.send(Event::Deliver { assignment, reply }),
                )
                .await
                .map_err(|_| "GTK gateway delivery queue timed out")?
                .map_err(|_| "GTK gateway channel closed")?;
                let delivered = tokio::time::timeout(Duration::from_secs(5), result).await;
                self.journal.runs[index].phase = if matches!(delivered, Ok(Ok(Ok(())))) {
                    "submitted"
                } else {
                    "uncertain"
                }
                .into();
                self.save().await?;
            }
            "recorded" => {
                let id = value["run_id"]
                    .as_str()
                    .ok_or("Invalid report acknowledgment")?;
                let sequence = value["sequence"]
                    .as_i64()
                    .filter(|s| *s >= 0)
                    .ok_or("Invalid report acknowledgment sequence")?;
                let run = self
                    .journal
                    .runs
                    .iter_mut()
                    .find(|r| r.assignment.run_id == id)
                    .ok_or("Unknown report run")?;
                if run.reports.iter().any(|r| {
                    r.sequence <= sequence && matches!(r.state.as_str(), "finished" | "failed")
                }) {
                    run.ended = true;
                }
                run.status = value["status"]
                    .as_str()
                    .ok_or("Invalid recorded status")?
                    .into();
                run.last_sequence = run.last_sequence.max(sequence);
                run.reports.retain(|r| r.sequence > sequence);
                let next = if run.terminal() {
                    None
                } else {
                    run.pending_report()
                };
                self.save().await?;
                if let Some(report) = next {
                    send(socket, report).await?;
                }
            }
            "heartbeat" => send(socket, json!({"type":"heartbeat"})).await?,
            "heartbeat_ack" => {}
            "error" => {
                return Err(
                    "Gateway rejected an operation; delivery paused for reconciliation".into(),
                )
            }
            _ => return Err("Unsupported gateway message".into()),
        }
        self.publish();
        Ok(())
    }

    /// Bound upgrade, frames, sends and silence; only one owned connection exists per instance.
    async fn connect(
        &mut self,
        requests: &mut mpsc::Receiver<Request>,
        snapshots: &mut watch::Receiver<Vec<Session>>,
    ) -> Result<(), String> {
        let endpoint = endpoint(&self.journal.config.url)?;
        if self.key.is_empty() {
            return Err("Gateway API key is missing; set it in Preferences".into());
        }
        let mut request = endpoint
            .as_str()
            .into_client_request()
            .map_err(|_| "Invalid gateway connection request")?;
        let mut authorization: tokio_tungstenite::tungstenite::http::HeaderValue =
            format!("Bearer {}", self.key)
                .parse()
                .map_err(|_| "Invalid bearer key")?;
        authorization.set_sensitive(true);
        request.headers_mut().insert("Authorization", authorization);
        let config = WebSocketConfig {
            max_message_size: Some(65536),
            max_frame_size: Some(65536),
            ..Default::default()
        };
        let (mut socket, _) = tokio::time::timeout(
            Duration::from_secs(10),
            tokio_tungstenite::connect_async_with_config(request, Some(config), false),
        )
        .await
        .map_err(|_| "Gateway connection timed out")?
        .map_err(|_| "Gateway unavailable or upgrade rejected")?;
        send(&mut socket, self.registration()?).await?;
        let mut heartbeat = tokio::time::interval(Duration::from_secs(20));
        let mut last_received = tokio::time::Instant::now();
        loop {
            tokio::select! {
                frame = socket.next() => {
                    match frame {
                        Some(Ok(Message::Text(text))) => {
                            last_received = tokio::time::Instant::now();
                            let value = serde_json::from_str(&text).map_err(|_| "Invalid gateway JSON")?;
                            self.receive(value, &mut socket).await?;
                        }
                        Some(Ok(Message::Ping(bytes))) => { socket.send(Message::Pong(bytes)).await.map_err(|_| "Gateway ping failed")?; }
                        Some(Ok(Message::Pong(_))) => {},
                        _ => return Err("Gateway disconnected; accepted prompts will not replay".into()),
                    }
                }
                request = requests.recv() => {
                    let Some(request) = request else { return Ok(()); };
                    let reconnect = matches!(request.action, Action::Configure{..});
                    let register = matches!(request.action, Action::Bind{..});
                    if let Some(value) = self.request(request, self.connection == "Connected").await? { send(&mut socket, value).await?; }
                    if reconnect { let _ = socket.close(None).await; return Ok(()); }
                    if register { send(&mut socket, self.registration()?).await?; }
                }
                changed = snapshots.changed() => {
                    if changed.is_err() { return Ok(()); }
                    self.sessions = snapshots.borrow_and_update().clone();
                    send(&mut socket, self.registration()?).await?;
                    self.publish();
                }
                _ = heartbeat.tick() => {
                    if last_received.elapsed() > Duration::from_secs(60) { return Err("Gateway heartbeat expired".into()); }
                    send(&mut socket, json!({"type":"heartbeat"})).await?;
                }
            }
        }
    }
}

/// Encode bounded version-one JSON and enforce the server's five-second write deadline.
async fn send(socket: &mut Socket, value: Value) -> Result<(), String> {
    let text = value.to_string();
    if text.len() > 65536 {
        return Err("Gateway message exceeds protocol limit".into());
    }
    tokio::time::timeout(Duration::from_secs(5), socket.send(Message::Text(text)))
        .await
        .map_err(|_| "Gateway send timed out")?
        .map_err(|_| "Gateway send failed".into())
}

/// Load on a worker and keep servicing disabled/offline configuration; errors never disable terminals.
pub async fn run(
    mut requests: mpsc::Receiver<Request>,
    mut snapshots: watch::Receiver<Vec<Session>>,
    view: watch::Sender<View>,
    events: mpsc::Sender<Event>,
) {
    let path = storage::path();
    let loading = path.clone();
    let loaded = tokio::task::spawn_blocking(move || {
        Ok::<_, String>((storage::load(&loading)?, storage::key(&loading)?))
    })
    .await;
    let (journal, key) = match loaded {
        Ok(Ok(state)) => state,
        _ => {
            view.send_replace(View {
                connection: "Cannot load gateway journal; integration paused".into(),
                ..Default::default()
            });
            while let Some(request) = requests.recv().await {
                let _ = request.reply.send(Err(
                    "Repair the gateway journal before enabling delivery".into(),
                ));
            }
            return;
        }
    };
    let mut worker = Worker {
        journal,
        key,
        path,
        sessions: Vec::new(),
        view,
        events,
        connection: "Disabled".into(),
    };
    if worker.save().await.is_err() {
        worker.connection = "Cannot persist gateway identity; integration paused".into();
        worker.publish();
        return;
    }
    loop {
        worker.sessions = snapshots.borrow_and_update().clone();
        if worker.journal.config.enabled {
            worker.connection = "Connecting".into();
            worker.publish();
            let result = worker.connect(&mut requests, &mut snapshots).await;
            // A severed acceptance or submission is uncertain; never reinterpret it as a new offer.
            for run in &mut worker.journal.runs {
                if matches!(run.phase.as_str(), "accepting" | "submitting") {
                    run.phase = "uncertain".into();
                }
            }
            if let Err(error) = worker.save().await {
                worker.connection = error;
                worker.publish();
                return;
            }
            worker.connection = result.err().unwrap_or_else(|| "Disconnected".into());
        } else {
            worker.connection = "Disabled".into();
        }
        worker.publish();
        if requests.is_closed() {
            return;
        }
        tokio::select! {
            request = requests.recv() => {
                let Some(request) = request else { return; };
                let _ = worker.request(request, false).await;
            }
            changed = snapshots.changed() => { if changed.is_err() { return; } }
            _ = tokio::time::sleep(Duration::from_secs(5)), if worker.journal.config.enabled => {},
        }
    }
}
