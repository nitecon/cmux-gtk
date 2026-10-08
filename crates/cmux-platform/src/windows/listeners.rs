//! Windows process identities; POSIX terminal-device attribution is unavailable with ConPTY.
use std::{io, net::IpAddr, path::Path};

/// A PID qualified by its native process creation time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessIdentity {
    /// Windows process ID.
    pub pid: u32,
    /// Process creation FILETIME ticks.
    pub start_ticks: u64,
}
/// A listening socket attributed to a qualified process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listener {
    /// Owning process generation.
    pub process: ProcessIdentity,
    /// Bind address.
    pub address: IpAddr,
    /// Host-order TCP port.
    pub port: u16,
    /// Observation identifier; Windows does not provide POSIX socket inodes.
    pub inode: u64,
}
/// Query a live process generation on a blocking worker.
pub fn identity(pid: u32) -> io::Result<ProcessIdentity> {
    let process = crate::windows::open_process(pid)?;
    Ok(ProcessIdentity {
        pid,
        start_ticks: crate::windows::process_start(process.0)?,
    })
}
/// ConPTY does not expose a kernel terminal device usable for POSIX listener attribution.
pub fn controlling_terminal(_process: ProcessIdentity) -> io::Result<Option<u64>> {
    Err(unsupported())
}
/// ConPTY has no POSIX terminal-device pathname.
pub fn terminal_device(_path: &Path) -> io::Result<u64> {
    Err(unsupported())
}
/// Discover at most 256 current descendants, validating generations around a Toolhelp snapshot.
pub fn process_tree(root: ProcessIdentity) -> io::Result<Vec<ProcessIdentity>> {
    use windows_sys::Win32::{
        Foundation::{ERROR_NO_MORE_FILES, INVALID_HANDLE_VALUE},
        System::Diagnostics::ToolHelp::*,
    };
    if identity(root.pid)? != root {
        return Ok(Vec::new());
    }
    // SAFETY: query-only snapshot, returned handle is owned and closed by RAII.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let snapshot = crate::windows::Handle(snapshot);
    // SAFETY: PROCESSENTRY32W has a defined zero initialization with dwSize supplied below.
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of_val(&entry) as u32;
    let mut parents = std::collections::HashMap::<u32, Vec<u32>>::new();
    // SAFETY: owned snapshot and correctly sized writable structure.
    let mut more = unsafe { Process32FirstW(snapshot.0, &mut entry) };
    let mut count = 0;
    while more != 0 {
        count += 1;
        if count > 65536 {
            return Err(io::Error::other("process snapshot exceeds bound"));
        }
        parents
            .entry(entry.th32ParentProcessID)
            .or_default()
            .push(entry.th32ProcessID);
        // SAFETY: the same snapshot and output remain live throughout iteration.
        more = unsafe { Process32NextW(snapshot.0, &mut entry) };
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() != Some(ERROR_NO_MORE_FILES as i32) {
        return Err(error);
    }
    let mut result = Vec::new();
    let mut pending = vec![root];
    let mut visited = std::collections::HashSet::new();
    while let Some(parent) = pending.pop() {
        if !visited.insert(parent.pid) {
            continue;
        }
        if visited.len() > 256 {
            return Err(io::Error::other("terminal process tree exceeds bound"));
        }
        if identity(parent.pid).ok() != Some(parent) {
            continue;
        }
        result.push(parent);
        for child in parents.get(&parent.pid).into_iter().flatten() {
            if let Ok(child) = identity(*child) {
                if child.start_ticks >= parent.start_ticks {
                    pending.push(child);
                }
            }
        }
    }
    if identity(root.pid)? != root {
        return Ok(Vec::new());
    }
    Ok(result)
}
/// TCP listener attribution is unavailable in this preview; never report an empty successful scan.
pub fn listening_tcp(_processes: &[ProcessIdentity]) -> io::Result<Vec<Listener>> {
    Err(unsupported())
}
fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "ConPTY listener attribution is unavailable in the Windows preview",
    )
}
