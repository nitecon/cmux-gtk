use std::path::PathBuf;
use uuid::Uuid;

/// Validate and normalize the user-facing inputs from the workspace wizard.
pub fn prepare_local_workspace(
    name: &str,
    working_directory: &std::path::Path,
) -> Result<(String, PathBuf), String> {
    let working_directory = working_directory
        .canonicalize()
        .map_err(|error| format!("Cannot open that folder: {error}"))?;
    if !working_directory.is_dir() {
        return Err("Choose a folder, not a file.".to_string());
    }
    let name = if name.trim().is_empty() {
        working_directory
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .unwrap_or("Workspace")
            .to_string()
    } else {
        name.trim().to_string()
    };
    Ok((name, working_directory))
}

/// Connection state for SSH remote workspaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    /// Not a remote workspace (local).
    Local,
    /// SSH connection established and healthy.
    Connected,
    /// SSH connection lost.
    Disconnected,
    /// Attempting to reconnect (with attempt count).
    Reconnecting(u32),
}

impl ConnectionState {
    /// Distinguish SSH workspaces even while disconnected or reconnecting.
    pub fn is_remote(&self) -> bool {
        !matches!(self, ConnectionState::Local)
    }

    /// Return sidebar status text; local workspaces have no connection indicator.
    pub fn display_text(&self) -> &str {
        match self {
            ConnectionState::Local => "",
            ConnectionState::Connected => "Connected",
            ConnectionState::Disconnected => "Disconnected",
            ConnectionState::Reconnecting(_) => "Reconnecting...",
        }
    }

    /// Select the connection indicator style without encoding retry counts.
    pub fn css_class(&self) -> &str {
        match self {
            ConnectionState::Local => "",
            ConnectionState::Connected => "connected",
            ConnectionState::Disconnected => "disconnected",
            ConnectionState::Reconnecting(_) => "reconnecting",
        }
    }
}

/// Workspace: one tab in the cmux sidebar.
/// Each workspace has an independent pane split tree (managed by SplitEngine in split_engine.rs).
/// The root GTK widget of a workspace's split tree is added as a named page in the GtkStack.
#[derive(Debug)]
pub struct Workspace {
    /// Bounded agent status and progress displayed in the sidebar.
    pub metadata: crate::workspace_metadata::Metadata,
    /// Unique workspace ID — used as the GtkStack page name.
    pub id: u64,
    /// Display name shown in the sidebar GtkListBox row.
    pub name: String,
    /// The name key used with GtkStack::add_named / set_visible_child_name.
    pub stack_page_name: String,
    /// Stable UUID for session persistence and v2 socket protocol identity.
    pub uuid: Uuid,
    /// Phase 4 NOTF-01: true when any pane in this workspace has unread bell activity.
    pub has_attention: bool,
    /// Phase 4: rate-limit desktop notifications to 1 per workspace per 5 seconds.
    pub last_notification: Option<std::time::Instant>,
    /// SSH remote target (e.g., "user@host"). None for local workspaces.
    pub remote_target: Option<String>,
    /// Directory local terminal panes start in. None uses Ghostty's default.
    pub working_directory: Option<PathBuf>,
    /// Connection state for remote workspaces.
    pub connection_state: ConnectionState,
    pub color: Option<String>,
    pub startup_script: Option<PathBuf>,
    pub remote_directory: Option<String>,
}

impl Workspace {
    /// Create a new workspace with a default "Workspace N" name.
    pub fn new(id: u64, display_number: usize) -> Self {
        let name = format!("Workspace {}", display_number);
        let stack_page_name = format!("workspace-{}", id);
        Self {
            id,
            name,
            stack_page_name,
            uuid: Uuid::new_v4(),
            has_attention: false,
            last_notification: None,
            metadata: Default::default(),
            color: None,
            startup_script: None,
            remote_directory: None,
            remote_target: None,
            working_directory: None,
            connection_state: ConnectionState::Local,
        }
    }

    /// Create a local workspace bound to a directory.
    pub fn new_bound(
        id: u64,
        display_number: usize,
        name: String,
        working_directory: PathBuf,
    ) -> Self {
        let mut workspace = Self::new(id, display_number);
        workspace.name = name;
        workspace.working_directory = Some(working_directory);
        workspace
    }

    /// Describe the complete launch location for tooltips and workspace metadata.
    pub fn location(&self) -> String {
        if let Some(target) = &self.remote_target {
            format!(
                "ssh://{}{}",
                target,
                self.remote_directory.as_deref().unwrap_or("")
            )
        } else if let Some(script) = &self.startup_script {
            format!("script: {}", script.display())
        } else {
            self.working_directory
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        }
    }

    /// Compact local paths and script names while preserving the SSH destination.
    pub fn subtitle(&self) -> String {
        if self.remote_target.is_some() {
            return self.location();
        }
        if let Some(script) = &self.startup_script {
            return format!(
                "script: {}",
                script.file_name().unwrap_or_default().to_string_lossy()
            );
        }
        compact_local_path(self.working_directory.as_deref())
    }

    /// Rename this workspace to a new display name.
    pub fn rename(&mut self, new_name: String) {
        self.name = new_name;
    }

    /// Create a new remote SSH workspace targeting the given host.
    pub fn new_remote(id: u64, display_number: usize, target: String) -> Self {
        let mut workspace = Self::new(id, display_number);
        workspace.name = crate::ssh_hosts::workspace_name_from_target(&target);
        workspace.remote_target = Some(target);
        workspace.connection_state = ConnectionState::Reconnecting(0);
        workspace
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Require a persisted workspace identity even before it is inserted into GTK state.
    #[test]
    fn workspace_new_has_uuid() {
        let w = Workspace::new(1, 1);
        // uuid must not be nil (all-zeros)
        assert_ne!(
            w.uuid,
            Uuid::nil(),
            "Workspace::new() must generate a non-nil UUID"
        );
    }

    /// Ensure independent workspaces cannot alias each other in session or socket lookup.
    #[test]
    fn workspace_uuids_are_unique() {
        let w1 = Workspace::new(1, 1);
        let w2 = Workspace::new(2, 2);
        assert_ne!(w1.uuid, w2.uuid, "Two workspaces must have distinct UUIDs");
    }

    /// Verify directory normalization, explicit name trimming and basename defaults.
    #[test]
    fn local_workspace_inputs_bind_an_existing_directory() {
        let directory =
            std::env::temp_dir().join(format!("cmux-workspace-binding-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let (name, bound_directory) =
            prepare_local_workspace("  Project Alpha  ", &directory).unwrap();
        assert_eq!(name, "Project Alpha");
        assert_eq!(bound_directory, directory.canonicalize().unwrap());
        let (default_name, _) = prepare_local_workspace("", &directory).unwrap();
        assert_eq!(
            default_name,
            directory.file_name().unwrap().to_string_lossy()
        );
        let _ = std::fs::remove_dir_all(directory);
    }

    /// Reject a regular file before it can become a terminal working directory.
    #[test]
    fn local_workspace_inputs_reject_a_file() {
        let file = std::env::temp_dir().join(format!(
            "cmux-workspace-binding-file-{}",
            std::process::id()
        ));
        std::fs::write(&file, b"not a directory").unwrap();
        assert_eq!(
            prepare_local_workspace("Project", &file).unwrap_err(),
            "Choose a folder, not a file."
        );
        let _ = std::fs::remove_file(file);
    }
}

/// Retain the root directory and basename, with the full path in the tooltip.
pub fn compact_local_path(path: Option<&std::path::Path>) -> String {
    let Some(path) = path else {
        return String::new();
    };
    let parts: Vec<_> = path
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(v) => Some(v.to_string_lossy()),
            _ => None,
        })
        .collect();
    match parts.as_slice() {
        [] => "/".into(),
        [one] => format!("/{one}"),
        [first, last] => format!("/{first}/{last}"),
        _ => format!("/{}/…/{}", parts[0], parts.last().unwrap()),
    }
}

/// Accept only six-digit RGB hex colors before interpolating into GTK CSS.
pub fn valid_workspace_color(color: &str) -> bool {
    color.len() == 7 && color.starts_with('#') && color[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// Quote one argument for the POSIX shell used by Ghostty's command option.
pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// Resolve a readable regular script file, returning a user-facing validation error.
pub fn prepare_startup_script(path: &std::path::Path) -> Result<PathBuf, String> {
    let path = path
        .canonicalize()
        .map_err(|e| format!("Cannot open startup script: {e}"))?;
    if !path.is_file() {
        return Err("Choose a startup script file.".into());
    }
    std::fs::File::open(&path).map_err(|e| format!("Cannot read startup script: {e}"))?;
    Ok(path)
}

/// Source the quoted script in sh, then inherit its directory and exports in a login shell.
pub fn startup_command(path: &std::path::Path) -> String {
    // Source in sh so exported variables and cd remain available to the login shell.
    let body = format!(
        ". {} && exec \"${{SHELL:-/bin/sh}}\" -l",
        shell_quote(&path.to_string_lossy())
    );
    format!("/bin/sh -c {}", shell_quote(&body))
}

/// Reject empty destinations, option injection, whitespace and control characters.
pub fn validate_ssh_target(target: &str) -> Result<(), String> {
    if target.is_empty()
        || target.starts_with('-')
        || target.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err("Enter an SSH alias or user@host without command-line options.".into());
    }
    Ok(())
}

#[cfg(test)]
mod workflow_tests {
    use super::*;

    /// Check local, script and SSH labels against their complete launch locations.
    #[test]
    fn workspace_locations_describe_launch_context() {
        let mut ws = Workspace::new_bound(1, 1, "Project".into(), "/opt/team/repo".into());
        assert_eq!(ws.subtitle(), "/opt/…/repo");
        assert_eq!(ws.location(), "/opt/team/repo");
        ws.startup_script = Some("/opt/start project.sh".into());
        assert_eq!(ws.subtitle(), "script: start project.sh");
        ws.remote_target = Some("alice@host".into());
        ws.remote_directory = Some("/srv/project".into());
        assert_eq!(ws.subtitle(), "ssh://alice@host/srv/project");
        assert_eq!(
            compact_local_path(Some(PathBuf::from("/opt/repo").as_path())),
            "/opt/repo"
        );
        assert_eq!(compact_local_path(Some(PathBuf::from("/").as_path())), "/");
    }

    /// Execute a script with shell-sensitive filename characters in its bound directory.
    #[test]
    fn startup_command_preserves_quoted_paths_and_environment() {
        let directory = std::env::temp_dir().join(format!("cmux-script-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let script = directory.join("startup 'quoted' $name.sh");
        let result = directory.join("result");
        std::fs::write(
            &script,
            format!(
                "printf '%s' \"$PWD\" > {}\nexit 0\n",
                shell_quote(&result.to_string_lossy())
            ),
        )
        .unwrap();
        let script = prepare_startup_script(&script).unwrap();
        let status = std::process::Command::new("/bin/sh")
            .args(["-c", &startup_command(&script)])
            .current_dir(&directory)
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(
            std::fs::read_to_string(result).unwrap(),
            directory.to_string_lossy()
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    /// Reject option/CSS injection while accepting supported host and RGB values.
    #[test]
    fn invalid_ssh_options_and_css_are_rejected() {
        for target in ["", "-oProxyCommand=bad", "host command", "host\ncommand"] {
            assert!(validate_ssh_target(target).is_err());
        }
        assert!(validate_ssh_target("alice@my-server").is_ok());
        assert!(valid_workspace_color("#abCD09"));
        assert!(!valid_workspace_color("red; color: black"));
    }
}
