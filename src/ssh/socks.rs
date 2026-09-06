//! Workspace-lifetime SOCKS endpoint; each request is admitted to exactly one SSH generation.
use super::bridge::SshBridge;
use std::{
    io,
    net::{Ipv4Addr, Ipv6Addr},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::mpsc,
    task::JoinSet,
};

/// A validated CONNECT request whose success reply still belongs to the forwarding worker.
pub(super) struct Request {
    pub socket: TcpStream,
    pub host: String,
    pub port: u16,
}

/// Clear generation admission only if this owner still publishes the current sender.
pub(super) struct Admission(pub Arc<SshBridge>, pub mpsc::Sender<Request>);
impl Drop for Admission {
    fn drop(&mut self) {
        let mut current = self.0.browser_proxy_requests.lock().unwrap();
        if current
            .as_ref()
            .is_some_and(|sender| sender.same_channel(&self.1))
        {
            *current = None;
        }
    }
}

/// Send a bounded SOCKS reply; the local bound address is not used by CONNECT clients.
pub(super) async fn reply(socket: &mut TcpStream, status: u8) -> io::Result<()> {
    tokio::time::timeout(
        Duration::from_secs(5),
        socket.write_all(&[5, status, 0, 1, 0, 0, 0, 0, 0, 0]),
    )
    .await
    .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
}

/// Parse bounded SOCKS5 no-auth CONNECT fields without resolving the destination locally.
async fn negotiate(socket: &mut TcpStream) -> io::Result<(String, u16)> {
    let invalid = || io::Error::from(io::ErrorKind::InvalidData);
    if socket.read_u8().await? != 5 {
        return Err(invalid());
    }
    let count = socket.read_u8().await? as usize;
    let mut methods = [0u8; 255];
    socket.read_exact(&mut methods[..count]).await?;
    if !methods[..count].contains(&0) {
        socket.write_all(&[5, 255]).await?;
        return Err(invalid());
    }
    socket.write_all(&[5, 0]).await?;
    let mut header = [0u8; 4];
    socket.read_exact(&mut header).await?;
    if header[..3] != [5, 1, 0] {
        reply(socket, 7).await?;
        return Err(invalid());
    }
    let host = match header[3] {
        1 => {
            let mut bytes = [0u8; 4];
            socket.read_exact(&mut bytes).await?;
            Ipv4Addr::from(bytes).to_string()
        }
        4 => {
            let mut bytes = [0u8; 16];
            socket.read_exact(&mut bytes).await?;
            Ipv6Addr::from(bytes).to_string()
        }
        3 => {
            let size = socket.read_u8().await? as usize;
            let mut bytes = [0u8; 255];
            socket.read_exact(&mut bytes[..size]).await?;
            let host = std::str::from_utf8(&bytes[..size]).map_err(|_| invalid())?;
            if host.is_empty()
                || !host
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b))
            {
                return Err(invalid());
            }
            host.to_owned()
        }
        _ => {
            reply(socket, 8).await?;
            return Err(invalid());
        }
    };
    let port = socket.read_u16().await?;
    if port == 0 {
        return Err(invalid());
    }
    Ok((host, port))
}

/// Retain the loopback bind through reconnects; cap handshakes and reject disconnected admission.
/// Lifecycle cancellation drops negotiation tasks; the bridge retains the bind until workspace release.
pub(super) async fn run(bridge: Arc<SshBridge>) {
    let existing = bridge.browser_proxy_listener.lock().unwrap().clone();
    let listener = match existing {
        Some(listener) => listener,
        None => {
            let Ok(listener) = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await else {
                return;
            };
            let listener = Arc::new(listener);
            *bridge.browser_proxy_listener.lock().unwrap() = Some(listener.clone());
            listener
        }
    };
    let Ok(address) = listener.local_addr() else {
        return;
    };
    bridge
        .browser_proxy_port
        .store(address.port(), std::sync::atomic::Ordering::Release);
    let mut tasks = JoinSet::new();
    loop {
        while tasks.try_join_next().is_some() {}
        tokio::select! {
            Some(_) = tasks.join_next(), if !tasks.is_empty() => {},
            accepted = listener.accept() => {
                let Ok((mut socket,_))=accepted else { break; };
                if tasks.len() >= 16 { super::forward_metrics::client_rejected(); continue; }
                // Capture generation at accept, not after a potentially slow greeting.
                let sender=bridge.browser_proxy_requests.lock().unwrap().clone();
                tasks.spawn(async move {
                    let parsed=tokio::time::timeout(Duration::from_secs(5),negotiate(&mut socket)).await;
                    if let Ok(Ok((host,port)))=parsed {
                        let request=Request{socket,host,port};
                        let rejected=match sender {
                            Some(sender) => sender.try_send(request).err().map(|error|error.into_inner()),
                            None => Some(request),
                        };
                        if let Some(mut request)=rejected {
                            super::forward_metrics::client_rejected();
                            let _=reply(&mut request.socket,1).await;
                        }
                    }
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Preserve bytes after CONNECT and forward a hostname without local DNS resolution.
    #[tokio::test]
    async fn hostname_negotiation_preserves_following_payload() {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let server = async {
            let (mut socket, _) = listener.accept().await.unwrap();
            let destination = negotiate(&mut socket).await.unwrap();
            assert_eq!(destination, ("remote.invalid".into(), 8080));
            let mut following = [0u8; 4];
            socket.read_exact(&mut following).await.unwrap();
            assert_eq!(&following, b"body");
            reply(&mut socket, 0).await.unwrap();
        };
        let client = async {
            let mut socket = TcpStream::connect(address).await.unwrap();
            socket.write_all(&[5, 1, 0]).await.unwrap();
            let mut response = [0u8; 2];
            socket.read_exact(&mut response).await.unwrap();
            assert_eq!(response, [5, 0]);
            socket.write_all(&[5, 1, 0, 3, 14]).await.unwrap();
            socket
                .write_all(b"remote.invalid\x1f\x90body")
                .await
                .unwrap();
            let mut reply = [0u8; 10];
            socket.read_exact(&mut reply).await.unwrap();
            assert_eq!(reply, [5, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
        };
        tokio::time::timeout(Duration::from_secs(3), async {
            tokio::join!(server, client);
        })
        .await
        .unwrap();
    }

    /// Unsupported authentication fails without admitting a destination or consuming further requests.
    #[tokio::test]
    async fn rejects_unsupported_authentication() {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let server = async {
            let (mut socket, _) = listener.accept().await.unwrap();
            assert_eq!(
                negotiate(&mut socket).await.unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        };
        let client = async {
            let mut socket = TcpStream::connect(address).await.unwrap();
            socket.write_all(&[5, 1, 2]).await.unwrap();
            let mut reply = [0u8; 2];
            socket.read_exact(&mut reply).await.unwrap();
            assert_eq!(reply, [5, 255]);
        };
        tokio::time::timeout(Duration::from_secs(3), async {
            tokio::join!(server, client);
        })
        .await
        .unwrap();
    }
}
