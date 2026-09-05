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

#[cfg(test)]
mod tests {
    use super::*;

    /// Exercise the production credential check using a real kernel socketpair.
    #[test]
    fn accepts_same_user_socketpair() {
        let (socket, _peer) = std::os::unix::net::UnixStream::pair().unwrap();
        assert!(same_user(&socket).unwrap());
    }

    /// Non-sockets must fail closed instead of accepting an unknown peer.
    #[test]
    fn rejects_non_socket_descriptor() {
        let file = std::fs::File::open("/dev/null").unwrap();
        assert!(same_user(&file).is_err());
    }
}
