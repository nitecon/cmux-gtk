//! One owned lifecycle connection and fsynced delivery queue; GTK alone owns native terminal input.
use super::{model::*, pipeline::Pipeline, storage, wire};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, oneshot, watch};

/// Global preference changes invalidate the owned connection before further delivery.
pub enum Action {
    Actor {
        actor: super::actor::Actor,
        repository: Option<String>,
        process: Option<cmux_platform::process::Identity>,
    },
    Configure {
        config: Config,
        key: Option<String>,
    },
}

/// A bounded settings operation with an explicit persistence result.
pub struct Request {
    pub action: Action,
    pub reply: oneshot::Sender<Result<Value, String>>,
}

/// GTK rechecks the live target and reports actual input separately from canonical task completion.
pub struct Delivery {
    pub message: Box<Message>,
    pub session: Session,
    pub reply: oneshot::Sender<Result<DeliveryOutcome, String>>,
}

/// One socket and its subscription/heartbeat lifetime; dropping it cancels this connection.
struct Connection {
    client: wire::Client,
    socket: wire::Socket,
    subscribed: bool,
    /// Events already present at subscription belong to an offline backlog, never live input.
    replay_through: i64,
    last_input: Instant,
    heartbeat: Instant,
    projects_at: Instant,
    registration_attempts: HashMap<String, Instant>,
    metadata_error: Option<String>,
    sent: HashMap<i64, String>,
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
    goal_registrations: Vec<GoalRegistration>,
}

/// A local registration is useful for routing only while its native executor generation matches.
struct GoalRegistration {
    process: cmux_platform::process::Identity,
    repository: String,
    origin: Origin,
    goal: Option<String>,
}

/// Multiple logical conversations on one executor are ambiguous; never select one by provider or order.
fn selected_goal(
    registrations: &[GoalRegistration],
    process: &cmux_platform::process::Identity,
) -> Option<GoalSelection> {
    let mut matches = registrations.iter().filter(|r| &r.process == process);
    let first = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some(GoalSelection {
        repository: first.repository.clone(),
        goal: first.goal.clone()?,
        origin: first.origin.clone(),
    })
}

impl Worker {
    /// Track owned running processes without any downstream UI, provider protocol or prompt observation.
    async fn sessions(&mut self, terminals: &[Terminal]) -> Vec<Session> {
        let mut sessions = active(terminals, &self.journal.instance_id).await;
        for session in &mut sessions {
            session.selected_goal = selected_goal(&self.goal_registrations, &session.process);
        }
        sessions
    }

    /// Publish bounded outcomes without task bodies, terminal screen contents or credentials.
    fn publish(&self) {
        // Copy only public metadata, not queued payloads or transient terminal grids.
        let receipts = self
            .journal
            .receipts
            .iter()
            .map(|r| Receipt {
                event_id: r.event_id.clone(),
                recipient_session_id: r.recipient().map(str::to_owned),
                outcome: r.outcome.clone(),
                reason: r.reason.clone(),
                confirmed: r.confirmed,
                ..Default::default()
            })
            .collect();
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
            prompt_surfaces: self
                .pipeline
                .pending
                .iter()
                .map(|pending| pending.target.terminal.surface_id.clone())
                .collect(),
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

    /// End live delivery with the connection; retained receipts reconcile without replaying input.
    async fn retire_pending(&mut self) -> Result<(), String> {
        self.pipeline.pending.clear();
        for receipt in &mut self.journal.receipts {
            if matches!(receipt.outcome.as_str(), "received" | "queued") {
                finalize(
                    receipt,
                    "skipped",
                    "Gateway connection ended before delivery; replay disabled",
                );
            }
        }
        self.save().await
    }

    /// Apply explicit global consent and retire pending input on disable, key or endpoint changes.
    async fn apply(&mut self, action: Action) -> Result<Value, String> {
        let (config, key) = match action {
            Action::Configure { config, key } => (config, key),
            Action::Actor {
                actor,
                repository,
                process,
            } => {
                let mut result = actor.announcement(repository.as_deref())?;
                let goal = normalize_goal(actor.goal.as_deref())?;
                result["goal"] = json!(goal);
                self.goal_registrations.retain(|r| r.origin != actor.origin);
                if let (Some(repository), Some(process)) = (repository, process) {
                    if self.goal_registrations.len() >= MAX_PENDING {
                        return Err("Session goal registration capacity reached".into());
                    }
                    self.goal_registrations.push(GoalRegistration {
                        process,
                        repository,
                        origin: actor.origin,
                        goal,
                    });
                    result["goal_routing"] = json!("native_executor_verified");
                } else {
                    result["goal_routing"] = json!("unbound");
                }
                return Ok(result);
            }
        };
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
    async fn receive(
        &mut self,
        mut event: Value,
        terminals: &[Terminal],
        replay_through: i64,
    ) -> Result<(), String> {
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
        } else if id <= replay_through {
            finalize(
                &mut receipt,
                "skipped",
                "Event predates the current gateway connection; replay disabled",
            );
        } else if !fresh_at(
            receipt
                .event
                .as_ref()
                .and_then(|e| e["created_at"].as_i64())
                .unwrap_or(0),
            now_millis(),
        ) {
            finalize(
                &mut receipt,
                "skipped",
                "Gateway event is older than five minutes or has no timestamp",
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
            receipt.candidates = self.sessions(terminals).await;
            for candidate in &mut receipt.candidates {
                candidate.terminal.screen = None;
            }
            if receipt.candidates.is_empty() {
                finalize(
                    &mut receipt,
                    "skipped",
                    "No running local application at event arrival",
                );
            }
        }
        self.journal.receipts.push_back(receipt);
        self.save().await?;
        self.publish();
        Ok(())
    }

    /// Refresh and heal metadata from verified local remotes, with one registration per tick and
    /// a thirty-second retry per repository. Metadata failures retain live input and socket ownership.
    async fn refresh_projects(
        &mut self,
        connection: &mut Connection,
        sessions: &[Session],
    ) -> Result<(), String> {
        if connection.projects_at.elapsed() >= Duration::from_secs(30) {
            connection.projects_at = Instant::now();
            match connection.client.projects().await {
                Ok(projects) => {
                    self.projects = projects;
                    connection.metadata_error = None;
                }
                Err(error) => connection.metadata_error = Some(error),
            }
            heartbeat(connection).await?;
        }
        connection
            .registration_attempts
            .retain(|remote, _| sessions.iter().any(|s| &s.repository == remote));
        if !self.journal.config.injection_approved {
            return Ok(());
        }
        let remote = sessions.iter().map(|s| &s.repository).find(|remote| {
            !self
                .projects
                .iter()
                .any(|p| p.upstream_urls.contains(remote))
                && connection
                    .registration_attempts
                    .get(*remote)
                    .is_none_or(|last| last.elapsed() >= Duration::from_secs(30))
        });
        if let Some(remote) = remote {
            connection
                .registration_attempts
                .insert(remote.clone(), Instant::now());
            heartbeat(connection).await?;
            match connection.client.register_repository(remote).await {
                Ok(()) => {
                    heartbeat(connection).await?;
                    match connection.client.projects().await {
                        Ok(projects) => {
                            self.projects = projects;
                            connection.metadata_error = None;
                        }
                        Err(error) => connection.metadata_error = Some(error),
                    }
                }
                Err(error) => connection.metadata_error = Some(error),
            }
        }
        Ok(())
    }

    /// Retry all fresh received events in arrival order; unresolved metadata never blocks peers.
    async fn route(
        &mut self,
        connection: &mut Connection,
        sessions: &[Session],
    ) -> Result<(), String> {
        let events: Vec<String> = self
            .journal
            .receipts
            .iter()
            .filter(|r| r.outcome == "received")
            .map(|r| r.event_id.clone())
            .collect();
        let mut changed = false;
        for event_id in events {
            let index = self
                .journal
                .receipts
                .iter()
                .position(|r| r.event_id == event_id && r.outcome == "received")
                .ok_or("Received gateway event changed during routing")?;
            let event = self.journal.receipts[index]
                .event
                .as_ref()
                .ok_or("Missing persisted lifecycle event")?;
            let result = connection.client.message(event).await;
            changed |= self.route_received(
                index,
                result,
                sessions,
                connection.metadata_error.as_deref(),
            );
        }
        if changed {
            self.save().await?;
            self.publish();
        }
        Ok(())
    }

    /// Preserve arrival-time attachments while metadata is unresolved; return whether persistence changed.
    fn route_received(
        &mut self,
        index: usize,
        result: Result<Option<Message>, String>,
        sessions: &[Session],
        metadata_error: Option<&str>,
    ) -> bool {
        let received = self.journal.receipts[index].clone();
        if !fresh_at(
            received
                .event
                .as_ref()
                .and_then(|e| e["created_at"].as_i64())
                .unwrap_or(0),
            now_millis(),
        ) {
            finalize(
                &mut self.journal.receipts[index],
                "skipped",
                "Gateway event is older than five minutes or has no timestamp",
            );
            return true;
        }
        match result {
            Ok(Some(message)) => {
                let eligible: Vec<Session> = sessions
                    .iter()
                    .filter(|s| {
                        received.candidates.iter().any(|c| {
                            c.same_attachment(s)
                                && (c.repository.is_empty() || c.repository == s.repository)
                                && (message.goal.is_none() || c.selected_goal == s.selected_goal)
                                && c.actor_origin
                                    .as_ref()
                                    .is_none_or(|origin| s.actor_origin.as_ref() == Some(origin))
                        })
                    })
                    .cloned()
                    .collect();
                let event_remote = received
                    .event
                    .as_ref()
                    .and_then(|e| e["canonical_remote"].as_str())
                    .and_then(repository);
                if !eligible.is_empty()
                    && self
                        .projects
                        .iter()
                        .find(|p| p.ident == message.project_ident)
                        .is_none_or(|p| {
                            p.upstream_urls.is_empty()
                                || event_remote
                                    .as_ref()
                                    .is_some_and(|remote| !p.upstream_urls.contains(remote))
                        })
                {
                    let reason = match metadata_error {
                        Some(error) => format!("Waiting for gateway repository metadata; retrying automatically: {error}"),
                        None => "Waiting for gateway repository metadata; retrying automatically".into(),
                    };
                    if self.journal.receipts[index].reason == reason {
                        return false;
                    }
                    self.journal.receipts[index].reason = reason;
                    self.journal.receipts[index].candidates = eligible;
                    self.journal.receipts[index].confirmed = false;
                    return true;
                }
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
        true
    }

    /// Report why native input is deferred without changing canonical task state.
    async fn observe(&mut self, sessions: &mut [Session]) -> Result<(), String> {
        self.agents = sessions.len();
        for receipt in self
            .journal
            .receipts
            .iter_mut()
            .filter(|r| r.outcome == "queued")
        {
            if let Some(session) = receipt
                .target
                .as_ref()
                .and_then(|target| sessions.iter().find(|s| target.same_target(s)))
            {
                receipt.reason = match session
                    .terminal
                    .screen
                    .as_ref()
                    .map(super::readiness::classify)
                {
                    None => "Waiting for a native prompt snapshot; capture unavailable or pending",
                    Some(InputState::Unknown) => "Native prompt layout is not recognized; waiting",
                    Some(InputState::Unfinished) => "Unfinished native input is preserved; waiting",
                    Some(InputState::Busy) => {
                        "Native agent is busy or showing a menu/dialog; waiting"
                    }
                    Some(InputState::EmptyReady) => "Native prompt is ready; waiting for delivery",
                }
                .into();
            }
        }
        Ok(())
    }

    /// Fsync a submitting fence, then request one GTK operation; lost replies become nonreplayable uncertainty.
    async fn drain_verified(&mut self, sessions: Vec<Session>) -> Result<(), String> {
        let Some((mut pending, ready)) = self.pipeline.next(&self.projects, &sessions) else {
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
                if !fresh_at(pending.message.created_at, now_millis()) {
                    "Gateway event is older than five minutes or has no timestamp"
                } else {
                    "Pinned agent process, repository or terminal changed"
                },
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
        let surface = pending.target.terminal.surface_id.clone();
        let outcome = {
            let (reply, result) = oneshot::channel();
            let delivery = Delivery {
                message: Box::new(pending.message.clone()),
                session: pending.target.clone(),
                reply,
            };
            tokio::time::timeout(Duration::from_secs(5), async {
                self.deliveries
                    .send(delivery)
                    .await
                    .map_err(|_| "uncertain")?;
                match result.await {
                    Ok(Ok(DeliveryOutcome::Injected)) => Ok(DeliveryOutcome::Injected),
                    Ok(Ok(DeliveryOutcome::Deferred)) => Ok(DeliveryOutcome::Deferred),
                    Ok(Ok(DeliveryOutcome::Uncertain)) => Err("uncertain"),
                    Ok(Err(_)) => Err("skipped"),
                    Err(_) => Err("uncertain"),
                }
            })
            .await
            .unwrap_or(Err("uncertain"))
        };
        match outcome {
            Ok(DeliveryOutcome::Injected) => finalize(
                &mut self.journal.receipts[index],
                "injected",
                "Submitted through native terminal input; model receipt is not yet confirmed",
            ),
            Ok(DeliveryOutcome::Deferred) => {
                let receipt = &mut self.journal.receipts[index];
                receipt.outcome = "queued".into();
                receipt.confirmed = false;
                receipt.reason = "Native prompt changed before submission; waiting".into();
                pending.target.terminal.screen = None;
                self.pipeline.pending.push_front(pending);
            }
            Ok(DeliveryOutcome::Uncertain) => {
                unreachable!("uncertain submissions are terminal errors")
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
                let latest = value["latest_event_id"]
                    .as_i64()
                    .filter(|id| *id >= cursor)
                    .ok_or("Invalid subscription tail")?;
                if value["protocol_version"] != 1
                    || value["consumer_id"] != self.journal.instance_id
                    || self.journal.cursor.is_some_and(|c| cursor < c)
                {
                    return Err("Gateway subscription mismatch".into());
                }
                self.journal.cursor = Some(cursor);
                self.save().await?;
                connection.subscribed = true;
                connection.replay_through = latest;
                self.connection = "Connected".into();
                self.publish();
                self.acknowledge(connection).await?;
            }
            Some("event") if connection.subscribed => {
                let id = wire::event_id(&value["event"])?;
                self.receive(value["event"].clone(), terminals, connection.replay_through)
                    .await?;
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
    if let Some(target) = &mut receipt.target {
        target.terminal.screen = None;
    }
}

/// Snapshot actual active foreground agents off GTK, before any slow repository lookup or receipt acknowledgment.
pub(super) async fn active(terminals: &[Terminal], instance_id: &str) -> Vec<Session> {
    if terminals.len() > MAX_PENDING {
        return Vec::new();
    }
    let terminals = terminals.to_vec();
    let instance_id = instance_id.to_owned();
    tokio::task::spawn_blocking(move || {
        terminals
            .into_iter()
            .filter_map(|terminal| {
                let process = cmux_platform::process::input_identity(terminal.foreground_pid)?;
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
) -> Result<Value, String> {
    let mut sessions = active(&terminals, &instance_id).await;
    let metadata = |session: &mut Session| {
        let mut context = session.context(&instance_id);
        context["binding_state"] = json!(if session.actor_origin.is_some() {
            "bound"
        } else {
            "unbound"
        });
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

/// Resolve each directory once per scan. An incomplete scan cannot prove a recipient disappeared.
async fn discover(candidates: Vec<Session>) -> Option<Vec<Session>> {
    if candidates.len() > MAX_PENDING {
        return None;
    }
    let started = Instant::now();
    let mut sessions = Vec::new();
    let mut repositories = HashMap::new();
    for mut session in candidates {
        if started.elapsed() >= Duration::from_secs(5) {
            return None;
        }
        let directory = session.terminal.directory.clone();
        if !repositories.contains_key(&directory) {
            repositories.insert(directory.clone(), upstream(&session.terminal).await);
        }
        if let Some(repository) = repositories.get(&directory).cloned().flatten() {
            session.repository = repository;
            sessions.push(session);
        }
    }
    Some(sessions)
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
) {
    let path = storage::path();
    let loading = path.clone();
    let loaded = tokio::task::spawn_blocking(move || {
        Ok::<_, String>((storage::load(&loading)?, storage::key(&loading)?))
    })
    .await;
    let result=async {
        let (journal,key)=loaded.map_err(|_| "Gateway storage worker stopped")??;
        let pipeline=Pipeline::default();
        let mut worker=Worker {journal,key,path,projects:Vec::new(),pipeline,view:view.clone(),deliveries,
            storage_failed:false,connection:"Disabled".into(),agents:0,goal_registrations:Vec::new()};
        worker.save().await?; worker.publish();
        let mut connection:Option<Connection>=None;
        let mut retry=Instant::now(); let mut tick=tokio::time::interval(Duration::from_millis(500));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                biased;
                request=requests.recv()=>{
                    let Some(request)=request else { return Ok::<_,String>(()); };
                    if matches!(&request.action, Action::Configure { .. }) { connection=None; worker.retire_pending().await?; }
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
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Ping(_)|tokio_tungstenite::tungstenite::Message::Pong(_)))=>{
                            connection.as_mut().unwrap().last_input=Instant::now();
                            Ok(())
                        },
                        _=>Err("Gateway disconnected; reconnecting".into()),
                    };
                    // A transport gap does not invalidate known-unsent, arrival-pinned messages.
                    // Retain only the in-memory queue; expiry/process fences still apply after reconnect.
                    // Restart loading and configuration changes continue to retire pending input.
                    if let Err(error)=result { worker.connection=error; connection=None; retry=Instant::now()+Duration::from_secs(5); }
                    if worker.storage_failed { return Err("Gateway storage failed; delivery paused".into()); }
                    worker.publish();
                }
                _=tick.tick()=>{
                    let current_terminals=snapshots.borrow().clone();
                    worker.sessions(&current_terminals).await;
                    worker.publish();
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
                                worker.projects=projects; connection=Some(Connection {client,socket,subscribed:false,replay_through:0,
                                    last_input:Instant::now(),heartbeat:Instant::now(),projects_at:Instant::now(),
                                    registration_attempts:HashMap::new(),metadata_error:None,sent:HashMap::new()});
                            }
                            Err(error)=>{ worker.connection=error; retry=Instant::now()+Duration::from_secs(5); }
                        }
                    }
                    if let Some(c)=connection.as_mut() {
                        let result=async {
                            if c.last_input.elapsed()>Duration::from_secs(if c.subscribed {60} else {10}) { return Err("Gateway stream timed out; reconnecting".into()); }
                            if !c.subscribed { return Ok(()); }
                            heartbeat(c).await?;
                            let terminals=snapshots.borrow().clone();
                            let candidates=worker.sessions(&terminals).await;
                            let Some(mut sessions)=discover(candidates).await else { return Ok(()); };
                            worker.observe(&mut sessions).await?;
                            worker.refresh_projects(c,&sessions).await?;
                            heartbeat(c).await?;
                            worker.route(c,&sessions).await?;
                            heartbeat(c).await?;
                            // Routing may have taken time: verify foreground generations again before fencing input.
                            let terminals=snapshots.borrow().clone();
                            let current=worker.sessions(&terminals).await;
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

    /// Selection is explicit, repository scoped and tied to one native generation, with no last-writer choice.
    #[test]
    fn goal_registration_never_guesses_an_actor_or_reuses_a_process() {
        let process = cmux_platform::process::Identity {
            pid: 42,
            start_ticks: 7,
            client: "codex".into(),
        };
        let origin = Origin {
            session_id: uuid::Uuid::new_v4().to_string(),
            instance_id: uuid::Uuid::new_v4().to_string(),
            provider: "codex".into(),
            os: "linux".into(),
        };
        let mut registrations = vec![GoalRegistration {
            process: process.clone(),
            repository: "github.com/org/repo".into(),
            origin: origin.clone(),
            goal: Some("alpha".into()),
        }];
        let selected = selected_goal(&registrations, &process).unwrap();
        assert_eq!(selected.goal, "alpha");
        assert_eq!(selected.origin, origin);
        let mut replacement = process.clone();
        replacement.start_ticks += 1;
        assert!(selected_goal(&registrations, &replacement).is_none());
        registrations[0].goal = None;
        assert!(selected_goal(&registrations, &process).is_none());
        registrations[0].goal = Some("alpha".into());
        registrations.push(GoalRegistration {
            process: process.clone(),
            repository: "github.com/org/repo".into(),
            origin: Origin {
                session_id: uuid::Uuid::new_v4().to_string(),
                ..origin
            },
            goal: Some("beta".into()),
        });
        assert!(selected_goal(&registrations, &process).is_none());
    }
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
            composer_active: false,
            input_revision: 0,
            screen: Some(ready_screen()),
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
    async fn fences_before_delivery_and_defers_queue_saturation() {
        let root =
            std::env::temp_dir().join(format!("cmux-gateway-worker-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("gateway.json");
        let (deliveries, mut receiver) = mpsc::channel::<Delivery>(2);
        let (view, view_rx) = watch::channel(View::default());
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
                composer_active: false,
                input_revision: 0,
                screen: Some(ready_screen()),
            },
            process,
            repository: "github.com/org/repo".into(),
            session_id: uuid::Uuid::new_v4().to_string(),
            actor_origin: None,
            selected_goal: None,
        };
        let message = Message {
            event_id: "event".into(),
            project_ident: "project".into(),
            task_id: "task".into(),
            kind: Kind::Created,
            created_at: now_millis(),
            text: "Please inspect the task".into(),
            author_id: None,
            source_instance: None,
            origin: None,
            goal: None,
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
            goal_registrations: Vec::new(),
        };
        // Metadata retries preserve the original process set and never adopt a later peer.
        let received = Receipt {
            event_id: "metadata".into(),
            outcome: "received".into(),
            event: Some(json!({"created_at":now_millis(), "canonical_remote":null})),
            candidates: vec![session.clone()],
            ..Default::default()
        };
        let metadata_message = Message {
            event_id: "metadata".into(),
            ..message.clone()
        };
        let mapped = worker.projects.clone();
        worker.projects[0].upstream_urls.clear();
        worker.journal.receipts.push_back(received.clone());
        assert!(worker.route_received(
            0,
            Ok(Some(metadata_message.clone())),
            std::slice::from_ref(&session),
            Some("Gateway metadata returned HTTP 503")
        ));
        assert_eq!(worker.journal.receipts[0].outcome, "received");
        assert!(worker.pipeline.pending.is_empty());
        assert!(worker.journal.receipts[0].reason.contains("503"));
        assert!(!worker.route_received(
            0,
            Ok(Some(metadata_message.clone())),
            std::slice::from_ref(&session),
            Some("Gateway metadata returned HTTP 503")
        ));
        worker.projects = mapped;
        let mut later = session.clone();
        later.terminal.surface_id = uuid::Uuid::new_v4().to_string();
        later.session_id = uuid::Uuid::new_v4().to_string();
        worker.route_received(
            0,
            Ok(Some(metadata_message.clone())),
            &[session.clone(), later],
            None,
        );
        assert_eq!(worker.pipeline.pending.len(), 1);
        assert_eq!(
            worker.pipeline.pending[0].target.session_id,
            session.session_id
        );
        worker.pipeline.pending.clear();
        worker.journal.receipts.clear();
        // A process replacement ends the wait, even if metadata resolves at the same time.
        worker.journal.receipts.push_back(received.clone());
        let mut replaced = session.clone();
        replaced.process.start_ticks += 1;
        worker.route_received(0, Ok(Some(metadata_message.clone())), &[replaced], None);
        assert_eq!(worker.journal.receipts[0].outcome, "skipped");
        assert!(worker.pipeline.pending.is_empty());
        worker.journal.receipts.clear();
        worker.journal.receipts.push_back(received.clone());
        let mut changed_remote = session.clone();
        changed_remote.repository = "github.com/other/repo".into();
        worker.route_received(
            0,
            Ok(Some(metadata_message.clone())),
            &[changed_remote],
            None,
        );
        assert_eq!(worker.journal.receipts[0].outcome, "skipped");
        assert!(worker.pipeline.pending.is_empty());
        worker.journal.receipts.clear();
        // A repaired mapping cannot resurrect expired input.
        let mut expired = received;
        expired.event = Some(json!({"created_at":now_millis() - MAX_EVENT_AGE_MS - 1}));
        worker.journal.receipts.push_back(expired);
        worker.route_received(
            0,
            Ok(Some(metadata_message)),
            std::slice::from_ref(&session),
            None,
        );
        assert_eq!(worker.journal.receipts[0].outcome, "skipped");
        assert!(worker.pipeline.pending.is_empty());
        worker.journal.receipts.clear();
        worker
            .pipeline
            .admit(
                &mut worker.journal,
                &worker.projects,
                std::slice::from_ref(&session),
                message.clone(),
            )
            .unwrap();
        worker.save().await.unwrap();
        worker.publish();
        assert!(view_rx
            .borrow()
            .prompt_surfaces
            .contains(&session.terminal.surface_id));
        let restarted = storage::load(&path).unwrap();
        assert_eq!(restarted.receipts[0].outcome, "skipped");
        assert!(restarted.receipts[0].payload.is_none());
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
                let delivered = &delivery.message;
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
        worker.publish();
        assert!(view_rx.borrow().prompt_surfaces.is_empty());
        let restored = storage::load(&path).unwrap();
        assert_eq!(restored.receipts[0].outcome, "injected");
        let mut replay = restored;
        let replay_message = Message {
            event_id: "event".into(),
            project_ident: "project".into(),
            task_id: "task".into(),
            kind: Kind::Created,
            created_at: now_millis(),
            text: "Replay".into(),
            author_id: None,
            source_instance: None,
            origin: None,
            goal: None,
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
        // A previously unattributed receipt cannot leak an echo if its exact origin becomes known.
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
            created_at: now_millis(),
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
