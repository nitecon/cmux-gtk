//! One owned asynchronous gateway service. Wire transport awaits the lifecycle-stream specification.
use super::{model::*, pipeline::Pipeline, storage};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, watch};

/// Settings and normalized stream ingress share one serialized owner; no per-project execution operations.
#[allow(dead_code)] // Projects/Message are connected by the forthcoming authenticated stream adapter.
pub enum Action {
    Configure { config: Config, key: Option<String> },
    Projects(Vec<Project>),
    Message(Message),
}

/// A bounded operation with an explicit result; stream acknowledgments will follow this local outcome.
pub struct Request {
    pub action: Action,
    pub reply: oneshot::Sender<Result<Value, String>>,
}

/// GTK performs the final live-target check and reports actual input separately from task completion.
pub struct Delivery {
    pub message: Message,
    pub session: Session,
    pub reply: oneshot::Sender<Result<DeliveryOutcome, String>>,
}

/// Owned state; credentials never enter a UI snapshot or terminal event body.
struct Worker {
    journal: Journal,
    path: std::path::PathBuf,
    key: String,
    projects: Vec<Project>,
    pipeline: Pipeline,
    view: watch::Sender<View>,
    deliveries: mpsc::Sender<Delivery>,
    storage_failed: bool,
}

impl Worker {
    /// Publish local status honestly while no wire endpoint has been agreed.
    fn publish(&self) {
        self.view.send_replace(View {
            connection: if self.journal.config.enabled {
                "Awaiting gateway stream specification"
            } else {
                "Disabled"
            }
            .into(),
            config: self.journal.config.clone(),
            pending: self.pipeline.pending.len(),
            receipts: self.journal.receipts.clone(),
        });
    }

    /// Persist on a blocking worker; failure stops the owner before terminal delivery.
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

    /// Apply settings or normalized events; no invented wire protocol or legacy execution connection.
    async fn apply(&mut self, action: Action, terminals: &[Terminal]) -> Result<Value, String> {
        match action {
            Action::Configure { config, key } => {
                let path = self.path.clone();
                let mut candidate = self.journal.clone();
                candidate.config = config;
                let clear = !candidate.config.enabled
                    || !candidate.config.injection_approved
                    || candidate.config.url != self.journal.config.url
                    || key.is_some();
                if clear {
                    for p in &self.pipeline.pending {
                        candidate.receipts.push_back(Receipt {
                            event_id: p.message.event_id.clone(),
                            outcome: "skipped".into(),
                        });
                    }
                }
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
                    self.projects.clear();
                }
                self.journal = candidate;
                self.publish();
                Ok(json!({"saved":true}))
            }
            Action::Projects(projects) => {
                if projects.len() > 4096 {
                    return Err("Too many gateway projects".into());
                }
                let mut ids = std::collections::HashSet::new();
                for p in &projects {
                    identity(&p.ident)?;
                    if !ids.insert(&p.ident)
                        || p.upstream_urls.len() > 16
                        || p.upstream_urls.iter().any(|u| repository(u).is_none())
                    {
                        return Err("Invalid gateway project metadata".into());
                    }
                }
                self.projects = projects;
                Ok(json!({"updated":true}))
            }
            Action::Message(mut message) => {
                // A bearer value must never be retained in event bodies or sent to an agent.
                if !self.key.is_empty() {
                    message.text = message.text.replace(&self.key, "[redacted]");
                }
                let sessions = discover(terminals).await;
                let outcome =
                    self.pipeline
                        .admit(&mut self.journal, &self.projects, &sessions, message)?;
                self.save().await?;
                self.publish();
                Ok(json!({"delivery":format!("{outcome:?}").to_lowercase()}))
            }
        }
    }

    /// Fsync a delivery fence before asking GTK to type; failed final checks retire without retargeting.
    async fn drain(&mut self, terminals: &[Terminal]) -> Result<(), String> {
        if self.pipeline.pending.is_empty() {
            return Ok(());
        }
        self.drain_verified(discover(terminals).await).await
    }

    /// Serialize verified-session delivery, fencing before GTK and allowing one input per readiness observation.
    async fn drain_verified(&mut self, mut sessions: Vec<Session>) -> Result<(), String> {
        while let Some((pending, ready)) = self.pipeline.next(&self.projects, &sessions) {
            self.journal.receipts.push_back(Receipt {
                event_id: pending.message.event_id.clone(),
                outcome: if ready { "submitting" } else { "skipped" }.into(),
            });
            self.save().await?;
            if ready {
                let (reply, result) = oneshot::channel();
                let surface = pending.target.terminal.surface_id.clone();
                let delivery = Delivery {
                    message: pending.message.clone(),
                    session: pending.target.clone(),
                    reply,
                };
                let sent = self.deliveries.send(delivery).await.is_ok();
                let outcome = if sent {
                    match result.await {
                        Ok(Ok(DeliveryOutcome::Injected)) => "injected",
                        Ok(Ok(DeliveryOutcome::Deferred)) => "deferred",
                        Ok(Err(_)) => "skipped",
                        Err(_) => "uncertain",
                    }
                } else {
                    "uncertain"
                };
                if outcome == "deferred" {
                    self.journal.receipts.pop_back();
                    self.pipeline.pending.push_front(pending);
                } else {
                    self.journal.receipts.back_mut().unwrap().outcome = outcome.into();
                }
                self.save().await?;
                // One observed empty prompt permits one submission only.
                for s in &mut sessions {
                    if s.terminal.surface_id == surface {
                        s.terminal.observation = None;
                    }
                }
                crate::diagnostics::record(
                    "gateway.message.delivery",
                    json!({"surface_id":surface,"outcome":outcome}),
                );
            }
        }
        self.publish();
        Ok(())
    }
}

/// Resolve active executable identities off GTK, and match the selected Git upstream with bounded Git I/O.
async fn discover(terminals: &[Terminal]) -> Vec<Session> {
    let mut sessions = Vec::new();
    // Incomplete snapshots could hide a second recipient; fail closed at the admission limit.
    if terminals.len() > MAX_PENDING {
        return sessions;
    }
    let started = std::time::Instant::now();
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
        sessions.push(Session {
            terminal: terminal.clone(),
            process,
            repository,
        });
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

/// Own local settings and prepared ingress; cancellation closes delivery replies and prevents delayed GTK input.
pub async fn run(
    mut requests: mpsc::Receiver<Request>,
    mut snapshots: watch::Receiver<Vec<Terminal>>,
    view: watch::Sender<View>,
    deliveries: mpsc::Sender<Delivery>,
) {
    let path = storage::path();
    let loading = path.clone();
    let loaded = tokio::task::spawn_blocking(move || {
        Ok::<_, String>((storage::load(&loading)?, storage::key(&loading)?))
    })
    .await;
    let result = async {
        let (journal, key) = loaded.map_err(|_| "Gateway storage worker stopped")??;
        let mut worker = Worker {
            journal,
            key,
            path,
            projects: Vec::new(),
            pipeline: Pipeline::default(),
            storage_failed: false,
            view: view.clone(),
            deliveries,
        };
        worker.save().await?;
        worker.publish();
        loop {
            tokio::select! {
                request = requests.recv() => {
                    let Some(request) = request else { return Ok::<_, String>(()); };
                    let terminals = snapshots.borrow_and_update().clone();
                    let result = worker.apply(request.action, &terminals).await;
                    let _ = request.reply.send(result);
                    if worker.storage_failed { return Err("Gateway storage failed; delivery paused".into()); }
                }
                changed = snapshots.changed() => { if changed.is_err() { return Ok(()); } }
            }
            let terminals = snapshots.borrow_and_update().clone();
            worker.drain(&terminals).await?;
        }
    }
    .await;
    if let Err(error) = result {
        view.send_modify(|v| v.connection = error);
        while let Some(request) = requests.recv().await {
            let _ = request.reply.send(Err(
                "Gateway delivery paused; restart after repairing storage".into(),
            ));
        }
    }
}

/// Record failed bounded Git subprocess cleanup without retaining paths or credentials.
fn cleanup_failed(error: &std::io::Error) {
    crate::diagnostics::event(format_args!("gateway Git child cleanup failed: {error}"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::pipeline::Admission;

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
                workspace_id: "workspace".into(),
                surface_id: "surface".into(),
                directory: "/repo".into(),
                foreground_pid: 42,
                input_revision: 3,
                input_pending: false,
                observation: Some(Observation {
                    process: process.clone(),
                    input_revision: 3,
                    input: InputState::EmptyReady,
                    actor_id: Some("recipient".into()),
                    observed_at: std::time::Instant::now(),
                }),
            },
            process,
            repository: "github.com/org/repo".into(),
        };
        let message = Message {
            event_id: "event".into(),
            project_ident: "project".into(),
            task_id: "task".into(),
            kind: Kind::Created,
            text: "Please inspect the task".into(),
            author_id: None,
            source_instance: None,
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
                assert_eq!(delivery.message.terminal_text(), message.terminal_text());
                delivery.reply.send(Ok(outcome)).unwrap();
            }
        });
        worker.drain_verified(vec![session.clone()]).await.unwrap();
        assert_eq!(worker.pipeline.pending.len(), 1);
        assert!(worker.journal.receipts.is_empty());
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
        };
        assert_eq!(
            worker
                .pipeline
                .admit(&mut replay, &worker.projects, &[session], replay_message)
                .unwrap(),
            Admission::Duplicate
        );
        // A journal failure never permits the next queued input to reach GTK.
        worker.path = root.clone();
        assert!(worker.save().await.is_err());
        assert!(worker.storage_failed);
        std::fs::remove_dir_all(root).unwrap();
    }
}
