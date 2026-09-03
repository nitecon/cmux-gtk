# Changelog

All notable changes to cmux GTK are documented here.

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
