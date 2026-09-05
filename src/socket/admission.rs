//! Connection admission bounds retained framing buffers and asynchronous handlers.
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use tokio::sync::{Semaphore, SemaphorePermit};

const CAPACITY: usize = 64;
static CONNECTIONS: Gate = Gate::new(CAPACITY);

/// Own fixed connection capacity and cumulative overload accounting.
struct Gate {
    permits: Semaphore,
    rejected: AtomicU64,
}

impl Gate {
    /// Construct a fixed-capacity gate without allocating a waiting queue.
    const fn new(capacity: usize) -> Self {
        Self {
            permits: Semaphore::const_new(capacity),
            rejected: AtomicU64::new(0),
        }
    }

    /// Admit immediately or count rejection; permit destruction releases capacity on every exit path.
    fn admit(&self) -> Option<SemaphorePermit<'_>> {
        self.permits
            .try_acquire()
            .map_err(|_| self.rejected.fetch_add(1, Relaxed))
            .ok()
    }
}

/// Acquire one process-wide connection slot after peer authentication and before spawning its handler.
pub(super) fn admit() -> Option<SemaphorePermit<'static>> {
    CONNECTIONS.admit()
}

/// Sample connection pressure independently without waiting or touching GTK state.
pub(crate) fn snapshot() -> serde_json::Value {
    serde_json::json!({
        "capacity": CAPACITY,
        "active": CAPACITY - CONNECTIONS.permits.available_permits(),
        "rejected": CONNECTIONS.rejected.load(Relaxed),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Saturation rejects immediately, and releasing an owned slot allows the next connection.
    #[test]
    fn capacity_is_reusable() {
        let gate = Gate::new(1);
        let permit = gate.admit().unwrap();
        assert!(gate.admit().is_none());
        assert_eq!(gate.rejected.load(Relaxed), 1);
        drop(permit);
        assert!(gate.admit().is_some());
    }

    /// Cancelling an active connection task returns its slot without an explicit cleanup callback.
    #[tokio::test]
    async fn cancellation_returns_capacity() {
        let gate = std::sync::Arc::new(Gate::new(1));
        let worker_gate = gate.clone();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let _permit = worker_gate.admit().unwrap();
            ready_tx.send(()).unwrap();
            std::future::pending::<()>().await;
        });
        tokio::time::timeout(std::time::Duration::from_secs(1), ready_rx)
            .await
            .unwrap()
            .unwrap();
        assert!(gate.admit().is_none());
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(gate.admit().is_some());
    }
}
