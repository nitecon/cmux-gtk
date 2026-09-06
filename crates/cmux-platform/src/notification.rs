//! Linux desktop notification command construction; the application owns admission and execution.

/// Construct a notify-send bell without launching it or invoking a shell.
/// The caller must bound concurrency and process lifetime; the body may contain user text.
pub fn terminal_bell(workspace_name: &str) -> std::process::Command {
    let mut command = std::process::Command::new("notify-send");
    command.args([
        "--app-name=cmux",
        "--icon=utilities-terminal",
        "--expire-time=5000",
        "Terminal Bell",
    ]);
    command.arg(format!("{workspace_name} - Terminal bell"));
    command
}
