# Refactor audit

Status: in progress. This document is a completion checklist, not a claim that the migration is finished.

## Inventory evidence

The initial tracked-source inventory contained 41 Rust files, four Go files and three C files outside Ghostty; no owned Swift, Objective-C or Zig source. The application binaries are Rust. `build.rs` builds the X11 C bridge and links the Ghostty archive. The only website CI job typechecks the inherited standalone `web` tree.

Legacy candidates include the standalone upstream marketing website, macOS instructions in `CONTRIBUTING.md`, tests that require missing Xcode/Swift projects, unused native stubs, historical planning notes and duplicated desktop assets. Audit references before removal. Some Python protocol tests are portable and must be retained or adapted rather than deleted by directory name. `scripts/cmux-cli` and the prompt probe currently import `tests_v2/cmux.py`.

Removed after reference inspection: the standalone `web` tree and its typecheck job, unused `glslang_stub.c` and `stubs.c`, and duplicate `resources/cmux.desktop`. The canonical packaged launcher remains `packaging/desktop/io.cmux.App.desktop`; the source SVG remains because icon generation consumes it. Replaced the macOS contribution guide with native build/documentation entry points.

The initial `cmux-platform` extraction centralizes XDG paths and peer authentication for desktop/CLI callers, plus the optional GTK/X11 native bridge. The library builds without GTK enabled. Remaining platform callers, native build concerns and the process-CWD heuristic still need review. The full workspace binary build passed after this extraction; no local tests were run.

Next stage adds Linux process resources, bounded structured logging, diagnostic snapshots, CLI/GTK correlation and executable CI coverage. First checkpoint CI exposed orphaned `cmux_linux` imports in Rust integration tests. Key-mapping assertions now live inside the current application test module; atomic-only wakeup tests were removed because they exercised standard-library atomics rather than the callback. Mainline CI must verify the correction and new tests before this stage is considered validated.

Keep protocol compatibility, licensing, operational documentation and the complete Ghostty submodule. macOS mentions describing upstream provenance or remote targets are not themselves obsolete code.

## Required completion evidence

- [x] Inspect stack, build roots, workflows and initial platform coupling.
- [x] Create architecture, component map and language standards.
- [x] Replace owned agent files with concise bullets, at most 50 words; root CLAUDE symlink to AGENTS.
- [ ] Remove verified unused/legacy owned artifacts and update all affected references and CI.
- [ ] Isolate Linux services in a component library and route all relevant callers through it.
- [ ] Refactor repeated behavior into shared functions and simplify component ownership.
- [ ] Document all retained owned functions with useful adjacent native comments.
- [ ] Apply language formatting and appropriate linting to retained owned code.
- [x] Look up applicable gateway patterns and document project adaptations.
- [ ] Implement correlated end-to-end diagnostics, resource metrics and bounded retention.
- [ ] Add repeatable optimized benchmarks, diagnostic collection and CI artifacts.
- [ ] Build all binaries and verify behavior through GitHub Actions.
- [ ] Audit every original requirement against final files and executable evidence.

Known issues to avoid hiding: session save lacks a shutdown flush (deferred feature task); CWD capture currently guesses a child process rather than identifying each surface reliably. Neither should be described as solved by moving files.
