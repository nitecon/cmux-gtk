use super::outbound::{Outbound, Receiver};
#[cfg(test)]
use base64::Engine;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
#[cfg(test)]
use tokio::sync::mpsc;

/// Per-pane stream state tracking.
pub struct PaneStream {
    pub stream_id: String,
    pub subscribed: bool,
}

/// Request to write data through the SSH tunnel to a specific stream.
pub struct WriteRequest {
    pub stream_id: String,
    pub data_base64: String,
    pub close: bool,
    pub resize: Option<(u16, u16)>,
}

/// Manages the mapping between local panes and remote proxy streams.
pub struct SshBridge {
    /// Latest bounded observations keyed by live remote surface context, invalidated on reconnect.
    pub listeners: Mutex<HashMap<u64, crate::ports::RemoteObservation>>,
    /// Maps pane_id -> stream state
    pub streams: Arc<Mutex<HashMap<u64, PaneStream>>>,
    pub contexts: Mutex<HashMap<u64, Arc<IoWriteContext>>>,
    pub changed: tokio::sync::Notify,
    pub directory: Mutex<Option<String>>,
    /// Maps stream_id -> pane_id (reverse lookup for incoming events)
    pub stream_to_pane: Arc<Mutex<HashMap<String, u64>>>,
    /// Channel to send write requests to the SSH tunnel task (swappable for reconnect)
    pub(super) write_tx: Arc<Mutex<Outbound>>,
    /// Receiver side of the write channel (taken by run_proxy_routing)
    write_rx: Mutex<Option<Receiver>>,
    /// Atomic counter for JSON-RPC request IDs
    pub next_rpc_id: Arc<AtomicU64>,
}

impl Default for SshBridge {
    /// Create an empty bridge with a fresh outbound channel.
    fn default() -> Self {
        Self::new()
    }
}

impl SshBridge {
    /// Create a workspace-owned bridge and its initial outbound request channel.
    pub fn new() -> Self {
        let (write_tx, write_rx) = Outbound::new();
        Self {
            listeners: Mutex::new(HashMap::new()),
            streams: Arc::new(Mutex::new(HashMap::new())),
            contexts: Mutex::new(HashMap::new()),
            changed: tokio::sync::Notify::new(),
            directory: Mutex::new(None),
            stream_to_pane: Arc::new(Mutex::new(HashMap::new())),
            write_tx: Arc::new(Mutex::new(write_tx)),
            write_rx: Mutex::new(Some(write_rx)),
            next_rpc_id: Arc::new(AtomicU64::new(10)), // Start after handshake IDs
        }
    }

    /// Create a distinct remote surface identity and register its lifetime with this bridge.
    pub fn create_context(&self, ssh_tx: crate::ssh::SshEventTx) -> Arc<IoWriteContext> {
        static NEXT_REMOTE_ID: AtomicU64 = AtomicU64::new(1 << 40);
        let id = NEXT_REMOTE_ID.fetch_add(1, Ordering::Relaxed);
        let ctx = Arc::new(IoWriteContext {
            pane_id: id,
            write_tx: self.write_tx.clone(),
            surface_ptr: std::sync::atomic::AtomicUsize::new(0),
            size: Mutex::new((80, 24)),
            stream_id: Mutex::new(None),
            eof_received: AtomicBool::new(false),
            ssh_tx,
        });
        self.contexts.lock().unwrap().insert(id, ctx.clone());
        ctx
    }

    /// Remove routing state and request closure of the associated remote PTY when present.
    pub fn remove_context(&self, id: u64) {
        self.listeners.lock().unwrap().remove(&id);
        self.contexts.lock().unwrap().remove(&id);
        if let Some(stream) = self.streams.lock().unwrap().remove(&id) {
            self.stream_to_pane
                .lock()
                .unwrap()
                .remove(&stream.stream_id);
            if !stream.stream_id.is_empty() {
                self.request_close(stream.stream_id);
            }
        }
    }

    /// Queue best-effort closure for a known remote stream; transport teardown owns failed delivery.
    pub(crate) fn request_close(&self, stream_id: String) {
        self.write_tx.lock().unwrap().control(WriteRequest {
            stream_id,
            data_base64: String::new(),
            close: true,
            resize: None,
        });
    }

    /// Take the write receiver for use in the proxy routing loop.
    /// On reconnect, creates a fresh channel pair and swaps the sender.
    pub(super) fn take_or_recreate_write_rx(&self) -> Receiver {
        let mut rx_guard = self.write_rx.lock().unwrap();
        if let Some(rx) = rx_guard.take() {
            return rx;
        }
        // Reconnect case: old rx was consumed. Create fresh channel.
        let (new_tx, new_rx) = Outbound::new();
        *self.write_tx.lock().unwrap() = new_tx;
        new_rx
    }

    /// Clear all stream state (for reconnect -- old streams are stale).
    pub fn clear_stream_ids(&self) {
        self.listeners.lock().unwrap().clear();
        if let Ok(contexts) = self.contexts.lock() {
            for ctx in contexts.values() {
                *ctx.stream_id.lock().unwrap() = None;
                ctx.eof_received.store(false, Ordering::Relaxed);
            }
        }
        if let Ok(mut streams) = self.streams.lock() {
            for ps in streams.values_mut() {
                ps.stream_id.clear();
                ps.subscribed = false;
            }
        }
        if let Ok(mut s2p) = self.stream_to_pane.lock() {
            s2p.clear();
        }
    }

    /// Register a new pane with its stream_id after proxy.open succeeds.
    pub fn register_pane(&self, pane_id: u64, stream_id: String) {
        if let Ok(mut streams) = self.streams.lock() {
            streams.insert(
                pane_id,
                PaneStream {
                    stream_id: stream_id.clone(),
                    subscribed: false,
                },
            );
        }
        if let Ok(mut s2p) = self.stream_to_pane.lock() {
            s2p.insert(stream_id, pane_id);
        }
    }

    /// Register a pane with placeholder stream state (no stream_id yet).
    /// Called at workspace creation time so run_proxy_routing can find the pane
    /// and open a remote stream for it after SSH handshake.
    pub fn register_pane_placeholder(&self, pane_id: u64) {
        if let Ok(mut streams) = self.streams.lock() {
            streams.insert(
                pane_id,
                PaneStream {
                    stream_id: String::new(),
                    subscribed: false,
                },
            );
        }
        self.changed.notify_one();
    }

    /// Mark a pane's stream as subscribed.
    pub fn mark_subscribed(&self, pane_id: u64) {
        if let Ok(mut streams) = self.streams.lock() {
            if let Some(ps) = streams.get_mut(&pane_id) {
                ps.subscribed = true;
            }
        }
    }

    /// Remove a pane's stream mapping (on close or EOF).
    pub fn remove_pane(&self, pane_id: u64) {
        self.listeners.lock().unwrap().remove(&pane_id);
        let stream_id = if let Ok(mut streams) = self.streams.lock() {
            streams.remove(&pane_id).map(|ps| ps.stream_id)
        } else {
            None
        };
        if let Some(sid) = stream_id {
            if let Ok(mut s2p) = self.stream_to_pane.lock() {
                s2p.remove(&sid);
            }
        }
    }

    /// Get the next JSON-RPC request ID.
    pub fn next_id(&self) -> u64 {
        self.next_rpc_id.fetch_add(1, Ordering::SeqCst)
    }
}

/// Context passed as userdata to the Ghostty io_write_cb callback.
/// Must be allocated with Arc and leaked via Arc::into_raw for the C callback.
pub struct IoWriteContext {
    pub pane_id: u64,
    pub(super) write_tx: Arc<Mutex<Outbound>>,
    pub surface_ptr: std::sync::atomic::AtomicUsize,
    pub size: Mutex<(u16, u16)>,
    /// Set after proxy.open returns the stream_id.
    pub stream_id: Mutex<Option<String>>,
    /// Set when remote shell exits -- next keypress triggers pane close.
    pub eof_received: AtomicBool,
    /// Channel to send close requests to the GTK main loop.
    pub ssh_tx: crate::ssh::SshEventTx,
}

/// C-compatible callback invoked by Ghostty when user types in a manual-mode surface.
/// Signature: void(*)(void* userdata, const char* data, uintptr_t len)
///
/// SAFETY: userdata must be a valid Arc<IoWriteContext> pointer created via Arc::into_raw.
/// This callback runs on the GTK main thread (same thread as key events).
pub unsafe extern "C" fn ssh_io_write_cb(
    userdata: *mut std::ffi::c_void,
    data: *const std::ffi::c_char,
    len: usize,
) {
    if userdata.is_null() || data.is_null() || len == 0 {
        return;
    }
    // SAFETY: userdata is an Arc<IoWriteContext> pointer -- we borrow without taking ownership.
    let ctx = &*(userdata as *const IoWriteContext);

    // After remote shell exits, any keypress closes the pane
    if ctx.eof_received.load(Ordering::Relaxed) {
        let _ = ctx.ssh_tx.try_send(crate::ssh::SshEvent::ClosePaneRequest {
            pane_id: ctx.pane_id,
        });
        return;
    }

    let bytes = std::slice::from_raw_parts(data as *const u8, len);
    if let Some(ref stream_id) = *ctx.stream_id.lock().unwrap() {
        ctx.write_tx.lock().unwrap().input(stream_id, bytes);
    }
}

impl IoWriteContext {
    /// Clamp terminal dimensions, coalesce unchanged sizes and enqueue a remote resize when connected.
    pub fn resize(&self, columns: u16, rows: u16) {
        let size = (columns.max(1), rows.max(1));
        let mut previous = self.size.lock().unwrap();
        if *previous == size {
            return;
        }
        *previous = size;
        if let Some(stream_id) = self.stream_id.lock().unwrap().clone() {
            self.write_tx.lock().unwrap().control(WriteRequest {
                stream_id,
                data_base64: String::new(),
                close: false,
                resize: Some(size),
            });
        }
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;

    /// Route input through the replacement sender and close only the retired remote stream.
    #[test]
    fn remote_contexts_keep_separate_streams_and_use_reconnected_sender() {
        let bridge = SshBridge::new();
        let (events, _) = mpsc::channel(16);
        let first = bridge.create_context(events.clone());
        let second = bridge.create_context(events);
        assert_ne!(first.pane_id, second.pane_id);
        bridge.register_pane(first.pane_id, "first-stream".into());
        bridge.register_pane(second.pane_id, "second-stream".into());
        *first.stream_id.lock().unwrap() = Some("first-stream".into());
        *second.stream_id.lock().unwrap() = Some("second-stream".into());
        drop(bridge.take_or_recreate_write_rx());
        let (mut reconnected, _failure) = bridge.take_or_recreate_write_rx();
        let payload = b"typed after reconnect";
        unsafe {
            ssh_io_write_cb(
                Arc::as_ptr(&second) as *mut _,
                payload.as_ptr().cast(),
                payload.len(),
            );
        }
        let write = reconnected.try_recv().unwrap();
        assert_eq!(write.stream_id, "second-stream");
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(write.data_base64)
                .unwrap(),
            payload
        );
        bridge.remove_context(first.pane_id);
        let close = reconnected.try_recv().unwrap();
        assert!(close.close);
        assert_eq!(close.stream_id, "first-stream");
        assert!(!bridge.contexts.lock().unwrap().contains_key(&first.pane_id));
        assert!(!bridge.streams.lock().unwrap().contains_key(&first.pane_id));
        assert!(bridge
            .contexts
            .lock()
            .unwrap()
            .contains_key(&second.pane_id));
    }
}
