//! Validate client-owned registration metadata without modifying provider prompts or terminal state.
use super::model::Origin;
use serde::Deserialize;
use serde_json::{json, Value};
/// Client-owned registration; native terminal generations never define a v2 logical identity.
#[derive(Clone, Debug, Deserialize)]
pub struct Actor {
    #[serde(default = "legacy_version")]
    pub version: u8,
    pub origin: Origin,
    pub provider_session_id: String,
    #[serde(default)]
    pub executor_generation: Vec<String>,
    #[serde(default)]
    pub base_id: Option<String>,
    #[serde(default)]
    pub session_slot: Option<u32>,
    #[serde(default)]
    pub actor_id: Option<String>,
}

/// Announcements without a version are the original client contract.
fn legacy_version() -> u8 {
    1
}

#[cfg(test)]
mod tests {
    use super::*;

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
            version: 1,
            origin: Origin {
                session_id: uuid::Uuid::new_v5(&instance, &name).to_string(),
                instance_id: instance.to_string(),
                provider: "codex".into(),
                os: "linux".into(),
            },
            provider_session_id: native.into(),
            executor_generation: epoch,
            base_id: None,
            session_slot: None,
            actor_id: None,
        }
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

    /// Registration uses the SDK's published project/conversation vectors, without process identity.
    #[test]
    fn registered_contract_vectors_and_plain_local_caller() {
        let vectors: Vec<Value> = serde_json::from_str(include_str!(
            "../../tests/fixtures/actor-registration-vectors.json"
        ))
        .unwrap();
        for vector in &vectors {
            let registered: Actor = serde_json::from_value(json!({
                "version":2,
                "origin":{"session_id":vector["session_id"],"instance_id":vector["instance_id"],
                    "provider":vector["provider"],"os":vector["os"]},
                "provider_session_id":vector["provider_session_id"],
                "base_id":vector["base_id"],"session_slot":vector["session_slot"],
                "actor_id":vector["actor_id"]
            }))
            .unwrap();
            registered.validate().unwrap();
            let echo = registered
                .announcement(Some("github.com/org/repo"))
                .unwrap();
            assert_eq!(echo["binding_state"], "unbound");
            assert_eq!(echo["origin"]["session_id"], registered.origin.session_id);
            assert_eq!(echo["actor_id"], registered.actor_id.as_deref().unwrap());
            assert!(echo.get("enrollment_token").is_none());
            assert!(registered.executor_generation.is_empty());
            let name = json!([
                "agent-tools-actor-v2",
                native_id(&registered.provider_session_id).unwrap()
            ])
            .to_string();
            assert_eq!(name, vector["name_utf8"].as_str().unwrap());
            let namespace = uuid::Uuid::parse_str(&registered.origin.instance_id).unwrap();
            let base = uuid::Uuid::new_v5(
                &namespace,
                vector["base_name_utf8"].as_str().unwrap().as_bytes(),
            );
            assert_eq!(base.to_string(), registered.base_id.as_deref().unwrap());
            if registered.origin.os == std::env::consts::OS {
                registered.verify_peer(std::process::id()).unwrap();
            } else {
                assert!(registered.verify_peer(std::process::id()).is_err());
            }
            let mut invalid = registered.clone();
            invalid.origin.session_id = uuid::Uuid::new_v4().to_string();
            assert!(invalid.validate().is_err());
            invalid = registered.clone();
            invalid.session_slot = Some(0);
            assert!(invalid.validate().is_err());
            invalid = registered;
            invalid.executor_generation = vec!["legacy-runtime".into()];
            assert!(invalid.validate().is_err());
        }
        assert_eq!(vectors[0]["session_id"], vectors[2]["session_id"]);
        assert_eq!(vectors[0]["session_id"], vectors[3]["session_id"]);
        assert_ne!(vectors[0]["session_id"], vectors[1]["session_id"]);
    }
}

/// Normalize a bounded native conversation identifier without selecting a terminal.
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
        if namespace.to_string() != self.origin.instance_id {
            return Err("Invalid actor namespace".into());
        }
        let native = native_id(&self.provider_session_id)?;
        if self.version == 2 {
            let base_id = self
                .base_id
                .as_deref()
                .ok_or("Missing registration base identity")?;
            let base =
                uuid::Uuid::parse_str(base_id).map_err(|_| "Invalid registration base identity")?;
            if base.to_string() != base_id {
                return Err("Invalid registration base identity".into());
            }
            let slot = self
                .session_slot
                .filter(|slot| *slot > 0)
                .ok_or("Invalid numbered session registration")?;
            if !self.executor_generation.is_empty() {
                return Err("Invalid numbered session registration".into());
            }
            if self.actor_id.as_deref() != Some(format!("{base_id}-{slot}").as_str()) {
                return Err("Readable actor identity does not match its registration".into());
            }
            let name = json!(["agent-tools-actor-v2", native]).to_string();
            if uuid::Uuid::new_v5(&base, name.as_bytes()).to_string() != self.origin.session_id {
                return Err("Actor identity does not match its registered conversation".into());
            }
            return Ok(());
        }
        if self.version != 1
            || self.base_id.is_some()
            || self.session_slot.is_some()
            || self.actor_id.is_some()
        {
            return Err("Unsupported actor registration".into());
        }
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
        if uuid::Uuid::new_v5(&namespace, &name).to_string() != self.origin.session_id {
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
        // The socket already authenticates the local caller. Registration is not
        // contingent on recognizing a provider's installation or executor role.
        if self.version == 2 {
            return Ok(());
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

    /// Echo a validated registration without creating membership or submitting terminal input.
    pub fn announcement(&self, repository: Option<&str>) -> Result<Value, String> {
        self.validate()?;
        let mut response = json!({"version":self.version,"origin":self.origin,
            "provider_session_id":native_id(&self.provider_session_id)?,
            "binding_state":"unbound","repository":repository});
        self.describe(&mut response);
        Ok(response)
    }

    /// Add versioned registration metadata to the announcement reply.
    pub fn describe(&self, context: &mut Value) {
        context["version"] = json!(self.version);
        if self.version == 2 {
            context["base_id"] = json!(self.base_id);
            context["session_slot"] = json!(self.session_slot);
            context["actor_id"] = json!(self.actor_id);
        } else {
            context["executor_generation"] = json!(self.executor_generation);
        }
    }
}
