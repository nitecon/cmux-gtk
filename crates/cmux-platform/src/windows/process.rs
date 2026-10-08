//! Native Windows process inspection without terminal content or environment data.
use crate::windows::{self, filetime};
use std::io;
use windows_sys::Win32::{
    Foundation::FILETIME,
    System::{ProcessStatus::*, Threading::*},
};

/// Spawn and wait with inherited console streams; successful completion exits with the child's status.
/// Windows has no exec replacement; spawn or wait failure returns the OS error to the caller.
pub fn replace_current(command: &mut std::process::Command) -> io::Error {
    match command.status() {
        Ok(status) => std::process::exit(status.code().unwrap_or(1)),
        Err(error) => error,
    }
}

/// CPU model discovery is unavailable in this preview; absence remains distinguishable from a label.
pub fn cpu_model() -> Option<String> {
    None
}

/// A point-in-time sample; unavailable measurements remain absent.
#[derive(Debug, Default)]
pub struct Resources {
    /// Cumulative user CPU microseconds, excluding children.
    pub cpu_user_us: Option<u64>,
    /// Cumulative kernel CPU microseconds, excluding children.
    pub cpu_system_us: Option<u64>,
    /// Current working-set memory in KiB.
    pub rss_kib: Option<u64>,
    /// Peak working-set memory in KiB.
    pub peak_rss_kib: Option<u64>,
    /// Live thread count, when sampled.
    pub threads: Option<u64>,
    /// POSIX descriptor count; unavailable on Windows.
    pub file_descriptors: Option<usize>,
}

/// Read this process's CPU use and working set from Windows; call on a blocking worker.
pub fn resources() -> io::Result<Resources> {
    // SAFETY: process pseudo-handle is borrowed; outputs are correctly sized owned structures.
    unsafe {
        let process = GetCurrentProcess();
        let (mut creation, mut exit, mut kernel, mut user) =
            std::mem::zeroed::<(FILETIME, FILETIME, FILETIME, FILETIME)>();
        if GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut memory: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        memory.cb = std::mem::size_of_val(&memory) as u32;
        let available = GetProcessMemoryInfo(
            process,
            &mut memory,
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ) != 0;
        Ok(Resources {
            cpu_user_us: Some(filetime(user) / 10),
            cpu_system_us: Some(filetime(kernel) / 10),
            rss_kib: available.then_some((memory.WorkingSetSize / 1024) as u64),
            peak_rss_kib: available.then_some((memory.PeakWorkingSetSize / 1024) as u64),
            ..Resources::default()
        })
    }
}

/// A verified native agent executable and its process generation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    /// Windows PID of the executable.
    pub pid: u64,
    /// Process creation FILETIME ticks; prevents PID reuse from retaining readiness.
    pub start_ticks: u64,
    /// Verified provider name: claude or codex.
    pub client: String,
}

/// Verify one native agent in the terminal's current tree; ambiguous trees remain ineligible.
pub fn agent_identity(pid: u64) -> Option<Identity> {
    let root = crate::listeners::identity(u32::try_from(pid).ok()?).ok()?;
    let candidates = crate::listeners::process_tree(root).ok()?;
    let mut verified = None;
    for process in candidates {
        if let Some(agent) = native_identity(u64::from(process.pid)) {
            if agent.start_ticks != process.start_ticks || verified.is_some() {
                return None;
            }
            verified = Some(agent);
        }
    }
    if crate::listeners::identity(root.pid).ok()? != root {
        return None;
    }
    verified
}

/// Windows creation timestamps are globally qualified; terminal fences use a per-app fallback namespace.
pub fn boot_identity() -> Option<String> {
    None
}

/// Encode native creation time using the existing agent-tools Windows actor contract.
pub fn executor_generation(executor: &Identity) -> Option<Vec<String>> {
    Some(vec![
        "windows-process-v1".into(),
        executor.pid.to_string(),
        executor.start_ticks.to_string(),
    ])
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    pid: u32,
    parent: u32,
    start: u64,
    executable: std::path::PathBuf,
    command: Vec<String>,
    environment: Vec<(String, String)>,
}

/// Query one same-user generation through the same native inspection library as agent-tools.
/// Invocation strings stay bounded; only provider-native environment identifiers leave this helper.
fn snapshot(pid: u32, environment: bool) -> Option<Snapshot> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let handle = windows::open_process(pid).ok()?;
    if windows::process_sid(handle.0).ok()? != windows::current_sid().ok()? {
        return None;
    }
    let start = windows::process_start(handle.0).ok()?;
    let mut system = System::new();
    let mut refresh = ProcessRefreshKind::nothing()
        .without_tasks()
        .with_exe(UpdateKind::Always)
        .with_cmd(UpdateKind::Always);
    if environment {
        refresh = refresh.with_environ(UpdateKind::Always);
    }
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
        true,
        refresh,
    );
    let process = system.process(Pid::from_u32(pid))?;
    let executable = process.exe()?.to_path_buf();
    let command: Vec<String> = process
        .cmd()
        .iter()
        .map(|arg| arg.to_str().map(str::to_owned))
        .collect::<Option<_>>()?;
    if command.is_empty() || command.iter().map(|v| v.len() + 1).sum::<usize>() > 16384 {
        return None;
    }
    let mut ids = Vec::new();
    if environment {
        if process.environ().is_empty()
            || process.environ().iter().map(|v| v.len()).sum::<usize>() > 65536
        {
            return None;
        }
        for field in process.environ() {
            let Some((key, value)) = field.to_str()?.split_once('=') else {
                continue;
            };
            if matches!(
                key,
                "CODEX_THREAD_ID" | "CODEX_SESSION_ID" | "CLAUDE_CODE_SESSION_ID"
            ) {
                ids.push((key.to_owned(), value.to_owned()));
            }
        }
    }
    let parent = process.parent().map(|p| p.as_u32()).unwrap_or(0);
    if windows::process_start(handle.0).ok()? != start
        || crate::listeners::identity(pid).ok()?.start_ticks != start
    {
        return None;
    }
    Some(Snapshot {
        pid,
        parent,
        start,
        executable,
        command,
        environment: ids,
    })
}

/// Native clients and their official Node launchers are provider boundaries, never generic scripts.
fn provider(process: &Snapshot) -> Option<&'static str> {
    let name = process
        .executable
        .file_name()?
        .to_str()?
        .to_ascii_lowercase();
    match name.as_str() {
        "codex.exe" => Some("codex"),
        "claude.exe" => Some("claude"),
        _ if process
            .executable
            .parent()
            .is_some_and(|path| path.ends_with(".local/share/claude/versions"))
            && name.trim_end_matches(".exe").split('.').count() == 3
            && name.trim_end_matches(".exe").split('.').all(|part| {
                !part.is_empty() && part.len() <= 16 && part.bytes().all(|b| b.is_ascii_digit())
            }) =>
        {
            Some("claude")
        }
        "node.exe" => {
            let script = process.command.get(1)?.replace('\\', "/");
            if script.ends_with("/@anthropic-ai/claude-code/cli.js") {
                Some("claude")
            } else if script.ends_with("/@openai/codex/bin/codex.js") {
                Some("codex")
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Convert native argv to the shared bounded provider-role parser without shell interpretation.
fn command_bytes(process: &Snapshot) -> Vec<u8> {
    process
        .command
        .iter()
        .flat_map(|arg| arg.bytes().chain(Some(0)))
        .collect()
}

/// Recognize a live native agent or official Claude Node runtime in a terminal tree.
fn native_identity(pid: u64) -> Option<Identity> {
    let process = snapshot(u32::try_from(pid).ok()?, false)?;
    let client = provider(&process)?;
    // Codex's Node package launches its Rust binary; counting both would make every npm install ambiguous.
    if client == "codex"
        && process
            .executable
            .file_name()?
            .to_str()?
            .eq_ignore_ascii_case("node.exe")
    {
        return None;
    }
    Some(Identity {
        pid,
        start_ticks: process.start,
        client: client.into(),
    })
}

/// Walk only the connected caller's ancestry and recheck every generation before accepting an executor.
pub fn agent_executor(caller_pid: u64, requested: &str) -> Option<Identity> {
    if !matches!(requested, "codex" | "claude") {
        return None;
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let mut pid = u32::try_from(caller_pid).ok()?;
    let mut lineage: Vec<Snapshot> = Vec::new();
    for _ in 0..32 {
        if pid == 0 || std::time::Instant::now() >= deadline || lineage.iter().any(|p| p.pid == pid)
        {
            return None;
        }
        let process = snapshot(pid, false)?;
        if lineage
            .last()
            .is_some_and(|child| process.start > child.start)
        {
            return None;
        }
        let found = provider(&process);
        if let Some(client) = found {
            if client != requested
                || !crate::provider::executor_role(client, &command_bytes(&process))
            {
                return None;
            }
            if client == "codex"
                && process
                    .executable
                    .file_name()?
                    .to_str()?
                    .eq_ignore_ascii_case("node.exe")
            {
                return None;
            }
        }
        pid = process.parent;
        lineage.push(process);
        if let Some(client) = found {
            for expected in &lineage {
                if std::time::Instant::now() >= deadline
                    || snapshot(expected.pid, false).as_ref() != Some(expected)
                {
                    return None;
                }
            }
            let executor = lineage.last()?;
            return Some(Identity {
                pid: u64::from(executor.pid),
                start_ticks: executor.start,
                client: client.into(),
            });
        }
    }
    None
}

/// Return only this caller's native conversation fields; no ancestor environment supplies session identity.
pub fn provider_invocation_ids(pid: u64) -> Option<Vec<(String, String)>> {
    Some(snapshot(u32::try_from(pid).ok()?, true)?.environment)
}

/// Observe the installed prompt hook's executable and argv through native process metadata.
pub fn agent_tools_hook(pid: u64) -> bool {
    let Some(process) = u32::try_from(pid).ok().and_then(|pid| snapshot(pid, false)) else {
        return false;
    };
    process
        .executable
        .file_name()
        .is_some_and(|n| n.eq_ignore_ascii_case("agent-tools.exe"))
        && process.command.get(1).is_some_and(|v| v == "hook")
        && process
            .command
            .get(2)
            .is_some_and(|v| matches!(v.as_str(), "user-prompt-submit" | "session-start"))
}
