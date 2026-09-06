//! Application-owned approvals, separate from agent-writable terminal bindings.
use crate::resume::ResumeBinding;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::BTreeMap;
use std::sync::OnceLock;

static SIGNING_KEY: OnceLock<Option<[u8; 32]>> = OnceLock::new();
const MAX_APPROVALS: usize = 128;

/// An exact launch reviewed in the UI. Presentation metadata is deliberately not authority.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct Approval {
    command: String,
    cwd: String,
    environment: BTreeMap<String, String>,
    signature: Vec<u8>,
}

impl Approval {
    /// Authenticate persisted authority with the app-owned key, including every execution input.
    fn valid_signature(&self) -> bool {
        if self.command.len() > 16384
            || self.cwd.len() > 16384
            || self.environment.len() > 64
            || self.signature.len() != 32
            || self.command.len()
                + self.cwd.len()
                + self
                    .environment
                    .iter()
                    .map(|(key, value)| key.len() + value.len())
                    .sum::<usize>()
                > 65536
        {
            return false;
        }
        let Some(key) = SIGNING_KEY.get().and_then(Option::as_ref) else {
            return false;
        };
        let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts this key size");
        mac.update(&payload(&self.command, &self.cwd, &self.environment));
        mac.verify_slice(&self.signature).is_ok()
    }
}

/// Bounded approval records travel through the single session writer; the signing key never does.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ResumePolicy {
    #[serde(default)]
    approvals: Vec<Approval>,
}

/// Initialize once before GTK starts. An unavailable key disables approval rather than trusting data.
pub fn initialize() {
    let path = crate::config::config_path().with_file_name("resume-approval.key");
    let key = cmux_platform::filesystem::load_or_create_secret(&path);
    if let Err(ref error) = key {
        eprintln!(
            "cmux: automatic resume approval unavailable: {}",
            error.kind()
        );
    }
    let _ = SIGNING_KEY.set(key.ok());
}

/// Canonical signing input includes a format/domain marker and exact command, directory and env.
fn payload(command: &str, cwd: &str, environment: &BTreeMap<String, String>) -> Vec<u8> {
    serde_json::to_vec(&("cmux-exact-resume-v1", command, cwd, environment))
        .expect("strings and string maps serialize")
}

impl ResumePolicy {
    /// Schedule approved local execution through the owning CLI, which rechecks approval at launch.
    /// Return to an interactive shell after command exit/failure so the restored terminal remains usable.
    pub fn launch_command(&self, binding: &ResumeBinding) -> Option<String> {
        if !self.allows(binding) {
            return None;
        }
        let cli = std::env::current_exe().ok()?.with_file_name("cmux");
        let cli = cli.to_str()?.replace('\'', "'\\''");
        crate::diagnostics::event(format_args!(
            "resume.launch stage=schedule location=local approval=exact"
        ));
        Some(format!(
            "'{cli}' restore --automatic; exec \"${{SHELL:-/bin/sh}}\" -i"
        ))
    }

    /// Discard excess or invalid records from disk before any launch decisions.
    pub fn validated(mut self) -> Self {
        self.approvals.truncate(MAX_APPROVALS);
        self.approvals.retain(Approval::valid_signature);
        self
    }

    /// Return whether a valid binding exactly matches a signed, explicitly reviewed launch.
    pub fn allows(&self, binding: &ResumeBinding) -> bool {
        if binding.validate().is_err() {
            return false;
        }
        self.approvals.iter().any(|approval| {
            approval.command == binding.command
                && Some(approval.cwd.as_str()) == binding.cwd.as_deref()
                && approval.environment == binding.environment
                && approval.valid_signature()
        })
    }

    /// Approve the current sanitized binding after UI review; requires an explicit absolute directory.
    pub fn approve(&mut self, binding: &ResumeBinding) -> Result<(), &'static str> {
        binding.validate()?;
        let cwd = binding
            .cwd
            .as_deref()
            .filter(|cwd| std::path::Path::new(cwd).is_absolute())
            .ok_or("Set an absolute resume directory before approving this command.")?;
        let mut sanitized = binding.clone();
        sanitized.sanitize_environment();
        if sanitized.environment != binding.environment {
            return Err("Remove secret or routing overrides before approval.");
        }
        let key = SIGNING_KEY
            .get()
            .and_then(Option::as_ref)
            .ok_or("The resume signing key is unavailable.")?;
        if self.allows(binding) {
            return Ok(());
        }
        if self.approvals.len() >= MAX_APPROVALS {
            return Err("Revoke an existing approval before adding another (limit 128).");
        }
        let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts this key size");
        mac.update(&payload(&binding.command, cwd, &binding.environment));
        self.approvals.push(Approval {
            command: binding.command.clone(),
            cwd: cwd.into(),
            environment: binding.environment.clone(),
            signature: mac.finalize().into_bytes().to_vec(),
        });
        Ok(())
    }

    /// Revoke every saved approval without modifying any terminal's manual resume binding.
    pub fn revoke_all(&mut self) {
        self.approvals.clear();
    }

    /// Number of validated approvals, for the preferences review panel.
    pub fn len(&self) -> usize {
        self.approvals.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Changing any execution input invalidates approval; round trips retain valid authority only.
    #[test]
    fn signed_approval_matches_exact_launch_and_rejects_tampering() {
        SIGNING_KEY.get_or_init(|| Some([73; 32]));
        let binding: ResumeBinding = serde_json::from_value(serde_json::json!({
            "command": "agent --resume 'native session'", "cwd": "/tmp/project",
            "environment": {"PROJECT": "literal $HOME"}
        }))
        .unwrap();
        let mut policy = ResumePolicy::default();
        assert!(!policy.allows(&binding));
        policy.approve(&binding).unwrap();
        policy.approve(&binding).unwrap();
        assert_eq!(policy.len(), 1);
        let bytes = serde_json::to_vec(&policy).unwrap();
        let restored: ResumePolicy = serde_json::from_slice(&bytes).unwrap();
        assert!(restored.validated().allows(&binding));
        for field in ["command", "cwd", "environment"] {
            let mut changed = binding.clone();
            match field {
                "command" => changed.command.push_str("; touch /tmp/unapproved"),
                "cwd" => changed.cwd = Some("/other/project".into()),
                _ => {
                    changed.environment.insert("EXTRA".into(), "value".into());
                }
            }
            assert!(!policy.allows(&changed));
            let mut tampered = policy.clone();
            tampered.approvals[0].command = changed.command.clone();
            tampered.approvals[0].cwd = changed.cwd.clone().unwrap();
            tampered.approvals[0].environment = changed.environment.clone();
            assert!(!tampered.allows(&changed));
            assert_eq!(tampered.validated().len(), 0);
        }
        policy.revoke_all();
        assert!(!policy.allows(&binding));
        let mut invalid = binding;
        invalid.cwd = None;
        assert!(policy.approve(&invalid).is_err());
    }
}
