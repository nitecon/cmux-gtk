//! Shared ownership and bounded cleanup for asynchronous companion tasks and child processes.

/// Allow normal exit for `grace`, then request a kill and wait at most five more seconds.
/// Return the exit status and whether forced termination was requested. The owned
/// child must have kill_on_drop enabled by its launcher for cancellation/error fallback.
/// A kernel-stuck process may outlive the deadline; this bounds waiting, not OS termination.
pub(crate) async fn reap_child(
    mut child: tokio::process::Child,
    grace: std::time::Duration,
) -> std::io::Result<(std::process::ExitStatus, bool)> {
    if let Ok(status) = tokio::time::timeout(grace, child.wait()).await {
        return status.map(|status| (status, false));
    }
    child.start_kill()?;
    tokio::time::timeout(std::time::Duration::from_secs(5), child.wait())
        .await
        .map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::TimedOut, "child reap deadline exceeded")
        })?
        .map(|status| (status, true))
}

/// Cancel a companion task when its owner leaves scope, including before first polling.
/// Aborting requests cancellation; callers that require completed cleanup must also await the task.
pub(crate) struct AbortOnDrop(pub(crate) tokio::task::AbortHandle);

impl Drop for AbortOnDrop {
    /// Request cancellation on normal return, error or owner-future destruction.
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Preserve ordinary exit codes and force/reap a direct child that does not exit during grace.
    #[tokio::test]
    async fn child_exit_and_kill_fallback() {
        let child = tokio::process::Command::new("sh")
            .args(["-c", "exit 7"])
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let (status, forced) = reap_child(child, Duration::from_secs(2)).await.unwrap();
        assert_eq!(status.code(), Some(7));
        assert!(!forced);
        let child = tokio::process::Command::new("sleep")
            .arg("30")
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let pid = child.id().unwrap();
        let (status, forced) = reap_child(child, Duration::ZERO).await.unwrap();
        assert!(!status.success());
        assert!(forced);
        assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
    }
}
