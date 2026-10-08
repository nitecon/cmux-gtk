//! One owned lifecycle connection and fsynced delivery queue; GTK alone owns native terminal input.
use super::{
    actor,
    model::*,
    pipeline::{Pending, Pipeline},
    readiness, storage, wire,
};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, oneshot, watch};

/// Global preference changes invalidate the owned connection before further delivery.
pub enum Action {
    Configure { config: Config, key: Option<String> },
}

/// A bounded settings operation with an explicit persistence result.
pub struct Request {
    pub action: Action,
    pub reply: oneshot::Sender<Result<Value, String>>,
}

/// GTK rechecks the live target and reports actual input separately from canonical task completion.
pub struct Delivery {
    pub payload: Payload,
    pub session: Session,
    pub reply: oneshot::Sender<Result<DeliveryOutcome, String>>,
}

/// Enrollment is local housekeeping and never impersonates a gateway task or emits gateway receipts.
pub enum Payload {
    Task(Box<Message>),
    Enrollment(String),
}

/// One socket and its subscription/heartbeat lifetime; dropping it cancels this connection.
struct Connection {
    client: wire::Client,
    socket: wire::Socket,
    subscribed: bool,
    last_input: Instant,
    heartbeat: Instant,
    projects_at: Instant,
    sent: HashMap<i64, String>,
}

/// Readiness requires a stable recognized prompt in two observations after every input change.
struct StablePrompt {
    process: cmux_platform::process::Identity,
    revision: u64,
    frame: String,
    since: Instant,
}

/// Serialized durable state; credentials and captured screen data never enter UI snapshots.
struct Worker {
    journal: Journal,
    path: std::path::PathBuf,
    key: String,
    projects: Vec<Project>,
    pipeline: Pipeline,
    view: watch::Sender<View>,
    deliveries: mpsc::Sender<Delivery>,
    storage_failed: bool,
    connection: String,
    agents: usize,
    prompts: HashMap<String, StablePrompt>,
    actors: std::sync::Arc<std::sync::Mutex<actor::Registry>>,
}

impl Worker {
    /// Publish bounded outcomes without task bodies, terminal screen contents or credentials.
    fn publish(&self) {
        let mut receipts = self.journal.receipts.clone();
        for r in &mut receipts {
            r.payload = None;
            r.event = None;
            r.candidates.clear();
            r.target = None;
        }
        self.view.send_replace(View {
            instance_id: self.journal.instance_id.clone(),
            connection: self.connection.clone(),
            config: self.journal.config.clone(),
            pending: self
                .journal
                .receipts
                .iter()
                .filter(|r| !r.terminal())
                .count(),
            projects: self.projects.len(),
            agents: self.agents,
            receipts,
        });
    }

    /// Persist off GTK; a failed fence pauses the service before any new input.
    async fn save(&mut self) -> Result<(), String> {
        let path = self.path.clone();
        let journal = self.journal.clone();
        let result = tokio::task::spawn_blocking(move || storage::save(&path, &journal))
            .await
            .map_err(|_| "Gateway storage worker stopped".to_owned())
            .and_then(|r| r);
        self.storage_failed |= result.is_err();
        result
    }

    /// Apply explicit global consent and retire pending input on disable, key or endpoint changes.
    async fn apply(&mut self, action: Action) -> Result<Value, String> {
        let Action::Configure { config, key } = action;
        let mut candidate = self.journal.clone();
        let changed_endpoint = config.url != candidate.config.url;
        let clear =
            changed_endpoint || !config.enabled || !config.injection_approved || key.is_some();
        candidate.config = config;
        if changed_endpoint {
            candidate.instance_id = uuid::Uuid::new_v4().to_string();
            candidate.cursor = None;
            candidate.receipts.clear();
        } else if clear {
            for receipt in &mut candidate.receipts {
                if !receipt.terminal() {
                    finalize(
                        receipt,
                        "skipped",
                        "Gateway preferences changed before delivery",
                    );
                }
            }
        }
        let path = self.path.clone();
        let saved = candidate.clone();
        let next_key = key.clone();
        let result = tokio::task::spawn_blocking(move || {
            if let Some(key) = &next_key {
                storage::save_key(&path, key)?;
            }
            storage::save(&path, &saved)
        })
        .await
        .map_err(|_| "Gateway storage worker stopped".to_owned())
        .and_then(|r| r);
        self.storage_failed |= result.is_err();
        result?;
        if let Some(key) = key {
            self.key = key;
        }
        if clear {
            self.pipeline.pending.clear();
            self.prompts.clear();
        }
        self.projects.clear();
        self.journal = candidate;
        self.connection = if self.journal.config.enabled {
            "Connecting"
        } else {
            "Disabled"
        }
        .into();
        self.publish();
        Ok(json!({"saved":true}))
    }

    /// Persist a newly offered event and its arrival-time process identities before acknowledging receipt.
    async fn receive(&mut self, mut event: Value, terminals: &[Terminal]) -> Result<(), String> {
        let id = wire::event_id(&event)?;
        let event_id = id.to_string();
        if self.journal.receipts.iter().any(|r| r.event_id == event_id) {
            for receipt in self
                .journal
                .receipts
                .iter_mut()
                .filter(|r| r.event_id == event_id)
            {
                receipt.confirmed = false;
            }
            return self.save().await;
        }
        // Confirmed old fences can be pruned because the durable cursor prevents historical input replay.
        while self.journal.receipts.len() > MAX_RECEIPTS - MAX_PENDING {
            let Some(index) = self.journal.receipts.iter().position(|r| {
                self.journal
                    .receipts
                    .iter()
                    .filter(|other| other.event_id == r.event_id)
                    .all(|other| other.terminal() && other.confirmed)
            }) else {
                return Err("Gateway receipt capacity reached; delivery paused".into());
            };
            let retired = self.journal.receipts[index].event_id.clone();
            self.journal.receipts.retain(|r| r.event_id != retired);
        }
        wire::redact(&mut event, &self.key);
        let mut receipt = Receipt {
            event_id,
            outcome: "received".into(),
            reason: "Persisted; matching the active agent".into(),
            event: Some(event),
            ..Default::default()
        };
        if receipt
            .event
            .as_ref()
            .is_some_and(|e| e.to_string().len() > 65536)
        {
            finalize(
                &mut receipt,
                "failed",
                "Redacted event exceeds the persistence limit",
            );
        } else if self.journal.cursor.is_some_and(|cursor| id <= cursor) {
            finalize(
                &mut receipt,
                "uncertain",
                "Previously acknowledged event has no retained delivery fence; replay disabled",
            );
        } else if !self.journal.config.injection_approved {
            finalize(
                &mut receipt,
                "skipped",
                "Experimental injection approval is off",
            );
        } else if self
            .journal
            .receipts
            .iter()
            .filter(|r| !r.terminal())
            .count()
            >= MAX_PENDING
        {
            finalize(&mut receipt, "failed", "Gateway message queue is full");
        } else {
            receipt.candidates = active(terminals, &self.journal.instance_id).await;
            if let Ok(actors) = self.actors.lock() {
                for candidate in &mut receipt.candidates {
                    candidate.actor_origin = actors.origin(candidate);
                }
            }
            if receipt.candidates.is_empty() {
                finalize(
                    &mut receipt,
                    "skipped",
                    "No active local Claude/Codex process at event arrival",
                );
            }
        }
        self.journal.receipts.push_back(receipt);
        self.save().await?;
        self.publish();
        Ok(())
    }

    /// Route one durable received event without delaying receipt of events for other projects.
    async fn route(
        &mut self,
        connection: &mut Connection,
        sessions: &[Session],
    ) -> Result<(), String> {
        let Some(index) = self
            .journal
            .receipts
            .iter()
            .position(|r| r.outcome == "received")
        else {
            return Ok(());
        };
        let received = self.journal.receipts[index].clone();
        let event = received
            .event
            .as_ref()
            .ok_or("Missing persisted lifecycle event")?;
        let project = event["project_ident"].as_str();
        let canonical = event["canonical_remote"].as_str().and_then(repository);
        if self
            .projects
            .iter()
            .find(|p| Some(p.ident.as_str()) == project)
            .is_none_or(|p| {
                canonical
                    .as_ref()
                    .is_some_and(|remote| !p.upstream_urls.contains(remote))
            })
        {
            self.projects = connection.client.projects().await?;
        }
        heartbeat(connection).await?;
        let result = connection
            .client
            .message(
                received
                    .event
                    .as_ref()
                    .ok_or("Missing persisted lifecycle event")?,
            )
            .await;
        match result {
            Ok(Some(message)) => {
                let eligible: Vec<Session> = sessions
                    .iter()
                    .filter(|s| {
                        received.candidates.iter().any(|c| {
                            c.same_attachment(s)
                                && c.actor_origin
                                    .as_ref()
                                    .is_none_or(|origin| s.actor_origin.as_ref() == Some(origin))
                        })
                    })
                    .cloned()
                    .collect();
                self.journal.receipts.remove(index);
                if let Err(error) =
                    self.pipeline
                        .admit(&mut self.journal, &self.projects, &eligible, message)
                {
                    let mut failed = received;
                    finalize(&mut failed, "failed", &error);
                    self.journal.receipts.push_back(failed);
                }
            }
            Ok(None) => finalize(
                &mut self.journal.receipts[index],
                "skipped",
                "Outgoing delegated tracking ticket; target task owns delivery",
            ),
            Err(error) => finalize(&mut self.journal.receipts[index], "failed", &error),
        }
        self.save().await?;
        self.publish();
        Ok(())
    }

    /// Attach a fresh readiness observation only after native layout, process generation and input stay stable.
    async fn observe(&mut self, sessions: &mut [Session]) -> Result<(), String> {
        let known = self.actors.lock().map(|a| a.actors()).unwrap_or_default();
        let dead = tokio::task::spawn_blocking(move || {
            known.into_iter().filter(|a| !a.live()).collect::<Vec<_>>()
        })
        .await
        .unwrap_or_default();
        let mut pinned = false;
        if let Ok(mut actors) = self.actors.lock() {
            actors.retire(&dead);
            actors.retain(sessions);
            for session in sessions.iter_mut() {
                session.actor_origin = actors.origin(session);
            }
            // Pin previously unknown logical actors exactly once after automatic enrollment.
            for pending in &mut self.pipeline.pending {
                if pending.target.actor_origin.is_none() {
                    pending.target.actor_origin = sessions
                        .iter()
                        .find(|s| pending.target.same_target(s))
                        .and_then(|s| s.actor_origin.clone());
                }
            }
            for receipt in self
                .journal
                .receipts
                .iter_mut()
                .filter(|r| r.outcome == "queued")
            {
                if let Some(target) = &mut receipt.target {
                    if target.actor_origin.is_none() {
                        target.actor_origin = sessions
                            .iter()
                            .find(|s| target.same_target(s))
                            .and_then(|s| s.actor_origin.clone());
                        pinned |= target.actor_origin.is_some();
                    }
                }
            }
            for receipt in self
                .journal
                .receipts
                .iter_mut()
                .filter(|r| r.outcome == "received")
            {
                for candidate in &mut receipt.candidates {
                    if candidate.actor_origin.is_none() {
                        candidate.actor_origin = sessions
                            .iter()
                            .find(|s| candidate.same_attachment(s))
                            .and_then(|s| s.actor_origin.clone());
                        pinned |= candidate.actor_origin.is_some();
                    }
                }
            }
        }
        // Persist first actor attribution even when its prompt remains busy; a restart cannot lose the fence.
        if pinned {
            self.save().await?;
        }
        self.agents = sessions.len();
        let mut live = HashSet::new();
        for session in sessions.iter_mut() {
            let terminal = &mut session.terminal;
            let surface = terminal.surface_id.clone();
            live.insert(surface.clone());
            let input = terminal
                .screen
                .as_ref()
                .map(|f| readiness::classify(&session.process.client, f))
                .unwrap_or_default();
            if input != InputState::EmptyReady || terminal.input_pending {
                self.prompts.remove(&surface);
                continue;
            }
            let grid = terminal.screen.as_ref().unwrap();
            let frame = json!({"cursor":grid["cursor"],"rows":grid["rows"],"columns":grid["columns"],
                "row_spans":grid["row_spans"],"styles":grid["styles"],"active_screen":grid["active_screen"]}).to_string();
            let prompt = self.prompts.entry(surface).or_insert_with(|| StablePrompt {
                process: session.process.clone(),
                revision: terminal.input_revision,
                frame: frame.clone(),
                since: Instant::now(),
            });
            if prompt.process != session.process
                || prompt.revision != terminal.input_revision
                || prompt.frame != frame
            {
                *prompt = StablePrompt {
                    process: session.process.clone(),
                    revision: terminal.input_revision,
                    frame,
                    since: Instant::now(),
                };
            }
            if prompt.since.elapsed() >= Duration::from_millis(500) {
                terminal.observation = Some(Observation {
                    process: session.process.clone(),
                    input_revision: terminal.input_revision,
                    input,
                    observed_at: terminal.captured_at.unwrap_or_else(Instant::now),
                });
            }
        }
        self.prompts.retain(|surface, _| live.contains(surface));
        for receipt in self
            .journal
            .receipts
            .iter_mut()
            .filter(|r| r.outcome == "queued")
        {
            if let Some(current) = receipt
                .target
                .as_ref()
                .and_then(|target| sessions.iter().find(|s| target.same_target(s)))
            {
                receipt.reason = if current.terminal.input_pending {
                    "Clipboard input is pending; waiting"
                } else {
                    match current.terminal.screen.as_ref().map(|f| readiness::classify(&current.process.client, f)).unwrap_or_default() {
                        InputState::Unknown => "Agent prompt layout is unrecognized; waiting for an editable empty prompt",
                        InputState::Busy => "Agent is busy or awaiting permission; waiting",
                        InputState::Unfinished => "User input is unfinished; waiting",
                        InputState::EmptyReady => "Waiting for a stable empty agent prompt",
                    }
                }.into();
            }
        }
        Ok(())
    }

    /// Fsync a submitting fence, then request one GTK operation; lost replies become nonreplayable uncertainty.
    async fn drain_verified(&mut self, sessions: Vec<Session>) -> Result<(), String> {
        if self.journal.config.enabled && self.journal.config.injection_approved {
            // One safe native input per tick. Enrollment precedes tasks and creates no model turn.
            for session in &sessions {
                let text = self
                    .actors
                    .lock()
                    .map_err(|_| "Agent identity state unavailable")?
                    .enrollment(session);
                if let Some(text) = text {
                    let (reply, result) = oneshot::channel();
                    let delivery = Delivery {
                        payload: Payload::Enrollment(text),
                        session: session.clone(),
                        reply,
                    };
                    let _ = tokio::time::timeout(Duration::from_secs(2), async {
                        if self.deliveries.send(delivery).await.is_ok() {
                            let _ = result.await;
                        }
                    })
                    .await;
                    self.prompts.remove(&session.terminal.surface_id);
                    return Ok(());
                }
            }
        }
        let mut available = sessions;
        {
            let actors = self
                .actors
                .lock()
                .map_err(|_| "Agent identity state unavailable")?;
            for session in &mut available {
                if actors.waiting(session) {
                    session.terminal.observation = None;
                }
            }
        }
        let Some((pending, ready)) = self.pipeline.next(&self.projects, &available) else {
            return Ok(());
        };
        let index = self
            .journal
            .receipts
            .iter()
            .position(|r| {
                r.event_id == pending.message.event_id
                    && r.recipient() == Some(pending.target.session_id.as_str())
            })
            .ok_or("Missing delivery fence")?;
        if !ready {
            finalize(
                &mut self.journal.receipts[index],
                "skipped",
                "Pinned agent process, repository or terminal changed",
            );
            self.save().await?;
            self.publish();
            return Ok(());
        }
        if pending
            .message
            .origin
            .as_ref()
            .is_some_and(|origin| pending.target.is_origin(origin, &self.journal.instance_id))
        {
            finalize(
                &mut self.journal.receipts[index],
                "skipped",
                "Event originated in this exact agent session",
            );
            self.save().await?;
            self.publish();
            return Ok(());
        }
        self.journal.receipts[index].outcome = "submitting".into();
        self.journal.receipts[index].confirmed = false;
        self.save().await?;
        let (reply, result) = oneshot::channel();
        let surface = pending.target.terminal.surface_id.clone();
        let delivery = Delivery {
            payload: Payload::Task(Box::new(pending.message.clone())),
            session: pending.target.clone(),
            reply,
        };
        let outcome = tokio::time::timeout(Duration::from_secs(2), async {
            self.deliveries
                .send(delivery)
                .await
                .map_err(|_| "uncertain")?;
            match result.await {
                Ok(Ok(DeliveryOutcome::Injected)) => Ok(DeliveryOutcome::Injected),
                Ok(Ok(DeliveryOutcome::Deferred)) => Ok(DeliveryOutcome::Deferred),
                Ok(Err(_)) => Err("skipped"),
                Err(_) => Err("uncertain"),
            }
        })
        .await
        .unwrap_or(Err("uncertain"));
        match outcome {
            Ok(DeliveryOutcome::Injected) => finalize(
                &mut self.journal.receipts[index],
                "injected",
                "Submitted to the active agent terminal",
            ),
            Ok(DeliveryOutcome::Deferred) => {
                let receipt = &mut self.journal.receipts[index];
                receipt.outcome = "queued".into();
                receipt.confirmed = false;
                receipt.reason =
                    "Input changed before submission; waiting for an empty prompt".into();
                self.pipeline.pending.push_front(pending);
            }
            Err(status) => finalize(
                &mut self.journal.receipts[index],
                status,
                if status == "uncertain" {
                    "Terminal submission could not be confirmed; automatic replay is disabled"
                } else {
                    "Agent terminal changed before final delivery"
                },
            ),
        }
        self.prompts.remove(&surface);
        self.save().await?;
        self.publish();
        crate::diagnostics::record(
            "gateway.message.delivery",
            json!({"surface_id":surface,"outcome":self.journal.receipts[index].outcome}),
        );
        Ok(())
    }

    /// Send changed delivery receipts once per connection; recorded confirms the gateway's actual saved state.
    async fn acknowledge(&mut self, connection: &mut Connection) -> Result<(), String> {
        let mut visited = HashSet::new();
        for receipt in &self.journal.receipts {
            heartbeat(connection).await?;
            if !visited.insert(&receipt.event_id) {
                continue;
            }
            let Ok(id) = receipt.event_id.parse::<i64>() else {
                continue;
            };
            let group: Vec<&Receipt> = self
                .journal
                .receipts
                .iter()
                .filter(|r| r.event_id == receipt.event_id)
                .collect();
            let (status, summary, signature) = receipt_report(&group);
            // One outstanding ACK per event makes a status-only recorded frame identify its exact snapshot.
            if group.iter().all(|r| r.confirmed) || connection.sent.contains_key(&id) {
                continue;
            }
            wire::send(&mut connection.socket,json!({"type":"ack","event_id":id,"status":status,
                "workspace_id":if group.len()==1 {receipt.target.as_ref().map(|s| &s.terminal.workspace_id)} else {None},
                "surface_id":if group.len()==1 {receipt.target.as_ref().map(|s| &s.terminal.surface_id)} else {None},
                "message":if group.len()==1 {receipt.reason.as_str()} else {"Broadcast delivery to pinned local agent sessions"},"summary":summary})).await?;
            connection.sent.insert(id, signature);
        }
        Ok(())
    }

    /// Validate server state before handling events; cursors advance only on subscribed or recorded frames.
    async fn incoming(
        &mut self,
        connection: &mut Connection,
        value: Value,
        terminals: &[Terminal],
    ) -> Result<(), String> {
        match value["type"].as_str() {
            Some("subscribed") if !connection.subscribed => {
                let cursor = value["cursor"]
                    .as_i64()
                    .filter(|c| *c >= 0)
                    .ok_or("Invalid subscribed cursor")?;
                if value["protocol_version"] != 1
                    || value["consumer_id"] != self.journal.instance_id
                    || self.journal.cursor.is_some_and(|c| cursor < c)
                {
                    return Err("Gateway subscription mismatch".into());
                }
                self.journal.cursor = Some(cursor);
                self.save().await?;
                connection.subscribed = true;
                self.connection = "Connected".into();
                self.publish();
                self.acknowledge(connection).await?;
            }
            Some("event") if connection.subscribed => {
                let id = wire::event_id(&value["event"])?;
                self.receive(value["event"].clone(), terminals).await?;
                connection.sent.remove(&id);
                self.acknowledge(connection).await?;
            }
            Some("recorded") if connection.subscribed => {
                let id = value["event_id"]
                    .as_i64()
                    .filter(|n| *n > 0)
                    .ok_or("Invalid receipt ID")?;
                let status = value["status"].as_str().ok_or("Invalid delivery receipt")?;
                let event_id = id.to_string();
                let group: Vec<&Receipt> = self
                    .journal
                    .receipts
                    .iter()
                    .filter(|r| r.event_id == event_id)
                    .collect();
                if group.is_empty() {
                    return Err("Unknown gateway receipt".into());
                }
                let (aggregate, _, signature) = receipt_report(&group);
                let sent = connection.sent.remove(&id);
                if aggregate == status {
                    for receipt in self
                        .journal
                        .receipts
                        .iter_mut()
                        .filter(|r| r.event_id == event_id)
                    {
                        receipt.confirmed = sent.as_ref() == Some(&signature);
                    }
                } else if matches!(status, "injected" | "skipped" | "failed" | "uncertain") {
                    for receipt in self
                        .journal
                        .receipts
                        .iter_mut()
                        .filter(|r| r.event_id == event_id)
                    {
                        finalize(
                            receipt,
                            status,
                            "Gateway retained a terminal outcome; automatic replay is disabled",
                        );
                        receipt.confirmed = true;
                    }
                    self.pipeline
                        .pending
                        .retain(|p| p.message.event_id != id.to_string());
                } else if !matches!(status, "received" | "queued") {
                    return Err("Invalid saved delivery status".into());
                }
                self.journal.cursor = Some(self.journal.cursor.unwrap_or(0).max(id));
                self.save().await?;
            }
            Some("heartbeat") if connection.subscribed => {
                wire::send(&mut connection.socket, json!({"type":"heartbeat"})).await?;
                connection.heartbeat = Instant::now();
            }
            Some("heartbeat_ack") if connection.subscribed => (),
            Some("error") => {
                return Err("Gateway rejected the subscription or delivery receipt".into())
            }
            _ => return Err("Unexpected gateway protocol message".into()),
        }
        Ok(())
    }
}

/// Encode bounded recipient detail and an exact acknowledgment snapshot under protocol v1.
fn receipt_report(group: &[&Receipt]) -> (&'static str, Option<String>, String) {
    let status = receipt_status(group);
    let recipients: Vec<Value> = group.iter().filter_map(|r| r.target.as_ref().map(|s|
        json!({"session_id":s.session_id,"surface_id":s.terminal.surface_id,"status":if r.outcome=="submitting" {"queued"} else {&r.outcome}}))).collect();
    let summary = (!recipients.is_empty()).then(|| json!({"recipients":recipients}).to_string());
    let signature = format!("{status}/{summary:?}");
    (status, summary, signature)
}

/// Check between bounded metadata and delivery operations so their deadlines cannot starve the stream heartbeat.
async fn heartbeat(connection: &mut Connection) -> Result<(), String> {
    if connection.heartbeat.elapsed() >= Duration::from_secs(10) {
        wire::send(&mut connection.socket, json!({"type":"heartbeat"})).await?;
        connection.heartbeat = Instant::now();
    }
    Ok(())
}

/// Release message bodies after a terminal outcome while retaining its routing and explanation.
fn finalize(receipt: &mut Receipt, status: &str, reason: &str) {
    receipt.outcome = status.into();
    receipt.reason = reason.into();
    receipt.confirmed = false;
    receipt.payload = None;
    receipt.event = None;
    receipt.candidates.clear();
}

/// Snapshot actual active foreground agents off GTK, before any slow repository lookup or receipt acknowledgment.
pub(super) async fn active(terminals: &[Terminal], instance_id: &str) -> Vec<Session> {
    if terminals.len() > MAX_PENDING {
        return Vec::new();
    }
    if terminals.len() > MAX_PENDING {
        return Vec::new();
    }
    let terminals = terminals.to_vec();
    let instance_id = instance_id.to_owned();
    tokio::task::spawn_blocking(move || {
        terminals
            .into_iter()
            .filter_map(|mut terminal| {
                let process = cmux_platform::process::agent_identity(terminal.foreground_pid)?;
                terminal.screen = None;
                terminal.captured_at = None;
                terminal.observation = None;
                Some(Session::identified(
                    terminal,
                    process,
                    String::new(),
                    &instance_id,
                ))
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}

/// Resolve context independently of the transport queue, without Git reads or screen capture.
pub async fn context(
    terminals: Vec<Terminal>,
    instance_id: String,
    surface_id: Option<String>,
    actors: std::sync::Arc<std::sync::Mutex<actor::Registry>>,
) -> Result<Value, String> {
    let mut sessions = active(&terminals, &instance_id).await;
    let mut actors = actors
        .lock()
        .map_err(|_| "Agent identity state unavailable")?;
    actors.retain(&sessions);
    let metadata = |session: &mut Session| {
        let actor = actors.actor(session);
        session.actor_origin = actor.as_ref().map(|a| a.origin.clone());
        let mut context = session.context(&instance_id);
        context["binding_state"] = json!(if actor.is_some() { "bound" } else { "unbound" });
        if let Some(actor) = actor {
            actor.describe(&mut context);
            context["provider_session_id"] = json!(actor.provider_session_id);
        }
        context
    };
    if let Some(surface_id) = surface_id {
        sessions
            .iter_mut()
            .find(|s| s.terminal.surface_id == surface_id)
            .map(metadata)
            .ok_or_else(|| "No verified active agent on this surface".to_owned())
    } else {
        Ok(json!({"sessions":sessions.iter_mut().map(metadata).collect::<Vec<_>>()}))
    }
}

/// Resolve foreground executable identities and selected Git upstreams within a bounded scan budget.
async fn discover(terminals: &[Terminal], instance_id: &str) -> Vec<Session> {
    if terminals.len() > MAX_PENDING {
        return Vec::new();
    }
    let mut sessions = Vec::new();
    let started = Instant::now();
    for terminal in terminals {
        if started.elapsed() >= Duration::from_secs(5) {
            return Vec::new();
        }
        let pid = terminal.foreground_pid;
        let process =
            tokio::task::spawn_blocking(move || cmux_platform::process::agent_identity(pid))
                .await
                .ok()
                .flatten();
        let Some(process) = process else {
            continue;
        };
        let Some(repository) = upstream(terminal).await else {
            continue;
        };
        sessions.push(Session::identified(
            terminal.clone(),
            process,
            repository,
            instance_id,
        ));
    }
    sessions
}

/// Discover the current branch's selected upstream remote, falling back to origin for untracked branches.
async fn upstream(terminal: &Terminal) -> Option<String> {
    let branch = git_output(
        &terminal.directory,
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
    )
    .await;
    let text = git_output(
        &terminal.directory,
        &[
            "config",
            "--get-regexp",
            "^(branch\\..*\\.remote|remote\\..*\\.url)$",
        ],
    )
    .await?;
    let config: Vec<(&str, &str)> = text
        .lines()
        .filter_map(|line| line.split_once(' '))
        .collect();
    let branch_key = branch
        .as_ref()
        .map(|b| format!("branch.{}.remote", b.trim()));
    let remote = config
        .iter()
        .find(|(key, _)| Some(*key) == branch_key.as_deref())
        .map(|(_, remote)| *remote)
        .unwrap_or("origin");
    let remote_key = format!("remote.{remote}.url");
    repository(config.iter().find(|(key, _)| *key == remote_key)?.1)
}

/// Run bounded local Git metadata reads without inherited Git overrides, prompts or hooks.
async fn git_output(directory: &std::path::Path, args: &[&str]) -> Option<String> {
    let mut command = tokio::process::Command::new("git");
    command
        .args([
            "--no-optional-locks",
            "-c",
            "core.hooksPath=/dev/null",
            "-C",
        ])
        .arg(directory)
        .args(args);
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    command.env("GIT_TERMINAL_PROMPT", "0");
    let output = crate::task::run_output(
        command,
        Duration::from_secs(2),
        64 * 1024,
        4096,
        cleanup_failed,
    )
    .await
    .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

/// Keep exactly one connection, reconnect after failure and restore queued events without retargeting them.
pub async fn run(
    mut requests: mpsc::Receiver<Request>,
    snapshots: watch::Receiver<Vec<Terminal>>,
    view: watch::Sender<View>,
    deliveries: mpsc::Sender<Delivery>,
    actors: std::sync::Arc<std::sync::Mutex<actor::Registry>>,
) {
    let path = storage::path();
    let loading = path.clone();
    let loaded = tokio::task::spawn_blocking(move || {
        Ok::<_, String>((storage::load(&loading)?, storage::key(&loading)?))
    })
    .await;
    let result=async {
        let (journal,key)=loaded.map_err(|_| "Gateway storage worker stopped")??;
        let mut pipeline=Pipeline::default();
        for receipt in &journal.receipts {
            if receipt.outcome=="queued" { pipeline.pending.push_back(Pending {
                message:receipt.payload.clone().ok_or("Missing queued message")?,target:receipt.target.clone().ok_or("Missing queued recipient")?
            }); }
        }
        let mut worker=Worker {journal,key,path,projects:Vec::new(),pipeline,view:view.clone(),deliveries,
            storage_failed:false,connection:"Disabled".into(),agents:0,prompts:HashMap::new(),actors};
        worker.save().await?; worker.publish();
        let mut connection:Option<Connection>=None;
        let mut retry=Instant::now(); let mut tick=tokio::time::interval(Duration::from_millis(500));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                biased;
                request=requests.recv()=>{
                    let Some(request)=request else { return Ok::<_,String>(()); };
                    connection=None;
                    let result=worker.apply(request.action).await; let _=request.reply.send(result);
                    if worker.storage_failed { return Err("Gateway storage failed; delivery paused".into()); }
                    retry=Instant::now();
                }
                frame=async { match connection.as_mut() {
                    Some(c)=>c.socket.next().await,
                    None=>std::future::pending().await,
                }}=>{
                    let result=match frame {
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text)))=>{
                            let c=connection.as_mut().unwrap(); c.last_input=Instant::now();
                            match serde_json::from_str(&text) {
                                Ok(value)=>{ let terminals=snapshots.borrow().clone(); worker.incoming(c,value,&terminals).await },
                                Err(_)=>Err("Invalid gateway stream JSON".into())
                            }
                        }
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Ping(_)|tokio_tungstenite::tungstenite::Message::Pong(_)))=>Ok(()),
                        _=>Err("Gateway disconnected; reconnecting".into()),
                    };
                    if let Err(error)=result { worker.connection=error; connection=None; retry=Instant::now()+Duration::from_secs(5); }
                    if worker.storage_failed { return Err("Gateway storage failed; delivery paused".into()); }
                    worker.publish();
                }
                _=tick.tick()=>{
                    if connection.is_none() && worker.journal.config.enabled && Instant::now()>=retry {
                        worker.connection="Connecting".into(); worker.publish();
                        let attempt=async {
                            let client=wire::Client::new(&worker.journal.config,&worker.key)?;
                            let projects=client.projects().await?;
                            let socket=client.connect(&worker.journal).await?;
                            Ok::<_,String>((client,projects,socket))
                        }.await;
                        match attempt {
                            Ok((client,projects,socket))=>{
                                worker.projects=projects; connection=Some(Connection {client,socket,subscribed:false,
                                    last_input:Instant::now(),heartbeat:Instant::now(),projects_at:Instant::now(),sent:HashMap::new()});
                            }
                            Err(error)=>{ worker.connection=error; retry=Instant::now()+Duration::from_secs(5); }
                        }
                    }
                    if let Some(c)=connection.as_mut() {
                        let result=async {
                            if c.last_input.elapsed()>Duration::from_secs(if c.subscribed {60} else {10}) { return Err("Gateway stream timed out; reconnecting".into()); }
                            if !c.subscribed { return Ok(()); }
                            heartbeat(c).await?;
                            if c.projects_at.elapsed()>=Duration::from_secs(30) { worker.projects=c.client.projects().await?; c.projects_at=Instant::now(); }
                            heartbeat(c).await?;
                            let terminals=snapshots.borrow().clone();
                            let mut sessions=discover(&terminals,&worker.journal.instance_id).await; worker.observe(&mut sessions).await?;
                            heartbeat(c).await?;
                            worker.route(c,&sessions).await?;
                            heartbeat(c).await?;
                            // Full-task hydration may have taken time: verify foreground generations again before fencing input.
                            let terminals=snapshots.borrow().clone();
                            let current=active(&terminals,&worker.journal.instance_id).await;
                            sessions.retain(|s| current.iter().any(|now| now.process==s.process && now.terminal.surface_id==s.terminal.surface_id));
                            worker.drain_verified(sessions).await?; worker.acknowledge(c).await?;
                            Ok::<_,String>(())
                        }.await;
                        if let Err(error)=result { worker.connection=error; connection=None; retry=Instant::now()+Duration::from_secs(5); }
                    }
                    if worker.storage_failed { return Err("Gateway storage failed; delivery paused".into()); }
                    worker.publish();
                }
            }
        }
    }.await;
    if let Err(error) = result {
        view.send_modify(|v| v.connection = error);
        while let Some(request) = requests.recv().await {
            let _ = request.reply.send(Err(
                "Gateway delivery paused; restart after repairing storage".into(),
            ));
        }
    }
}

/// Record failed bounded Git child cleanup without credentials or repository paths.
fn cleanup_failed(error: &std::io::Error) {
    crate::diagnostics::event(format_args!("gateway Git child cleanup failed: {error}"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::pipeline::Admission;

    /// An older queued ACK cannot confirm a newer recipient result with the same aggregate status.
    #[test]
    fn recipient_ack_snapshots_distinguish_partial_progress() {
        let mut first = Receipt {
            event_id: "1".into(),
            outcome: "queued".into(),
            ..Default::default()
        };
        let mut second = first.clone();
        let terminal = Terminal {
            workspace_id: uuid::Uuid::new_v4().to_string(),
            surface_id: uuid::Uuid::new_v4().to_string(),
            directory: "/repo".into(),
            foreground_pid: 42,
            input_revision: 0,
            input_pending: false,
            observation: None,
            screen: None,
            captured_at: None,
        };
        let process = cmux_platform::process::Identity {
            pid: 42,
            start_ticks: 10,
            client: "codex".into(),
        };
        first.target = Some(Session::identified(
            terminal.clone(),
            process.clone(),
            String::new(),
            "instance",
        ));
        let mut peer = terminal;
        peer.surface_id = uuid::Uuid::new_v4().to_string();
        second.target = Some(Session::identified(
            peer,
            process,
            String::new(),
            "instance",
        ));
        let before = receipt_report(&[&first, &second]);
        assert_eq!(before.0, "queued");
        first.outcome = "injected".into();
        let after = receipt_report(&[&first, &second]);
        assert_eq!(after.0, "queued");
        assert_ne!(before.2, after.2);
        second.outcome = "uncertain".into();
        assert_eq!(receipt_report(&[&first, &second]).0, "uncertain");
    }

    /// Exercise real persistence and GTK-channel delivery after the platform/discovery boundary.
    #[tokio::test]
    async fn fences_before_delivery_and_defers_input_races() {
        let root =
            std::env::temp_dir().join(format!("cmux-gateway-worker-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("gateway.json");
        let (deliveries, mut receiver) = mpsc::channel::<Delivery>(2);
        let (view, _) = watch::channel(View::default());
        let mut journal = Journal::default();
        journal.config.enabled = true;
        journal.config.injection_approved = true;
        let projects = vec![Project {
            ident: "project".into(),
            upstream_urls: vec!["https://github.com/org/repo".into()],
        }];
        let process = cmux_platform::process::Identity {
            pid: 42,
            start_ticks: 10,
            client: "codex".into(),
        };
        let session = Session {
            terminal: Terminal {
                workspace_id: uuid::Uuid::new_v4().to_string(),
                surface_id: uuid::Uuid::new_v4().to_string(),
                directory: "/repo".into(),
                foreground_pid: 42,
                input_revision: 3,
                input_pending: false,
                screen: None,
                captured_at: None,
                observation: Some(Observation {
                    process: process.clone(),
                    input_revision: 3,
                    input: InputState::EmptyReady,
                    observed_at: std::time::Instant::now(),
                }),
            },
            process,
            repository: "github.com/org/repo".into(),
            session_id: uuid::Uuid::new_v4().to_string(),
            actor_origin: None,
        };
        let message = Message {
            event_id: "event".into(),
            project_ident: "project".into(),
            task_id: "task".into(),
            kind: Kind::Created,
            text: "Please inspect the task".into(),
            author_id: None,
            source_instance: None,
            origin: None,
        };
        let mut worker = Worker {
            journal,
            path: path.clone(),
            key: String::new(),
            projects,
            pipeline: Pipeline::default(),
            view,
            deliveries,
            storage_failed: false,
            connection: "Connected".into(),
            agents: 1,
            prompts: HashMap::new(),
            actors: std::sync::Arc::default(),
        };
        worker
            .pipeline
            .admit(
                &mut worker.journal,
                &worker.projects,
                std::slice::from_ref(&session),
                message.clone(),
            )
            .unwrap();
        let inspecting = path.clone();
        let gtk = tokio::spawn(async move {
            for outcome in [DeliveryOutcome::Deferred, DeliveryOutcome::Injected] {
                let delivery = tokio::time::timeout(Duration::from_secs(5), receiver.recv())
                    .await
                    .unwrap()
                    .unwrap();
                let journal = storage::load(&inspecting).unwrap();
                // load converts submitting to uncertain, proving the fence preceded the GTK request.
                assert_eq!(journal.receipts[0].event_id, "event");
                assert_eq!(journal.receipts[0].outcome, "uncertain");
                let Payload::Task(delivered) = &delivery.payload else {
                    panic!("unexpected enrollment");
                };
                assert_eq!(delivered.terminal_text(), message.terminal_text());
                delivery.reply.send(Ok(outcome)).unwrap();
            }
        });
        worker.drain_verified(vec![session.clone()]).await.unwrap();
        assert_eq!(worker.pipeline.pending.len(), 1);
        assert_eq!(worker.journal.receipts[0].outcome, "queued");
        worker.drain_verified(vec![session.clone()]).await.unwrap();
        gtk.await.unwrap();
        assert!(worker.pipeline.pending.is_empty());
        let restored = storage::load(&path).unwrap();
        assert_eq!(restored.receipts[0].outcome, "injected");
        let mut replay = restored;
        let replay_message = Message {
            event_id: "event".into(),
            project_ident: "project".into(),
            task_id: "task".into(),
            kind: Kind::Created,
            text: "Replay".into(),
            author_id: None,
            source_instance: None,
            origin: None,
        };
        assert_eq!(
            worker
                .pipeline
                .admit(
                    &mut replay,
                    &worker.projects,
                    std::slice::from_ref(&session),
                    replay_message
                )
                .unwrap(),
            Admission::Duplicate
        );
        // Arrival before enrollment cannot leak an echo once the exact logical actor becomes known.
        let origin = Origin {
            session_id: uuid::Uuid::new_v4().to_string(),
            instance_id: uuid::Uuid::new_v4().to_string(),
            provider: "codex".into(),
            os: "linux".into(),
        };
        let late = Message {
            event_id: "late-echo".into(),
            project_ident: "project".into(),
            task_id: "task".into(),
            kind: Kind::Commented,
            text: "Own comment".into(),
            author_id: Some("worker".into()),
            source_instance: None,
            origin: Some(origin.clone()),
        };
        worker
            .pipeline
            .admit(
                &mut worker.journal,
                &worker.projects,
                std::slice::from_ref(&session),
                late,
            )
            .unwrap();
        let mut bound = session;
        bound.actor_origin = Some(origin.clone());
        worker
            .pipeline
            .pending
            .back_mut()
            .unwrap()
            .target
            .actor_origin = Some(origin);
        worker.drain_verified(vec![bound]).await.unwrap();
        assert_eq!(worker.journal.receipts.back().unwrap().outcome, "skipped");
        assert!(worker
            .journal
            .receipts
            .back()
            .unwrap()
            .reason
            .contains("exact agent session"));
        // A journal failure never permits the next queued input to reach GTK.
        worker.path = root.clone();
        assert!(worker.save().await.is_err());
        assert!(worker.storage_failed);
        std::fs::remove_dir_all(root).unwrap();
    }
}
