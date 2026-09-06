//! Ordered, bounded browser input with reserved capacity for key releases.
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::mpsc;

type Batch = Vec<Value>;
const CAPACITY: usize = 64;

/// Own one input worker and the reserved release slots for physically pressed keys.
/// Dropping this owner aborts in-flight I/O and discards pending input for the closed manager.
pub(super) struct InputQueue {
    sender: mpsc::Sender<Batch>,
    releases: HashMap<u32, (mpsc::OwnedPermit<Batch>, Value)>,
    task: tokio::task::JoinHandle<()>,
}

impl InputQueue {
    /// Start a single ordered consumer; each daemon exchange retains the transport deadline.
    pub(super) fn new(runtime: &tokio::runtime::Handle, path: PathBuf) -> Self {
        let (sender, mut receiver) = mpsc::channel::<Batch>(CAPACITY);
        let task = runtime.spawn(async move {
            while let Some(batch) = receiver.recv().await {
                let mut failed = false;
                for request in batch {
                    // Attempt release even if the preceding press exchange failed.
                    failed |= super::transport::request_async(&path, &request, None)
                        .await
                        .is_err();
                }
                if failed {
                    crate::diagnostics::record(
                        "browser.input.exchange_failed",
                        serde_json::json!({}),
                    );
                }
            }
        });
        Self {
            sender,
            releases: HashMap::new(),
            task,
        }
    }

    /// Admit a whole mouse batch without waiting; no partial click enters a full queue.
    pub(super) fn send(&self, batch: Batch) -> bool {
        self.sender.try_send(batch).is_ok()
    }

    /// Reserve release capacity before admitting a first press; repeats use ordinary queue slots.
    /// Releases of admitted presses cannot be rejected because other events filled the queue.
    pub(super) fn key(&mut self, physical: u32, pressed: bool, request: Value) -> bool {
        if !pressed {
            if let Some((permit, _)) = self.releases.remove(&physical) {
                permit.send(vec![request]);
            }
            // An unadmitted press has nothing to release.
            return true;
        }
        if self.releases.contains_key(&physical) {
            return self.send(vec![request]);
        }
        let Ok(press) = self.sender.clone().try_reserve_owned() else {
            return false;
        };
        let Ok(release) = self.sender.clone().try_reserve_owned() else {
            return false;
        };
        let mut released = request.clone();
        released["type"] = "keyUp".into();
        released["id"] = format!("cmux-{}", uuid::Uuid::new_v4()).into();
        if let Some(fields) = released.as_object_mut() {
            fields.remove("text");
        }
        self.releases.insert(physical, (release, released));
        press.send(vec![request]);
        true
    }

    /// Release all admitted keys on focus loss using their already reserved queue slots.
    pub(super) fn release_keys(&mut self) {
        for (_, (permit, request)) in self.releases.drain() {
            permit.send(vec![request]);
        }
    }
}

impl Drop for InputQueue {
    /// Cancel the worker without waiting on the GTK thread.
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

    /// Read and acknowledge one real daemon input request, retaining its decoded event for assertions.
    async fn receive(listener: &cmux_platform::local_socket::Listener) -> Value {
        let (peer, _) = listener.accept().await.unwrap();
        let mut peer = tokio::io::BufReader::new(peer);
        let mut line = String::new();
        peer.read_line(&mut line).await.unwrap();
        let request = serde_json::from_str(&line).unwrap();
        peer.get_mut().write_all(b"{}\n").await.unwrap();
        request
    }

    /// Saturation preserves whole clicks and reserved key-up capacity; blur emits text-free releases.
    #[tokio::test]
    async fn ordered_input_reserves_key_releases() {
        let path = std::env::temp_dir().join(format!("cmux-input-{}.sock", uuid::Uuid::new_v4()));
        let listener = cmux_platform::local_socket::Listener::bind(&path).unwrap();
        let mut queue = InputQueue::new(&tokio::runtime::Handle::current(), path.clone());
        let down =
            serde_json::json!({"action":"input_keyboard", "type":"keyDown", "key":"a", "text":"a"});
        assert!(queue.key(38, true, down.clone()));
        assert!(queue.send(vec![
            serde_json::json!({"type":"mousePressed"}),
            serde_json::json!({"type":"mouseReleased"})
        ]));
        for _ in 0..CAPACITY - 3 {
            assert!(queue.send(vec![serde_json::json!({"type":"mouseWheel"})]));
        }
        assert!(!queue.send(vec![
            serde_json::json!({"type":"mousePressed"}),
            serde_json::json!({"type":"mouseReleased"})
        ]));
        assert!(!queue.key(39, true, down.clone()));
        assert!(queue.key(38, false, serde_json::json!({"type":"keyUp"})));
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            assert_eq!(receive(&listener).await["type"], "keyDown");
            assert_eq!(receive(&listener).await["type"], "mousePressed");
            assert_eq!(receive(&listener).await["type"], "mouseReleased");
            for _ in 0..CAPACITY - 3 {
                assert_eq!(receive(&listener).await["type"], "mouseWheel");
            }
            assert_eq!(receive(&listener).await["type"], "keyUp");
            assert!(queue.key(38, true, down));
            queue.release_keys();
            assert_eq!(receive(&listener).await["type"], "keyDown");
            let released = receive(&listener).await;
            assert_eq!(released["type"], "keyUp");
            assert!(released.get("text").is_none());
        })
        .await
        .unwrap();
        drop(queue);
        std::fs::remove_file(path).unwrap();
    }

    /// Destroying the input owner closes an unanswered exchange instead of leaving a live worker.
    #[tokio::test]
    async fn dropping_queue_cancels_exchange() {
        let path =
            std::env::temp_dir().join(format!("cmux-input-drop-{}.sock", uuid::Uuid::new_v4()));
        let listener = cmux_platform::local_socket::Listener::bind(&path).unwrap();
        let queue = InputQueue::new(&tokio::runtime::Handle::current(), path.clone());
        assert!(queue.send(vec![serde_json::json!({"type":"mousePressed"})]));
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            let (peer, _) = listener.accept().await.unwrap();
            let mut peer = tokio::io::BufReader::new(peer);
            let mut line = String::new();
            peer.read_line(&mut line).await.unwrap();
            drop(queue);
            let mut byte = [0];
            assert_eq!(peer.read(&mut byte).await.unwrap(), 0);
        })
        .await
        .unwrap();
        std::fs::remove_file(path).unwrap();
    }
}
