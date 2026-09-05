//! Linux desktop notification delivery isolated from workspace policy.

/// Send a desktop notification for a bell in the given workspace.
/// Uses `notify-send` subprocess to send notifications via org.freedesktop.Notifications.
///
/// We use a subprocess instead of notify-rust (zbus D-Bus client) because GNOME Shell
/// destroys notifications when the D-Bus sender name vanishes. With notify-rust in a
/// spawned thread, the zbus connection drops when the thread exits, causing GNOME Shell's
/// FdoNotificationDaemonSource._onNameVanished() to destroy the notification immediately.
/// `notify-send` avoids this because it's a separate process whose D-Bus lifetime is
/// independent of cmux.
pub fn terminal_bell(workspace_name: &str) {
    let body = format!("{} - Terminal bell", workspace_name);
    std::thread::spawn(move || {
        let result = std::process::Command::new("notify-send")
            .arg("--app-name=cmux")
            .arg("--icon=utilities-terminal")
            .arg("--expire-time=5000")
            .arg("Terminal Bell")
            .arg(&body)
            .status();
        match result {
            Ok(status) if !status.success() => {
                eprintln!("cmux: notify-send exited with {status}");
            }
            Err(e) => {
                eprintln!("cmux: failed to run notify-send: {e}");
            }
            _ => {}
        }
    });
}
