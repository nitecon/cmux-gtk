//! Connection-owned forwarding of discovered services through the existing remote RPC stream protocol.
use super::{
    bridge::SshBridge,
    tunnel::{request_remote, PendingMap},
    writer::RpcWriter,
};
use base64::Engine;
use std::{
    collections::{BTreeSet, HashMap},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, BufWriter},
    sync::{mpsc, watch, Semaphore},
    task::JoinSet,
};

/// Bounded incoming proxy routes, separate from terminal routing so slow clients cannot stall RPC replies.
#[derive(Default)]
pub struct Routes(Mutex<HashMap<String, mpsc::Sender<Vec<u8>>>>);
impl Routes {
    /// Install a fresh stream route atomically; a duplicate never replaces or retires its existing owner.
    fn register(&self, stream: &str) -> Result<mpsc::Receiver<Vec<u8>>, String> {
        let mut routes = self.0.lock().unwrap();
        if routes.contains_key(stream) {
            return Err("duplicate proxy stream".into());
        }
        if routes.len() >= 16 {
            return Err("proxy route capacity exceeded".into());
        }
        let (sender, receiver) = mpsc::channel(16);
        routes.insert(stream.to_owned(), sender);
        Ok(receiver)
    }

    /// Consume known proxy events without awaiting client I/O; overload closes that client route.
    pub fn event(&self, stream: &str, event: &str, message: &serde_json::Value) -> bool {
        let mut routes = self.0.lock().unwrap();
        let Some(sender) = routes.get(stream) else {
            return false;
        };
        match event {
            "proxy.stream.data" => {
                let data = message
                    .get("data_base64")
                    .and_then(|value| value.as_str())
                    .filter(|text| text.len() <= 43692)
                    .and_then(|text| base64::engine::general_purpose::STANDARD.decode(text).ok())
                    .filter(|bytes| bytes.len() <= 32768);
                if data.is_none_or(|data| sender.try_send(data).is_err()) {
                    routes.remove(stream);
                    crate::diagnostics::record(
                        "ssh.forward.client_rejected",
                        serde_json::json!({"reason":"invalid_or_full_input"}),
                    );
                }
            }
            "proxy.stream.eof" | "proxy.stream.error" => {
                routes.remove(stream);
            }
            _ => {}
        }
        true
    }
}

/// Shared connection transport; no mutex is held over a remote exchange.
struct Transport {
    writer: Arc<RpcWriter<BufWriter<tokio::process::ChildStdin>>>,
    pending: PendingMap,
    bridge: Arc<SshBridge>,
}
impl Transport {
    /// Preserve the existing RPC deadline, identity and tracing for each forwarding operation.
    async fn call(
        &self,
        method: &'static str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let id = self.bridge.next_id();
        let response = request_remote(
            &self.writer,
            &self.pending,
            id,
            method,
            serde_json::json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
        )
        .await?;
        if response.get("ok").and_then(|value| value.as_bool()) != Some(true) {
            return Err("remote forwarding request rejected".into());
        }
        Ok(response.get("result").cloned().unwrap_or_default())
    }
}

/// Retire a known stream even on cancellation; the existing bounded control channel owns delivery.
struct ClientOwner {
    transport: Arc<Transport>,
    stream: String,
}
impl Drop for ClientOwner {
    fn drop(&mut self) {
        self.transport
            .bridge
            .proxy_routes
            .0
            .lock()
            .unwrap()
            .remove(&self.stream);
        self.transport.bridge.request_close(self.stream.clone());
    }
}

/// Open one remote stream, install data routing before subscribing, then copy bounded chunks both ways.
/// A service-stop waits for an in-flight open to settle so its returned stream can be retired.
async fn client(
    transport: Arc<Transport>,
    socket: tokio::net::TcpStream,
    remote: SocketAddr,
    mut stop: watch::Receiver<bool>,
) -> Result<(), String> {
    let destination = match remote.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    let opened = transport
        .call(
            "proxy.open",
            serde_json::json!({"host":destination.to_string(),"port":remote.port()}),
        )
        .await?;
    let stream = opened
        .get("stream_id")
        .and_then(|value| value.as_str())
        .filter(|id| !id.is_empty() && id.len() <= super::outbound::MAX_STREAM_ID)
        .ok_or("invalid proxy stream")?
        .to_owned();
    let mut incoming = match transport.bridge.proxy_routes.register(&stream) {
        Ok(incoming) => incoming,
        Err(error) => {
            if error == "proxy route capacity exceeded" {
                transport.bridge.request_close(stream);
            }
            return Err(error);
        }
    };
    let owner = ClientOwner {
        transport: transport.clone(),
        stream: stream.clone(),
    };
    if *stop.borrow() {
        return Ok(());
    }
    transport
        .call(
            "proxy.stream.subscribe",
            serde_json::json!({"stream_id":stream}),
        )
        .await?;
    let (mut read, mut write) = socket.into_split();
    let outbound = async {
        let mut bytes = [0u8; 32768];
        loop {
            let count = read
                .read(&mut bytes)
                .await
                .map_err(|_| "local proxy read failed")?;
            if count == 0 {
                return Ok::<(), String>(());
            }
            let data = base64::engine::general_purpose::STANDARD.encode(&bytes[..count]);
            transport
                .call(
                    "proxy.write",
                    serde_json::json!({"stream_id":stream,"data_base64":data}),
                )
                .await?;
        }
    };
    let inbound = async {
        while let Some(data) = incoming.recv().await {
            write
                .write_all(&data)
                .await
                .map_err(|_| "local proxy write failed")?;
        }
        Ok::<(), String>(())
    };
    let result = tokio::select! {
        biased;
        _ = stop.changed() => Ok(()),
        result = outbound => result,
        result = inbound => result,
    };
    drop(owner);
    result
}

/// Accept clients with a connection-wide cap; stop closes admission and drains bounded in-flight opens.
async fn listener(
    transport: Arc<Transport>,
    listener: tokio::net::TcpListener,
    remote: SocketAddr,
    mut stop: watch::Receiver<bool>,
    permits: Arc<Semaphore>,
) {
    let local_port = listener.local_addr().ok().map(|address| address.port());
    let (client_stop, client_receiver) = watch::channel(false);
    let mut clients = JoinSet::new();
    loop {
        tokio::select! {
            biased;
            _ = stop.changed() => break,
            Some(_) = clients.join_next(), if !clients.is_empty() => {},
            accepted = listener.accept() => {
                let Ok((socket, _)) = accepted else { break; };
                let Ok(permit) = permits.clone().try_acquire_owned() else { continue; };
                let transport = transport.clone(); let stop = client_receiver.clone();
                clients.spawn(async move {
                    let _permit = permit;
                    let result = client(transport, socket, remote, stop).await;
                    crate::diagnostics::record("ssh.forward.client_complete", serde_json::json!({"outcome":if result.is_ok(){"success"}else{"error"}}));
                });
            }
        }
    }
    drop(listener);
    let _ = client_stop.send(true);
    {
        let mut published = transport.bridge.forwarded.lock().unwrap();
        if published.get(&remote).copied() == local_port {
            published.remove(&remote);
        }
    }
    while clients.join_next().await.is_some() {}
}

/// Run a bounded forwarding supervisor for exactly one SSH connection generation.
/// Listener removal signals graceful close; cancelling this task tears down all tasks with the connection.
pub(super) async fn run(
    writer: Arc<RpcWriter<BufWriter<tokio::process::ChildStdin>>>,
    pending: PendingMap,
    bridge: Arc<SshBridge>,
) {
    let transport = Arc::new(Transport {
        writer,
        pending,
        bridge: bridge.clone(),
    });
    let permits = Arc::new(Semaphore::new(16));
    let mut active: HashMap<SocketAddr, watch::Sender<bool>> = HashMap::new();
    let mut tasks = JoinSet::new();
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        while tasks.try_join_next().is_some() {}
        let desired: BTreeSet<_> = bridge
            .listeners
            .lock()
            .unwrap()
            .values()
            .filter_map(|(_, rows)| rows.as_ref())
            .flatten()
            .map(|row| SocketAddr::new(row.address, row.port))
            .take(256)
            .collect();
        active.retain(|remote, stop| {
            if desired.contains(remote) && !stop.is_closed() {
                true
            } else {
                let _ = stop.send(true);
                bridge.forwarded.lock().unwrap().remove(remote);
                false
            }
        });
        for remote in desired {
            if active.contains_key(&remote) {
                continue;
            }
            if active.len() >= 16 || tasks.len() >= 32 {
                break;
            }
            let local =
                match tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, remote.port())).await {
                    Ok(listener) => listener,
                    Err(_) => match tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await {
                        Ok(listener) => listener,
                        Err(_) => continue,
                    },
                };
            let Ok(address) = local.local_addr() else {
                continue;
            };
            let (stop, receiver) = watch::channel(false);
            bridge
                .forwarded
                .lock()
                .unwrap()
                .insert(remote, address.port());
            active.insert(remote, stop);
            tasks.spawn(listener(
                transport.clone(),
                local,
                remote,
                receiver,
                permits.clone(),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Duplicate registration preserves the first receiver; overload closes its route rather than dropping bytes silently.
    #[test]
    fn proxy_routes_preserve_ownership_and_retire_overload() {
        let routes = Routes::default();
        let mut receiver = routes.register("one").unwrap();
        assert!(routes.register("one").is_err());
        let message = serde_json::json!({"data_base64":"eA=="});
        assert!(routes.event("one", "proxy.stream.data", &message));
        assert_eq!(receiver.try_recv().unwrap(), b"x");
        for _ in 0..16 {
            assert!(routes.event("one", "proxy.stream.data", &message));
        }
        assert!(routes.event("one", "proxy.stream.data", &message));
        assert!(!routes.event("one", "proxy.stream.data", &message));
        for _ in 0..16 {
            assert_eq!(receiver.try_recv().unwrap(), b"x");
        }
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::error::TryRecvError::Disconnected)
        ));
    }
}
