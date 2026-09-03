<h1 align="center">cmux GTK</h1>
<p align="center">A GPU-accelerated terminal multiplexer with tabs, splits, workspaces, browser automation, and socket CLI control — powered by Ghostty</p>

> ⭐ **Want a trusted Homebrew install?** Star this repository to help cmux GTK
> qualify for inclusion in Homebrew's official Cask repository. Once accepted,
> users can install it without explicitly trusting a third-party tap.

<p align="center">
  <img src="./docs/assets/main-first-image.png" alt="cmux screenshot" width="900" />
</p>

## About

cmux for Linux is a full native port of [cmux](https://github.com/manaflow-ai/cmux) (originally a macOS Swift/AppKit app) rebuilt in Rust on GTK4. It provides the same experience — tabs, splits, workspaces, notifications, browser automation, and a scriptable socket API — running natively on Linux with GPU-accelerated terminal rendering via Ghostty.

Built for developers running multiple AI coding agents (Claude Code, Codex, etc.) in parallel who need visibility into which agent needs attention and the ability to script browser interactions alongside terminal sessions.

## Features

- **GPU-accelerated terminal** — Powered by libghostty with GTK4 GtkGLArea rendering
- **Workspaces, tabs, and split panes** — Organize parallel agent sessions
- **Notification system** — Per-pane bell tracking, sidebar indicators, desktop notifications
- **In-app browser** — CDP-based browser automation with accessibility tree snapshots, element interaction, and JS evaluation via [agent-browser](https://github.com/vercel-labs/agent-browser)
- **Scriptable CLI** — `cmux` CLI with 34+ subcommands for workspaces, panes, surfaces, and browser control
- **Socket API** — v2 JSON-RPC over Unix socket with SO_PEERCRED auth
- **SSH remote workspaces** — cmuxd-remote deployment with bidirectional PTY proxy and reconnect
- **Ghostty compatible** — Reads your existing `~/.config/ghostty/config` for themes, fonts, and colors
- **Session persistence** — Atomic save/restore of full split tree topology with divider ratios

## Install

### Homebrew on Linux (recommended)

Homebrew 6 requires explicit trust before loading Casks from third-party taps.
Trust only the cmux GTK Cask, then install it:

```bash
brew tap nitecon/cmux-gtk
brew trust --cask nitecon/cmux-gtk/cmux-gtk
brew install --cask nitecon/cmux-gtk/cmux-gtk
```

The trust decision is stored by Homebrew for future upgrades. It applies only
to `cmux-gtk`, not every current or future Cask in the tap.

```bash
brew upgrade --cask cmux-gtk
```

Launch cmux from your desktop application menu or run `cmux-app`. The installed
commands are `cmux` and `cmux-app`.

### Debian / Ubuntu (.deb)

```bash
sudo dpkg -i cmux-gtk_0.1.0_amd64.deb
sudo apt-get install -f  # install dependencies if needed
```

### Fedora / RHEL (.rpm)

```bash
sudo rpm -i cmux-gtk-0.1.0-1.x86_64.rpm
```

### Build from source

```bash
# Prerequisite: Rust toolchain (the setup script installs system libraries and Zig)
git clone --recurse-submodules https://github.com/nitecon/cmux-gtk.git
cd cmux-gtk
./scripts/setup-linux-dev.sh # install dev dependencies and build libghostty
cargo build --release --bin cmux --bin cmux-app
```

cmux uses an existing `agent-browser` from `CMUX_AGENT_BROWSER`, `PATH`, or its
package installation path. Browser panes remain disabled when it is absent and
cmux prints the upstream installation command. This keeps agent-browser optional
and independently upgradeable.

### Direct binary install

```bash
curl -fsSL https://raw.githubusercontent.com/nitecon/cmux-gtk/main/scripts/install-linux.sh | bash
```

Direct binary installs check for updates quietly at most once per hour. Run
`cmux update` to update immediately, or set `CMUX_NO_UPDATE=1` to disable
automatic checks. Homebrew, Debian, RPM, and AppImage installations remain
owned by their package manager.

## Browser Automation

Agents running inside cmux can discover and use browser automation via the `cmux browser` CLI:

```bash
# Open a site (https:// auto-prepended if no scheme)
cmux browser open slashdot.org            # returns surface:1 handle

# Interact with the page
cmux browser surface:1 snapshot --interactive  # accessibility tree with element refs
cmux browser surface:1 click e3               # click element by ref
cmux browser surface:1 fill e5 "search term"  # fill input field
cmux browser surface:1 eval 'document.title'  # evaluate JavaScript

# Navigation
cmux browser surface:1 goto example.com
cmux browser surface:1 back
cmux browser surface:1 forward
cmux browser surface:1 reload

# Management
cmux browser list                          # list browser surfaces
cmux browser close --surface surface:1     # close a surface
```

Browser commands default to JSON output (agents are the primary consumers). Use `--no-json` for human-readable output.

## CLI Reference

```bash
cmux --help                    # all commands
cmux browser --help            # browser subcommands

# Terminal management
cmux list-workspaces           # list all workspaces
cmux new-workspace             # create workspace
cmux list-surfaces             # list terminal surfaces
cmux split --direction horizontal  # split current pane
cmux list-panes                # list all panes

# System
cmux identify                  # instance info (version, platform, pid)
cmux ping                      # check connectivity
cmux raw <method> --params '{}' # send arbitrary JSON-RPC
```

### Socket Path

The cmux socket is at `$XDG_RUNTIME_DIR/cmux/cmux.sock` (typically `/run/user/$UID/cmux/cmux.sock`).

Override with `CMUX_SOCKET` environment variable or `--socket` flag.

## Agent Skills

When installed via .deb or .rpm, agent skills are available at `/usr/share/cmux/skills/`:

- **cmux** — Core terminal multiplexer skill (workspaces, panes, surfaces, socket CLI)
- **cmux-browser** — Browser automation skill (open sites, interact with pages, extract data)

A `CLAUDE.md` at `/usr/share/cmux/CLAUDE.md` references skill paths so Claude Code discovers them automatically.

## Architecture

- **Language:** Rust
- **UI toolkit:** GTK4 via gtk4-rs
- **Terminal engine:** Ghostty (manaflow-ai fork) via libghostty C FFI
- **Async runtime:** tokio + glib spawn_local bridge
- **Browser automation:** agent-browser daemon with CDP protocol
- **Remote sessions:** Go daemon (cmuxd-remote) reused from macOS codebase
- **Socket protocol:** v2 JSON-RPC, wire-compatible with macOS cmux

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| Ctrl+Shift+T | New workspace |
| Ctrl+1–8 | Jump to workspace 1–8 |
| Ctrl+Shift+D | Split right |
| Ctrl+D | Split down |
| Ctrl+Shift+W | Close workspace |
| Ctrl+W | Close pane |
| Ctrl+Shift+] / [ | Next / previous workspace |
| Ctrl+Tab / Ctrl+Shift+Tab | Next / previous pane |
| Ctrl+Shift+F | Find |
| Ctrl+Shift+K | Clear scrollback |

Shortcuts are configurable via TOML config file.

## Building Packages

```bash
# Build all release binaries
cargo build --release --bin cmux --bin cmux-app

# Optional: resolve an installed or locally linked agent-browser
./scripts/resolve-agent-browser.sh

# Build .deb
./packaging/scripts/build-deb.sh

# Build .rpm
./packaging/scripts/build-rpm.sh

# Validate packages
./packaging/scripts/validate-deb.sh
./packaging/scripts/validate-rpm.sh
```

## Continuous Integration and Releases

Pull requests and commits to `main` run the development CI suite: workspace
checks, Clippy correctness vetting, debug builds, unit tests, remote-daemon
tests, and the existing macOS/web checks.

Pushing a semantic version tag such as `v0.2.0` runs the Linux release workflow
without rerunning the development suite. It builds against the Debian 12 glibc
baseline, validates the binaries and packages, publishes the tarball, checksum,
DEB, and RPM to the GitHub release, then updates
[`nitecon/homebrew-cmux-gtk`](https://github.com/nitecon/homebrew-cmux-gtk). The workflow
can also be dispatched manually without publishing for an artifact-only dry run.

## License

This project is licensed under the GNU Affero General Public License v3.0 or later (`AGPL-3.0-or-later`).

See `LICENSE` for the full text.

## Upstream

Linux port of [cmux](https://github.com/manaflow-ai/cmux) by [manaflow-ai](https://github.com/manaflow-ai).
