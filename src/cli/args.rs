//! Shared command schema for the CLI, completions and man-page generator.

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum DiffSource {
    Unstaged,
    Staged,
    Branch,
    LastTurn,
}

#[derive(Clone, Copy, Debug, serde::Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub(crate) enum DiffLayout {
    Unified,
    Split,
}

#[derive(Subcommand)]
pub enum CommentCommands {
    /// List pending review comments for a Git repository
    List {
        #[arg(long)]
        repo: Option<std::path::PathBuf>,
        /// Include comments already marked consumed
        #[arg(long)]
        all: bool,
    },
    /// Add a durable line-anchored review comment
    Add {
        #[arg(long)]
        repo: Option<std::path::PathBuf>,
        #[arg(long)]
        file: String,
        #[arg(long, default_value = "new")]
        side: String,
        #[arg(long)]
        line: u32,
        #[arg(long)]
        end_line: Option<u32>,
        #[arg(long, default_value = "")]
        line_text: String,
        #[arg(long)]
        message: String,
    },
    /// Delete one review comment by UUID
    Delete {
        id: String,
        #[arg(long)]
        repo: Option<std::path::PathBuf>,
    },
    /// Mark selected or all pending comments as delivered to an agent
    Consume {
        ids: Vec<String>,
        #[arg(long)]
        repo: Option<std::path::PathBuf>,
        #[arg(long, conflicts_with = "ids")]
        all: bool,
    },
}

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
    #[arg(long, global = true)]
    pub(super) color: Option<String>,

    #[command(subcommand)]
    pub(super) command: Commands,
}

/// Supported CLI operations, independent of socket transport and desktop state.
#[derive(Subcommand)]
pub enum Commands {
    /// Open a bounded patch or Git comparison in an agent-accessible diff surface
    Diff {
        /// Unified patch file, or '-' to read standard input
        input: Option<String>,
        /// Git source when no patch file is supplied
        #[arg(long, value_enum)]
        source: Option<DiffSource>,
        #[arg(long, conflicts_with_all = ["source", "staged", "branch", "last_turn", "input"])]
        unstaged: bool,
        #[arg(long, conflicts_with_all = ["source", "unstaged", "branch", "last_turn", "input"])]
        staged: bool,
        #[arg(long, conflicts_with_all = ["source", "unstaged", "staged", "last_turn", "input"])]
        branch: bool,
        #[arg(long, conflicts_with_all = ["source", "unstaged", "staged", "branch", "input"])]
        last_turn: bool,
        /// Destination workspace UUID; defaults to the caller or selected workspace
        #[arg(long)]
        workspace: Option<String>,
        /// Place the viewer immediately to the right of this surface UUID
        #[arg(long)]
        surface: Option<String>,
        /// Select a provider session-specific last-turn baseline
        #[arg(long, alias = "agent-session")]
        session: Option<String>,
        /// Repository or child path used by Git sources
        #[arg(long, alias = "repo", alias = "path")]
        cwd: Option<std::path::PathBuf>,
        /// Explicit base ref for a branch comparison
        #[arg(long, alias = "branch-base")]
        base: Option<String>,
        #[arg(long)]
        title: Option<String>,
        #[arg(long, value_enum, default_value = "unified")]
        layout: DiffLayout,
        #[arg(long)]
        font_size: Option<f64>,
        /// Focus the new viewer after it opens
        #[arg(long)]
        focus: bool,
        /// Preserve the currently focused surface (the default)
        #[arg(long, conflicts_with = "focus")]
        no_focus: bool,
    },
    /// Manage durable diff-review comments keyed by Git repository
    Comments {
        #[command(subcommand)]
        command: CommentCommands,
    },
    /// Open a bounded manifest-aware Linux project inspector
    Project {
        /// Project directory or manifest path
        #[arg(default_value = ".")]
        path: std::path::PathBuf,
        /// Destination workspace UUID; defaults to the caller or selected workspace
        #[arg(long)]
        workspace: Option<String>,
        /// Place the inspector immediately to the right of this surface UUID
        #[arg(long)]
        surface: Option<String>,
        /// Focus the new inspector after it opens
        #[arg(long)]
        focus: bool,
        /// Preserve the currently focused surface (the default)
        #[arg(long, conflicts_with = "focus")]
        no_focus: bool,
    },
    /// Launch Claude Code teams with teammate panes translated into native cmux splits
    ClaudeTeams {
        /// Arguments forwarded verbatim to Claude Code
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Private tmux compatibility endpoint used only by managed team launchers
    #[command(name = "tmux-compat-internal", hide = true, alias = "__tmux-compat")]
    TmuxCompat {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Execute an explicitly requested project command after checking its inspected fingerprint
    ProjectRun {
        action: String,
        #[arg(long)]
        fingerprint: String,
        #[arg(long)]
        workspace: Option<String>,
        /// Confirm a reviewed action that requires an additional destructive decision
        #[arg(long)]
        confirm: bool,
    },
    /// Inspect resolved project actions and their source files without running them
    ProjectActions {
        #[arg(long, conflicts_with = "workspace")]
        directory: Option<std::path::PathBuf>,
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Install and receive native agent session hooks
    Hooks {
        #[command(subcommand)]
        command: HookCommands,
    },
    /// Execute this terminal's saved manual resume command in the calling terminal
    Restore {
        #[arg(long, env = "CMUX_SURFACE_ID")]
        surface: Option<String>,
        #[arg(long)]
        checkpoint: Option<String>,
        /// Require a current application-signed approval before executing
        #[arg(long)]
        automatic: bool,
    },
    /// Manage persistent terminal surface state
    Surface {
        #[command(subcommand)]
        command: SurfaceCommands,
    },
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
    /// Create a first-class remote workspace with SSH management
    Ssh {
        destination: String,
        #[arg(long, default_value = "ssh", value_parser = ["ssh", "mosh"])]
        transport: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        directory: Option<String>,
    },
    /// Create a remote workspace using Mosh for interactive terminals
    Mosh {
        destination: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        directory: Option<String>,
    },
    /// Create a roaming Mosh terminal attached to a named remote tmux session
    MoshTmux {
        destination: String,
        #[arg(long, default_value = "main")]
        session: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        directory: Option<String>,
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
    /// Reorder listed workspaces first, retaining the relative order of all others
    ReorderWorkspaces {
        #[arg(long, value_delimiter = ',', required = true)]
        order: Vec<String>,
        #[arg(long)]
        dry_run: bool,
    },
    /// List persistent workspace groups and their members
    ListWorkspaceGroups,
    /// Create an empty persistent workspace group
    CreateWorkspaceGroup {
        name: String,
        #[arg(long)]
        color: Option<String>,
    },
    /// Update a workspace group's presentation or collapse state
    UpdateWorkspaceGroup {
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, conflicts_with = "clear_color")]
        color: Option<String>,
        #[arg(long)]
        clear_color: bool,
        #[arg(long)]
        collapsed: Option<bool>,
        #[arg(long)]
        position: Option<usize>,
    },
    /// Assign workspaces to a group; omit --group to make them ungrouped
    AssignWorkspaceGroup {
        #[arg(long)]
        group: Option<String>,
        #[arg(long, value_delimiter = ',', required = true)]
        workspaces: Vec<String>,
    },
    /// Delete a group while retaining its workspaces
    DeleteWorkspaceGroup { id: String },
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
    /// Move a live surface tab into another pane in the same workspace
    MoveSurface {
        /// Surface UUID
        id: String,
        /// Destination pane reference (pane:N)
        #[arg(long)]
        pane: Option<String>,
        /// Destination workspace UUID; defaults to the pane owner or source workspace
        #[arg(long)]
        workspace: Option<String>,
        /// Zero-based insertion position; defaults to the end
        #[arg(long)]
        position: Option<usize>,
        /// Preserve current focus instead of selecting the moved surface
        #[arg(long)]
        no_focus: bool,
    },
    /// Reorder a surface tab inside its current pane
    ReorderSurface { id: String, position: usize },
    /// Move a surface into a newly split pane next to a target pane
    DragSurfaceToSplit {
        id: String,
        #[arg(long)]
        pane: String,
        #[arg(long, value_parser = ["left", "right", "up", "down"])]
        direction: String,
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
    /// Capture recent terminal history as bounded VT text (up to 2,000 rows and 256 KiB)
    ReadScrollback {
        /// Target surface ID (default: focused)
        #[arg(long)]
        id: Option<String>,
    },
    /// Check native terminal availability and pane attention
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
        /// Pane reference (pane:N) or a surface UUID
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

    /// Set a keyed status in a workspace sidebar
    SetStatus {
        key: String,
        value: String,
        #[arg(long)]
        icon: Option<String>,
        #[arg(long)]
        color: Option<String>,
        #[arg(long, default_value_t = 0)]
        priority: i32,
        #[arg(long, default_value = "plain", value_parser = ["plain", "markdown"])]
        format: String,
        #[arg(long, alias = "link")]
        url: Option<String>,
        #[arg(long, alias = "tab", env = "CMUX_WORKSPACE_ID")]
        workspace: Option<String>,
    },
    /// Publish a keyed multiline Markdown summary
    ReportMetaBlock {
        key: String,
        markdown: String,
        #[arg(long, default_value_t = 0)]
        priority: i32,
        #[arg(long, alias = "tab", env = "CMUX_WORKSPACE_ID")]
        workspace: Option<String>,
    },
    /// Remove a keyed Markdown summary
    ClearMetaBlock {
        key: String,
        #[arg(long, alias = "tab", env = "CMUX_WORKSPACE_ID")]
        workspace: Option<String>,
    },
    /// List retained Markdown summaries
    ListMetaBlocks {
        #[arg(long, alias = "tab", env = "CMUX_WORKSPACE_ID")]
        workspace: Option<String>,
    },
    /// Clear one sidebar status key
    ClearStatus {
        key: String,
        #[arg(long, alias = "tab", env = "CMUX_WORKSPACE_ID")]
        workspace: Option<String>,
    },
    /// List attributed listening ports without changing workspace selection
    Ports {
        #[arg(long, alias = "tab", env = "CMUX_WORKSPACE_ID")]
        workspace: Option<String>,
        #[arg(long)]
        surface: Option<String>,
    },
    /// List workspace status entries and progress
    ListStatus {
        #[arg(long, alias = "tab", env = "CMUX_WORKSPACE_ID")]
        workspace: Option<String>,
    },
    /// Set determinate workspace progress from zero to one
    SetProgress {
        value: f64,
        #[arg(long, default_value = "")]
        label: String,
        #[arg(long, alias = "tab", env = "CMUX_WORKSPACE_ID")]
        workspace: Option<String>,
    },
    /// Clear workspace progress
    ClearProgress {
        #[arg(long, alias = "tab", env = "CMUX_WORKSPACE_ID")]
        workspace: Option<String>,
    },

    // -- Notification commands --
    /// Deliver a notification to a terminal without changing focus
    Notify {
        #[arg(long, default_value = "Notification")]
        title: String,
        #[arg(long, default_value = "")]
        subtitle: String,
        #[arg(long, default_value = "")]
        body: String,
        #[arg(long)]
        workspace: Option<String>,
        #[arg(long)]
        surface: Option<String>,
    },
    /// Inspect, read, dismiss and navigate notification history
    Notifications {
        #[command(subcommand)]
        command: NotificationCommands,
    },
    /// List notifications
    ListNotifications,
    /// Clear a notification
    ClearNotification {
        /// Workspace UUID (legacy alias; notifications clear supports explicit scopes)
        id: String,
    },

    // -- Browser subcommand group (agent primary interface) --
    /// Browser automation (agent primary interface)
    #[command(subcommand)]
    Browser(BrowserCommand),
}

/// Inbox operations share the socket's exact notification and target identities.
#[derive(Subcommand)]
pub enum NotificationCommands {
    /// List retained messages and read state
    List,
    /// Remove all messages, or messages in an explicit workspace/surface scope
    Clear {
        /// Clear messages attributed to this calling terminal using native identity
        #[arg(long, conflicts_with_all = ["workspace", "surface"])]
        caller: bool,
        #[arg(long)]
        workspace: Option<String>,
        #[arg(long)]
        surface: Option<String>,
    },
    /// Mark a message, a workspace/surface scope, or all messages read without focus changes
    MarkRead {
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        workspace: Option<String>,
        #[arg(long)]
        surface: Option<String>,
        #[arg(long)]
        all: bool,
    },
    /// Remove one message or all previously read messages
    Dismiss {
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        all_read: bool,
    },
    /// Focus the exact terminal referenced by a message
    Open { id: String },
    /// Focus the most recent unread message's terminal
    JumpToUnread,
}

/// Surface operations grouped to match upstream command spelling.
#[derive(Subcommand)]
pub enum SurfaceCommands {
    /// Register or inspect a saved resume command (does not execute it)
    Resume {
        #[command(subcommand)]
        command: ResumeCommands,
    },
}

/// Hook installation and provider events; other providers are added through the same ingestion boundary.
#[derive(Subcommand)]
pub enum HookCommands {
    /// Install supported hooks while preserving unrelated agent configuration
    Setup {
        /// Agent provider (currently claude); omitted discovers available supported providers
        agent: Option<String>,
    },
    /// Receive a Claude Code hook payload on stdin
    Claude {
        #[command(subcommand)]
        event: ClaudeHookEvent,
    },
    /// Receive a Codex lifecycle hook payload on stdin
    Codex {
        #[command(subcommand)]
        event: CodexHookEvent,
    },
    /// Receive a Grok lifecycle hook payload on stdin
    Grok {
        #[command(subcommand)]
        event: JsonHookEvent,
    },
    /// Receive a Gemini lifecycle hook payload on stdin
    Gemini {
        #[command(subcommand)]
        event: JsonHookEvent,
    },
    /// Receive a Kiro CLI agent hook payload on stdin
    Kiro {
        #[command(subcommand)]
        event: JsonHookEvent,
    },
    /// Receive an Antigravity hook payload on stdin
    Antigravity {
        #[command(subcommand)]
        event: JsonHookEvent,
    },
    /// Receive a Hermes Agent YAML hook payload on stdin
    HermesAgent {
        #[command(subcommand)]
        event: JsonHookEvent,
    },
    /// Receive a Kimi Code TOML hook payload on stdin
    Kimi {
        #[command(subcommand)]
        event: JsonHookEvent,
    },
    /// Receive a GitHub Copilot lifecycle hook payload on stdin
    Copilot {
        #[command(subcommand)]
        event: JsonHookEvent,
    },
    /// Receive a CodeBuddy lifecycle hook payload on stdin
    Codebuddy {
        #[command(subcommand)]
        event: JsonHookEvent,
    },
    /// Receive a Factory Droid lifecycle hook payload on stdin
    Factory {
        #[command(subcommand)]
        event: JsonHookEvent,
    },
    /// Receive a Qoder lifecycle hook payload on stdin
    Qoder {
        #[command(subcommand)]
        event: JsonHookEvent,
    },
    /// Receive an OpenCode plugin lifecycle payload on stdin
    Opencode {
        #[command(subcommand)]
        event: JsonHookEvent,
    },
    /// Receive a Cursor Agent lifecycle hook payload on stdin
    Cursor {
        #[command(subcommand)]
        event: JsonHookEvent,
    },
    /// Receive a Pi coding agent extension lifecycle payload on stdin
    Pi {
        #[command(subcommand)]
        event: JsonHookEvent,
    },
    /// Receive an OMP extension lifecycle payload on stdin
    Omp {
        #[command(subcommand)]
        event: JsonHookEvent,
    },
    /// Receive a Campfire extension lifecycle payload on stdin
    Campfire {
        #[command(subcommand)]
        event: JsonHookEvent,
    },
    /// Receive an Amp plugin lifecycle payload on stdin
    Amp {
        #[command(subcommand)]
        event: JsonHookEvent,
    },
    /// Receive a Rovo Dev YAML hook payload on stdin
    Rovodev {
        #[command(subcommand)]
        event: RovoHookEvent,
    },
}

/// Claude session lifecycle and per-turn attention events.
#[derive(Clone, Copy, Subcommand)]
pub enum ClaudeHookEvent {
    SessionStart,
    PromptSubmit,
    SessionEnd,
    Stop,
    Notification,
}

#[derive(Clone, Copy, Subcommand)]
pub enum CodexHookEvent {
    SessionStart,
    PromptSubmit,
    SessionEnd,
    Stop,
}

#[derive(Clone, Copy, Subcommand)]
pub enum JsonHookEvent {
    SessionStart,
    PromptSubmit,
    SessionEnd,
    Stop,
    Notification,
}

#[derive(Clone, Copy, Subcommand)]
pub enum RovoHookEvent {
    PromptSubmit,
    Stop,
}

/// Manual resume binding controls; automatic execution is a separate hook policy.
#[derive(Subcommand)]
pub enum ResumeCommands {
    Set {
        #[arg(long, env = "CMUX_SURFACE_ID")]
        surface: Option<String>,
        #[arg(long)]
        shell: String,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        checkpoint: Option<String>,
        #[arg(long)]
        cwd: Option<String>,
        #[arg(long)]
        name: Option<String>,
    },
    Show {
        #[arg(long, env = "CMUX_SURFACE_ID")]
        surface: Option<String>,
    },
    Clear {
        #[arg(long, env = "CMUX_SURFACE_ID")]
        surface: Option<String>,
        #[arg(long)]
        checkpoint: Option<String>,
    },
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
        /// Chrome profile name or persistent profile directory used by agent-browser
        #[arg(long)]
        profile: Option<String>,
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

#[cfg(test)]
mod color_argument_tests {
    use super::*;

    /// Output-color fallback must not propagate as an explicit status text color.
    #[test]
    fn unstyled_status_has_no_inherited_color() {
        let cli = Cli::try_parse_from(["cmux", "set-status", "agent", "working"]).unwrap();
        let Commands::SetStatus { color, .. } = cli.command else {
            panic!("wrong command");
        };
        assert!(color.is_none());
        assert!(cli.color.is_none());
        let cli = Cli::try_parse_from([
            "cmux",
            "set-status",
            "agent",
            "working",
            "--color",
            "#123456",
        ])
        .unwrap();
        let Commands::SetStatus { color, .. } = cli.command else {
            panic!("wrong command");
        };
        assert_eq!(color.as_deref(), Some("#123456"));
        let cli = Cli::try_parse_from(["cmux", "list-workspaces", "--color", "never"]).unwrap();
        assert_eq!(cli.color.as_deref(), Some("never"));
    }
}
