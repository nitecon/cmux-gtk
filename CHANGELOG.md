# Changelog

All notable changes to cmux GTK are documented here.

## [Unreleased]

### Changed

- Add structured diagnostic snapshots, process resource sampling, bounded log delivery and CLI/GTK request correlation.
- Add an optimized CLI-to-GTK benchmark with revision-tagged CI artifacts; isolate installation ownership and notifications in the Linux library.
- Share atomic persistence across session, preferences, geometry and SSH host settings; remove duplicate header-button and obsolete input-test code.
- Document the architecture, component boundaries, language standards and observability requirements for the ongoing refactor.
- Centralize Linux paths, peer authentication and the optional GTK/X11 placement bridge in a platform library.
- Remove the inherited marketing website, unused native linker stubs and duplicate desktop launcher; replace obsolete contribution instructions.

## [0.6.4] - 2026-10-07

### Added

- Expose verified process-session identity, provider and OS through local `gateway.session` and `gateway.sessions` RPCs for agent-tools provenance and harness coordination.

### Changed

- Broadcast task comments and completion to matching project peers, suppressing only the exact originating session and instance.
- Keep separate durable recipient queues and submission fences, preserving busy/draft gating, process replacement and reconnect safety.
- Confirm complete recipient ACK snapshots so delayed queued confirmations cannot hide newer peer-delivery progress.

## [0.6.3] - 2026-10-07

### Fixed

- Preserve agent terminal cursor and screen state during gateway injection by letting the foreground agent render submitted messages instead of writing a second annotation into its output stream.

## [0.6.2] - 2026-10-06

### Fixed

- Recognize Codex's shaded input area separately from its status footer so gateway messages drain at empty prompts while drafts, attachments and permission prompts remain blocked.
- Exclude permission and approval words in previous Codex replies from the shaded prompt's readiness check.

## [0.6.1] - 2026-10-06

### Fixed

- Recognize Claude's native versioned executable path and still-running binaries unlinked by updates, so native-installed Claude sessions can receive gateway messages.

## [0.6.0] - 2026-10-06

### Added

- Subscribe once to agent-gateway v1.19's authenticated task lifecycle stream across all projects, automatically matching Git upstream repositories.
- Deliver ordinary/delegated tasks, user/agent/system comments and completion notifications directly to active Claude/Codex terminals behind one global experimental approval.
- Queue busy or unfinished prompts, show marked colored injections, and persist reconnect cursors and delivery fences without gateway hooks or per-project execution settings.
- Show connection, queue and delivery reasons in preferences; fetch truncated task context and fence uncertain submissions against automatic replay.

## [0.5.0] - 2026-10-06

### Added

- Opt-in authenticated gateway WebSocket connection with a dedicated Agent Gateway tab in Preferences for the address, API key and per-workspace project mapping, plus CLI configuration; requires agent-gateway v1.18.0 or later.
- Existing Claude/Codex native-session registration, delegated-task notifications and confirmed submission to the exact interactive terminal after durable gateway acknowledgment.
- Bounded progress, question and outcome reporting, with persisted sequence numbers and acceptance/submission markers to prevent prompt replay after disconnect or restart.
- Isolated real-WebSocket/native-terminal CI coverage for delivery ordering, busy-session rejection, exact routing, reporting and reconnect reconciliation.

## [0.4.0] - 2026-10-02

### Added

- Add a native Git side panel alongside Files, with a version tree of recent commits and selectable per-file diffs.
- Open Git on Current changes by default, showing staged, unstaged and untracked changes with a one-click return and refresh; restore the Git tab with the session.
- Add a live preference to invert mouse-wheel and touchpad scrolling.

### Fixed

- Preserve control-key input in applications using the Kitty keyboard protocol, including Claude Code, Neovim and fish.
- Keep touchpad scrolling smooth and scale precision deltas for the display.
- Update the Homebrew cask template to use declarative preflight steps and ordered dependency stanzas.

## [0.3.1] - 2026-10-02

### Fixed

- Open Markdown files in a native Files tab in the right-hand column, reusing existing columns and restoring the tab with the workspace.
- Replace the flat file list with expandable folders and a resizable preview.
- Preserve Markdown line breaks and code indentation, and render tables as aligned, bordered cells with inline formatting.

## [0.3.0] - 2026-10-02

### Added

- Header Markdown file browser listing the active workspace's documents with a rendered reader.

## [0.2.1] - 2026-09-07

### Fixed

- Submit Claude Teams split and respawn commands with a separate Enter key so bracketed paste cannot prevent execution.
- Close the previous workspace after an explicitly confirmed restart.
- Keep diff headers outside source line numbering to prevent duplicate review comments.
- Match diff viewer configuration field names to enable comment creation and font-size settings.
- Correct CI fixtures for graceful group restoration, diff comment interactions and remote browser surface transfer.

## [0.1.8] - 2026-09-05

### Added

- Persistent startup-script and SSH workspace launch details, remote folder selection, and launch-context inheritance for terminal tabs and splits.
- Compact workspace location subtitles, drag/menu reordering, and persistent background colors.
- Executable Linux clipboard, workspace launch, widget lifecycle and memory churn checks in CI; documented the upstream six-month review.

### Fixed

- Route asynchronous clipboard reads to the requesting terminal and handle Ctrl+Shift+C/V directly in terminal widgets.
- Bound browser frame, session snapshot, mouse motion and SSH output delivery; remove widget callback reference cycles and cancel remote tasks when workspaces close.
- Keep remote streams separate across sibling tabs, propagate terminal size, and close remote PTYs with their terminals.
- Upload remote helpers atomically and recover on retry after an interrupted deployment.
- Keep CLI workspace ordering aligned with sidebar order and saved sessions.
- Preserve launch command buffers through terminal lifetime and route input correctly in freshly restored workspaces.
- Avoid GTK's affected dmabuf texture ownership path on GTK 4.16–4.22.4, and redraw only terminals targeted by Ghostty render requests.

## [0.1.7] - 2026-09-05

### Added

- Remembered normal window size and maximized state, plus window position on X11. Wayland placement remains controlled by the compositor.

- Added a Preferences dialog with a persistent terminal font size that applies to existing and new tabs.

- Added persistent lifecycle diagnostics and panic backtraces at `$XDG_STATE_HOME/cmux/cmux.log`.

### Fixed

- Restored terminal keyboard focus when clicking back from the browser URL bar and prevented stale browser callbacks from stealing focus.

- Deferred browser URL restoration when closing a terminal tab synchronously reveals a browser tab, preventing a re-entrant state borrow from aborting cmux.

## [0.1.6] - 2026-09-04

### Added

- Added hover close buttons and right-click **Close Tab** actions to terminal and browser surface tabs.

### Fixed

- Stopped terminal PTYs and render threads before removing their pane widgets, preventing crashes and stale callbacks during close.
- Detached surviving nested panes before reparenting them, preventing GTK parenting assertions when closing a split.
- Made `surface.close` close the addressed surface tab and only remove its pane when it is the final tab.

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
