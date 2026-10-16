//! Windows desktop entry point; commands and updater operations remain in the CLI.

use super::CliError;
use std::os::windows::process::CommandExt;

/// Start the GUI from this installation without waiting for its application lifetime.
/// The child owns its normal diagnostics; no terminal handles or process handles are retained.
pub(crate) fn launch_desktop() -> Result<(), CliError> {
    super::updater::windows::ensure_launch_allowed()
        .map_err(|error| CliError::Command(format!("{error:#}")))?;
    let launch = || -> std::io::Result<()> {
        let executable = std::env::current_exe()?;
        let directory = executable
            .parent()
            .ok_or_else(|| std::io::Error::other("cannot locate CMUX installation"))?;
        std::process::Command::new(directory.join("cmux-app.exe"))
            .env("XDG_DATA_DIRS", directory.join("share"))
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW for the GUI child.
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        Ok(())
    };
    launch().map_err(|error| CliError::Command(format!("cannot start CMUX desktop: {error}")))
}
