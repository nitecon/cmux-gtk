//! Bounded asynchronous execution of the public browser CLI.

use serde_json::Value;
use std::io;
use std::path::Path;
use std::time::Duration;

const MAX_STDOUT_BYTES: u64 = 4 * 1024 * 1024;
const MAX_STDERR_BYTES: u64 = 64 * 1024;

/// Run one public CLI command with bounded pipes and a fifteen-second deadline.
/// Cancellation kills the direct child through Tokio; descendant daemon ownership
/// remains with BrowserManager. Errors omit raw stdout and stderr.
pub(super) async fn run(
    binary: &Path,
    session: &str,
    args: &[&str],
    trace_id: uuid::Uuid,
) -> Result<Value, String> {
    let mut command = tokio::process::Command::new(binary);
    command
        .arg("--session")
        .arg(session)
        .arg("--json")
        .args(args);
    run_command(
        command,
        if args == ["get", "url"] {
            "cli_url_refresh"
        } else {
            "cli_command"
        },
        trace_id,
    )
    .await
}

/// Start the private preview session through the public CLI using bounded worker pipes.
/// Honor an explicit browser choice; the caller discovers an optional system browser off GTK.
pub(super) async fn start(
    binary: &Path,
    session: &str,
    browser: Option<&Path>,
    url: &str,
    proxy_port: Option<u16>,
    trace_id: uuid::Uuid,
) -> Result<Value, String> {
    let mut command = tokio::process::Command::new(binary);
    command
        .arg("--session")
        .arg(session)
        .arg("--json")
        .env("AGENT_BROWSER_SESSION", session)
        .env("AGENT_BROWSER_STREAM_PORT", "0");
    if let Some(browser) = browser {
        command.arg("--executable-path").arg(browser);
    }
    if let Some(port) = proxy_port {
        command
            .arg("--proxy")
            .arg(format!("socks5://127.0.0.1:{port}"))
            .arg("--proxy-bypass")
            .arg("<-loopback>");
    }
    command.args(["open", url]);
    run_command(command, "cli_startup", trace_id).await
}

/// Share pipe bounds, child cleanup, protocol decoding and timing across public CLI operations.
async fn run_command(
    command: tokio::process::Command,
    kind: &'static str,
    trace_id: uuid::Uuid,
) -> Result<Value, String> {
    let mut activity = super::metrics::Activity::begin(kind, Some(trace_id));
    let output = match execute(command, Duration::from_secs(15)).await {
        Ok(output) => output,
        Err(error) => {
            crate::diagnostics::record(
                "browser.cli.failed",
                serde_json::json!({
                    "trace_id": trace_id, "stage": kind,
                    "error_kind": format!("{:?}", error.kind()), "os_error": error.raw_os_error(),
                }),
            );
            activity.finish(match error.kind() {
                io::ErrorKind::TimedOut => "timeout",
                io::ErrorKind::InvalidData => "output_limit",
                _ => "io_error",
            });
            return Err(format!("Browser CLI exchange failed: {error}"));
        }
    };
    let result = decode_output(output);
    activity.finish(if result.is_ok() {
        "success"
    } else {
        "command_or_protocol_error"
    });
    result
}

/// Decode the public CLI envelope shared by worker and remaining synchronous callers.
/// Invalid output and failed commands return errors without exposing captured pipe contents.
/// Move the data payload out of its envelope; never clone a potentially large result.
pub(super) fn decode_output(output: std::process::Output) -> Result<Value, String> {
    if !output.status.success() {
        return Err("Browser CLI command failed".to_string());
    }
    if output.stdout.len() as u64 > MAX_STDOUT_BYTES
        || output.stderr.len() as u64 > MAX_STDERR_BYTES
    {
        return Err("Browser CLI output limit exceeded".to_string());
    }
    let mut payload: Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| "Browser CLI returned invalid JSON".to_string())?;
    if !payload.is_object() {
        return Err("Browser CLI returned a non-object response".to_string());
    }
    match payload.get("success") {
        Some(Value::Bool(false)) => return Err("Browser CLI command failed".to_string()),
        Some(Value::Bool(true)) | None => {}
        Some(_) => return Err("Browser CLI returned invalid success status".to_string()),
    }
    Ok(match payload.get_mut("data") {
        Some(data) => data.take(),
        None => payload,
    })
}

/// Apply browser pipe budgets while preserving structured cleanup-failure diagnostics.
async fn execute(
    command: tokio::process::Command,
    timeout: Duration,
) -> io::Result<std::process::Output> {
    crate::task::run_output(
        command,
        timeout,
        MAX_STDOUT_BYTES,
        MAX_STDERR_BYTES,
        |error| {
            crate::diagnostics::record(
                "browser.cli.cleanup_failed",
                serde_json::json!({"error_kind": format!("{:?}", error.kind())}),
            );
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The public launch child receives explicit proxy and loopback routing without shell parsing.
    #[tokio::test]
    async fn startup_passes_remote_proxy_arguments() {
        use std::os::unix::fs::PermissionsExt;
        let directory =
            std::env::temp_dir().join(format!("cmux-proxy-start-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let binary = directory.join("browser-fixture");
        std::fs::write(&binary, "#!/usr/bin/env python3\nimport json,sys\nprint(json.dumps({'success':True,'data':sys.argv[1:]}))\n").unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let args = start(
            &binary,
            "private-session",
            None,
            "http://localhost:3000/path",
            Some(23456),
            uuid::Uuid::new_v4(),
        )
        .await
        .unwrap();
        std::fs::remove_dir_all(directory).unwrap();
        assert_eq!(
            args,
            serde_json::json!([
                "--session",
                "private-session",
                "--json",
                "--proxy",
                "socks5://127.0.0.1:23456",
                "--proxy-bypass",
                "<-loopback>",
                "open",
                "http://localhost:3000/path"
            ])
        );
    }

    /// Decode actual child output without treating malformed envelopes or failures as success.
    #[tokio::test]
    async fn public_cli_response_contract() {
        for (payload, expected) in [
            (
                r#"{"success":true,"data":{"url":"https://example.test/λ"}}"#,
                serde_json::json!({"url":"https://example.test/λ"}),
            ),
            (r#"{"success":true,"data":null}"#, Value::Null),
            (
                r#"{"url":"https://example.test"}"#,
                serde_json::json!({"url":"https://example.test"}),
            ),
        ] {
            let mut command = shell("printf '%s' \"$1\"");
            command.arg("fixture").arg(payload);
            let output = execute(command, Duration::from_secs(2)).await.unwrap();
            assert_eq!(decode_output(output).unwrap(), expected);
        }
        for payload in [
            "[]",
            "null",
            "false",
            "7",
            r#"{"success":"true"}"#,
            r#"{"success":false,"error":"private details"}"#,
            "not JSON",
        ] {
            let mut command = shell("printf '%s' \"$1\"");
            command.arg("fixture").arg(payload);
            let output = execute(command, Duration::from_secs(2)).await.unwrap();
            let error = decode_output(output).unwrap_err();
            assert!(!error.contains("private details"));
        }
        let output = execute(
            shell("printf 'private details'; exit 7"),
            Duration::from_secs(2),
        )
        .await
        .unwrap();
        assert_eq!(
            decode_output(output).unwrap_err(),
            "Browser CLI command failed"
        );
    }

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
        assert_eq!(
            crate::task::read_bounded(b"1234".as_slice(), 4)
                .await
                .unwrap(),
            b"1234"
        );
        assert!(crate::task::read_bounded(b"12345".as_slice(), 4)
            .await
            .is_err());
    }
}
