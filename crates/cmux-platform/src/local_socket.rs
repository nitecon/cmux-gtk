//! Native synchronous local connection setup, independent of application framing.

use socket2::{Domain, SockAddr, Socket, Type};
use std::io;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

/// Connect within a positive retry budget and return a blocking stream with I/O timeouts.
///
/// Linux reports a full Unix listener backlog as WouldBlock. Retry with a fresh
/// nonblocking socket every ten milliseconds, releasing failed descriptors before
/// waiting. Other connection errors propagate immediately. Filesystem resolution
/// and kernel scheduling are outside the userspace deadline guarantee. Framing,
/// authentication and per-exchange total deadlines remain the caller's policy.
pub fn connect(path: &Path, timeout: Duration) -> io::Result<UnixStream> {
    if timeout.is_zero() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "zero connection timeout",
        ));
    }
    let started = Instant::now();
    let address = SockAddr::unix(path)?;
    loop {
        let remaining = timeout
            .checked_sub(started.elapsed())
            .filter(|left| !left.is_zero())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    "local connection deadline exceeded",
                )
            })?;
        let socket = Socket::new(Domain::UNIX, Type::STREAM, None)?;
        socket.set_nonblocking(true)?;
        match socket.connect(&address) {
            Ok(()) => {
                socket.set_nonblocking(false)?;
                socket.set_read_timeout(Some(timeout))?;
                socket.set_write_timeout(Some(timeout))?;
                return Ok(UnixStream::from(OwnedFd::from(socket)));
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                drop(socket);
                std::thread::sleep(remaining.min(Duration::from_millis(10)));
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    /// Own a unique bound socket pathname and remove it after the listener closes.
    struct Listener {
        socket: Option<Socket>,
        path: PathBuf,
    }

    impl Listener {
        /// Bind a real local listener with one pending-connection slot for saturation coverage.
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "cmux-connect-{}-{}.sock",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let socket = Socket::new(Domain::UNIX, Type::STREAM, None).unwrap();
            socket.bind(&SockAddr::unix(&path).unwrap()).unwrap();
            let listener = Self {
                socket: Some(socket),
                path,
            };
            listener.socket.as_ref().unwrap().listen(0).unwrap();
            listener
        }
    }

    impl Drop for Listener {
        /// Release the listener before unlinking its exclusively owned pathname, including on panic.
        fn drop(&mut self) {
            drop(self.socket.take());
            let _ = std::fs::remove_file(&self.path);
        }
    }

    /// Exchange bytes over the returned blocking stream and check its configured I/O bounds.
    #[test]
    fn connects_and_configures_io() {
        let listener = Listener::new();
        let timeout = Duration::from_millis(100);
        let mut client = connect(&listener.path, timeout).unwrap();
        let (peer, _) = listener.socket.as_ref().unwrap().accept().unwrap();
        let mut peer = UnixStream::from(OwnedFd::from(peer));
        assert_eq!(client.read_timeout().unwrap(), Some(timeout));
        assert_eq!(client.write_timeout().unwrap(), Some(timeout));
        peer.write_all(b"hello").unwrap();
        let mut bytes = [0; 5];
        client.read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"hello");
        let error = client.read(&mut bytes).unwrap_err();
        assert!(matches!(
            error.kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
        ));
    }

    /// A saturated kernel backlog expires and later connections recover after the listener accepts its pending peer.
    #[test]
    fn backlog_timeout_then_recovers() {
        let listener = Listener::new();
        let _first = connect(&listener.path, Duration::from_secs(1)).unwrap();
        let started = Instant::now();
        let error = connect(&listener.path, Duration::from_millis(30)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(2));
        let (_accepted, _) = listener.socket.as_ref().unwrap().accept().unwrap();
        let mut next = connect(&listener.path, Duration::from_secs(1)).unwrap();
        let (peer, _) = listener.socket.as_ref().unwrap().accept().unwrap();
        let mut peer = UnixStream::from(OwnedFd::from(peer));
        next.write_all(b"x").unwrap();
        let mut byte = [0];
        peer.read_exact(&mut byte).unwrap();
        assert_eq!(byte, [b'x']);
    }

    /// Reject zero budgets and nonexistent endpoints without entering a retry loop.
    #[test]
    fn rejects_invalid_budget_and_missing_endpoint() {
        let listener = Listener::new();
        assert_eq!(
            connect(&listener.path, Duration::ZERO).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        let missing = listener.path.with_extension("missing");
        assert_eq!(
            connect(&missing, Duration::from_secs(1))
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
    }
}
