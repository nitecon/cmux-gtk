//! Bounded message routing, independent of WebSocket framing and native terminal handles.
use super::model::*;
use std::collections::VecDeque;

/// An event pinned to the unique active recipient that existed when the event arrived.
#[derive(Clone, Debug)]
pub struct Pending {
    pub message: Message,
    pub target: Session,
}

/// Queue outcomes are distinct from task completion and from eventual transport acknowledgments.
#[derive(Debug, PartialEq, Eq)]
pub enum Admission {
    Duplicate,
    Skipped,
    Queued,
}

/// A bounded FIFO and durable receipt journal; the worker serializes mutations and fsynced fences.
#[derive(Default)]
pub struct Pipeline {
    pub pending: VecDeque<Pending>,
}

impl Pipeline {
    /// Broadcast to arrival-time peers in one matching project, excluding only the exact origin session.
    pub fn admit(
        &mut self,
        journal: &mut Journal,
        projects: &[Project],
        sessions: &[Session],
        message: Message,
    ) -> Result<Admission, String> {
        message.validate()?;
        if journal
            .receipts
            .iter()
            .any(|r| r.event_id == message.event_id)
            || self
                .pending
                .iter()
                .any(|p| p.message.event_id == message.event_id)
        {
            return Ok(Admission::Duplicate);
        }
        // Reserve receipt capacity for all pending messages; never evict a fence and silently replay input.
        if journal.receipts.len() >= MAX_RECEIPTS {
            return Err("Gateway receipt capacity reached; delivery paused".into());
        }
        let candidates: Vec<&Session> = sessions
            .iter()
            .filter(|session| {
                matching_project(&session.repository, projects)
                    == Some(message.project_ident.as_str())
            })
            .collect();
        if !journal.config.enabled || !journal.config.injection_approved || candidates.is_empty() {
            journal.receipts.push_back(Receipt {
                event_id: message.event_id,
                outcome: "skipped".into(),
                reason: if !journal.config.injection_approved {
                    "Experimental injection approval is off"
                } else if candidates.is_empty() {
                    "No active agent in a matching repository"
                } else {
                    "Gateway is disabled"
                }
                .into(),
                ..Default::default()
            });
            return Ok(Admission::Skipped);
        }
        let is_origin = |session: &Session| {
            message
                .origin
                .as_ref()
                .is_some_and(|o| session.is_origin(o, &journal.instance_id))
        };
        let recipients = candidates.iter().filter(|s| !is_origin(s)).count();
        if self.pending.len() + recipients > MAX_PENDING {
            return Err("Gateway message queue is full".into());
        }
        if journal.receipts.len() + candidates.len() > MAX_RECEIPTS {
            return Err("Gateway receipt capacity reached; delivery paused".into());
        }
        for target in candidates {
            let own = is_origin(target);
            journal.receipts.push_back(Receipt {
                event_id: message.event_id.clone(),
                recipient_session_id: Some(target.session_id.clone()),
                outcome: if own { "skipped" } else { "queued" }.into(),
                reason: if own {
                    "Event originated in this exact agent session"
                } else {
                    "Waiting for an empty, idle agent prompt"
                }
                .into(),
                payload: (!own).then(|| message.clone()),
                target: Some(target.clone()),
                ..Default::default()
            });
            if !own {
                self.pending.push_back(Pending {
                    message: message.clone(),
                    target: target.clone(),
                });
            }
        }
        Ok(if recipients == 0 {
            Admission::Skipped
        } else {
            Admission::Queued
        })
    }

    /// Remove one ready event without overtaking earlier messages for its terminal; retire changed targets.
    pub fn next(&mut self, projects: &[Project], sessions: &[Session]) -> Option<(Pending, bool)> {
        for index in 0..self.pending.len() {
            let pending = &self.pending[index];
            let current = sessions.iter().find(|s| {
                pending.target.same_target(s)
                    && matching_project(&s.repository, projects)
                        == Some(pending.message.project_ident.as_str())
            });
            if current.is_none() {
                return self.pending.remove(index).map(|p| (p, false));
            }
            if self
                .pending
                .iter()
                .take(index)
                .any(|p| p.target.same_target(&pending.target))
            {
                continue;
            }
            let current = current.unwrap();
            if current.ready() {
                let mut pending = self.pending.remove(index)?;
                pending.target = current.clone();
                return Some((pending, true));
            }
        }
        None
    }
}

/// Resolve one project by normalized upstream URL; shared admission/recheck policy rejects ambiguity.
fn matching_project<'a>(remote: &str, projects: &'a [Project]) -> Option<&'a str> {
    let mut matches = projects.iter().filter(|p| {
        p.upstream_urls
            .iter()
            .filter_map(|url| repository(url))
            .any(|url| url == remote)
    });
    let first = matches.next()?;
    if matches.next().is_some() {
        None
    } else {
        Some(&first.ident)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a transport-independent verified session with a positive observation for routing tests.
    fn session(surface: &str, input: InputState) -> Session {
        let process = cmux_platform::process::Identity {
            pid: 42,
            start_ticks: 10,
            client: "codex".into(),
        };
        Session {
            terminal: Terminal {
                workspace_id: "workspace".into(),
                surface_id: surface.into(),
                directory: "/repo".into(),
                foreground_pid: 42,
                input_revision: 3,
                input_pending: false,
                screen: None,
                captured_at: None,
                observation: Some(Observation {
                    process: process.clone(),
                    input_revision: 3,
                    input,
                    observed_at: std::time::Instant::now(),
                }),
            },
            process,
            repository: "github.com/org/repo".into(),
            session_id: uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, surface.as_bytes())
                .to_string(),
            actor_origin: None,
            codex_thread_id: None,
        }
    }

    /// Native provider queues preserve a real draft and do not depend on terminal prompt classification.
    #[test]
    fn managed_queue_preserves_draft_and_pins_exact_conversation() {
        let mut managed = session("native", InputState::Unfinished);
        managed.terminal.input_pending = true;
        managed.terminal.observation = None;
        managed.codex_thread_id = Some(uuid::Uuid::new_v4().to_string());
        assert!(managed.ready());
        let mut other = managed.clone();
        other.codex_thread_id = Some(uuid::Uuid::new_v4().to_string());
        assert!(!managed.same_target(&other));
        other.codex_thread_id = None;
        assert!(!managed.same_target(&other));
        assert!(!other.ready());
    }

    /// Build representative lifecycle content without imposing a wire event format.
    fn message(id: &str, kind: Kind) -> Message {
        Message {
            event_id: id.into(),
            project_ident: "project".into(),
            task_id: "task".into(),
            kind,
            text: "Please inspect the task".into(),
            author_id: Some("human".into()),
            source_instance: None,
            origin: None,
        }
    }

    /// Global opt-in replaces manual project execution settings.
    fn context() -> (Journal, Vec<Project>) {
        let mut journal = Journal::default();
        journal.config.enabled = true;
        journal.config.injection_approved = true;
        (
            journal,
            vec![Project {
                ident: "project".into(),
                upstream_urls: vec!["git@github.com:org/repo.git".into()],
            }],
        )
    }

    /// All task lifecycle events route by repository, deduplicate and wait for an empty agent prompt.
    #[test]
    fn lifecycle_waits_and_deduplicates() {
        let (mut journal, projects) = context();
        let mut pipeline = Pipeline::default();
        let mut sessions = vec![session("surface", InputState::Busy)];
        for (id, kind) in [
            ("new", Kind::Created),
            ("comment", Kind::Commented),
            ("done", Kind::Completed),
        ] {
            assert_eq!(
                pipeline
                    .admit(&mut journal, &projects, &sessions, message(id, kind))
                    .unwrap(),
                Admission::Queued
            );
            assert_eq!(
                pipeline
                    .admit(&mut journal, &projects, &sessions, message(id, kind))
                    .unwrap(),
                Admission::Duplicate
            );
        }
        assert!(pipeline.next(&projects, &sessions).is_none());
        sessions[0].terminal.observation.as_mut().unwrap().input = InputState::Unfinished;
        assert!(pipeline.next(&projects, &sessions).is_none());
        sessions[0].terminal.observation.as_mut().unwrap().input = InputState::EmptyReady;
        let (pending, ready) = pipeline.next(&projects, &sessions).unwrap();
        assert!(ready);
        assert_eq!(pending.message.event_id, "new");
        assert!(pending.message.terminal_text().starts_with(START));
        assert!(pending.message.terminal_text().ends_with(STOP));
        journal.receipts.push_back(Receipt {
            event_id: "new".into(),
            outcome: "injected".into(),
            ..Default::default()
        });
        assert_eq!(
            pipeline
                .admit(
                    &mut journal,
                    &projects,
                    &sessions,
                    message("new", Kind::Created)
                )
                .unwrap(),
            Admission::Duplicate
        );
        // Even an unchanged ready observation expires on input before the next delivery.
        sessions[0].terminal.input_revision += 1;
        assert!(pipeline.next(&projects, &sessions).is_none());
        assert!(!pipeline.next(&projects, &[]).unwrap().1);
    }

    /// Missing agents, ambiguous projects and missing consent never queue input; authors alone do not suppress comments.
    #[test]
    fn skips_unroutable_events_and_preserves_agent_comments() {
        let (journal, projects) = context();
        let s = session("one", InputState::EmptyReady);
        let scenarios = [vec![]];
        for sessions in scenarios {
            assert_eq!(
                Pipeline::default()
                    .admit(
                        &mut journal.clone(),
                        &projects,
                        &sessions,
                        message("event", Kind::Created)
                    )
                    .unwrap(),
                Admission::Skipped
            );
        }
        let mut own = message("own", Kind::Commented);
        own.author_id = Some("recipient".into());
        assert_eq!(
            Pipeline::default()
                .admit(
                    &mut journal.clone(),
                    &projects,
                    std::slice::from_ref(&s),
                    own
                )
                .unwrap(),
            Admission::Queued
        );
        let mut other = projects.clone();
        other.push(Project {
            ident: "other".into(),
            upstream_urls: projects[0].upstream_urls.clone(),
        });
        assert_eq!(
            Pipeline::default()
                .admit(
                    &mut journal.clone(),
                    &other,
                    std::slice::from_ref(&s),
                    message("event", Kind::Created)
                )
                .unwrap(),
            Admission::Skipped
        );
        let mut unapproved = journal.clone();
        unapproved.config.injection_approved = false;
        assert_eq!(
            Pipeline::default()
                .admit(
                    &mut unapproved,
                    &projects,
                    std::slice::from_ref(&s),
                    message("event", Kind::Created)
                )
                .unwrap(),
            Admission::Skipped
        );
        let mut sourced = message("sourced", Kind::Created);
        sourced.source_instance = Some(journal.instance_id.clone());
        assert_eq!(
            Pipeline::default()
                .admit(&mut journal.clone(), &projects, &[s], sourced)
                .unwrap(),
            Admission::Queued
        );
    }

    /// Same-provider peers receive comments and completion independently while the exact origin stays silent.
    #[test]
    fn broadcasts_to_peers_and_suppresses_only_exact_origin() {
        let (mut journal, projects) = context();
        let own = session("own", InputState::EmptyReady);
        let ready = session("ready", InputState::EmptyReady);
        let busy = session("busy", InputState::Busy);
        let sessions = vec![own.clone(), ready.clone(), busy.clone()];
        let mut pipeline = Pipeline::default();
        for (event, kind) in [("comment", Kind::Commented), ("completed", Kind::Completed)] {
            let mut message = message(event, kind);
            message.origin = Some(Origin {
                session_id: own.session_id.clone(),
                instance_id: journal.instance_id.clone(),
                provider: "codex".into(),
                os: "linux".into(),
            });
            assert_eq!(
                pipeline
                    .admit(&mut journal, &projects, &sessions, message.clone())
                    .unwrap(),
                Admission::Queued
            );
            assert_eq!(
                pipeline
                    .admit(&mut journal, &projects, &sessions, message)
                    .unwrap(),
                Admission::Duplicate
            );
        }
        assert_eq!(journal.receipts.len(), 6);
        assert!(journal
            .receipts
            .iter()
            .filter(|r| r.recipient() == Some(own.session_id.as_str()))
            .all(|r| r.outcome == "skipped" && r.payload.is_none()));
        for event in ["comment", "completed"] {
            let (pending, valid) = pipeline.next(&projects, &sessions).unwrap();
            assert!(valid);
            assert_eq!(pending.target.session_id, ready.session_id);
            assert_eq!(pending.message.event_id, event);
        }
        assert!(pipeline.next(&projects, &sessions).is_none());
        let mut idle = sessions;
        idle[2].terminal.observation.as_mut().unwrap().input = InputState::EmptyReady;
        for event in ["comment", "completed"] {
            let (pending, valid) = pipeline.next(&projects, &idle).unwrap();
            assert!(valid);
            assert_eq!(pending.target.session_id, busy.session_id);
            assert_eq!(pending.message.event_id, event);
        }
    }

    /// Transport reconnection preserves identity; process replacement and another installation cannot share it.
    #[test]
    fn session_identity_follows_process_generation_and_instance() {
        let original = session("surface", InputState::EmptyReady);
        let instance = uuid::Uuid::new_v4().to_string();
        let identify = |process: cmux_platform::process::Identity, namespace: &str| {
            Session::identified(
                original.terminal.clone(),
                process,
                original.repository.clone(),
                namespace,
            )
        };
        let first = identify(original.process.clone(), &instance);
        assert_eq!(
            first.session_id,
            identify(original.process.clone(), &instance).session_id
        );
        let mut replaced = original.process.clone();
        replaced.start_ticks += 1;
        assert_ne!(first.session_id, identify(replaced, &instance).session_id);
        assert_ne!(
            first.session_id,
            identify(original.process, &uuid::Uuid::new_v4().to_string()).session_id
        );
    }

    /// An event never changes recipients when its process exits or project metadata changes.
    #[test]
    fn retires_changed_targets_and_metadata() {
        let (mut journal, projects) = context();
        let mut sessions = vec![session("one", InputState::Busy)];
        let mut pipeline = Pipeline::default();
        pipeline
            .admit(
                &mut journal,
                &projects,
                &sessions,
                message("first", Kind::Created),
            )
            .unwrap();
        sessions[0].process.start_ticks += 1;
        assert!(!pipeline.next(&projects, &sessions).unwrap().1);
        pipeline
            .admit(
                &mut journal,
                &projects,
                &sessions,
                message("second", Kind::Created),
            )
            .unwrap();
        assert!(!pipeline.next(&[], &sessions).unwrap().1);
        let mut ready = session("one", InputState::EmptyReady);
        ready.terminal.input_pending = true;
        assert!(!ready.ready());
    }

    /// Queue and receipt saturation apply backpressure without losing previously accepted messages.
    #[test]
    fn bounded_queue_and_receipts() {
        let (mut journal, projects) = context();
        let sessions = [session("one", InputState::Unknown)];
        let mut pipeline = Pipeline::default();
        for i in 0..MAX_PENDING {
            pipeline
                .admit(
                    &mut journal,
                    &projects,
                    &sessions,
                    message(&i.to_string(), Kind::Created),
                )
                .unwrap();
        }
        assert!(pipeline
            .admit(
                &mut journal,
                &projects,
                &sessions,
                message("full", Kind::Created)
            )
            .is_err());
        assert_eq!(pipeline.pending.len(), MAX_PENDING);
        journal.receipts = (0..MAX_RECEIPTS)
            .map(|i| Receipt {
                event_id: format!("receipt-{i}"),
                outcome: "injected".into(),
                ..Default::default()
            })
            .collect();
        assert!(pipeline
            .admit(
                &mut journal,
                &projects,
                &sessions,
                message("capacity", Kind::Created)
            )
            .is_err());
    }

    /// SSH/HTTPS normalization retains repository case and refuses unsafe terminal text.
    #[test]
    fn remote_and_message_boundaries() {
        assert_eq!(
            repository("git@GitHub.com:org/Repo.git"),
            repository("https://github.com/org/Repo/")
        );
        assert_ne!(
            repository("https://github.com/org/Repo"),
            repository("https://github.com/org/repo")
        );
        assert_ne!(
            repository("ssh://git@host:2222/org/repo"),
            repository("https://host/org/repo")
        );
        assert!(repository("/local/repo").is_none());
        assert!(endpoint("https://gateway.example/v1/execution/connect").is_err());
        assert!(endpoint("http://gateway.example").is_err());
        assert!(endpoint("http://127.0.0.1:1234").is_ok());
        let mut m = message("bad", Kind::Commented);
        m.author_id = None;
        assert!(m.validate().is_err());
        m.kind = Kind::Created;
        for bad in ["bad\rinput", "bad\x1b[31m", START, STOP] {
            m.text = bad.into();
            assert!(m.validate().is_err());
        }
        let mut s = session("one", InputState::EmptyReady);
        s.terminal.observation.as_mut().unwrap().observed_at -= std::time::Duration::from_secs(2);
        assert!(!s.ready());
        s.terminal.observation.as_mut().unwrap().observed_at = std::time::Instant::now();
        s.process.start_ticks += 1;
        assert!(!s.ready());
    }
}
