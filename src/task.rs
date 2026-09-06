//! Shared ownership guards for asynchronous companion tasks.

/// Cancel a companion task when its owner leaves scope, including before first polling.
/// Aborting requests cancellation; callers that require completed cleanup must also await the task.
pub(crate) struct AbortOnDrop(pub(crate) tokio::task::AbortHandle);

impl Drop for AbortOnDrop {
    /// Request cancellation on normal return, error or owner-future destruction.
    fn drop(&mut self) {
        self.0.abort();
    }
}
