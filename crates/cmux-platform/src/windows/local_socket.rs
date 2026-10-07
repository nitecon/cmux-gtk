//! Local-only, owner-secured Windows named pipes with the shared byte-stream contract.

use crate::windows::PrivateSecurity;
use std::os::windows::io::{AsRawHandle, RawHandle};
use std::{
    cell::Cell,
    io,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::windows::named_pipe::{ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions},
};

enum Pipe {
    Server(NamedPipeServer),
    Client(NamedPipeClient),
}

/// Shared Windows pipe handle; one reader and one writer may poll it concurrently.
#[derive(Clone)]
pub struct Stream(Arc<Pipe>);

impl AsRawHandle for Stream {
    fn as_raw_handle(&self) -> RawHandle {
        match &*self.0 {
            Pipe::Server(p) => p.as_raw_handle(),
            Pipe::Client(p) => p.as_raw_handle(),
        }
    }
}

impl Stream {
    /// Connect asynchronously; a busy pipe retries until the caller's deadline cancels this future.
    pub async fn connect(path: impl AsRef<Path>) -> io::Result<Self> {
        loop {
            match ClientOptions::new().open(path.as_ref()) {
                Ok(pipe) => return Ok(Self(Arc::new(Pipe::Client(pipe)))),
                Err(error) if error.raw_os_error() == Some(231) => {
                    tokio::time::sleep(Duration::from_millis(10)).await
                }
                Err(error) => return Err(error),
            }
        }
    }

    /// Split ownership into a reader and a writer borrowing the same live kernel pipe.
    pub fn into_split(self) -> (ReadHalf, WriteHalf) {
        (ReadHalf(self.clone()), WriteHalf(self))
    }

    fn read(&self, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        loop {
            let ready = match &*self.0 {
                Pipe::Server(p) => p.poll_read_ready(cx),
                Pipe::Client(p) => p.poll_read_ready(cx),
            };
            std::task::ready!(ready)?;
            let bytes = buf.initialize_unfilled();
            let result = match &*self.0 {
                Pipe::Server(p) => p.try_read(bytes),
                Pipe::Client(p) => p.try_read(bytes),
            };
            match result {
                Ok(n) => {
                    buf.advance(n);
                    return Poll::Ready(Ok(()));
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                Err(e) if matches!(e.raw_os_error(), Some(109 | 233)) => {
                    return Poll::Ready(Ok(()))
                }
                Err(e) => return Poll::Ready(Err(e)),
            }
        }
    }

    fn write(&self, cx: &mut Context<'_>, bytes: &[u8]) -> Poll<io::Result<usize>> {
        loop {
            let ready = match &*self.0 {
                Pipe::Server(p) => p.poll_write_ready(cx),
                Pipe::Client(p) => p.poll_write_ready(cx),
            };
            std::task::ready!(ready)?;
            let result = match &*self.0 {
                Pipe::Server(p) => p.try_write(bytes),
                Pipe::Client(p) => p.try_write(bytes),
            };
            match result {
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                other => return Poll::Ready(other),
            }
        }
    }
}

impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        self.read(cx, buf)
    }
}
impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.write(cx, bytes)
    }
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

/// Owned read side; the handle remains alive while either side exists.
pub struct ReadHalf(Stream);
impl AsyncRead for ReadHalf {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        self.0.read(cx, buf)
    }
}
/// Owned write side, retaining access to the full pipe for disconnect monitoring.
pub struct WriteHalf(Stream);
impl AsRef<Stream> for WriteHalf {
    fn as_ref(&self) -> &Stream {
        &self.0
    }
}
impl AsyncWrite for WriteHalf {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.0.write(cx, bytes)
    }
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

fn server(path: &Path, first: bool) -> io::Result<NamedPipeServer> {
    let security = PrivateSecurity::new()?;
    let mut attributes = security.attributes();
    // SAFETY: attributes and its owned descriptor remain valid until synchronous pipe creation returns.
    unsafe {
        ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .create_with_security_attributes_raw(
                path,
                (&mut attributes as *mut windows_sys::Win32::Security::SECURITY_ATTRIBUTES).cast(),
            )
    }
}

/// Tokio listener with an owner-only DACL and rejection of remote clients.
pub struct Listener {
    path: PathBuf,
    pending: tokio::sync::Mutex<NamedPipeServer>,
}
impl Listener {
    /// Bind inside an entered Tokio runtime; existing owners make the first-instance claim fail.
    pub fn bind(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_owned();
        Ok(Self {
            pending: tokio::sync::Mutex::new(server(&path, true)?),
            path,
        })
    }
    /// Accept a client and prepare the next server instance before returning the connection.
    pub async fn accept(&self) -> io::Result<(Stream, ())> {
        let mut pending = self.pending.lock().await;
        pending.connect().await?;
        let next = server(&self.path, false)?;
        let connected = std::mem::replace(&mut *pending, next);
        Ok((Stream(Arc::new(Pipe::Server(connected))), ()))
    }
}

/// Blocking CLI adapter; clones share the kernel connection and its private I/O runtime.
pub struct BlockingStream {
    runtime: Arc<tokio::runtime::Runtime>,
    stream: Stream,
    read_timeout: Cell<Option<Duration>>,
    write_timeout: Cell<Option<Duration>>,
    closed: Arc<AtomicBool>,
}
impl BlockingStream {
    /// Clone the local stream ownership without reconnecting.
    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            runtime: self.runtime.clone(),
            stream: self.stream.clone(),
            read_timeout: self.read_timeout.clone(),
            write_timeout: self.write_timeout.clone(),
            closed: self.closed.clone(),
        })
    }
    /// Bound subsequent reads; zero is invalid.
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        check_timeout(timeout)?;
        self.read_timeout.set(timeout);
        Ok(())
    }
    /// Bound subsequent writes; zero is invalid.
    pub fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        check_timeout(timeout)?;
        self.write_timeout.set(timeout);
        Ok(())
    }
    /// Retire both halves after a failed exchange; named pipes do not support POSIX half-close.
    pub fn shutdown(&self, _how: std::net::Shutdown) -> io::Result<()> {
        self.closed.store(true, Ordering::Release);
        Ok(())
    }
}
fn check_timeout(timeout: Option<Duration>) -> io::Result<()> {
    if timeout.is_some_and(|v| v.is_zero()) {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "zero I/O timeout",
        ))
    } else {
        Ok(())
    }
}
async fn bounded<T>(
    timeout: Option<Duration>,
    future: impl std::future::Future<Output = io::Result<T>>,
) -> io::Result<T> {
    match timeout {
        Some(timeout) => tokio::time::timeout(timeout, future)
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "pipe I/O deadline exceeded"))?,
        None => future.await,
    }
}
impl io::Read for BlockingStream {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if self.closed.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "retired connection",
            ));
        }
        use tokio::io::AsyncReadExt;
        self.runtime.block_on(bounded(
            self.read_timeout.get(),
            AsyncReadExt::read(&mut self.stream, bytes),
        ))
    }
}
impl io::Write for BlockingStream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.closed.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "retired connection",
            ));
        }
        use tokio::io::AsyncWriteExt;
        self.runtime.block_on(bounded(
            self.write_timeout.get(),
            AsyncWriteExt::write(&mut self.stream, bytes),
        ))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Connect with a positive retry budget and enforce the same per-I/O timeout on the returned adapter.
pub fn connect(path: &Path, timeout: Duration) -> io::Result<BlockingStream> {
    check_timeout(Some(timeout))?;
    let runtime = Arc::new(
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?,
    );
    let stream = runtime.block_on(bounded(Some(timeout), Stream::connect(path)))?;
    Ok(BlockingStream {
        runtime,
        stream,
        read_timeout: Cell::new(Some(timeout)),
        write_timeout: Cell::new(Some(timeout)),
        closed: Arc::new(AtomicBool::new(false)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exercise real named-pipe ownership, exclusive binding and cloned blocking CLI I/O.
    #[tokio::test]
    async fn local_pipe_roundtrip() {
        let path = PathBuf::from(format!(
            r"\\.\pipe\cmux-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let listener = Listener::bind(&path).unwrap();
        assert!(Listener::bind(&path).is_err());
        let client_path = path.clone();
        let client = std::thread::spawn(move || {
            use std::io::{Read, Write};
            let mut reader = connect(&client_path, Duration::from_secs(3)).unwrap();
            let mut writer = reader.try_clone().unwrap();
            writer.write_all(b"request\n").unwrap();
            let mut response = [0; 3];
            reader.read_exact(&mut response).unwrap();
            assert_eq!(&response, b"ok\n");
        });
        let (mut stream, _) = tokio::time::timeout(Duration::from_secs(3), listener.accept())
            .await
            .unwrap()
            .unwrap();
        assert!(crate::peer::same_user(&stream).unwrap());
        assert!(!crate::peer::disconnected(&stream).unwrap());
        let mut request = [0; 8];
        tokio::io::AsyncReadExt::read_exact(&mut stream, &mut request)
            .await
            .unwrap();
        assert_eq!(&request, b"request\n");
        tokio::io::AsyncWriteExt::write_all(&mut stream, b"ok\n")
            .await
            .unwrap();
        tokio::task::spawn_blocking(move || client.join())
            .await
            .unwrap()
            .unwrap();
    }
}
