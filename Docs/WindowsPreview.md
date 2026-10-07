# Windows native preview

This experimental x86-64 build uses the same GTK workspace UI and Ghostty terminal engine as Linux, with native ConPTY shells. It requires Windows 10 version 1809 or newer, or Windows 11, and a driver supporting desktop OpenGL 3.3.

Extract the entire preview archive to a writable folder. Run `launch-cmux.cmd` to open CMUX. Keep its executables, DLLs and `share` directory together; MSYS2 is not required on the destination computer. From PowerShell in that folder, use `./cmux.exe ping` or `./cmux.exe --json list-workspaces` to check the running app. Add the folder to PATH to use `cmux` in other terminals.

The default shell is the Windows command prompt. A Ghostty `command` setting can select PowerShell or another native shell. CLI control uses a local named pipe restricted to the current Windows user. Configuration and application data default to the corresponding `config`, `data`, `state` and `cache` directories under `%LOCALAPPDATA%`, each containing a `cmux` directory; absolute XDG overrides remain supported.

Gateway routing uses the shared Linux lifecycle and readiness logic. Windows process verification accepts native `codex.exe` and `claude.exe` in a terminal's process tree; ambiguous or unverified interpreter processes remain ineligible for injection. Configure the same gateway URL and key through CMUX settings on the Windows machine. Live Windows delegation still needs acceptance on the destination machine.

Desktop notification toasts, workspace startup scripts and automatic TCP-port attribution are unavailable in this preview. The in-app inbox and workspace attention indicators remain available. Linux command paths and externally installed browser services need Windows-compatible equivalents. Self-update is disabled until Windows releases are supported; update by replacing the extracted preview folder.

Diagnostics live in `%LOCALAPPDATA%\state\cmux` by default. Include the preview's `build.json` and diagnostic log when reporting startup problems. CI smoke evidence, when successful, is included as `smoke-result.json` and `smoke.log`.

Builds are produced on `feature/windows-native` by the Windows preview GitHub Actions workflow using MSYS2 UCRT64, GTK4, native Rust and the Ghostty-pinned Zig version. This preview does not merge into main or publish a stable Windows release.
