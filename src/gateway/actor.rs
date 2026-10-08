//! Bind client-owned logical actor origins to separately fenced native terminal generations.
//! Tokens are local, ephemeral and never enter lifecycle journals or gateway requests.
use super::model::{Origin, Session, MAX_PENDING};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

pub const START: &str = "<cmux-session-enrollment>";
pub const STOP: &str = "</cmux-session-enrollment>";

/// Shared agent-tools-actor-v1 inputs; terminal membership is deliberately absent from the hash.
#[derive(Clone, Debug, Deserialize)]
pub struct Actor {
    pub origin: Origin,
    pub provider_session_id: String,
    pub executor_generation: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::model::{InputState, Observation, Terminal};

    fn actor(native: &str) -> Actor {
        let instance = uuid::Uuid::parse_str("00112233-4455-4677-8899-aabbccddeeff").unwrap();
        let epoch = vec![
            "linux-proc-v1".to_owned(),
            "11111111-2222-4333-8444-555555555555".to_owned(),
            "1234".to_owned(),
            "12345".to_owned(),
        ];
        let name = serde_json::to_vec(&json!([
            "agent-tools-actor-v1",
            "linux",
            "codex",
            native,
            epoch
        ]))
        .unwrap();
        Actor {
            origin: Origin {
                session_id: uuid::Uuid::new_v5(&instance, &name).to_string(),
                instance_id: instance.to_string(),
                provider: "codex".into(),
                os: "linux".into(),
            },
            provider_session_id: native.into(),
            executor_generation: epoch,
        }
    }

    fn session(name: &str) -> Session {
        let process = cmux_platform::process::Identity {
            pid: 42,
            start_ticks: 10,
            client: "codex".into(),
        };
        Session {
            terminal: Terminal {
                workspace_id: "workspace".into(),
                surface_id: name.into(),
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
                    observed_at: Instant::now(),
                }),
            },
            process,
            repository: "github.com/org/repo".into(),
            session_id: name.into(),
            actor_origin: None,
        }
    }

    fn token(text: &str) -> String {
        serde_json::from_str::<Value>(
            text.strip_prefix(START)
                .unwrap()
                .strip_suffix(STOP)
                .unwrap(),
        )
        .unwrap()["enrollment_token"]
            .as_str()
            .unwrap()
            .into()
    }

    /// Root consumes the same published bytes as the client across OSes, providers and namespace resets.
    #[test]
    fn shared_contract_vectors_and_forged_identity() {
        let vectors: Vec<Value> = serde_json::from_str(include_str!(
            "../../tests/fixtures/actor-origin-vectors.json"
        ))
        .unwrap();
        for vector in vectors {
            let actor:Actor = serde_json::from_value(json!({"origin":{"session_id":vector["session_id"],"instance_id":vector["instance_id"],"provider":vector["provider"],"os":vector["os"]},"provider_session_id":vector["provider_session_id"],"executor_generation":vector["executor_generation"]})).unwrap();
            actor.validate().unwrap();
            let name = json!([
                "agent-tools-actor-v1",
                actor.origin.os,
                actor.origin.provider,
                native_id(&actor.provider_session_id).unwrap(),
                actor.executor_generation
            ])
            .to_string();
            assert_eq!(name, vector["name_utf8"].as_str().unwrap());
            let mut forged = actor.clone();
            forged.origin.session_id = uuid::Uuid::new_v4().to_string();
            assert!(forged.validate().is_err());
        }
        assert!(actor("has spaces").validate().is_err());
        assert!(actor("valid").verify_peer(std::process::id()).is_err());
    }

    /// Hook capability and recipient token are separate proofs; none can be replaced by a surface or CLI hint.
    #[test]
    fn hook_only_enrollment_is_atomic_generation_fenced_and_one_time() {
        let current = vec![session("first"), session("peer")];
        let a = actor("thread-a");
        let mut registry = Registry::default();
        assert_eq!(
            registry
                .announce(a.clone(), None, None, &current, false)
                .unwrap()["binding_state"],
            "unbound"
        );
        assert!(registry.enrollment(&current[0]).is_none());
        registry
            .announce(a.clone(), None, None, &current, true)
            .unwrap();
        let text = registry.enrollment(&current[0]).unwrap();
        let nonce = token(&text);
        assert!(registry.accepts_enrollment(&text, &current[0]));
        assert!(!registry.accepts_enrollment(&text, &current[1]));
        assert!(registry
            .announce(a.clone(), Some(&nonce), None, &current, false)
            .is_err());
        assert!(registry
            .announce(
                a.clone(),
                Some(&nonce),
                Some("github.com/other/repo"),
                &current,
                true
            )
            .is_err());
        let bound = registry
            .announce(
                a.clone(),
                Some(&nonce),
                Some("github.com/org/repo"),
                &current,
                true,
            )
            .unwrap();
        assert_eq!(bound["recipient_session_id"], "first");
        assert_eq!(registry.origin(&current[0]), Some(a.origin.clone()));
        assert!(!registry.accepts_enrollment(&text, &current[0]));
        assert!(registry
            .announce(a.clone(), Some(&nonce), None, &current, true)
            .is_err());
        let peer_nonce = token(&registry.enrollment(&current[1]).unwrap());
        assert!(registry
            .announce(a.clone(), Some(&peer_nonce), None, &current, true)
            .is_err());
        let b = actor("thread-b");
        registry
            .announce(b.clone(), Some(&peer_nonce), None, &current, true)
            .unwrap();
        assert_ne!(registry.origin(&current[0]), registry.origin(&current[1]));
        // Transport reattachment does not change actor identity; terminal replacement invalidates membership.
        let mut replacement = current.clone();
        replacement[0].process.start_ticks += 1;
        registry.retain(&replacement);
        assert!(registry.origin(&current[0]).is_none());
        assert_eq!(registry.origin(&current[1]), Some(b.origin));
        assert_eq!(
            registry
                .announce(a, None, None, &replacement, false)
                .unwrap()["binding_state"],
            "unbound"
        );
    }

    /// Native conversation reset cannot retarget a known queued actor; renewal verifies each terminal independently.
    #[test]
    fn renewal_expiry_readiness_and_dead_executor() {
        let current = vec![session("first")];
        let a = actor("thread-a");
        let mut registry = Registry::default();
        registry
            .announce(a.clone(), None, None, &current, true)
            .unwrap();
        let nonce = token(&registry.enrollment(&current[0]).unwrap());
        registry
            .announce(a.clone(), Some(&nonce), None, &current, true)
            .unwrap();
        let mut pinned = current[0].clone();
        pinned.actor_origin = registry.origin(&pinned);
        let b = actor("thread-b");
        registry
            .announce(b.clone(), None, None, &current, true)
            .unwrap();
        assert!(registry.waiting(&current[0]));
        let mut busy = current[0].clone();
        busy.terminal.observation = None;
        assert!(registry.enrollment(&busy).is_none());
        let old = token(&registry.enrollment(&current[0]).unwrap());
        registry.challenges.get_mut("first").unwrap().sent_at -= Duration::from_secs(31);
        assert!(registry
            .announce(b.clone(), Some(&old), None, &current, true)
            .is_err());
        let fresh = token(&registry.enrollment(&current[0]).unwrap());
        assert_ne!(old, fresh);
        registry
            .announce(b.clone(), Some(&fresh), None, &current, true)
            .unwrap();
        let mut now = current[0].clone();
        now.actor_origin = registry.origin(&now);
        assert!(!pinned.same_target(&now));
        assert!(now.is_origin(&b.origin, "unrelated-consumer"));
        assert!(!now.is_origin(&a.origin, "unrelated-consumer"));
        registry.retire(&[a]); // A late old-generation recheck cannot remove the replacement actor.
        assert_eq!(registry.origin(&now), Some(b.origin.clone()));
        registry.retire(&[b]);
        assert!(registry.origin(&now).is_none());
        assert!(registry.waiting(&now));
    }
}

/// Canonical provider identity accepts UUIDs and bounded opaque printable native IDs.
pub fn native_id(value: &str) -> Result<String, String> {
    if value.is_empty() || value.len() > 256 || !value.bytes().all(|b| b.is_ascii_graphic()) {
        return Err("Invalid provider session identity".into());
    }
    Ok(uuid::Uuid::parse_str(value)
        .map(|id| id.to_string())
        .unwrap_or_else(|_| value.to_owned()))
}

impl Actor {
    /// Recompute the public contract, rejecting client-chosen identities inconsistent with its inputs.
    pub fn validate(&self) -> Result<(), String> {
        self.origin.validate()?;
        let namespace = uuid::Uuid::parse_str(&self.origin.instance_id)
            .map_err(|_| "Invalid actor namespace")?;
        let native = native_id(&self.provider_session_id)?;
        if self.executor_generation.is_empty()
            || self.executor_generation.len() > 4
            || self
                .executor_generation
                .iter()
                .any(|v| v.len() > 128 || !v.bytes().all(|b| b.is_ascii_graphic()))
        {
            return Err("Invalid executor generation".into());
        }
        let name = serde_json::to_vec(&json!([
            "agent-tools-actor-v1",
            self.origin.os,
            self.origin.provider,
            native,
            self.executor_generation
        ]))
        .map_err(|_| "Cannot encode actor identity")?;
        if namespace.to_string() != self.origin.instance_id
            || uuid::Uuid::new_v5(&namespace, &name).to_string() != self.origin.session_id
        {
            return Err("Actor identity does not match its namespace and runtime".into());
        }
        Ok(())
    }

    /// Kernel peer ancestry proves executor generation, never the daemon creator's terminal.
    pub fn verify_peer(&self, peer_pid: u32) -> Result<(), String> {
        self.validate()?;
        if self.origin.os != std::env::consts::OS {
            return Err("Actor OS does not match local host".into());
        }
        let executor =
            cmux_platform::process::agent_executor(peer_pid.into(), &self.origin.provider)
                .ok_or("Cannot verify caller's provider executor")?;
        let expected = cmux_platform::process::executor_generation(&executor)
            .ok_or("Cannot verify native executor generation")?;
        if self.executor_generation != expected {
            return Err("Actor executor generation is stale or mismatched".into());
        }
        // Only the connected invocation's own native IDs are relevant, never an ancestor's environment.
        let env = cmux_platform::process::provider_invocation_ids(peer_pid.into())
            .ok_or("Cannot verify caller invocation context")?;
        let keys: &[&str] = if self.origin.provider == "codex" {
            &["CODEX_THREAD_ID", "CODEX_SESSION_ID"]
        } else {
            &["CLAUDE_CODE_SESSION_ID"]
        };
        for (key, value) in env.iter().filter(|(key, _)| keys.contains(&key.as_str())) {
            if native_id(value)? != native_id(&self.provider_session_id)? {
                return Err(format!("Caller {key} does not match provider session"));
            }
        }
        Ok(())
    }

    /// One normalized actor, independent of changing directories or terminal reattachment.
    fn same(&self, other: &Self) -> bool {
        self.origin == other.origin
            && self.executor_generation == other.executor_generation
            && native_id(&self.provider_session_id).ok()
                == native_id(&other.provider_session_id).ok()
    }

    /// Recheck the executor on a blocking worker; a live TUI cannot keep a dead backend actor alive.
    pub fn live(&self) -> bool {
        if self.origin.os != std::env::consts::OS {
            return false;
        }
        let pid_index = if cfg!(windows) { 1 } else { 2 };
        let Some(pid) = self
            .executor_generation
            .get(pid_index)
            .and_then(|value| value.parse::<u64>().ok())
        else {
            return false;
        };
        cmux_platform::process::agent_executor(pid, &self.origin.provider)
            .filter(|p| p.pid == pid)
            .and_then(|p| cmux_platform::process::executor_generation(&p))
            .is_some_and(|generation| generation == self.executor_generation)
    }
}

/// Token delivery and a current terminal generation are necessary for membership; announce alone is not.
struct Challenge {
    session: Session,
    token: String,
    sent_at: Instant,
}
#[derive(Clone)]
struct Binding {
    session: Session,
    actor: Actor,
}

/// At most one generation-fenced binding and enrollment challenge per local terminal.
#[derive(Default)]
pub struct Registry {
    bindings: HashMap<String, Binding>,
    challenges: HashMap<String, Challenge>,
    /// A verified updated client requested local enrollment; older hooks receive no bootstrap input.
    providers: std::collections::HashSet<String>,
    seen_actors: std::collections::VecDeque<(String, String)>,
    renew: std::collections::HashSet<String>,
}

/// Native delivery identity remains independent from client-owned actor identity.
fn same_terminal(a: &Session, b: &Session) -> bool {
    a.same_attachment(b)
}

impl Registry {
    /// Snapshot metadata before blocking runtime verification, without holding the GTK-shared lock.
    pub fn actors(&self) -> Vec<Actor> {
        self.bindings.values().map(|b| b.actor.clone()).collect()
    }

    /// Remove only the generations verified dead; a concurrent new binding is not invalidated.
    pub fn retire(&mut self, dead: &[Actor]) {
        self.bindings.retain(|key, binding| {
            if dead.iter().any(|a| a.same(&binding.actor)) {
                self.renew.insert(key.clone());
                false
            } else {
                true
            }
        });
    }
    /// Remove closed or replaced attachments; tokens never migrate to another generation.
    pub fn retain(&mut self, current: &[Session]) {
        self.bindings
            .retain(|_, binding| current.iter().any(|s| same_terminal(&binding.session, s)));
        self.challenges
            .retain(|_, challenge| current.iter().any(|s| same_terminal(&challenge.session, s)));
        self.renew
            .retain(|key| current.iter().any(|s| &s.session_id == key));
    }

    /// Read the exact actor of a currently verified attachment, without changing receipt identity.
    pub fn origin(&self, session: &Session) -> Option<Origin> {
        self.actor(session).map(|actor| actor.origin)
    }

    /// Metadata only: no enrollment proof or terminal capture enters discovery responses.
    pub fn actor(&self, session: &Session) -> Option<Actor> {
        self.bindings
            .get(&session.session_id)
            .filter(|b| same_terminal(&b.session, session))
            .map(|b| b.actor.clone())
    }

    /// A supported hook must establish or renew membership before any lifecycle input can drain.
    pub fn waiting(&self, session: &Session) -> bool {
        self.providers.contains(&session.process.client)
            && (self.origin(session).is_none() || self.renew.contains(&session.session_id))
    }

    /// GTK validates enrollment text against a reserved token, never an arbitrary caller-supplied prompt.
    pub fn accepts_enrollment(&self, text: &str, session: &Session) -> bool {
        self.challenges.get(&session.session_id).is_some_and(|c| {
            same_terminal(&c.session, session)
                && c.sent_at.elapsed() < Duration::from_secs(30)
                && text
                    == format!(
                        "{START}{}{STOP}",
                        json!({"version":1,"enrollment_token":c.token})
                    )
        })
    }

    /// Reserve one bounded bootstrap input after readiness; observe binding, not PTY send, as success.
    pub fn enrollment(&mut self, session: &Session) -> Option<String> {
        if !self.waiting(session)
            || !session.ready()
            || self.challenges.len() >= MAX_PENDING
                && !self.challenges.contains_key(&session.session_id)
        {
            return None;
        }
        if self
            .challenges
            .get(&session.session_id)
            .is_some_and(|c| c.sent_at.elapsed() < Duration::from_secs(30))
        {
            return None;
        }
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let text = format!(
            "{START}{}{STOP}",
            json!({"version":1,"enrollment_token":token})
        );
        let mut metadata = session.clone();
        metadata.terminal.screen = None;
        metadata.terminal.observation = None;
        metadata.terminal.captured_at = None;
        self.challenges.insert(
            session.session_id.clone(),
            Challenge {
                session: metadata,
                token,
                sent_at: Instant::now(),
            },
        );
        Some(text)
    }

    /// A no-token announcement enables bootstrap only; a token atomically binds its live recipient.
    pub fn announce(
        &mut self,
        actor: Actor,
        token: Option<&str>,
        repository: Option<&str>,
        current: &[Session],
        hook: bool,
    ) -> Result<Value, String> {
        self.retain(current);
        if token.is_some() && !hook {
            return Err("Enrollment must come from the provider prompt hook".into());
        }
        if hook {
            self.providers.insert(actor.origin.provider.clone());
        }
        if hook && token.is_none() && !self.bindings.values().any(|b| b.actor.same(&actor)) {
            let key = (
                actor.origin.session_id.clone(),
                actor.origin.instance_id.clone(),
            );
            if !self.seen_actors.contains(&key) {
                if self.seen_actors.len() >= MAX_PENDING {
                    self.seen_actors.pop_front();
                }
                self.seen_actors.push_back(key);
                // A new native conversation/runtime may occupy an unchanged TUI process.
                // Probe current attachments through their own hooks; never choose one by repository.
                for session in current
                    .iter()
                    .filter(|s| s.process.client == actor.origin.provider)
                {
                    self.renew.insert(session.session_id.clone());
                }
            }
        }
        if let Some(token) = token {
            if token.len() != 64
                || !token
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            {
                return Err("Invalid enrollment token".into());
            }
            let key = self
                .challenges
                .iter()
                .find(|(_, c)| c.token == token)
                .map(|(key, _)| key.clone())
                .ok_or("Enrollment token is unknown or consumed")?;
            let challenge = self.challenges.get(&key).ok_or("Enrollment unavailable")?;
            if challenge.sent_at.elapsed() >= Duration::from_secs(30)
                || challenge.session.process.client != actor.origin.provider
            {
                return Err("Enrollment token is stale or belongs to another provider".into());
            }
            if !current.iter().any(|s| same_terminal(&challenge.session, s)) {
                return Err("Enrollment terminal generation was replaced".into());
            }
            let session = challenge.session.clone();
            if repository
                .is_some_and(|repo| !session.repository.is_empty() && repo != session.repository)
            {
                return Err("Actor repository does not match enrollment recipient".into());
            }
            if self
                .bindings
                .values()
                .any(|b| b.actor.same(&actor) && !same_terminal(&b.session, &session))
            {
                return Err("Actor is already bound to another live terminal".into());
            }
            if self
                .bindings
                .get(&key)
                .is_some_and(|b| !b.actor.same(&actor))
                && !self.renew.contains(&key)
            {
                return Err("Terminal is already bound to another actor".into());
            }
            if self.bindings.len() >= MAX_PENDING && !self.bindings.contains_key(&key) {
                return Err("Actor binding capacity exceeded".into());
            }
            self.challenges.remove(&key);
            self.renew.remove(&key);
            self.bindings.insert(
                key,
                Binding {
                    session,
                    actor: actor.clone(),
                },
            );
        }
        let binding = self.bindings.values().find(|b| b.actor.same(&actor));
        let mut response = json!({"version":1,"origin":actor.origin,
            "provider_session_id":native_id(&actor.provider_session_id)?,"executor_generation":actor.executor_generation,
            "binding_state":if binding.is_some() {"bound"} else {"unbound"}});
        response["repository"] = json!(repository);
        if let Some(binding) = binding {
            if repository.is_some_and(|repo| {
                !binding.session.repository.is_empty() && repo != binding.session.repository
            }) {
                return Err("Actor repository does not match its native terminal".into());
            }
            response["surface_id"] = json!(binding.session.terminal.surface_id);
            response["workspace_id"] = json!(binding.session.terminal.workspace_id);
            response["recipient_session_id"] = json!(binding.session.session_id);
        }
        Ok(response)
    }
}
