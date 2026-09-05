# Contributing

Start with [Architecture](Docs/Architecture.md), which links the component map, language standards, build instructions and verification policy.

This repository builds the native Rust/GTK desktop and its CLI, with Ghostty as a complete submodule and Go for remote PTYs. Install dependencies with `./scripts/setup-linux-dev.sh`, then build with `cargo build --workspace --bins`.

Run tests through GitHub Actions, not locally. Add executable behavior coverage for changes, especially resource ownership, focus, persistence and protocol compatibility. Keep designs simple and share repeated behavior through documented functions. The [refactor audit](Docs/RefactorAudit.md) tracks the ongoing architecture migration; [observability](Docs/Observability.md) defines benchmark and diagnostic requirements.
