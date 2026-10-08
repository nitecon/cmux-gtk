//! Linux process resource inspection without terminal content or environment data.

use crate::provider::executor_role;
use std::io;
use std::io::Read;

/// Read a bounded Linux boot generation once on a worker; None means unavailable or unsupported.
pub fn boot_identity() -> Option<String> {
    static BOOT: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    BOOT.get_or_init(|| {
        let text = crate::filesystem::read_text_bounded(
            std::path::Path::new("/proc/sys/kernel/random/boot_id"),
            128,
        )
        .ok()?;
        let value = text.trim();
        (!value.is_empty()
            && value.len() <= 64
            && value.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'))
        .then(|| value.to_owned())
    })
    .clone()
}

/// Replace the calling process with a prepared command, retaining its terminal descriptors.
/// Success never returns; an exec failure returns the OS error without spawning a second process.
pub fn replace_current(command: &mut std::process::Command) -> io::Error {
    use std::os::unix::process::CommandExt;
    command.exec()
}

/// Read the first kernel CPU model name from at most 64 KiB of procfs data.
/// Blocking worker-only I/O; None means unavailable or unsupported, not generic hardware.
/// This identifies one reported model, not every CPU in a heterogeneous system.
pub fn cpu_model() -> Option<String> {
    let mut bytes = Vec::new();
    std::fs::File::open("/proc/cpuinfo")
        .ok()?
        .take(64 * 1024)
        .read_to_end(&mut bytes)
        .ok()?;
    parse_cpu_model(std::str::from_utf8(&bytes).ok()?)
}

/// Select a complete, nonempty model-name line; reject oversized or control-bearing labels.
fn parse_cpu_model(cpuinfo: &str) -> Option<String> {
    cpuinfo.split_inclusive('\n').find_map(|line| {
        if !line.ends_with('\n') {
            return None;
        }
        let (key, value) = line.split_once(':')?;
        let value = value.trim();
        (key.trim() == "model name"
            && !value.is_empty()
            && value.len() <= 256
            && !value.chars().any(char::is_control))
        .then(|| value.to_owned())
    })
}

/// A point-in-time resource sample for this process, in kernel-reported units.
#[derive(Debug, Default)]
pub struct Resources {
    /// Cumulative user-mode CPU microseconds across this process's threads, excluding children.
    pub cpu_user_us: Option<u64>,
    /// Cumulative kernel-mode CPU microseconds across this process's threads, excluding children.
    pub cpu_system_us: Option<u64>,
    /// Resident memory in KiB, or None when unavailable.
    pub rss_kib: Option<u64>,
    /// Peak resident memory in KiB, or None when unavailable.
    pub peak_rss_kib: Option<u64>,
    /// Number of live kernel threads, or None when unavailable.
    pub threads: Option<u64>,
    /// Open file descriptors, excluding the directory used for this sample.
    pub file_descriptors: Option<usize>,
}

/// Read this process's Linux resources; reject unavailable, invalid or over-64-KiB status data.
///
/// Performs blocking filesystem I/O; call on a worker, never on GTK's main thread.
pub fn resources() -> io::Result<Resources> {
    let status =
        crate::filesystem::read_text_bounded(std::path::Path::new("/proc/self/status"), 64 * 1024)?;
    let mut sample = parse_status(&status);
    sample.file_descriptors = std::fs::read_dir("/proc/self/fd")
        .ok()
        .map(|entries| entries.filter_map(Result::ok).count().saturating_sub(1));
    if let Some((user, system)) = cpu_times() {
        sample.cpu_user_us = Some(user);
        sample.cpu_system_us = Some(system);
    }
    Ok(sample)
}

/// Sample cumulative CPU use for all threads, keeping syscall failure distinct from zero.
fn cpu_times() -> Option<(u64, u64)> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: usage points to writable storage of the required size; RUSAGE_SELF
    // needs no external handle. Read the initialized structure only after success.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: successful getrusage initialized the structure above.
    let usage = unsafe { usage.assume_init() };
    Some((timeval_us(usage.ru_utime)?, timeval_us(usage.ru_stime)?))
}

/// Convert a normalized kernel timeval to microseconds without signed casts or overflow.
fn timeval_us(value: libc::timeval) -> Option<u64> {
    let seconds = u64::try_from(value.tv_sec).ok()?;
    let micros = u64::try_from(value.tv_usec).ok()?;
    if micros >= 1_000_000 {
        return None;
    }
    seconds.checked_mul(1_000_000)?.checked_add(micros)
}

/// Extract known numeric status fields, leaving missing or malformed values absent.
fn parse_status(status: &str) -> Resources {
    let mut sample = Resources::default();
    for line in status.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let number = value
            .split_whitespace()
            .next()
            .and_then(|value| value.parse().ok());
        match key {
            "VmRSS" => sample.rss_kib = number,
            "VmHWM" => sample.peak_rss_kib = number,
            "Threads" => sample.threads = number,
            _ => {}
        }
    }
    sample
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CPU indices and unrelated fields are not model identities; truncated or invalid labels stay absent.
    #[test]
    fn cpu_model_uses_complete_kernel_labels() {
        assert_eq!(
            parse_cpu_model(
                "processor : 0\nmodel name\t: Example CPU 12\nmodel name: Second CPU\n"
            ),
            Some("Example CPU 12".into())
        );
        assert_eq!(parse_cpu_model("processor: 0\nHardware: board\n"), None);
        assert_eq!(parse_cpu_model("model name: truncated"), None);
        assert_eq!(
            parse_cpu_model("model name: \nmodel name: valid\n"),
            Some("valid".into())
        );
        assert_eq!(parse_cpu_model("model name: bad\0label\n"), None);
        assert_eq!(
            parse_cpu_model(&format!("model name: {}\n", "x".repeat(257))),
            None
        );
    }

    /// Missing and malformed values remain distinguishable from measured zero.
    #[test]
    fn handles_partial_status() {
        let sample = parse_status("Name:\tcmux\nVmRSS:\t1024 kB\nThreads:\tinvalid\n");
        assert_eq!(sample.rss_kib, Some(1024));
        assert_eq!(sample.threads, None);
        assert_eq!(sample.peak_rss_kib, None);
    }

    /// Reject malformed kernel time values and preserve exact fractional microseconds.
    #[test]
    fn cpu_time_conversion() {
        assert_eq!(
            timeval_us(libc::timeval {
                tv_sec: 2,
                tv_usec: 3
            }),
            Some(2_000_003)
        );
        assert_eq!(
            timeval_us(libc::timeval {
                tv_sec: -1,
                tv_usec: 0
            }),
            None
        );
        assert_eq!(
            timeval_us(libc::timeval {
                tv_sec: 0,
                tv_usec: 1_000_000
            }),
            None
        );
    }

    /// Exercise live procfs access, including the descriptor-count path.
    #[test]
    fn samples_current_process() {
        let sample = resources().unwrap();
        assert!(sample.rss_kib.unwrap() > 0);
        assert!(sample.threads.unwrap() > 0);
        assert!(sample.file_descriptors.unwrap() >= 3);
        let again = resources().unwrap();
        assert!(again.cpu_user_us.unwrap() >= sample.cpu_user_us.unwrap());
        assert!(again.cpu_system_us.unwrap() >= sample.cpu_system_us.unwrap());
    }
}

/// A live foreground agent generation; start ticks prevent PID reuse from reusing readiness.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    /// Kernel PID of the verified foreground executable.
    pub pid: u64,
    /// Kernel start time in ticks since boot, used to identify this process generation.
    pub start_ticks: u64,
    /// Verified provider name: claude or codex.
    pub client: String,
}

/// Verify a foreground Claude/Codex executable from bounded procfs metadata on a blocking worker.
/// Generic interpreters, shell command strings and exited processes never establish agent identity.
pub fn agent_identity(pid: u64) -> Option<Identity> {
    if pid == 0 || pid > i32::MAX as u64 {
        return None;
    }
    let root = std::path::PathBuf::from(format!("/proc/{pid}"));
    let stat = crate::filesystem::read_text_bounded(&root.join("stat"), 4096).ok()?;
    let start_ticks = process_start(&stat)?;
    let executable = std::fs::read_link(root.join("exe")).ok()?;
    let name = executable
        .file_name()?
        .to_str()?
        .trim_end_matches(" (deleted)");
    let client = match native_agent(&executable) {
        Some(client) => client,
        None if name == "node" => {
            let mut bytes = Vec::new();
            std::fs::File::open(root.join("cmdline"))
                .ok()?
                .take(4097)
                .read_to_end(&mut bytes)
                .ok()?;
            if bytes.len() > 4096 {
                return None;
            }
            node_agent(&bytes)?
        }
        None => return None,
    }
    .to_owned();
    // Confirm that the metadata belonged to the same still-live process generation.
    let after = crate::filesystem::read_text_bounded(&root.join("stat"), 4096).ok()?;
    if process_start(&after)? != start_ticks {
        return None;
    }
    Some(Identity {
        pid,
        start_ticks,
        client,
    })
}

/// Recognize named native clients and Claude's documented versioned native-install layout.
/// Procfs appends `(deleted)` to a still-running executable after an updater unlinks its old version.
fn native_agent(executable: &std::path::Path) -> Option<&'static str> {
    let name = executable
        .file_name()?
        .to_str()?
        .trim_end_matches(" (deleted)");
    match name {
        "claude" => Some("claude"),
        "codex" => Some("codex"),
        _ if executable
            .parent()
            .is_some_and(|p| p.ends_with(".local/share/claude/versions")) =>
        {
            let mut parts = name.split('.');
            let version = (0..3).all(|_| {
                parts.next().is_some_and(|p| {
                    !p.is_empty() && p.len() <= 16 && p.bytes().all(|b| b.is_ascii_digit())
                })
            }) && parts.next().is_none();
            version.then_some("claude")
        }
        _ => None,
    }
}

/// Parse the kernel start-time field after the parenthesized command, rejecting zombies and truncation.
fn process_start(stat: &str) -> Option<u64> {
    let fields: Vec<&str> = stat.rsplit_once(')')?.1.split_whitespace().collect();
    if matches!(*fields.first()?, "Z" | "X" | "x") {
        return None;
    }
    fields.get(19)?.parse().ok()
}

/// Recognize only known provider CLI entry points in node's script argument, never arbitrary arguments.
fn node_agent(cmdline: &[u8]) -> Option<&'static str> {
    let script = std::str::from_utf8(cmdline.split(|b| *b == 0).nth(1)?).ok()?;
    if script.ends_with("/@anthropic-ai/claude-code/cli.js") {
        Some("claude")
    } else if script.ends_with("/@openai/codex/bin/codex.js") {
        Some("codex")
    } else {
        None
    }
}

/// Encode a verified executor in the shared agent-tools actor-generation contract.
pub fn executor_generation(executor: &Identity) -> Option<Vec<String>> {
    Some(vec![
        "linux-proc-v1".into(),
        boot_identity()?,
        executor.pid.to_string(),
        executor.start_ticks.to_string(),
    ])
}

/// Find the actual provider executor of a local caller on a blocking worker.
/// Thread identity is supplied separately by the provider invocation; ancestor environment and
/// terminal identity are never used. Every hop is bounded and rechecked against PID reuse.
pub fn agent_executor(caller_pid: u64, provider: &str) -> Option<Identity> {
    if !matches!(provider, "codex" | "claude") {
        return None;
    }
    let mut pid = caller_pid;
    for _ in 0..32 {
        if pid == 0 || pid > i32::MAX as u64 {
            return None;
        }
        let root = std::path::PathBuf::from(format!("/proc/{pid}"));
        let before = crate::filesystem::read_text_bounded(&root.join("stat"), 4096).ok()?;
        let start = process_start(&before)?;
        let fields: Vec<_> = before.rsplit_once(')')?.1.split_whitespace().collect();
        let parent: u64 = fields.get(1)?.parse().ok()?;
        if let Some(identity) = agent_identity(pid) {
            if identity.client != provider || identity.start_ticks != start {
                return None;
            }
            let mut command = Vec::new();
            std::fs::File::open(root.join("cmdline"))
                .ok()?
                .take(16 * 1024 + 1)
                .read_to_end(&mut command)
                .ok()?;
            if command.len() > 16 * 1024 || !executor_role(provider, &command) {
                return None;
            }
            // Codex's official Node entrypoint launches Rust; it is not a model executor.
            if provider == "codex"
                && std::fs::read_link(root.join("exe"))
                    .ok()?
                    .file_name()?
                    .to_str()?
                    == "node"
            {
                return None;
            }
            return (agent_identity(pid)? == identity).then_some(identity);
        }
        let after = crate::filesystem::read_text_bounded(&root.join("stat"), 4096).ok()?;
        let after_fields: Vec<_> = after.rsplit_once(')')?.1.split_whitespace().collect();
        if process_start(&after)? != start
            || after_fields.get(1)?.parse::<u64>().ok()? != parent
            || parent == pid
        {
            return None;
        }
        pid = parent;
    }
    None
}

/// Read only provider-native identifiers from a bounded caller environment; no terminal or secret data escapes.
pub fn provider_invocation_ids(pid: u64) -> Option<Vec<(String, String)>> {
    if pid == 0 || pid > i32::MAX as u64 {
        return None;
    }
    let path = std::path::PathBuf::from(format!("/proc/{pid}/environ"));
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(65537)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 65536 {
        return None;
    }
    let mut result = Vec::new();
    for field in bytes.split(|b| *b == 0) {
        let Some(index) = field.iter().position(|b| *b == b'=') else {
            continue;
        };
        let key = &field[..index];
        if matches!(
            key,
            b"CODEX_THREAD_ID" | b"CODEX_SESSION_ID" | b"CLAUDE_CODE_SESSION_ID"
        ) {
            result.push((
                std::str::from_utf8(key).ok()?.to_owned(),
                std::str::from_utf8(&field[index + 1..]).ok()?.to_owned(),
            ));
        }
    }
    Some(result)
}

/// Observe the configured agent-tools hook execution rather than trusting a capability flag in JSON.
pub fn agent_tools_hook(pid: u64) -> bool {
    let path = std::path::PathBuf::from(format!("/proc/{pid}"));
    let Ok(exe) = std::fs::read_link(path.join("exe")) else {
        return false;
    };
    if exe.file_name().is_none_or(|name| name != "agent-tools") {
        return false;
    }
    let mut bytes = Vec::new();
    let Ok(mut file) = std::fs::File::open(path.join("cmdline")) else {
        return false;
    };
    if file.by_ref().take(4097).read_to_end(&mut bytes).is_err() || bytes.len() > 4096 {
        return false;
    }
    let args: Vec<_> = bytes.split(|b| *b == 0).collect();
    args.get(1) == Some(&b"hook".as_slice())
        && args
            .get(2)
            .is_some_and(|value| matches!(*value, b"user-prompt-submit" | b"session-start"))
}

#[cfg(test)]
mod agent_tests {
    use super::*;

    /// Frontends, Node launchers and utilities cannot become a replacement executor after a daemon dies.
    #[test]
    fn executor_roles_require_execution_evidence() {
        for args in [
            b"codex\0app-server\0".as_slice(),
            b"codex\0exec\0hi\0",
            b"codex\0--no-daemon\0",
            b"codex\0resume\0--no-daemon\0",
        ] {
            assert!(executor_role("codex", args));
        }
        for args in [
            b"codex\0".as_slice(),
            b"codex\0login\0--no-daemon\0",
            b"codex\0unknown\0--no-daemon\0",
            b"codex\0app-server\0generate-ts\0",
            b"codex\0resume\0",
            b"codex\0--no-daemon\0--help\0",
            b"codex\0--no-daemon\0-m\0",
        ] {
            assert!(!executor_role("codex", args));
        }
    }

    /// Versioned native installs remain agents after updater unlinking; generic versioned binaries never qualify.
    #[test]
    fn recognizes_native_install_executables() {
        for (path, client) in [
            ("/usr/bin/codex", Some("codex")),
            ("/usr/bin/claude (deleted)", Some("claude")),
            (
                "/home/user/.local/share/claude/versions/2.1.288",
                Some("claude"),
            ),
            (
                "/home/user/.local/share/claude/versions/2.1.287 (deleted)",
                Some("claude"),
            ),
            ("/home/user/.local/share/claude/versions/helper", None),
            ("/home/user/.local/share/claude/versions/2.1.2.3", None),
            ("/home/user/.local/share/unrelated/versions/2.1.288", None),
            ("/usr/bin/bash", None),
        ] {
            assert_eq!(native_agent(std::path::Path::new(path)), client);
        }
    }

    /// Command mentions and generic scripts are not live provider identities.
    #[test]
    fn recognizes_only_provider_entry_points() {
        assert_eq!(
            node_agent(b"node\0/usr/lib/node_modules/@anthropic-ai/claude-code/cli.js\0"),
            Some("claude")
        );
        assert_eq!(
            node_agent(b"node\0/usr/lib/node_modules/@openai/codex/bin/codex.js\0"),
            Some("codex")
        );
        assert_eq!(node_agent(b"node\0-e\0claude\0"), None);
        assert_eq!(node_agent(b"node\0/tmp/claude.js\0"), None);
        assert_eq!(agent_identity(0), None);
        assert_eq!(agent_identity(std::process::id().into()), None);
    }

    /// A command containing spaces or closing parentheses cannot shift the process-generation field.
    #[test]
    fn process_generation_rejects_exited_and_truncated_metadata() {
        let fields = format!("S {} 12345", vec!["0"; 18].join(" "));
        assert_eq!(
            process_start(&format!("123 (a tricky ) name) {fields}")),
            Some(12345)
        );
        assert_eq!(process_start("123 (claude) Z 0 0"), None);
        assert_eq!(process_start("123 (codex) S 0 0"), None);
    }
}
