use gtk4::prelude::*;
use serde_json::Value;
mod cli;
mod discovery;
mod frames;
mod input;
mod input_queue;
mod mapped;
pub(crate) mod metrics;
mod motion;
mod pixels;
mod stream;
pub(crate) mod ui;
pub use motion::spawn_motion_forwarder;
pub(crate) mod transport;
pub use discovery::agent_browser_available;
use discovery::{find_system_chrome, which_agent_browser};
use std::path::PathBuf;
use uuid::Uuid;

/// Build the shared public CLI viewport arguments without shell interpretation.
fn viewport_command(width: i32, height: i32) -> Vec<String> {
    vec![
        "set".into(),
        "viewport".into(),
        width.to_string(),
        height.to_string(),
    ]
}

/// GTK-owned close tasks retained until application exit drains them before stopping Tokio.
pub type ShutdownTasks = std::rc::Rc<std::cell::RefCell<tokio::task::JoinSet<()>>>;

/// Finish browser close exchanges after GTK exits, allowing at most seven seconds for all tasks.
/// Deadline expiry aborts and reaps remaining tasks; it does not prove daemon termination.
pub async fn drain_shutdown(mut tasks: tokio::task::JoinSet<()>) {
    let result = tokio::time::timeout(std::time::Duration::from_secs(7), async {
        while let Some(result) = tasks.join_next().await {
            if result.is_err() {
                crate::diagnostics::record("browser.shutdown.task_failed", serde_json::json!({}));
            }
        }
    })
    .await;
    if result.is_err() {
        crate::diagnostics::record(
            "browser.shutdown.drain_timeout",
            serde_json::json!({"budget_ms": 7000}),
        );
        tasks.shutdown().await;
    }
}

/// Startup intent shared by GTK preview actions and socket lifecycle requests.
pub(crate) enum StartupRequest {
    Preview(String),
    Open(Value),
    Stream,
}

/// Public endpoint code and safe startup/exchange detail for response delivery.
#[derive(Debug)]
pub(crate) struct StartupError {
    pub code: &'static str,
    pub message: String,
}
impl From<String> for StartupError {
    /// Classify discovery and public CLI initialization failures as daemon errors.
    fn from(message: String) -> Self {
        Self {
            code: "daemon_error",
            message,
        }
    }
}

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
    navigation_shutdown: tokio::sync::watch::Sender<bool>,
    binary_path: Option<PathBuf>,
    stream_task: Option<tokio::task::JoinHandle<()>>,
    input_queue: Option<input_queue::InputQueue>,
    mapped_navigation: Option<mapped::MappedNavigation>,

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
            navigation_shutdown: tokio::sync::watch::channel(false).0,
            binary_path: None,
            stream_task: None,
            input_queue: None,
            mapped_navigation: None,
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

    /// Initialize and execute a browser lifecycle transaction off GTK under shared admission and cancellation.
    /// Return its executable and response for the originating manager to apply; preserve endpoint error codes.
    pub(crate) fn startup_async(
        &self,
        request: StartupRequest,
        trace_id: Uuid,
    ) -> impl std::future::Future<Output = Result<(PathBuf, Value), StartupError>> + Send + 'static
    {
        let session = self.session_name.clone();
        let binary = self.binary_path.clone();
        let socket_dir = Self::agent_browser_socket_dir();
        let socket_path = self.daemon_socket_path();
        let permit = self.navigation_gate.clone().try_acquire_owned();
        let mut shutdown = self.navigation_shutdown.subscribe();
        async move {
            let permit = std::sync::Arc::new(permit.map_err(|_| {
                "Browser navigation unavailable or already in progress".to_string()
            })?);
            if *shutdown.borrow() {
                return Err("Browser manager stopped".to_string().into());
            }
            let prepare = async move {
                let discovery_permit = permit.clone();
                let (binary, chrome) = tokio::task::spawn_blocking(move || {
                    // A cancelled waiter cannot release admission while discovery still runs.
                    let _permit = discovery_permit;
                    let binary = binary.or_else(which_agent_browser);
                    let chrome = if std::env::var_os("AGENT_BROWSER_EXECUTABLE_PATH").is_none() {
                        find_system_chrome()
                    } else {
                        None
                    };
                    (binary, chrome)
                })
                .await
                .map_err(|_| "Browser discovery worker failed".to_string())?;
                let binary = binary.ok_or_else(|| "agent-browser is not installed; browser panes are unavailable. Install it with: npm install -g agent-browser && agent-browser install".to_string())?;
                tokio::fs::create_dir_all(socket_dir)
                    .await
                    .map_err(|error| {
                        format!("Failed to create browser socket directory: {error}")
                    })?;
                let ready = tokio::net::UnixStream::connect(&socket_path).await.is_ok();
                if let StartupRequest::Preview(url) = &request {
                    cli::start(&binary, &session, chrome.as_deref(), url, trace_id).await?;
                } else if !ready {
                    cli::start(
                        &binary,
                        &session,
                        chrome.as_deref(),
                        "about:blank",
                        trace_id,
                    )
                    .await?;
                }
                let result = match request {
                    StartupRequest::Preview(_) => {
                        if cli::run(&binary, &session, &["stream", "enable"], trace_id)
                            .await
                            .is_err()
                        {
                            crate::diagnostics::record(
                                "browser.preview.stream_enable.failed",
                                serde_json::json!({"trace_id": trace_id}),
                            );
                        }
                        Value::Null
                    }
                    StartupRequest::Open(params) => {
                        let response = transport::request_async(
                            &socket_path,
                            &Self::command_request("navigate", params),
                        )
                        .await
                        .map_err(|message| StartupError {
                            code: "browser_error",
                            message,
                        })?;
                        let _ = transport::request_async(
                            &socket_path,
                            &Self::command_request("stream_enable", serde_json::json!({})),
                        )
                        .await;
                        response
                    }
                    StartupRequest::Stream => transport::request_async(
                        &socket_path,
                        &Self::command_request("stream_enable", serde_json::json!({})),
                    )
                    .await
                    .map_err(|message| StartupError {
                        code: "stream_error",
                        message,
                    })?,
                };
                Ok((binary, result))
            };
            tokio::select! {
                biased;
                _ = shutdown.changed() => Err("Browser manager stopped".to_string().into()),
                result = tokio::time::timeout(std::time::Duration::from_secs(15), prepare) => {
                    result.unwrap_or_else(|_| {
                        crate::diagnostics::record("browser.preview.startup.timeout", serde_json::json!({
                            "trace_id": trace_id, "budget_ms": 15_000,
                        }));
                        Err("Browser preview startup deadline exceeded".to_string().into())
                    })
                }
            }
        }
    }

    /// Prepare an existing or new GTK preview through the same bounded startup transaction.
    fn prepare_preview_async(
        &self,
        url: String,
        trace_id: Uuid,
    ) -> impl std::future::Future<Output = Result<PathBuf, String>> + Send + 'static {
        let startup = self.startup_async(StartupRequest::Preview(url), trace_id);
        async move {
            startup
                .await
                .map(|(binary, _)| binary)
                .map_err(|error| error.message)
        }
    }

    /// Capture manager identity for a worker result that must not update a replacement manager.
    pub(crate) fn session_identity(&self) -> String {
        self.session_name.clone()
    }

    /// Install the executable only when this manager still owns the startup completion.
    pub(crate) fn install_startup(&mut self, session: &str, binary: PathBuf) -> bool {
        if self.session_name != session {
            return false;
        }
        self.binary_path = Some(binary);
        self.preview_state = PreviewState::Connected;
        true
    }

    /// Coalesce mapped-tab destinations on an owned worker after browser initialization.
    fn queue_mapped_url(
        &mut self,
        runtime: &tokio::runtime::Handle,
        url: String,
        visible: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) {
        let Some(binary) = self.binary_path.clone() else {
            return;
        };
        let session = self.session_name.clone();
        let gate = self.navigation_gate.clone();
        self.mapped_navigation
            .get_or_insert_with(|| mapped::MappedNavigation::new(runtime, binary, session, gate))
            .navigate(url, visible);
    }

    /// Prepare history navigation and URL refresh for worker execution after daemon startup.
    /// Navigation shares one admission slot with URL-entry operations.
    pub fn navigate_async(
        &self,
        command: String,
        trace_id: uuid::Uuid,
    ) -> impl std::future::Future<Output = Result<Option<String>, String>> + Send + 'static {
        self.navigation_commands(vec![vec![command]], true, trace_id)
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
            commands.push(viewport_command(width, height));
        }
        commands.push(vec!["open".into(), url]);
        self.navigation_commands(commands, true, trace_id)
    }

    /// Set an allocated preview size on the bounded CLI worker without requesting URL refresh.
    /// Invalid geometry performs no child I/O; overlap follows the shared navigation admission policy.
    fn resize_async(
        &self,
        width: i32,
        height: i32,
        trace_id: Uuid,
    ) -> impl std::future::Future<Output = Result<(), String>> + Send + 'static {
        let operation =
            self.navigation_commands(vec![viewport_command(width, height)], false, trace_id);
        async move {
            if width <= 0 || height <= 0 {
                return Err("Browser viewport dimensions must be positive".to_string());
            }
            operation.await.map(|_| ())
        }
    }

    /// Execute ordered public CLI commands with optional URL refresh while owning one admission permit.
    /// Reject overlap before spawning children; dropping the future releases its slot.
    /// The entire sequence shares a fifteen-second deadline, including URL refresh.
    fn navigation_commands(
        &self,
        commands: Vec<Vec<String>>,
        refresh_url: bool,
        trace_id: uuid::Uuid,
    ) -> impl std::future::Future<Output = Result<Option<String>, String>> + Send + 'static {
        let mut shutdown = self.navigation_shutdown.subscribe();
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
            let _permit = permit
                .map_err(|_| "Browser navigation unavailable or already in progress".to_string())?;
            let cancelled = || "Browser manager stopped".to_string();
            if *shutdown.borrow() {
                return Err(cancelled());
            }
            let operation = async {
                let binary =
                    binary.ok_or_else(|| "Browser daemon has not been initialized".to_string())?;
                for command in commands {
                    let args: Vec<&str> = command.iter().map(String::as_str).collect();
                    cli::run(&binary, &session, &args, trace_id).await?;
                }
                if refresh_url {
                    let data = cli::run(&binary, &session, &["get", "url"], trace_id).await?;
                    Ok(data.get("url").and_then(Value::as_str).map(str::to_owned))
                } else {
                    Ok(None)
                }
            };
            tokio::select! {
                biased;
                _ = shutdown.changed() => Err(cancelled()),
                result = tokio::time::timeout(std::time::Duration::from_secs(15), operation) => {
                    result.unwrap_or_else(|_| {
                        crate::diagnostics::record("browser.navigation.timeout", serde_json::json!({
                            "trace_id": trace_id,
                            "budget_ms": 15_000,
                        }));
                        Err("Browser navigation deadline exceeded".to_string())
                    })
                },
            }
        }
    }

    /// Lazily attach an ordered input worker to this manager and runtime.
    fn input_queue(&mut self, runtime: &tokio::runtime::Handle) -> &mut input_queue::InputQueue {
        let path = self.daemon_socket_path();
        self.input_queue
            .get_or_insert_with(|| input_queue::InputQueue::new(runtime, path))
    }

    /// Queue an entire mouse gesture; report overload without retaining payloads in diagnostics.
    fn queue_mouse(&mut self, runtime: &tokio::runtime::Handle, events: Vec<Value>) -> bool {
        let batch = events
            .into_iter()
            .map(|params| Self::command_request("input_mouse", params))
            .collect();
        let admitted = self.input_queue(runtime).send(batch);
        if !admitted {
            crate::diagnostics::record("browser.input.overloaded", serde_json::json!({}));
        }
        admitted
    }

    /// Queue physical key transitions while reserving a slot for every admitted key release.
    fn queue_key(
        &mut self,
        runtime: &tokio::runtime::Handle,
        physical: u32,
        pressed: bool,
        params: Value,
    ) -> bool {
        let request = Self::command_request("input_keyboard", params);
        let admitted = self.input_queue(runtime).key(physical, pressed, request);
        if !admitted {
            crate::diagnostics::record("browser.input.overloaded", serde_json::json!({}));
        }
        admitted
    }

    /// Release browser keys when the preview loses GTK focus.
    fn release_input_keys(&mut self) {
        if let Some(queue) = self.input_queue.as_mut() {
            queue.release_keys();
        }
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

    /// Fetch a snapshot with bounded async transport and prepare display text off GTK.
    /// Formatting uses a blocking worker; cancellation may leave only that bounded CPU job finishing.
    pub fn snapshot_async(
        &self,
    ) -> impl std::future::Future<Output = Result<String, String>> + Send + 'static {
        let request = self.send_command_async("snapshot", serde_json::json!({}));
        async move {
            // Keep admission through formatting, even if the caller cancels after
            // the blocking worker starts. Rapid toggles cannot queue unbounded jobs.
            static SNAPSHOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
            let permit = SNAPSHOTS
                .try_acquire()
                .map_err(|_| "Snapshot capacity reached".to_string())?;
            let response = request.await?;
            tokio::task::spawn_blocking(move || {
                let _permit = permit;
                snapshot_text(response)
            })
            .await
            .map_err(|error| format!("Snapshot formatting failed: {error}"))?
        }
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

    /// Close navigation admission and wake admitted operations so their child futures are dropped.
    fn stop_navigation(&self) {
        self.navigation_gate.close();
        self.navigation_shutdown.send_replace(true);
    }

    /// Cancel local work immediately and return an owned, bounded daemon-close operation.
    /// Allow up to one second for admitted navigation to release before the five-second exchange.
    pub fn shutdown(mut self) -> impl std::future::Future<Output = ()> + Send + 'static {
        self.stop_navigation();
        self.input_queue.take();
        self.mapped_navigation.take();
        self.stop_stream();
        let gate = self.navigation_gate.clone();
        let close = self.send_command_async("close", serde_json::json!({}));
        async move {
            let mut activity = metrics::Activity::begin("shutdown", None);
            let _ = tokio::time::timeout(std::time::Duration::from_secs(1), async {
                while gate.available_permits() == 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await;
            activity.finish(if close.await.is_ok() {
                "response_received"
            } else {
                "transport_error"
            });
        }
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
    /// Cancel owned navigation and frame work without issuing blocking browser commands.
    fn drop(&mut self) {
        self.stop_navigation();
        self.input_queue.take();
        self.mapped_navigation.take();
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
    picture.set_content_fit(gtk4::ContentFit::Contain);
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

/// Move textual snapshot payloads out of the response or serialize structured fallback compactly.
/// Compact JSON avoids indentation amplification of the transport's bounded response.
fn snapshot_text(mut response: Value) -> Result<String, String> {
    for field in ["data", "result"] {
        if let Some(value) = response.get_mut(field) {
            if value.is_string() {
                if let Value::String(text) = value.take() {
                    return Ok(text);
                }
            }
        }
    }
    serde_json::to_string(&response).map_err(|error| format!("Invalid snapshot: {error}"))
}

#[cfg(test)]
mod manager_tests {
    use super::*;

    /// Text fields preserve Unicode and emptiness; nested fallback does not gain indentation.
    #[test]
    fn snapshot_display_preserves_payload_without_indentation_growth() {
        assert_eq!(
            snapshot_text(serde_json::json!({"data": "λ\ntext"})).unwrap(),
            "λ\ntext"
        );
        assert_eq!(
            snapshot_text(serde_json::json!({"data": "", "result": "ignored"})).unwrap(),
            ""
        );
        assert_eq!(
            snapshot_text(serde_json::json!({"data": {}, "result": "fallback"})).unwrap(),
            "fallback"
        );
        let mut nested = serde_json::json!([0, 1, 2, 3]);
        for _ in 0..64 {
            nested = serde_json::json!([nested]);
        }
        let compact = serde_json::to_string(&nested).unwrap();
        let display = snapshot_text(nested.clone()).unwrap();
        assert_eq!(display.len(), compact.len());
        assert_eq!(serde_json::from_str::<Value>(&display).unwrap(), nested);
    }

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
        assert!(browser.resize_async(800, 600, Uuid::new_v4()).await.is_ok());
        assert!(browser.resize_async(0, 600, Uuid::new_v4()).await.is_err());
        assert!(browser
            .resize_async(640, 480, Uuid::new_v4())
            .await
            .is_err());
        let session_name = browser.session_name.clone();
        let abandoned = browser.navigate_async("after_drop".into(), Uuid::new_v4());
        drop(browser);
        assert!(abandoned.await.is_err());
        let calls =
            std::fs::read_to_string(binary.with_file_name("browser fixture.calls")).unwrap();
        let expected = [
            "back", "get", "fail", "forward", "get", "set", "open", "get", "open", "get", "set",
            "set", "set",
        ]
        .map(|command| format!("{session_name} {command}\n"))
        .concat();
        assert_eq!(calls, expected);
        std::fs::remove_dir_all(directory).unwrap();
    }

    /// An already-running daemon receives open/stream actions without restarting or resetting its URL.
    #[tokio::test]
    async fn rpc_startup_reuses_ready_daemon() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
        let directory = std::env::temp_dir().join(format!("cmux-rpc-start-{}", Uuid::new_v4()));
        cmux_platform::filesystem::create_private_directory(&directory).unwrap();
        let mut browser = BrowserManager::new();
        browser.session_name = directory.join("browser").to_string_lossy().into_owned();
        browser.binary_path = Some(directory.join("must-not-be-executed"));
        let listener = tokio::net::UnixListener::bind(browser.daemon_socket_path()).unwrap();
        let server = tokio::spawn(async move {
            for action in ["", "navigate", "stream_enable", "", "stream_enable"] {
                let (peer, _) = listener.accept().await.unwrap();
                let mut peer = tokio::io::BufReader::new(peer);
                let mut line = String::new();
                peer.read_line(&mut line).await.unwrap();
                if action.is_empty() {
                    assert!(line.is_empty());
                    continue;
                }
                let request: Value = serde_json::from_str(&line).unwrap();
                assert_eq!(request["action"], action);
                if action == "navigate" {
                    assert_eq!(request["url"], "https://example.test");
                }
                peer.get_mut()
                    .write_all(b"{\"surface_id\":\"fixture\"}\n")
                    .await
                    .unwrap();
            }
        });
        let (_, opened) = browser
            .startup_async(
                StartupRequest::Open(serde_json::json!({"url":"https://example.test"})),
                Uuid::new_v4(),
            )
            .await
            .unwrap();
        assert_eq!(opened["surface_id"], "fixture");
        let (_, streaming) = browser
            .startup_async(StartupRequest::Stream, Uuid::new_v4())
            .await
            .unwrap();
        assert_eq!(streaming["surface_id"], "fixture");
        tokio::time::timeout(std::time::Duration::from_secs(3), server)
            .await
            .unwrap()
            .unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    }

    /// Shutdown cancels admitted navigation synchronously and the exit drain awaits a real close reply.
    #[tokio::test]
    async fn shutdown_drains_close_after_navigation_cancellation() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
        let directory = std::env::temp_dir().join(format!("cmux-shutdown-{}", Uuid::new_v4()));
        cmux_platform::filesystem::create_private_directory(&directory).unwrap();
        let mut browser = BrowserManager::new();
        // An absolute fixture identity keeps the fake daemon outside user runtime directories.
        browser.session_name = directory.join("browser").to_string_lossy().into_owned();
        let path = browser.daemon_socket_path();
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let navigation = browser.navigate_async("back".into(), Uuid::new_v4());
        let gate = browser.navigation_gate.clone();
        let close = browser.shutdown();
        assert!(gate.is_closed());
        assert_eq!(navigation.await.unwrap_err(), "Browser manager stopped");
        let (finished_tx, mut finished_rx) = tokio::sync::oneshot::channel();
        let mut tasks = tokio::task::JoinSet::new();
        tasks.spawn(async move {
            close.await;
            let _ = finished_tx.send(());
        });
        let server = async {
            let (peer, _) = listener.accept().await.unwrap();
            let mut peer = tokio::io::BufReader::new(peer);
            let mut line = String::new();
            peer.read_line(&mut line).await.unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["action"], "close");
            assert!(matches!(
                finished_rx.try_recv(),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)
            ));
            peer.get_mut()
                .write_all(b"{\"success\":true}\n")
                .await
                .unwrap();
        };
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            tokio::join!(drain_shutdown(tasks), server);
        })
        .await
        .unwrap();
        finished_rx.await.unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    }

    /// Preview startup orders public CLI calls, rejects overlap and cancels its direct child on shutdown.
    #[tokio::test]
    async fn preview_startup_is_ordered_and_cancellable() {
        let directory =
            std::env::temp_dir().join(format!("cmux-preview-startup-{}", Uuid::new_v4()));
        cmux_platform::filesystem::create_private_directory(&directory).unwrap();
        let binary = directory.join("browser fixture");
        std::fs::write(
            &binary,
            br#"#!/bin/sh
[ "$1" = '--session' ] && [ "$3" = '--json' ] || exit 2
session="$2"
shift 3
if [ "$1" = '--executable-path' ]; then shift 2; fi
printf '%s %s\n' "$session" "$1" >> "$0.calls"
if [ -e "$0.hang" ]; then
    printf '%s' $$ > "$0.pid"
    exec sleep 60
fi
if [ "$1" = 'open' ]; then
    [ "$AGENT_BROWSER_SESSION" = "$session" ] && [ "$AGENT_BROWSER_STREAM_PORT" = '0' ] || exit 3
    printf '%s' "$2" > "$0.url"
elif [ "$1" = 'stream' ]; then
    [ "$2" = 'enable' ] || exit 4
else
    exit 5
fi
printf '%s\n' '{"success":true,"data":{}}'
"#,
        )
        .unwrap();
        cmux_platform::filesystem::set_executable_permissions(&binary).unwrap();
        let mut browser = BrowserManager::new();
        browser.binary_path = Some(binary.clone());
        let startup = browser.prepare_preview_async("about:blank".into(), Uuid::new_v4());
        assert!(browser
            .prepare_preview_async("about:blank".into(), Uuid::new_v4())
            .await
            .is_err());
        assert_eq!(startup.await.unwrap(), binary);
        let calls = std::fs::read_to_string(directory.join("browser fixture.calls")).unwrap();
        assert_eq!(
            calls,
            format!(
                "{} open\n{} stream\n",
                browser.session_name, browser.session_name
            )
        );
        assert_eq!(
            std::fs::read_to_string(directory.join("browser fixture.url")).unwrap(),
            "about:blank"
        );
        let saved_url = "https://example.test/a b?x=$(false)&y=日本語";
        browser
            .prepare_preview_async(saved_url.into(), Uuid::new_v4())
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(directory.join("browser fixture.url")).unwrap(),
            saved_url
        );
        std::fs::write(directory.join("browser fixture.hang"), b"").unwrap();
        let task =
            tokio::spawn(browser.prepare_preview_async("about:blank".into(), Uuid::new_v4()));
        let pid_path = directory.join("browser fixture.pid");
        let pid = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if let Ok(text) = tokio::fs::read_to_string(&pid_path).await {
                    if let Ok(pid) = text.parse::<u32>() {
                        break pid;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        browser.stop_navigation();
        assert_eq!(task.await.unwrap().unwrap_err(), "Browser manager stopped");
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while PathBuf::from(format!("/proc/{pid}")).exists() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    }

    /// URL refresh receives only the budget left after the history command and is cancelled on expiry.
    #[tokio::test]
    async fn navigation_sequence_has_one_deadline() {
        let directory =
            std::env::temp_dir().join(format!("cmux-navigation-budget-{}", Uuid::new_v4()));
        cmux_platform::filesystem::create_private_directory(&directory).unwrap();
        let binary = directory.join("browser");
        std::fs::write(
            &binary,
            br#"#!/bin/sh
printf '%s\n' "$4" >> "$0.calls"
if [ "$4" = 'back' ]; then
    sleep 8
    printf '%s\n' '{"success":true,"data":{}}'
else
    printf '%s' $$ > "$0.pid"
    exec sleep 60
fi
"#,
        )
        .unwrap();
        cmux_platform::filesystem::set_executable_permissions(&binary).unwrap();
        let mut browser = BrowserManager::new();
        browser.binary_path = Some(binary.clone());
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(20),
            browser.navigate_async("back".into(), Uuid::new_v4()),
        )
        .await
        .unwrap();
        assert_eq!(result.unwrap_err(), "Browser navigation deadline exceeded");
        assert_eq!(
            std::fs::read_to_string(binary.with_extension("calls")).unwrap(),
            "back\nget\n"
        );
        let pid = std::fs::read_to_string(binary.with_extension("pid")).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while std::path::Path::new(&format!("/proc/{pid}")).exists() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        // Expiration must release admission before a subsequent operation.
        let next = browser.navigation_gate.clone().try_acquire_owned().unwrap();
        drop(next);
        std::fs::remove_dir_all(directory).unwrap();
    }

    /// Dropping the manager cancels a running CLI child before its command deadline.
    #[tokio::test]
    async fn manager_drop_cancels_live_navigation() {
        let directory =
            std::env::temp_dir().join(format!("cmux-navigation-drop-{}", Uuid::new_v4()));
        cmux_platform::filesystem::create_private_directory(&directory).unwrap();
        let binary = directory.join("browser");
        std::fs::write(
            &binary,
            b"#!/bin/sh\nprintf '%s' $$ > \"$0.pid\"\nexec sleep 60\n",
        )
        .unwrap();
        cmux_platform::filesystem::set_executable_permissions(&binary).unwrap();
        let mut browser = BrowserManager::new();
        browser.binary_path = Some(binary);
        let task = tokio::spawn(browser.navigate_async("back".into(), Uuid::new_v4()));
        let pid = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                if let Ok(text) = std::fs::read_to_string(directory.join("browser.pid")) {
                    if let Ok(pid) = text.parse::<u32>() {
                        break pid;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        drop(browser);
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(3), task)
                .await
                .unwrap()
                .unwrap()
                .is_err()
        );
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while std::path::Path::new(&format!("/proc/{pid}")).exists() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
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
