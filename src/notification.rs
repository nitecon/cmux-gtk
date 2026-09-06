//! Bounded desktop delivery, separate from persistent pane/workspace attention.

use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use uuid::Uuid;

static DELIVERIES: Semaphore = Semaphore::const_new(4);

/// Admit at most four desktop commands without queueing; overload preserves application attention.
/// The runtime owns admitted tasks. Its shutdown cancels them and requests direct-child termination.
pub(crate) fn send(
    runtime: &tokio::runtime::Handle,
    command: std::process::Command,
    workspace: Uuid,
) -> Option<tokio::task::JoinHandle<()>> {
    send_inner(runtime, command, workspace, None)
}

/// Admit a desktop message with a stable inbox action target, sharing the bounded bell worker pool.
pub(crate) fn send_message(
    runtime: &tokio::runtime::Handle,
    command: std::process::Command,
    workspace: Uuid,
    notification: Uuid,
) -> Option<tokio::task::JoinHandle<()>> {
    send_inner(runtime, command, workspace, Some(notification))
}

/// Own one notification helper and its bounded output/deadline; no raw payload enters diagnostics.
fn send_inner(
    runtime: &tokio::runtime::Handle,
    command: std::process::Command,
    workspace: Uuid,
    notification: Option<Uuid>,
) -> Option<tokio::task::JoinHandle<()>> {
    let permit = match DELIVERIES.try_acquire() {
        Ok(permit) => permit,
        Err(_) => {
            crate::diagnostics::record(
                "notification.delivery.rejected",
                serde_json::json!({"workspace": workspace, "notification": notification, "reason": "capacity"}),
            );
            return None;
        }
    };
    Some(runtime.spawn(async move {
        let _permit = permit;
        let mut delivery = Delivery {
            workspace,
            notification,
            trace_id: Uuid::new_v4(),
            started: Instant::now(),
            outcome: "cancelled",
            os_error: None,
        };
        crate::diagnostics::record(
            "notification.delivery.begin",
            serde_json::json!({"workspace": workspace, "notification": notification, "trace_id": delivery.trace_id}),
        );
        let mut command = tokio::process::Command::from(command);
        command
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let status = if let Some(notification) = notification {
            match crate::task::run_output(command, Duration::from_secs(15), 256, 4096, |error| {
                crate::diagnostics::event(format_args!(
                    "notification.cleanup outcome=error kind={:?}",
                    error.kind()
                ));
            })
            .await
            {
                Ok(output) => {
                    if output.status.success()
                        && output.stdout == b"default\n"
                        && !crate::ghostty::events::push(
                            crate::ghostty::events::Event::OpenNotification(notification),
                        )
                    {
                        delivery.outcome = "action_queue_full";
                        return;
                    }
                    Ok(output.status)
                }
                Err(error) => Err(error),
            }
        } else {
            crate::task::run_status(&mut command, Duration::from_secs(5)).await
        };
        match status {
            Ok(status) => {
                delivery.outcome = if status.success() {
                    "success"
                } else {
                    "exit_error"
                }
            }
            Err(error) => {
                delivery.os_error = error.raw_os_error();
                delivery.outcome = if error.kind() == std::io::ErrorKind::TimedOut {
                    "timeout"
                } else {
                    "io_error"
                };
            }
        }
    }))
}

/// Record task completion even on cancellation, without command arguments or desktop payloads.
struct Delivery {
    workspace: Uuid,
    notification: Option<Uuid>,
    trace_id: Uuid,
    started: Instant,
    outcome: &'static str,
    os_error: Option<i32>,
}

impl Drop for Delivery {
    /// Finish the admitted delivery's diagnostic lifetime; success means process exit, not presentation.
    fn drop(&mut self) {
        crate::diagnostics::record(
            "notification.delivery.complete",
            serde_json::json!({
                "workspace": self.workspace, "notification": self.notification, "trace_id": self.trace_id,
                "duration_us": self.started.elapsed().as_micros() as u64,
                "outcome": self.outcome, "os_error": self.os_error,
            }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exercise real child admission, overload rejection, cancellation/reaping and execution expiry.
    #[tokio::test]
    async fn desktop_delivery_bounds_and_cleanup() {
        let root = std::env::temp_dir().join(format!("cmux-notify-{}", Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let runtime = tokio::runtime::Handle::current();
        let workspace = Uuid::new_v4();
        let mut tasks = Vec::new();
        let mut guards = Vec::new();
        for index in 0..4 {
            let mut command = std::process::Command::new("sh");
            command.args(["-c", "printf '%s' $$ > \"$1\"; exec sleep 30", "sh"]);
            command.arg(root.join(index.to_string()));
            let task = send(&runtime, command, workspace).unwrap();
            guards.push(crate::task::AbortOnDrop(task.abort_handle()));
            tasks.push(task);
        }
        let mut rejected = std::process::Command::new("touch");
        rejected.arg(root.join("rejected"));
        assert!(send(&runtime, rejected, workspace).is_none());
        let pids = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let pids: Option<Vec<u32>> = (0..4)
                    .map(|index| {
                        std::fs::read_to_string(root.join(index.to_string()))
                            .ok()?
                            .parse()
                            .ok()
                    })
                    .collect();
                if let Some(pids) = pids {
                    break pids;
                }
                assert!(
                    tasks.iter().all(|task| !task.is_finished()),
                    "delivery exited before PID evidence"
                );
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("children did not start");
        drop(guards);
        for task in tasks {
            assert!(task.await.unwrap_err().is_cancelled());
        }
        tokio::time::timeout(Duration::from_secs(3), async {
            while pids
                .iter()
                .any(|pid| std::path::Path::new(&format!("/proc/{pid}")).exists())
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("cancelled notification children survived");
        assert!(!root.join("rejected").exists());

        let mut command = std::process::Command::new("sleep");
        command.arg("30");
        let task = send(&runtime, command, workspace).expect("cancellation leaked admission");
        let guard = crate::task::AbortOnDrop(task.abort_handle());
        tokio::time::timeout(Duration::from_secs(12), task)
            .await
            .expect("execution deadline failed")
            .unwrap();
        drop(guard);
        let task = send(&runtime, std::process::Command::new("true"), workspace).unwrap();
        task.await.unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
