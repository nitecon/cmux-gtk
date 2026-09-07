//! Application-owned approvals, separate from agent-writable terminal bindings.
use crate::resume::ResumeBinding;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::BTreeMap;
use std::sync::OnceLock;

static SIGNING_KEY: OnceLock<Option<[u8; 32]>> = OnceLock::new();
const MAX_APPROVALS: usize = 128;

/// A launch reviewed in the UI, optionally scoped to initial literal arguments.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct Approval {
    command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    command_prefix: Option<Vec<String>>,
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
        if self.command_prefix.as_ref().is_some_and(|prefix| {
            prefix.is_empty()
                || prefix.iter().map(String::len).sum::<usize>() > 16384
                || prefix.len() > 8192
        }) {
            return false;
        }
        let Some(key) = SIGNING_KEY.get().and_then(Option::as_ref) else {
            return false;
        };
        let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts this key size");
        mac.update(&payload(
            &self.command,
            &self.cwd,
            &self.environment,
            self.command_prefix.as_deref(),
        ));
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

/// Canonical signing input binds the policy mode, reviewed command/prefix, directory and environment.
fn payload(
    command: &str,
    cwd: &str,
    environment: &BTreeMap<String, String>,
    prefix: Option<&[String]>,
) -> Vec<u8> {
    match prefix {
        Some(prefix) => {
            serde_json::to_vec(&("cmux-prefix-resume-v1", command, prefix, cwd, environment))
        }
        None => serde_json::to_vec(&("cmux-exact-resume-v1", command, cwd, environment)),
    }
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
            "resume.launch stage=schedule location=local approval=signed"
        ));
        Some(format!(
            "'{cli}' restore --automatic; exec \"${{SHELL:-/bin/sh}}\" -i"
        ))
    }

    /// Build one approved command for a newly opened remote interactive shell.
    /// The subshell keeps directory and environment changes out of the surrounding session.
    pub fn remote_shell_input(&self, binding: &ResumeBinding) -> Option<Vec<u8>> {
        let mut command = self.remote_shell_command(binding, "remote_ssh")?;
        command.push('\r');
        Some(command.into_bytes())
    }

    /// Render an approved binding for a transport-owned remote shell command argument.
    pub fn remote_shell_command(
        &self,
        binding: &ResumeBinding,
        location: &'static str,
    ) -> Option<String> {
        if !self.allows(binding) {
            return None;
        }
        let mut command = String::from("(");
        if let Some(directory) = binding.cwd.as_deref().filter(|value| !value.is_empty()) {
            command.push_str("cd ");
            command.push_str(&crate::workspace::shell_quote(directory));
            command.push_str(" && ");
        }
        command.push_str("env");
        for (key, value) in &binding.environment {
            command.push(' ');
            command.push_str(key);
            command.push('=');
            command.push_str(&crate::workspace::shell_quote(value));
        }
        command.push_str(" /bin/sh -lc ");
        command.push_str(&crate::workspace::shell_quote(&binding.command));
        command.push(')');
        if command.len() > 128 * 1024 {
            return None;
        }
        crate::diagnostics::event(format_args!(
            "resume.launch stage=schedule location={location} approval=signed"
        ));
        Some(command)
    }

    /// Discard excess or invalid records from disk before any launch decisions.
    pub fn validated(mut self) -> Self {
        self.approvals.truncate(MAX_APPROVALS);
        self.approvals.retain(Approval::valid_signature);
        self
    }

    /// Match a signed exact command or literal argument prefix, with exact directory and environment.
    pub fn allows(&self, binding: &ResumeBinding) -> bool {
        if binding.validate().is_err() {
            return false;
        }
        self.approvals.iter().any(|approval| {
            let matches_command = match &approval.command_prefix {
                Some(prefix) => crate::resume_command::literal_arguments(&binding.command)
                    .is_some_and(|arguments| arguments.starts_with(prefix)),
                None => approval.command == binding.command,
            };
            matches_command
                && Some(approval.cwd.as_str()) == binding.cwd.as_deref()
                && approval.environment == binding.environment
                && approval.valid_signature()
        })
    }

    /// Approve the current sanitized binding after UI review; requires an explicit absolute directory.
    pub fn approve(&mut self, binding: &ResumeBinding) -> Result<(), &'static str> {
        self.approve_launch(binding, None)
    }

    /// Review a literal argument prefix, retaining exact directory and environment scope.
    pub fn approve_prefix(
        &mut self,
        binding: &ResumeBinding,
        prefix: &str,
    ) -> Result<(), &'static str> {
        let arguments = crate::resume_command::literal_arguments(&binding.command).ok_or(
            "Prefix approval requires a literal command without shell expansion or control syntax.",
        )?;
        let prefix = crate::resume_command::literal_arguments(prefix)
            .filter(|prefix| arguments.starts_with(prefix))
            .ok_or("Enter complete initial command arguments as the prefix.")?;
        self.approve_launch(binding, Some(prefix))
    }

    /// Sign a reviewed exact or prefix launch after validating its complete execution context.
    fn approve_launch(
        &mut self,
        binding: &ResumeBinding,
        prefix: Option<Vec<String>>,
    ) -> Result<(), &'static str> {
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
        if self.approvals.iter().any(|approval| {
            approval.command == binding.command
                && approval.command_prefix == prefix
                && Some(approval.cwd.as_str()) == binding.cwd.as_deref()
                && approval.environment == binding.environment
                && approval.valid_signature()
        }) {
            return Ok(());
        }
        if self.approvals.len() >= MAX_APPROVALS {
            return Err("Revoke an existing approval before adding another (limit 128).");
        }
        let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts this key size");
        mac.update(&payload(
            &binding.command,
            cwd,
            &binding.environment,
            prefix.as_deref(),
        ));
        self.approvals.push(Approval {
            command: binding.command.clone(),
            command_prefix: prefix,
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

    /// Prefix authority permits a new checkpoint but not changed executables, context or shell control.
    #[test]
    fn signed_prefix_scope_and_integrity() {
        SIGNING_KEY.get_or_init(|| Some([73; 32]));
        let mut binding: ResumeBinding = serde_json::from_value(serde_json::json!({
            "command": "'/opt/agent cli' --resume 'session one'", "cwd": "/tmp/project",
            "environment": {"PROJECT": "literal $HOME"}
        }))
        .unwrap();
        let mut policy = ResumePolicy::default();
        policy
            .approve_prefix(&binding, "'/opt/agent cli' --resume")
            .unwrap();
        binding.command = "'/opt/agent cli' --resume 'session two'".into();
        assert!(policy.allows(&binding));
        let restored: ResumePolicy =
            serde_json::from_slice(&serde_json::to_vec(&policy).unwrap()).unwrap();
        assert!(restored.validated().allows(&binding));
        for command in [
            "'/opt/agent cli-other' --resume id",
            "'/opt/agent cli' --resume id; true",
            "'/opt/agent cli' --resume $(true)",
            "'/opt/agent cli' --resume id | true",
        ] {
            let mut changed = binding.clone();
            changed.command = command.into();
            assert!(!policy.allows(&changed));
        }
        let mut changed = binding.clone();
        changed.cwd = Some("/other".into());
        assert!(!policy.allows(&changed));
        changed = binding.clone();
        changed.environment.clear();
        assert!(!policy.allows(&changed));
        policy.approvals[0].command_prefix = Some(vec!["/opt/agent cli".into()]);
        assert!(
            !policy.allows(&binding),
            "changing a persisted prefix must invalidate its signature"
        );
    }

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

    #[test]
    fn approved_remote_input_quotes_directory_environment_and_command() {
        SIGNING_KEY.get_or_init(|| Some([73; 32]));
        let binding: ResumeBinding = serde_json::from_value(serde_json::json!({
            "command": "agent --resume 'a b'", "cwd": "/srv/a b",
            "environment": {"PROJECT": "x'y"}
        }))
        .unwrap();
        let mut policy = ResumePolicy::default();
        policy.approve(&binding).unwrap();
        let input = String::from_utf8(policy.remote_shell_input(&binding).unwrap()).unwrap();
        assert!(input.starts_with("(cd '/srv/a b' && env PROJECT='x'\\''y' /bin/sh -lc "));
        assert!(input.ends_with(")\r"));
        let mut rejected = binding;
        rejected.command = "other".into();
        assert!(policy.remote_shell_input(&rejected).is_none());
    }
}
