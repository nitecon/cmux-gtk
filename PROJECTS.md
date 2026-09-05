# Project status

This repository maintains the Linux Rust/GTK port of cmux. The inherited February 2026 macOS completion log described Sparkle, Swift, Bonsplit and WKWebView features; it was not evidence of Linux implementation or validation. That log remains available in Git history.

- [Architecture](Docs/Architecture.md) defines the supported stack and development rules.
- [Components](Docs/Components.md) maps current implementation ownership.
- [Refactor audit](Docs/RefactorAudit.md) records completed work, remaining gaps and verification evidence.
- [Changelog](CHANGELOG.md) records Linux releases.
- [Observability](Docs/Observability.md) describes diagnostics and benchmark coverage.

The architecture refactor remains in progress. Session resume, resume hooks and agent feature parity remain a separate deferred scope; historical upstream completion claims do not activate or complete that work.
