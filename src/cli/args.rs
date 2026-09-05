//! Shared command schema for the CLI, completions and man-page generator.

use clap::{Parser, Subcommand};

/// Parsed global flags and the selected terminal-multiplexer operation.
#[derive(Parser)]
#[command(name = "cmux", version = env!("CMUX_VERSION"), about = "Control cmux terminal multiplexer")]
pub struct Cli {
    /// Path to the cmux socket (overrides discovery)
    #[arg(long, global = true, env = "CMUX_SOCKET")]
    pub(super) socket: Option<String>,

    /// Output raw JSON responses
    #[arg(long, global = true)]
    pub(super) json: bool,

    /// Suppress JSON output for browser commands (browser defaults to JSON)
    #[arg(long, global = true)]
    pub(super) no_json: bool,

    /// Verbose output (connection info to stderr)
    #[arg(short, long, global = true)]
    pub(super) verbose: bool,

    /// Color mode: always, never, auto
    #[arg(long, global = true, default_value = "auto")]
    pub(super) color: String,

    #[command(subcommand)]
    pub(super) command: Commands,
}

/// Supported CLI operations, independent of socket transport and desktop state.
#[derive(Subcommand)]
pub enum Commands {
    /// Update a self-managed cmux installation
    Update,
    /// Ping the running cmux instance
    Ping,
    /// Show cmux instance identity (version, platform, pid)
    Identify,
    /// List supported socket commands
    Capabilities,
    /// Show process resources and diagnostic logging health
    Diagnostics,
    /// List all workspaces
    ListWorkspaces,
    /// Show the current workspace
    CurrentWorkspace,
    /// Send an arbitrary JSON-RPC method
    Raw {
        /// The method name (e.g. "workspace.list")
        method: String,
        /// JSON params string
        #[arg(long, default_value = "{}")]
        params: String,
    },

    // -- Workspace management --
    /// Create a new workspace
    NewWorkspace {
        /// Display name (defaults to the selected folder name)
        #[arg(long)]
        name: Option<String>,
        /// Folder new terminals in this workspace start in
        #[arg(long, value_name = "PATH")]
        cwd: Option<String>,
    },
    /// Select a workspace by ID
    SelectWorkspace {
        /// Workspace UUID
        id: String,
    },
    /// Close a workspace by ID
    CloseWorkspace {
        /// Workspace UUID
        id: String,
    },
    /// Rename a workspace
    RenameWorkspace {
        /// Workspace UUID
        id: String,
        /// New name
        name: String,
    },
    /// Switch to next workspace
    NextWorkspace,
    /// Switch to previous workspace
    PrevWorkspace,
    /// Switch to last active workspace
    LastWorkspace,
    /// Reorder a workspace
    ReorderWorkspace {
        /// Workspace UUID
        id: String,
        /// Target position (0-indexed)
        position: usize,
    },

    // -- Surface commands --
    /// List all surfaces
    ListSurfaces,
    /// Split a surface
    Split {
        /// Split direction: horizontal or vertical
        #[arg(long, default_value = "horizontal")]
        direction: String,
        /// Target surface ID (default: focused)
        #[arg(long)]
        id: Option<String>,
    },
    /// Focus a surface by ID
    FocusSurface {
        /// Surface UUID
        id: String,
    },
    /// Close a surface by ID
    CloseSurface {
        /// Surface UUID
        id: String,
    },
    /// Send text to a surface
    SendText {
        /// Text to send
        text: String,
        /// Target surface ID (default: focused)
        #[arg(long)]
        id: Option<String>,
    },
    /// Send one literal character to a terminal surface
    SendKey {
        /// Literal character (named key combinations are not supported)
        key: String,
        /// Target surface ID (default: focused)
        #[arg(long)]
        id: Option<String>,
    },
    /// Read current terminal viewport text (up to 256 KiB)
    ReadText {
        /// Target surface ID (default: focused)
        #[arg(long)]
        id: Option<String>,
    },
    /// Check surface health
    Health {
        /// Target surface ID (default: focused)
        #[arg(long)]
        id: Option<String>,
    },
    /// Refresh a surface
    Refresh {
        /// Target surface ID (default: focused)
        #[arg(long)]
        id: Option<String>,
    },

    // -- Pane commands --
    /// List all panes
    ListPanes,
    /// Focus a pane
    FocusPane {
        /// Pane ID (default: next)
        id: Option<String>,
    },
    /// Switch to last focused pane
    LastPane,

    // -- Window commands --
    /// List all windows
    ListWindows,
    /// Show current window info
    CurrentWindow,

    // -- Debug commands --
    /// Show layout tree
    Layout,
    /// Type text into the focused terminal
    Type {
        /// Text to type
        text: String,
    },

    // -- Notification commands --
    /// List notifications
    ListNotifications,
    /// Clear a notification
    ClearNotification {
        /// Notification ID
        id: String,
    },

    // -- Browser subcommand group (agent primary interface) --
    /// Browser automation (agent primary interface)
    #[command(subcommand)]
    Browser(BrowserCommand),
}

/// Browser subcommands for `cmux browser <action>` / `cmux browser <surface> <action>`.
/// Browser operations translated to the socket protocol by the command runner.
#[derive(Subcommand)]
pub enum BrowserCommand {
    /// Open a URL in the browser pane
    Open {
        /// URL to open
        url: String,
        /// Target workspace ID
        #[arg(long)]
        workspace: Option<String>,
    },
    /// List browser surfaces
    List,
    /// Close browser surface(s)
    Close {
        /// Surface reference (surface:N or UUID); closes all if omitted
        #[arg(long)]
        surface: Option<String>,
    },
    /// Take a browser snapshot (accessibility tree / DOM text)
    Snapshot {
        /// Surface reference (surface:N or UUID)
        surface: String,
        /// Include interactive element annotations
        #[arg(long)]
        interactive: bool,
        /// Compact output
        #[arg(long)]
        compact: bool,
        /// Maximum depth
        #[arg(long)]
        max_depth: Option<u32>,
    },
    /// Click an element
    Click {
        /// Surface reference (surface:N or UUID)
        surface: String,
        /// Target element (e1 or CSS selector)
        target: String,
        /// Take snapshot after action
        #[arg(long)]
        snapshot_after: bool,
    },
    /// Fill an input field (clears first, then types)
    Fill {
        /// Surface reference (surface:N or UUID)
        surface: String,
        /// Target element (CSS selector)
        target: String,
        /// Value to fill
        text: String,
        /// Take snapshot after action
        #[arg(long)]
        snapshot_after: bool,
    },
    /// Type text into an element
    #[command(name = "type")]
    BrowserType {
        /// Surface reference (surface:N or UUID)
        surface: String,
        /// CSS selector of the element
        selector: String,
        /// Text to type
        text: String,
    },
    /// Press a key (e.g. "Enter", "Tab", "Escape")
    Press {
        /// Surface reference (surface:N or UUID)
        surface: String,
        /// Key name
        key: String,
    },
    /// Hover over an element
    Hover {
        /// Surface reference (surface:N or UUID)
        surface: String,
        /// CSS selector of the element
        selector: String,
    },
    /// Scroll the page
    Scroll {
        /// Surface reference (surface:N or UUID)
        surface: String,
        /// Direction: up, down, left, right
        direction: String,
        /// Amount in pixels
        #[arg(long, default_value = "300")]
        amount: i32,
    },
    /// Select an option from a dropdown
    #[command(name = "select")]
    Select {
        /// Surface reference (surface:N or UUID)
        surface: String,
        /// CSS selector of the select element
        selector: String,
        /// Value to select
        value: String,
    },
    /// Evaluate JavaScript in the browser
    Eval {
        /// Surface reference (surface:N or UUID)
        surface: String,
        /// JavaScript expression to evaluate
        expression: String,
    },
    /// Wait for a condition
    Wait {
        /// Surface reference (surface:N or UUID)
        surface: String,
        /// CSS selector to wait for
        #[arg(long)]
        selector: Option<String>,
        /// Text to wait for
        #[arg(long)]
        text: Option<String>,
        /// URL substring to wait for
        #[arg(long)]
        url_contains: Option<String>,
        /// Load state to wait for
        #[arg(long)]
        load_state: Option<String>,
        /// JavaScript function to wait for
        #[arg(long)]
        function: Option<String>,
        /// Timeout in milliseconds
        #[arg(long, default_value = "30000")]
        timeout_ms: u64,
    },
    /// Navigate to a URL
    Goto {
        /// Surface reference (surface:N or UUID)
        surface: String,
        /// URL to navigate to
        url: String,
    },
    /// Go back in browser history
    Back {
        /// Surface reference (surface:N or UUID)
        surface: String,
    },
    /// Go forward in browser history
    Forward {
        /// Surface reference (surface:N or UUID)
        surface: String,
    },
    /// Reload the current page
    Reload {
        /// Surface reference (surface:N or UUID)
        surface: String,
    },
    /// Get the current page URL
    #[command(name = "get-url")]
    GetUrl {
        /// Surface reference (surface:N or UUID)
        surface: String,
    },
    /// Get the current page title
    #[command(name = "get-title")]
    GetTitle {
        /// Surface reference (surface:N or UUID)
        surface: String,
    },
    /// Get text content of an element
    #[command(name = "get-text")]
    GetText {
        /// Surface reference (surface:N or UUID)
        surface: String,
        /// CSS selector of the element
        selector: String,
    },
    /// Get HTML content of an element
    #[command(name = "get-html")]
    GetHtml {
        /// Surface reference (surface:N or UUID)
        surface: String,
        /// CSS selector of the element
        selector: String,
    },
    /// Take a browser screenshot (base64 PNG)
    Screenshot {
        /// Surface reference (surface:N or UUID)
        surface: String,
    },
    /// Enable browser streaming
    #[command(name = "stream-enable")]
    StreamEnable,
    /// Disable browser streaming
    #[command(name = "stream-disable")]
    StreamDisable,
}
