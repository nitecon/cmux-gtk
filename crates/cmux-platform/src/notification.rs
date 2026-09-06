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

/// Construct an actionable Linux notification without a shell. The caller bounds process lifetime,
/// captures the selected action and routes it to the original application message identity.
pub fn message(title: &str, subtitle: &str, body: &str) -> std::process::Command {
    let mut command = std::process::Command::new("notify-send");
    command.args([
        "--app-name=cmux",
        "--icon=utilities-terminal",
        "--expire-time=10000",
        "--action=default=Open terminal",
        "--wait",
        "--",
    ]);
    let text = if subtitle.is_empty() {
        body.to_owned()
    } else {
        format!("{subtitle}\n{body}")
    };
    // Freedesktop notification bodies may be interpreted as markup; preserve plain message text.
    command.arg(title).arg(
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;"),
    );
    command
}
