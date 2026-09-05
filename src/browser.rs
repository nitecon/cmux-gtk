use gtk4::prelude::*;
use serde_json::Value;
mod cli;
mod discovery;
mod frames;
pub(crate) mod metrics;
mod motion;
mod pixels;
mod stream;
pub use motion::spawn_motion_forwarder;
pub(crate) mod transport;
pub use discovery::agent_browser_available;
use discovery::{find_system_chrome, which_agent_browser};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use uuid::Uuid;

/// Preview pane state tracked by BrowserManager.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewState {
    Empty,
    Connected,
    Streaming,
}

/// Own daemon discovery and the preview stream task for this application session.
pub struct BrowserManager {
    session_name: String,
    navigation_gate: std::sync::Arc<tokio::sync::Semaphore>,
    binary_path: Option<PathBuf>,
    stream_task: Option<tokio::task::JoinHandle<()>>,

    pub preview_state: PreviewState,
}

impl BrowserManager {
    /// Create an idle manager with a private daemon identity for its lifetime.
    /// Defer executable discovery and daemon startup until needed; separate managers
    /// cannot navigate or shut down each other through a shared session name.
    pub fn new() -> Self {
        BrowserManager {
            session_name: format!("cmux-{}", Uuid::new_v4().simple()),
            navigation_gate: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
            binary_path: None,
            stream_task: None,
            preview_state: PreviewState::Empty,
        }
    }

    /// Mirrors agent-browser/cli/src/connection.rs socket dir discovery.
    fn agent_browser_socket_dir() -> PathBuf {
        if let Ok(dir) = std::env::var("AGENT_BROWSER_SOCKET_DIR") {
            if !dir.is_empty() {
                return PathBuf::from(dir);
            }
        }
        if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
            if !dir.is_empty() {
                return PathBuf::from(dir).join("agent-browser");
            }
        }
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(".agent-browser");
        }
        std::env::temp_dir().join("agent-browser")
    }

    /// Resolve the command socket for the selected agent-browser session.
    pub fn daemon_socket_path(&self) -> PathBuf {
        Self::agent_browser_socket_dir().join(format!("{}.sock", self.session_name))
    }

    /// Locate the daemon's file advertising its dynamically allocated stream port.
    pub fn stream_port_path(&self) -> PathBuf {
        Self::agent_browser_socket_dir().join(format!("{}.stream", self.session_name))
    }

    /// Probe socket acceptance synchronously without issuing a browser command.
    fn daemon_ready(&self) -> bool {
        std::os::unix::net::UnixStream::connect(self.daemon_socket_path()).is_ok()
    }

    /// Auto-start the agent-browser daemon (D-05).
    pub fn ensure_daemon(&mut self) -> Result<(), String> {
        // Find an explicitly configured, installed, packaged, or locally linked binary.
        let binary_path = which_agent_browser().ok_or_else(|| {
            "agent-browser is not installed; browser panes are unavailable. Install it with: npm install -g agent-browser && agent-browser install"
                .to_string()
        })?;
        self.binary_path = Some(binary_path.clone());

        if self.daemon_ready() {
            return Ok(());
        }

        // Create socket dir if needed.
        let socket_dir = Self::agent_browser_socket_dir();
        std::fs::create_dir_all(&socket_dir).map_err(|e| {
            format!(
                "Failed to create socket dir {}: {}",
                socket_dir.display(),
                e
            )
        })?;

        // Use the public CLI to launch the browser and its daemon. The old
        // AGENT_BROWSER_DAEMON entry point is private and has changed between
        // releases, which defeated using an unpinned installation.
        let mut command = Command::new(&binary_path);
        command
            .arg("--session")
            .arg(&self.session_name)
            .env("AGENT_BROWSER_SESSION", &self.session_name)
            .env("AGENT_BROWSER_STREAM_PORT", "0")
            .stdin(Stdio::null());

        // Ubuntu's AppArmor policy can reject the downloaded Chrome for
        // Testing sandbox. Prefer an installed, sandboxed browser when one is
        // available, while honoring the user's explicit agent-browser choice.
        if std::env::var_os("AGENT_BROWSER_EXECUTABLE_PATH").is_none() {
            if let Some(browser) = find_system_chrome() {
                command.arg("--executable-path").arg(browser);
            }
        }
        command.arg("open").arg("about:blank");

        let output = command
            .output()
            .map_err(|e| format!("Failed to launch agent-browser: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("agent-browser failed to launch: {}", stderr.trim()));
        }

        // Poll daemon_ready() with 200ms intervals, up to 50 retries (10s).
        for _ in 0..50 {
            std::thread::sleep(std::time::Duration::from_millis(200));
            if self.daemon_ready() {
                self.preview_state = PreviewState::Connected;
                return Ok(());
            }
        }

        Err("agent-browser daemon failed to start within 10 seconds".to_string())
    }

    /// Run a supported public agent-browser CLI command for this cmux session.
    /// Lifecycle and navigation use the public CLI because private daemon
    /// action semantics can change between independently installed releases.
    pub fn run_cli(&mut self, args: &[&str]) -> Result<Value, String> {
        self.ensure_daemon()?;
        let binary = self
            .binary_path
            .as_ref()
            .ok_or_else(|| "agent-browser executable could not be resolved".to_string())?;
        let output = Command::new(binary)
            .arg("--session")
            .arg(&self.session_name)
            .arg("--json")
            .args(args)
            .output()
            .map_err(|e| format!("Failed to run agent-browser: {e}"))?;
        cli::decode_output(output)
    }

    /// Prepare history navigation and URL refresh for worker execution after daemon startup.
    /// Navigation shares one admission slot with URL-entry operations.
    pub fn navigate_async(
        &self,
        command: String,
        trace_id: uuid::Uuid,
    ) -> impl std::future::Future<Output = Result<Option<String>, String>> + Send + 'static {
        self.navigation_commands(vec![vec![command]], trace_id)
    }

    /// Prepare viewport sizing, URL navigation and address refresh as one admitted operation.
    /// Skip unknown viewport sizes; a failed sizing command prevents partial navigation.
    pub fn open_async(
        &self,
        url: String,
        viewport: Option<(i32, i32)>,
        trace_id: uuid::Uuid,
    ) -> impl std::future::Future<Output = Result<Option<String>, String>> + Send + 'static {
        let mut commands = Vec::new();
        if let Some((width, height)) = viewport.filter(|(width, height)| *width > 0 && *height > 0)
        {
            commands.push(vec![
                "set".into(),
                "viewport".into(),
                width.to_string(),
                height.to_string(),
            ]);
        }
        commands.push(vec!["open".into(), url]);
        self.navigation_commands(commands, trace_id)
    }

    /// Execute ordered public CLI commands and refresh the URL while owning one admission permit.
    /// Reject overlap before spawning children; dropping the future releases its slot.
    fn navigation_commands(
        &self,
        commands: Vec<Vec<String>>,
        trace_id: uuid::Uuid,
    ) -> impl std::future::Future<Output = Result<Option<String>, String>> + Send + 'static {
        let binary = self.binary_path.clone();
        let session = self.session_name.clone();
        let permit = self.navigation_gate.clone().try_acquire_owned();
        crate::diagnostics::record(
            "browser.navigation.admission",
            serde_json::json!({
                "trace_id": trace_id,
                "outcome": if permit.is_ok() { "admitted" } else { "overlap_rejected" },
            }),
        );
        async move {
            let _permit =
                permit.map_err(|_| "Browser navigation already in progress".to_string())?;
            let binary =
                binary.ok_or_else(|| "Browser daemon has not been initialized".to_string())?;
            for command in commands {
                let args: Vec<&str> = command.iter().map(String::as_str).collect();
                cli::run(&binary, &session, &args, trace_id).await?;
            }
            let data = cli::run(&binary, &session, &["get", "url"], trace_id).await?;
            Ok(data.get("url").and_then(Value::as_str).map(str::to_owned))
        }
    }

    /// Send a newline-delimited JSON command to the daemon socket.
    pub fn send_command(&self, action: &str, params: Value) -> Result<Value, String> {
        transport::request(
            &self.daemon_socket_path(),
            &Self::command_request(action, params),
        )
    }

    /// Prepare owned request data on GTK; perform socket I/O when polled on the async runtime.
    pub fn send_command_async(
        &self,
        action: &str,
        params: Value,
    ) -> impl std::future::Future<Output = Result<Value, String>> + Send + 'static {
        let path = self.daemon_socket_path();
        let request = Self::command_request(action, params);
        async move { transport::request_async(&path, &request).await }
    }

    /// Add a fresh protocol identity and action to caller-owned parameters.
    fn command_request(action: &str, params: Value) -> Value {
        let req_id = format!("cmux-{}", Uuid::new_v4());
        let mut request = if let Value::Object(map) = params {
            Value::Object(map)
        } else {
            Value::Object(serde_json::Map::new())
        };
        request
            .as_object_mut()
            .unwrap()
            .insert("id".to_string(), Value::String(req_id));
        request
            .as_object_mut()
            .unwrap()
            .insert("action".to_string(), Value::String(action.to_string()));

        request
    }

    /// Read the bounded stream-port advertisement for this manager's daemon.
    pub fn read_stream_port(&self) -> Result<u16, String> {
        read_stream_port_file(&self.stream_port_path())
            .map_err(|error| format!("Failed to read browser stream port: {error}"))
    }

    /// Cancel and release the frame reader; repeated calls are harmless.
    fn stop_stream(&mut self) {
        if let Some(task) = self.stream_task.take() {
            task.abort();
        }
    }

    /// Shut down the daemon and clean up.
    pub fn shutdown(&mut self) {
        // Try to send close command (best-effort).
        let _ = self.send_command("close", serde_json::json!({"id": "cmux-shutdown"}));

        self.stop_stream();
    }

    /// Connect to the agent-browser stream WebSocket and start forwarding
    /// decoded JPEG frames to GTK through a latest-value channel.
    /// Immutable shared bytes avoid copying the JPEG when GTK consumes it.
    pub fn start_stream(
        &mut self,
        runtime: &tokio::runtime::Handle,
        picture: gtk4::Picture,
    ) -> Result<(), String> {
        let port = self.read_stream_port()?;
        self.stop_stream();
        self.stream_task = Some(stream::start(runtime, port, picture));

        self.preview_state = PreviewState::Streaming;
        Ok(())
    }
}

impl Drop for BrowserManager {
    /// Cancel the owned frame reader without blocking destruction or issuing browser commands.
    fn drop(&mut self) {
        self.stop_stream();
    }
}

/// Widgets returned by create_preview_pane for callers to connect signals.
#[derive(Clone)]
pub struct PreviewPaneWidgets {
    pub container: gtk4::Box,
    pub picture: gtk4::Picture,
    pub url_entry: gtk4::Entry,
    pub back_btn: gtk4::Button,
    pub forward_btn: gtk4::Button,
    pub reload_btn: gtk4::Button,
    pub go_btn: gtk4::Button,
    pub devtools_btn: gtk4::ToggleButton,
    pub pane_id: u64,
    pub uuid: Uuid,
}

/// Create a browser preview pane widget (nav bar + Picture + status overlay).
/// Returns PreviewPaneWidgets so callers can connect button signals.
pub fn create_preview_pane(next_pane_id: u64) -> PreviewPaneWidgets {
    let uuid = Uuid::new_v4();
    let picture = gtk4::Picture::new();
    picture.add_css_class("browser-preview");
    picture.set_can_shrink(true);
    picture.set_hexpand(true);
    picture.set_vexpand(true);

    let overlay = gtk4::Overlay::new();
    overlay.add_css_class("preview-container");
    overlay.set_child(Some(&picture));
    overlay.set_vexpand(true);

    // Empty state label (shown when no stream is active)
    let empty_label = gtk4::Label::new(Some("No browser preview"));
    empty_label.add_css_class("preview-empty");
    empty_label.set_halign(gtk4::Align::Center);
    empty_label.set_valign(gtk4::Align::Center);
    overlay.add_overlay(&empty_label);

    // Navigation bar buttons
    let back_btn = gtk4::Button::with_label("\u{25C0}");
    back_btn.add_css_class("browser-nav-btn");
    back_btn.set_tooltip_text(Some("Back"));

    let forward_btn = gtk4::Button::with_label("\u{25B6}");
    forward_btn.add_css_class("browser-nav-btn");
    forward_btn.set_tooltip_text(Some("Forward"));

    let reload_btn = gtk4::Button::with_label("\u{21BB}");
    reload_btn.add_css_class("browser-nav-btn");
    reload_btn.set_tooltip_text(Some("Reload"));

    // URL entry inside the nav bar
    let url_entry = gtk4::Entry::new();
    url_entry.set_placeholder_text(Some("Enter URL..."));
    url_entry.set_text("about:blank");
    url_entry.add_css_class("browser-url-bar");
    url_entry.set_hexpand(true);

    let go_btn = gtk4::Button::with_label("\u{2192}");
    go_btn.add_css_class("browser-nav-btn");
    go_btn.add_css_class("browser-nav-go");
    go_btn.set_tooltip_text(Some("Go"));

    let devtools_btn = gtk4::ToggleButton::with_label("{ }");
    devtools_btn.add_css_class("browser-nav-btn");
    devtools_btn.add_css_class("browser-nav-devtools");
    devtools_btn.set_tooltip_text(Some("Developer Tools"));

    // Navigation bar: horizontal box with buttons + URL entry
    let nav_bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    nav_bar.add_css_class("browser-nav-bar");
    nav_bar.append(&back_btn);
    nav_bar.append(&forward_btn);
    nav_bar.append(&reload_btn);
    nav_bar.append(&url_entry);
    nav_bar.append(&go_btn);
    nav_bar.append(&devtools_btn);

    // Vertical box: nav bar on top, picture overlay below
    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    vbox.append(&nav_bar);
    vbox.append(&overlay);

    PreviewPaneWidgets {
        container: vbox,
        picture,
        url_entry,
        back_btn,
        forward_btn,
        reload_btn,
        go_btn,
        devtools_btn,
        pane_id: next_pane_id,
        uuid,
    }
}

/// Read at most 64 bytes of daemon metadata and require a nonzero TCP port.
/// Performs blocking file I/O; invalid contents are excluded from errors.
fn read_stream_port_file(path: &std::path::Path) -> std::io::Result<u16> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(65)
        .read_to_end(&mut bytes)?;
    let invalid = || {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid browser stream port",
        )
    };
    if bytes.len() > 64 {
        return Err(invalid());
    }
    std::str::from_utf8(&bytes)
        .ok()
        .and_then(|text| text.trim().parse::<u16>().ok())
        .filter(|port| *port != 0)
        .ok_or_else(invalid)
}

#[cfg(test)]
mod manager_tests {
    use super::*;

    /// Independent managers resolve different command and stream endpoints without starting daemons.
    #[test]
    fn manager_sessions_are_isolated() {
        let first = BrowserManager::new();
        let second = BrowserManager::new();
        assert_ne!(first.daemon_socket_path(), second.daemon_socket_path());
        assert_ne!(first.stream_port_path(), second.stream_port_path());
        assert_eq!(
            first.daemon_socket_path().file_stem(),
            first.stream_port_path().file_stem()
        );
    }

    /// Execute the public CLI path to verify ordered history commands and admission cleanup.
    #[tokio::test]
    async fn history_operations_are_ordered_and_release_capacity() {
        let directory = std::env::temp_dir().join(format!("cmux-navigation-{}", Uuid::new_v4()));
        cmux_platform::filesystem::create_private_directory(&directory).unwrap();
        let binary = directory.join("browser fixture");
        std::fs::write(
            &binary,
            br#"#!/bin/sh
[ "$1" = '--session' ] && [ "$3" = '--json' ] || exit 2
printf '%s %s\n' "$2" "$4" >> "$0.calls"
case "$4" in
    get)
        [ "$5" = 'url' ] || exit 3
        printf '%s\n' '{"success":true,"data":{"url":"https://example.test/restored"}}'
        ;;
    set)
        [ "$5" = 'viewport' ] && [ "$6" = '800' ] && [ "$7" = '600' ] || exit 4
        printf '%s\n' '{"success":true,"data":{}}'
        ;;
    open)
        [ "$5" = 'https://example.test/a b?x=$(false)' ] || exit 5
        printf '%s\n' '{"success":true,"data":{}}'
        ;;
    fail) exit 7 ;;
    *) printf '%s\n' '{"success":true,"data":{}}' ;;
esac
"#,
        )
        .unwrap();
        cmux_platform::filesystem::set_executable_permissions(&binary).unwrap();
        let mut browser = BrowserManager::new();
        browser.binary_path = Some(binary.clone());
        let first = browser.navigate_async("back".into(), Uuid::new_v4());
        assert!(browser
            .navigate_async("overlap".into(), Uuid::new_v4())
            .await
            .is_err());
        assert_eq!(
            first.await.unwrap().as_deref(),
            Some("https://example.test/restored")
        );
        assert!(browser
            .navigate_async("fail".into(), Uuid::new_v4())
            .await
            .is_err());
        // An unpolled operation still owns admission and must release it on drop.
        drop(browser.navigate_async("cancelled".into(), Uuid::new_v4()));
        assert!(browser
            .navigate_async("forward".into(), Uuid::new_v4())
            .await
            .unwrap()
            .is_some());
        let url = "https://example.test/a b?x=$(false)".to_string();
        let open = browser.open_async(url.clone(), Some((800, 600)), Uuid::new_v4());
        assert!(browser
            .navigate_async("overlap".into(), Uuid::new_v4())
            .await
            .is_err());
        assert!(open.await.unwrap().is_some());
        assert!(browser
            .open_async(url.clone(), Some((0, 600)), Uuid::new_v4())
            .await
            .unwrap()
            .is_some());
        // A viewport failure must stop the sequence before open and URL refresh.
        assert!(browser
            .open_async(url, Some((640, 480)), Uuid::new_v4())
            .await
            .is_err());
        let calls =
            std::fs::read_to_string(binary.with_file_name("browser fixture.calls")).unwrap();
        let expected = [
            "back", "get", "fail", "forward", "get", "set", "open", "get", "open", "get", "set",
        ]
        .map(|command| format!("{} {command}\n", browser.session_name))
        .concat();
        assert_eq!(calls, expected);
        std::fs::remove_dir_all(directory).unwrap();
    }

    /// Real port files accept valid advertisements and reject oversized or unusable ports.
    #[test]
    fn stream_port_files_are_bounded() {
        let path = std::env::temp_dir().join(format!("cmux-port-{}", Uuid::new_v4()));
        for valid in [b"1".as_slice(), b"65535\n"] {
            std::fs::write(&path, valid).unwrap();
            assert!(read_stream_port_file(&path).is_ok());
        }
        for invalid in [
            b"0".as_slice(),
            b"65536",
            b"",
            b"invalid",
            &[255],
            &[b'1'; 65],
        ] {
            std::fs::write(&path, invalid).unwrap();
            assert_eq!(
                read_stream_port_file(&path).unwrap_err().kind(),
                std::io::ErrorKind::InvalidData
            );
        }
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            read_stream_port_file(&path).unwrap_err().kind(),
            std::io::ErrorKind::NotFound
        );
    }
}
