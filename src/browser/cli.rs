//! Bounded asynchronous execution of the public browser CLI.

use serde_json::Value;
use std::io;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};

/// Run one public CLI command with bounded pipes and a fifteen-second deadline.
/// Cancellation kills the direct child through Tokio; descendant daemon ownership
/// remains with BrowserManager. Errors omit raw stdout and stderr.
pub(super) async fn run(binary: &Path, session: &str, args: &[&str]) -> Result<Value, String> {
    let mut command = tokio::process::Command::new(binary);
    command
        .arg("--session")
        .arg(session)
        .arg("--json")
        .args(args);
    let output = execute(command, Duration::from_secs(15))
        .await
        .map_err(|error| format!("Browser CLI exchange failed: {error}"))?;
    decode_output(output)
}

/// Decode the public CLI envelope shared by worker and remaining synchronous callers.
/// Invalid output and failed commands return errors without exposing captured pipe contents.
pub(super) fn decode_output(output: std::process::Output) -> Result<Value, String> {
    let payload: Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| "Browser CLI returned invalid JSON".to_string())?;
    if !output.status.success() || payload.get("success") == Some(&Value::Bool(false)) {
        return Err("Browser CLI command failed".to_string());
    }
    Ok(payload.get("data").cloned().unwrap_or(payload))
}

/// Drain stdout and stderr concurrently while waiting for the direct child.
/// Timeout, output overflow and I/O failure kill and reap the child before returning.
async fn execute(
    mut command: tokio::process::Command,
    timeout: Duration,
) -> io::Result<std::process::Output> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let stdout = child.stdout.take().expect("configured stdout pipe");
    let stderr = child.stderr.take().expect("configured stderr pipe");
    let result = tokio::time::timeout(timeout, async {
        let (stdout, stderr, status) = tokio::try_join!(
            read_bounded(stdout, 4 * 1024 * 1024),
            read_bounded(stderr, 64 * 1024),
            child.wait(),
        )?;
        Ok(std::process::Output {
            status,
            stdout,
            stderr,
        })
    })
    .await
    .unwrap_or_else(|_| {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "browser CLI deadline exceeded",
        ))
    });
    if result.is_err() {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    result
}

/// Read one child pipe up to its byte budget; detect overflow with one extra byte.
async fn read_bounded(reader: impl AsyncRead + Unpin, limit: u64) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes).await?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "browser CLI output limit exceeded",
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an isolated shell child for real pipe and timeout behavior in CI.
    fn shell(script: &str) -> tokio::process::Command {
        let mut command = tokio::process::Command::new("/bin/sh");
        command.args(["-c", script]);
        command
    }

    /// Both output pipes drain, failing exit status survives, and a silent child times out.
    #[tokio::test]
    async fn child_output_and_deadline() {
        let output = execute(
            shell("printf output; printf error >&2; exit 7"),
            Duration::from_secs(2),
        )
        .await
        .unwrap();
        assert_eq!(output.stdout, b"output");
        assert_eq!(output.stderr, b"error");
        assert_eq!(output.status.code(), Some(7));
        let error = execute(shell("exec sleep 60"), Duration::from_millis(30))
            .await
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }

    /// Cancelling the task kills and eventually reaps its direct child without waiting for its sleep.
    #[tokio::test]
    async fn cancelled_child_is_reaped() {
        let marker = std::env::temp_dir().join(format!("cmux-cli-child-{}", uuid::Uuid::new_v4()));
        let mut command = shell("echo $$ > \"$1\"; exec sleep 60");
        command.arg("fixture").arg(&marker);
        let task = tokio::spawn(execute(command, Duration::from_secs(15)));
        let pid = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Ok(text) = std::fs::read_to_string(&marker) {
                    if let Ok(pid) = text.trim().parse::<u32>() {
                        break pid;
                    }
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        tokio::time::timeout(Duration::from_secs(3), async {
            while std::path::Path::new(&format!("/proc/{pid}")).exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        std::fs::remove_file(marker).unwrap();
    }

    /// Pipe overflow is rejected before collecting unlimited child output.
    #[tokio::test]
    async fn output_budget() {
        let error = execute(shell("head -c 65537 /dev/zero >&2"), Duration::from_secs(2))
            .await
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(read_bounded(b"1234".as_slice(), 4).await.unwrap(), b"1234");
        assert!(read_bounded(b"12345".as_slice(), 4).await.is_err());
    }
}
