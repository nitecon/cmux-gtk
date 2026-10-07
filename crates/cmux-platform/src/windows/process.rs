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

/// Recognize a live native executable belonging to the same Windows user.
fn native_identity(pid: u64) -> Option<Identity> {
    let process = windows::open_process(u32::try_from(pid).ok()?).ok()?;
    if windows::process_sid(process.0).ok()? != windows::current_sid().ok()? {
        return None;
    }
    let start_ticks = windows::process_start(process.0).ok()?;
    let mut path = [0u16; 32768];
    let mut length = path.len() as u32;
    // SAFETY: live query handle and a properly sized writable UTF-16 buffer.
    if unsafe { QueryFullProcessImageNameW(process.0, 0, path.as_mut_ptr(), &mut length) } == 0 {
        return None;
    }
    let path = String::from_utf16(&path[..length as usize]).ok()?;
    let executable = std::path::Path::new(&path)
        .file_name()?
        .to_str()?
        .to_ascii_lowercase();
    let client = match executable.as_str() {
        "codex.exe" => "codex",
        "claude.exe" => "claude",
        _ => return None,
    };
    if windows::process_start(process.0).ok()? != start_ticks {
        return None;
    }
    Some(Identity {
        pid,
        start_ticks,
        client: client.into(),
    })
}
