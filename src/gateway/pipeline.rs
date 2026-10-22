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
                    && session.accepts_goal(message.goal.as_deref())
            })
            .collect();
        if !fresh_at(message.created_at, now_millis())
            || !journal.config.enabled
            || !journal.config.injection_approved
            || candidates.is_empty()
        {
            journal.receipts.push_back(Receipt {
                event_id: message.event_id,
                outcome: "skipped".into(),
                reason: if !fresh_at(message.created_at, now_millis()) {
                    "Gateway event is older than five minutes or has no timestamp"
                } else if !journal.config.injection_approved {
                    "Experimental injection approval is off"
                } else if candidates.is_empty() {
                    if message.goal.is_some() {
                        "No active agent selected this goal in a matching repository"
                    } else {
                        "No active agent in a matching repository"
                    }
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
            // A delivery fence retains identity, never a historical render grid.
            // Readiness is re-observed from the current session in next().
            let mut target = target.clone();
            target.terminal.screen = None;
            journal.receipts.push_back(Receipt {
                event_id: message.event_id.clone(),
                recipient_session_id: Some(target.session_id.clone()),
                outcome: if own { "skipped" } else { "queued" }.into(),
                reason: if own {
                    "Event originated in this exact agent session"
                } else {
                    "Waiting for an empty native prompt"
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
            if !fresh_at(pending.message.created_at, now_millis()) {
                return self.pending.remove(index).map(|p| (p, false));
            }
            let current = sessions.iter().find(|s| {
                pending.target.same_target(s)
                    && s.accepts_goal(pending.message.goal.as_deref())
                    && (pending.message.goal.is_none()
                        || pending.target.selected_goal == s.selected_goal)
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

    /// Create a transport-independent verified session with a local executor for routing tests.
    fn session(surface: &str, composer_active: bool) -> Session {
        Session {
            terminal: Terminal {
                workspace_id: "workspace".into(),
                surface_id: surface.into(),
                directory: "/repo".into(),
                foreground_pid: 42,
                composer_active: false,
                input_revision: 0,
                screen: composer_active.then(ready_screen),
            },
            process: cmux_platform::process::Identity {
                pid: 42,
                start_ticks: 10,
                client: "codex".into(),
            },
            repository: "github.com/org/repo".into(),
            session_id: uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, surface.as_bytes())
                .to_string(),
            actor_origin: None,
            selected_goal: None,
        }
    }

    /// Build representative lifecycle content without imposing a wire event format.
    fn message(id: &str, kind: Kind) -> Message {
        Message {
            event_id: id.into(),
            project_ident: "project".into(),
            task_id: "task".into(),
            kind,
            created_at: now_millis(),
            text: "Please inspect the task".into(),
            author_id: Some("human".into()),
            source_instance: None,
            origin: None,
            goal: None,
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

    /// Named work reaches only arrival-time goal peers; legacy ungrouped events still broadcast.
    #[test]
    fn goals_isolate_repository_peers_without_changing_legacy_broadcasts() {
        let (mut journal, projects) = context();
        let mut a = session("a", true);
        let mut b = session("b", true);
        let ungrouped = session("ungrouped", true);
        let origin = Origin {
            session_id: uuid::Uuid::new_v4().to_string(),
            instance_id: uuid::Uuid::new_v4().to_string(),
            provider: "codex".into(),
            os: "linux".into(),
        };
        a.selected_goal = Some(GoalSelection {
            repository: a.repository.clone(),
            goal: "alpha".into(),
            origin: origin.clone(),
        });
        b.selected_goal = Some(GoalSelection {
            repository: b.repository.clone(),
            goal: "beta".into(),
            origin,
        });
        let mut other_repo = a.clone();
        other_repo.repository = "github.com/org/other".into();
        let sessions = [a.clone(), b.clone(), ungrouped, other_repo];
        let mut pipeline = Pipeline::default();
        let mut grouped = message("goal", Kind::Created);
        grouped.goal = Some("alpha".into());
        assert_eq!(
            pipeline
                .admit(&mut journal, &projects, &sessions, grouped.clone())
                .unwrap(),
            Admission::Queued
        );
        assert_eq!(pipeline.pending.len(), 1);
        assert_eq!(pipeline.pending[0].target.session_id, a.session_id);
        assert!(pipeline.next(&projects, &sessions).unwrap().1);

        grouped.event_id = "no-match".into();
        grouped.goal = Some("missing".into());
        assert_eq!(
            pipeline
                .admit(&mut journal, &projects, &sessions, grouped)
                .unwrap(),
            Admission::Skipped
        );
        assert_eq!(journal.receipts.back().unwrap().outcome, "skipped");
        assert!(pipeline.pending.is_empty());

        assert_eq!(
            pipeline
                .admit(
                    &mut journal,
                    &projects,
                    &sessions,
                    message("legacy", Kind::Completed)
                )
                .unwrap(),
            Admission::Queued
        );
        assert_eq!(pipeline.pending.len(), 3);

        // Equal goal names do not override exact actor echo suppression.
        let mut pipeline = Pipeline::default();
        b.selected_goal.as_mut().unwrap().goal = "alpha".into();
        b.selected_goal.as_mut().unwrap().origin.session_id = uuid::Uuid::new_v4().to_string();
        let mut echo = message("echo", Kind::Commented);
        echo.goal = Some("alpha".into());
        echo.origin = Some(a.selected_goal.as_ref().unwrap().origin.clone());
        pipeline
            .admit(&mut journal, &projects, &[a, b.clone()], echo)
            .unwrap();
        assert_eq!(pipeline.pending.len(), 1);
        assert_eq!(pipeline.pending[0].target.session_id, b.session_id);
    }

    /// Clearing, switching goal or replacing a logical actor retires queued work, without retargeting it.
    #[test]
    fn goal_selection_is_rechecked_before_input() {
        for change in ["clear", "switch", "actor", "repository"] {
            let (mut journal, projects) = context();
            let mut peer = session("peer", false);
            peer.selected_goal = Some(GoalSelection {
                repository: peer.repository.clone(),
                goal: "alpha".into(),
                origin: Origin {
                    session_id: uuid::Uuid::new_v4().to_string(),
                    instance_id: uuid::Uuid::new_v4().to_string(),
                    provider: "codex".into(),
                    os: "linux".into(),
                },
            });
            let mut event = message("event", Kind::Commented);
            event.goal = Some("alpha".into());
            let mut pipeline = Pipeline::default();
            pipeline
                .admit(&mut journal, &projects, &[peer.clone()], event)
                .unwrap();
            assert!(pipeline.next(&projects, &[peer.clone()]).is_none());
            match change {
                "clear" => peer.selected_goal = None,
                "switch" => peer.selected_goal.as_mut().unwrap().goal = "beta".into(),
                "actor" => {
                    peer.selected_goal.as_mut().unwrap().origin.session_id =
                        uuid::Uuid::new_v4().to_string()
                }
                _ => {
                    peer.selected_goal.as_mut().unwrap().repository = "github.com/org/other".into()
                }
            }
            peer.terminal.screen = Some(ready_screen());
            assert!(!pipeline.next(&projects, &[peer]).unwrap().1, "{change}");
            assert!(pipeline.pending.is_empty());
        }
    }

    /// Stale events never queue, and live events expire even while their executor is unavailable.
    #[test]
    fn skips_stale_events_and_expires_queued_input() {
        let (mut journal, projects) = context();
        let sessions = vec![session("surface", false)];
        let mut pipeline = Pipeline::default();
        let mut old = message("old", Kind::Commented);
        old.created_at = now_millis() - MAX_EVENT_AGE_MS - 1;
        assert_eq!(
            pipeline
                .admit(&mut journal, &projects, &sessions, old)
                .unwrap(),
            Admission::Skipped
        );
        assert!(pipeline.pending.is_empty());
        assert_eq!(
            pipeline
                .admit(
                    &mut journal,
                    &projects,
                    &sessions,
                    message("live", Kind::Created)
                )
                .unwrap(),
            Admission::Queued
        );
        pipeline.pending[0].message.created_at = now_millis() - MAX_EVENT_AGE_MS - 1;
        assert!(!pipeline.next(&projects, &sessions).unwrap().1);
        assert!(pipeline.pending.is_empty());
    }

    /// All task lifecycle events route by repository, deduplicate and wait only for a local executor.
    #[test]
    fn lifecycle_waits_for_executor_and_deduplicates() {
        let (mut journal, projects) = context();
        let mut pipeline = Pipeline::default();
        let mut sessions = vec![session("surface", false)];
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
        assert!(journal.receipts.iter().all(|receipt| receipt
            .target
            .as_ref()
            .is_none_or(|target| target.terminal.screen.is_none())));
        assert!(pipeline
            .pending
            .iter()
            .all(|pending| pending.target.terminal.screen.is_none()));
        sessions[0].terminal.screen = None;
        assert!(pipeline.next(&projects, &sessions).is_none());
        sessions[0].terminal.screen = Some(ready_screen());
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
        // Detaching the local executor delays input; losing the target retires it.
        sessions[0].terminal.screen = None;
        assert!(pipeline.next(&projects, &sessions).is_none());
        assert!(!pipeline.next(&projects, &[]).unwrap().1);
    }

    /// Missing agents, ambiguous projects and missing consent never queue input; authors alone do not suppress comments.
    #[test]
    fn skips_unroutable_events_and_preserves_agent_comments() {
        let (journal, projects) = context();
        let s = session("one", true);
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
        let own = session("own", true);
        let ready = session("ready", true);
        let busy = session("busy", false);
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
        idle[2].terminal.screen = Some(ready_screen());
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
        let original = session("surface", true);
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
        let mut sessions = vec![session("one", false)];
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
        let mut ready = session("one", true);
        ready.terminal.screen = None;
        assert!(!ready.ready());
    }

    /// Queue and receipt saturation apply backpressure without losing previously accepted messages.
    #[test]
    fn bounded_queue_and_receipts() {
        let (mut journal, projects) = context();
        let sessions = [session("one", false)];
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

    /// SSH/HTTPS normalization folds repository case and refuses unsafe terminal text.
    #[test]
    fn remote_and_message_boundaries() {
        assert_eq!(
            repository("git@GitHub.com:org/Repo.git"),
            repository("https://github.com/org/Repo/")
        );
        assert_eq!(
            repository("https://github.com/org/Repo"),
            repository("https://github.com/org/repo")
        );
        assert_eq!(
            repository("git@GitHub.com:NiTeCoN/SRE.GIT"),
            Some("github.com/nitecon/sre".into())
        );
        assert_eq!(
            repository("https://GitHub.com/NiTeCoN/XLauncher/"),
            Some("github.com/nitecon/xlauncher".into())
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
        let s = session("one", true);
        assert!(s.ready());
        let mut changed = s.clone();
        changed.process.start_ticks += 1;
        assert!(!s.same_target(&changed));
    }
}
