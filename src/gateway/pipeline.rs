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
    /// Route only to one verified session of one uniquely matching project; no-agent events are discarded.
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
        if journal.receipts.len() + self.pending.len() >= MAX_RECEIPTS {
            return Err("Gateway receipt capacity reached; delivery paused".into());
        }
        let candidates: Vec<&Session> = sessions
            .iter()
            .filter(|session| {
                matching_project(&session.repository, projects)
                    == Some(message.project_ident.as_str())
            })
            .collect();
        let target = if journal.config.enabled
            && journal.config.injection_approved
            && message.source_instance.as_deref() != Some(&journal.instance_id)
            && candidates.len() == 1
        {
            let candidate = candidates[0];
            if message.kind == Kind::Commented {
                // Without recipient attribution, self-comment suppression cannot be established.
                candidate
                    .terminal
                    .observation
                    .as_ref()
                    .filter(|o| o.process == candidate.process)
                    .and_then(|o| o.actor_id.as_ref())
                    .filter(|actor| Some(*actor) != message.author_id.as_ref())
                    .map(|_| candidate)
            } else {
                Some(candidate)
            }
        } else {
            None
        };
        let Some(target) = target else {
            journal.receipts.push_back(Receipt {
                event_id: message.event_id,
                outcome: "skipped".into(),
            });
            return Ok(Admission::Skipped);
        };
        if self.pending.len() >= MAX_PENDING {
            return Err("Gateway message queue is full".into());
        }
        self.pending.push_back(Pending {
            message,
            target: target.clone(),
        });
        Ok(Admission::Queued)
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
                observation: Some(Observation {
                    process: process.clone(),
                    input_revision: 3,
                    input,
                    actor_id: Some("recipient".into()),
                    observed_at: std::time::Instant::now(),
                }),
            },
            process,
            repository: "github.com/org/repo".into(),
        }
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

    /// Missing agents, ambiguous recipients/projects, own comments and missing consent never queue input.
    #[test]
    fn skips_unroutable_and_looping_events() {
        let (journal, projects) = context();
        let s = session("one", InputState::EmptyReady);
        let scenarios = [
            vec![],
            vec![s.clone(), session("two", InputState::EmptyReady)],
        ];
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
            Admission::Skipped
        );
        let mut unknown = s.clone();
        unknown.terminal.observation = None;
        assert_eq!(
            Pipeline::default()
                .admit(
                    &mut journal.clone(),
                    &projects,
                    &[unknown],
                    message("comment", Kind::Commented)
                )
                .unwrap(),
            Admission::Skipped
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
            Admission::Skipped
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
