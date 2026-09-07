use serde::{Deserialize, Serialize};

/// Interactive PTY transport; SSH remains the management/browser transport in both modes.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TerminalTransport {
    #[default]
    Ssh,
    Mosh,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TerminalProfile {
    #[default]
    Shell,
    Tmux,
}

#[derive(Clone, Debug)]
pub struct MoshLaunch {
    pub target: String,
    pub directory: Option<String>,
    pub profile: TerminalProfile,
    pub tmux_session: Option<String>,
}

impl MoshLaunch {
    pub fn command(&self, resume: Option<&str>) -> Result<String, &'static str> {
        build_mosh_command(
            &self.target,
            self.directory.as_deref(),
            &self.profile,
            self.tmux_session.as_deref(),
            resume,
        )
    }
}

pub fn validate_tmux_session(value: &str) -> Result<(), &'static str> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    {
        return Err("tmux session must contain 1..64 letters, digits, dots, underscores or dashes");
    }
    Ok(())
}

/// Build a local PTY command which probes Mosh 1.4+ and falls back visibly to direct SSH.
pub fn mosh_command(
    target: &str,
    directory: Option<&str>,
    profile: &TerminalProfile,
    tmux_session: Option<&str>,
) -> Result<String, &'static str> {
    build_mosh_command(target, directory, profile, tmux_session, None)
}

fn build_mosh_command(
    target: &str,
    directory: Option<&str>,
    profile: &TerminalProfile,
    tmux_session: Option<&str>,
    resume: Option<&str>,
) -> Result<String, &'static str> {
    crate::workspace::validate_ssh_target(target).map_err(|_| "invalid SSH target")?;
    if directory.is_some_and(|value| !value.starts_with('/') || value.contains('\0')) {
        return Err("remote directory must be an absolute path");
    }
    if matches!(profile, TerminalProfile::Tmux) {
        validate_tmux_session(tmux_session.unwrap_or("main"))?;
    }
    let ssh_options = [
        "ssh",
        "-o",
        "ServerAliveInterval=15",
        "-o",
        "ServerAliveCountMax=3",
        "-o",
        "ConnectTimeout=10",
        "-o",
        "BatchMode=yes",
    ];
    let ssh_command = ssh_options
        .iter()
        .map(|value| crate::workspace::shell_quote(value))
        .collect::<Vec<_>>()
        .join(" ");
    let remote = remote_command(directory, profile, tmux_session, resume);
    let mut fallback = format!(
        "exec {ssh_command} -t {}",
        crate::workspace::shell_quote(target)
    );
    if let Some(remote) = &remote {
        fallback.push(' ');
        fallback.push_str(&crate::workspace::shell_quote(remote));
    }
    let probe = format!(
        "{ssh_command} -T {} {}",
        crate::workspace::shell_quote(target),
        crate::workspace::shell_quote(
            "/bin/sh -c 'command -v mosh-server >/dev/null 2>&1 || exit 127'"
        )
    );
    let mut mosh = format!(
        "exec \"$cmux_mosh\" --experimental-remote-ip=proxy {} -- {}",
        crate::workspace::shell_quote(&format!("--ssh={ssh_command}")),
        crate::workspace::shell_quote(target)
    );
    if let Some(remote) = remote {
        mosh.push(' ');
        mosh.push_str(&crate::workspace::shell_quote("/bin/sh"));
        mosh.push(' ');
        mosh.push_str(&crate::workspace::shell_quote("-lc"));
        mosh.push(' ');
        mosh.push_str(&crate::workspace::shell_quote(&remote));
    }
    let script = format!(
        "cmux_mosh=$(command -v mosh 2>/dev/null || true)\n\
         if [ -z \"$cmux_mosh\" ]; then printf '%s\\n' '[cmux] Mosh is not installed locally; continuing over SSH.' >&2; {fallback}; fi\n\
         case $(\"$cmux_mosh\" --help 2>&1 || true) in *--experimental-remote-ip=*) ;; *) printf '%s\\n' '[cmux] The local Mosh client lacks required SSH integration; continuing over SSH.' >&2; {fallback} ;; esac\n\
         {probe}\n\
         cmux_mosh_probe_status=$?\n\
         if [ \"$cmux_mosh_probe_status\" -eq 127 ]; then printf '%s\\n' '[cmux] mosh-server is not installed on the remote host; continuing over SSH.' >&2; {fallback}; fi\n\
         if [ \"$cmux_mosh_probe_status\" -ne 0 ]; then printf '%s\\n' '[cmux] Could not verify remote Mosh support; continuing over SSH.' >&2; {fallback}; fi\n\
         {mosh}"
    );
    Ok(format!(
        "/bin/sh -c {}",
        crate::workspace::shell_quote(&script)
    ))
}

fn remote_command(
    directory: Option<&str>,
    profile: &TerminalProfile,
    tmux_session: Option<&str>,
    resume: Option<&str>,
) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(directory) = directory {
        parts.push(format!(
            "cd {} || exit 1",
            crate::workspace::shell_quote(directory)
        ));
    }
    match profile {
        TerminalProfile::Shell if resume.is_some() => {
            parts.push(resume.unwrap().into());
            parts.push("exec \"${SHELL:-/bin/sh}\" -l".into());
        }
        TerminalProfile::Shell if directory.is_some() => {
            parts.push("exec \"${SHELL:-/bin/sh}\" -l".into())
        }
        TerminalProfile::Shell => {}
        TerminalProfile::Tmux => {
            let mut command = format!(
                "exec tmux new-session -A -s {}",
                crate::workspace::shell_quote(tmux_session.unwrap_or("main"))
            );
            if let Some(resume) = resume {
                command.push(' ');
                command.push_str(&crate::workspace::shell_quote(&format!(
                    "{resume}; exec \"${{SHELL:-/bin/sh}}\" -l"
                )));
            }
            parts.push(command);
        }
    }
    (!parts.is_empty()).then(|| parts.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn mosh_builder_probes_and_preserves_remote_intent() {
        let command = mosh_command(
            "dev@example",
            Some("/opt/team repo"),
            &TerminalProfile::Tmux,
            Some("agent-main"),
        )
        .unwrap();
        assert!(command.contains("--experimental-remote-ip=proxy"));
        assert!(command.contains("mosh-server"));
        assert!(command.contains("agent-main"));
        assert!(command.contains("/opt/team repo"));
        assert!(command.contains("continuing over SSH"));
        assert!(!command.contains("eval"));
    }

    #[test]
    fn mosh_builder_rejects_ambiguous_inputs() {
        assert!(mosh_command("host; touch /tmp/no", None, &TerminalProfile::Shell, None).is_err());
        assert!(mosh_command("host", Some("tmp"), &TerminalProfile::Shell, None).is_err());
        assert!(mosh_command("host", None, &TerminalProfile::Tmux, Some("bad session")).is_err());
    }

    #[test]
    fn restored_mosh_wraps_resume_on_the_remote_host() {
        let launch = MoshLaunch {
            target: "dev@example".into(),
            directory: Some("/srv/team repo".into()),
            profile: TerminalProfile::Shell,
            tmux_session: None,
        };
        let command = launch.command(Some("(env PROJECT='safe' /bin/sh -lc 'agent --resume id')"));
        let command = command.unwrap();
        assert!(command.contains("agent --resume id"));
        assert!(command.contains("dev@example"));
        assert!(command.contains("/srv/team repo"));
        assert!(command.contains("exec \"${SHELL:-/bin/sh}\" -l"));
    }

    #[test]
    fn mosh_command_executes_probed_client_with_ssh_management_options() {
        let root = std::env::temp_dir().join(format!("cmux-mosh-builder-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let ssh = root.join("ssh");
        let mosh = root.join("mosh");
        std::fs::write(
            &ssh,
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$SSH_LOG\"\nexit 0\n",
        )
        .unwrap();
        std::fs::write(
            &mosh,
            "#!/bin/sh\nif [ \"$1\" = --help ]; then printf '%s\\n' '  --experimental-remote-ip=(local|remote|proxy)'; exit 0; fi\nprintf '%s\\n' \"$*\" >> \"$MOSH_LOG\"\n",
        )
        .unwrap();
        for path in [&ssh, &mosh] {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let ssh_log = root.join("ssh.log");
        let mosh_log = root.join("mosh.log");
        let command = mosh_command(
            "dev@example",
            Some("/srv/repo"),
            &TerminalProfile::Tmux,
            Some("agent-main"),
        )
        .unwrap();
        let status = std::process::Command::new("/bin/sh")
            .args(["-c", &command])
            .env("PATH", &root)
            .env("SSH_LOG", &ssh_log)
            .env("MOSH_LOG", &mosh_log)
            .status()
            .unwrap();
        assert!(status.success());
        let ssh_args = std::fs::read_to_string(&ssh_log).unwrap();
        assert!(ssh_args.contains("BatchMode=yes"));
        assert!(ssh_args.contains("mosh-server"));
        let mosh_args = std::fs::read_to_string(&mosh_log).unwrap();
        assert!(mosh_args.contains("--experimental-remote-ip=proxy"));
        assert!(mosh_args.contains("dev@example"));
        assert!(mosh_args.contains("agent-main"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
