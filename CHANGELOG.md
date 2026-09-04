# Changelog

All notable changes to cmux GTK are documented here.

## [0.1.5] - 2026-09-04

### Fixed

- Rendered embedded Ghostty terminals on GTK's application thread so terminal tabs display their shell prompt and accept input.
- Preserved terminal rendering when split panes reparent and re-realize their `GtkGLArea` widgets.
- Deferred terminal initialization until GTK provides a non-zero allocation and corrected physical sizing for scaled displays.

## [0.1.4] - 2026-09-04

### Added

- Added terminal and browser surface tabs inside the focused workspace pane, matching upstream cmux's workspace and pane hierarchy.
- Added `Ctrl+T` for a new terminal tab and `Ctrl+Shift+L` for a new browser tab.
- Persisted terminal tabs, browser tabs, the selected surface, and browser URLs across restarts.

### Fixed

- Initialized a live Ghostty terminal surface when creating a terminal tab.
- Made the browser URL bar accept keyboard input by limiting browser-page key forwarding to the rendered page.
- Switched browser launch and navigation to agent-browser's supported public CLI and detected independently installed NVM versions from desktop launches.

## [0.1.3] - 2026-09-04

### Added

- Added a Create Workspace wizard with optional naming, direct path entry, and a native folder browser.
- Bound local workspaces to their chosen directory so initial terminals, new splits, and restored sessions start there.
- Added `cmux new-workspace --cwd PATH [--name NAME]` and exposed workspace directories through the socket API for automation and verification.

### Changed

- Persisted workspace directory bindings while remaining compatible with existing session files.
- Validated socket-requested workspace paths off the GTK main thread and rejected missing paths without mutating the UI.

## [0.1.2] - 2026-09-04

### Fixed

- Prevented Ghostty's bundled HarfBuzz, FreeType, and other static-library symbols from overriding GTK's system libraries and crashing the application during startup.
- Stopped the Homebrew launcher from replacing a working host GTK and graphics stack; Homebrew libraries are now used only when the host cannot resolve the application dependencies.
- Added release checks that reject binaries exposing the bundled HarfBuzz or FreeType ABI.

## [0.1.1] - 2026-09-02

### Fixed

- Made release archives portable across current Linuxbrew systems by statically linking libxml2 and its versioned ICU and liblzma dependency closure.
- Added release-time ELF checks that reject distro-specific XML runtime dependencies.
- Restored the Zig linker wrapper required by Linux Cargo builds.

### Changed

- Removed the inherited Swift/macOS application, Xcode workflows, tracked Node dependencies, and obsolete macOS-only assets.
- Simplified contributor instructions for the Rust and GTK Linux application.

## [0.1.0] - 2026-09-02

### Added

- Native Rust and GTK4 Linux application powered by Ghostty.
- Workspaces, tabs, split panes, notifications, session persistence, and socket CLI control.
- Optional browser automation through an independently installed `agent-browser`.
- SSH remote workspaces through the Go `cmuxd-remote` daemon.
- Direct-install automatic updates with package-manager ownership detection.
- Linux desktop launcher and icons for GNOME, KDE, XFCE, and compatible environments.
- Debian, RPM, release archive, and Linux Homebrew Cask distribution.
