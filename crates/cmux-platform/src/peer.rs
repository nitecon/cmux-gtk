//! Kernel-authenticated ownership checks for local control connections.

use std::io;
use std::os::fd::{AsFd, AsRawFd};

/// Check whether the connected peer belongs to this process's real user ID.
///
/// Accepts borrowed standard-library or Tokio sockets. Returns false for a
/// different user and an I/O error if the kernel cannot provide credentials.
pub fn same_user(socket: &impl AsFd) -> io::Result<bool> {
    let fd = socket.as_fd();
    let mut credential = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: fd stays borrowed for the call. Both output pointers reference
    // initialized, correctly sized and aligned stack values exclusive to us.
    let result = unsafe {
        libc::getsockopt(
            fd.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credential as *mut libc::ucred).cast(),
            &mut length,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    if length as usize != std::mem::size_of::<libc::ucred>() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid peer credential size",
        ));
    }
    // SAFETY: getuid takes no arguments and has no caller preconditions.
    Ok(credential.uid == unsafe { libc::getuid() })
}

/// Check full peer hangup or socket failure without consuming data or waiting.
/// A write-half shutdown alone is not a hangup: the caller may still read its response.
pub fn disconnected(socket: &impl AsFd) -> io::Result<bool> {
    let fd = socket.as_fd();
    let mut poll = libc::pollfd {
        fd: fd.as_raw_fd(),
        events: 0,
        revents: 0,
    };
    // SAFETY: the descriptor remains borrowed; poll references one initialized stack entry,
    // and timeout zero guarantees this kernel observation never waits for readiness.
    let result = unsafe { libc::poll(&mut poll, 1, 0) };
    if result < 0 {
        return Err(io::Error::last_os_error());
    }
    if poll.revents & libc::POLLNVAL != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid monitored socket",
        ));
    }
    Ok(poll.revents & (libc::POLLHUP | libc::POLLERR) != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exercise the production credential check using a real kernel socketpair.
    #[test]
    fn accepts_same_user_socketpair() {
        let (socket, _peer) = std::os::unix::net::UnixStream::pair().unwrap();
        assert!(same_user(&socket).unwrap());
    }

    /// Data and write-half closure remain usable; full closure is detectable even with unread bytes.
    #[test]
    fn distinguishes_half_close_from_disconnect() {
        use std::io::Write;
        let (socket, mut peer) = std::os::unix::net::UnixStream::pair().unwrap();
        peer.write_all(b"pending request").unwrap();
        assert!(!disconnected(&socket).unwrap());
        peer.shutdown(std::net::Shutdown::Write).unwrap();
        assert!(!disconnected(&socket).unwrap());
        drop(peer);
        assert!(disconnected(&socket).unwrap());
    }

    /// Non-sockets must fail closed instead of accepting an unknown peer.
    #[test]
    fn rejects_non_socket_descriptor() {
        let file = std::fs::File::open("/dev/null").unwrap();
        assert!(same_user(&file).is_err());
    }
}
