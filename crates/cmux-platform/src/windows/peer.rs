//! Kernel-authenticated Windows pipe peers and non-consuming disconnect checks.
use std::{io, os::windows::io::AsRawHandle, ptr};
use windows_sys::Win32::System::Pipes::{GetNamedPipeClientProcessId, PeekNamedPipe};

/// Require the client process token's user SID to match CMUX's current user.
pub fn same_user(stream: &impl AsRawHandle) -> io::Result<bool> {
    let pid = credentials(stream)?.pid;
    let process = crate::windows::open_process(pid)?;
    Ok(crate::windows::process_sid(process.0)? == crate::windows::current_sid()?)
}

/// Kernel-authenticated caller metadata, independent of request-supplied actor IDs.
pub struct Credentials {
    /// Native Windows PID of the connected named-pipe client.
    pub pid: u32,
}

/// Read the actual client PID from a server-side named-pipe handle.
pub fn credentials(stream: &impl AsRawHandle) -> io::Result<Credentials> {
    let mut pid = 0;
    // SAFETY: pipe is borrowed for the call; output points to initialized exclusive storage.
    if unsafe { GetNamedPipeClientProcessId(stream.as_raw_handle(), &mut pid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if pid == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid pipe peer PID",
        ));
    }
    Ok(Credentials { pid })
}

/// Observe a broken pipe without consuming queued request bytes or blocking.
pub fn disconnected(stream: &impl AsRawHandle) -> io::Result<bool> {
    // SAFETY: no buffer is provided, so this only queries the live pipe's connection state.
    if unsafe {
        PeekNamedPipe(
            stream.as_raw_handle(),
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    } != 0
    {
        return Ok(false);
    }
    let error = io::Error::last_os_error();
    if matches!(error.raw_os_error(), Some(109 | 233)) {
        Ok(true)
    } else {
        Err(error)
    }
}
