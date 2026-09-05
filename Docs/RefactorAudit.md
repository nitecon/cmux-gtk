# Refactor audit

Status: in progress. This document is a completion checklist, not a claim that the migration is finished.

## Inventory evidence

The initial tracked-source inventory contained 41 Rust files, four Go files and three C files outside Ghostty; no owned Swift, Objective-C or Zig source. The application binaries are Rust. `build.rs` builds the X11 C bridge and links the Ghostty archive. The only website CI job typechecks the inherited standalone `web` tree.

Legacy candidates include the standalone upstream marketing website, macOS instructions in `CONTRIBUTING.md`, tests that require missing Xcode/Swift projects, unused native stubs, historical planning notes and duplicated desktop assets. Audit references before removal. Some Python protocol tests are portable and must be retained or adapted rather than deleted by directory name. `scripts/cmux-cli` and the prompt probe currently import `tests_v2/cmux.py`.

Removed after reference inspection: the standalone `web` tree and its typecheck job, unused `glslang_stub.c` and `stubs.c`, and duplicate `resources/cmux.desktop`. The canonical packaged launcher remains `packaging/desktop/io.cmux.App.desktop`; the source SVG remains because icon generation consumes it. Replaced the macOS contribution guide with native build/documentation entry points.

The initial `cmux-platform` extraction centralizes XDG paths and peer authentication for desktop/CLI callers, plus the optional GTK/X11 native bridge. The library builds without GTK enabled. Remaining platform callers, native build concerns and the process-CWD heuristic still need review. The full workspace binary build passed after this extraction; no local tests were run.

Next stage adds Linux process resources, bounded structured logging, diagnostic snapshots, CLI/GTK correlation and executable CI coverage. First checkpoint CI exposed orphaned `cmux_linux` imports in Rust integration tests. Key-mapping assertions now live inside the current application test module; atomic-only wakeup tests were removed because they exercised standard-library atomics rather than the callback. Mainline CI must verify the correction and new tests before this stage is considered validated.

Removed ten additional source/metadata checks for absent SwiftUI sources, Xcode schemes, paid macOS CI runners, GhosttyKit downloads, DMG creation and SwiftPM retry workflows. Their referenced runtime/build targets do not exist in this port. Portable executable protocol scenarios remain under review.

Linux installation ownership and desktop bell delivery now live in the platform library. SSH bridge construction owns its outbound channel, eliminating three duplicate setup sites plus an unused output-event channel and unused sender accessor. Updater functions now document I/O, validation, replacement ordering and cleanup contracts. Diagnostic retention now bounds oversized inherited logs as well as new output.

Session, preferences, window geometry and SSH host persistence now use one Linux atomic-write helper with unique private staging files and cleanup on error. Tests exercise concurrent writers and failed replacement. This does not implement the deferred shutdown flush. Preferences/window-state functions now have adjacent contract documentation, and header button construction shares one action-binding helper.

CI run 33979749528 identified a second obsolete assumption in the migrated input tests: the port passes raw keycodes to Ghostty and no longer implements `map_keycode_to_ghostty`. Removed the obsolete test module and expanded the existing production modifier tests rather than adding a compatibility function solely for old tests. CI still needs to verify that correction and subsequent changes.

Keep protocol compatibility, licensing, operational documentation and the complete Ghostty submodule. macOS mentions describing upstream provenance or remote targets are not themselves obsolete code.

The CLI completion/man generator now imports the shared argument schema alone, removing its dependency on socket and updater implementation. Removed unused split-tree no-op surface setters, duplicate pane-ID collection, an unused restore wrapper, an uncalled browser overlay helper and unused SSH receiver alias. Remote workspace construction now shares local identity/default initialization. Removed header button-list fields that were parsed but never rendered; legacy keys remain accepted by TOML parsing. Updated retained modules with native function contracts and Rust formatting.

Added a bounded diagnostic time-series collector for issue reports, with private output permissions, correlation UUIDs, resource snapshots and explicit partial-failure categories. Its real-application scenario runs in CI. The preceding checkpoint (a009f762, run 33980669281) passed Go tests and Linux runtime checks through clipboard, memory churn, diagnostics and SSH restoration; optimized benchmark execution is still pending as of this update. Current changes compile across all Rust targets and pass Python syntax checks; no tests were run locally.

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

Known issue to avoid hiding: session save lacks a shutdown flush (deferred feature task). Directory tracking now uses each surface's native reports; terminals that do not report changes retain only an explicit launch directory or an empty unknown value.

Run 33980669281 completed successfully, including the optimized round-trip benchmark and artifact upload. The first baseline is recorded in Observability. Follow-up work bounds lifecycle formatting and record serialization before allocating complete output, and adds a GTK heartbeat plus aggregate RPC outcomes to background snapshots. Browser socket calls still run synchronously on GTK in several handlers; extracting and scheduling browser transport remains required before claiming that boundary is complete.

Linux GL dispatch and GTK context/presentation callbacks now live in the platform library's optional GTK feature, including ownership of the libGL link dependency. The Ghostty adapter supplies the callbacks through its existing ABI. Clipboard callback contracts are documented, and empty content arrays return before pointer access. The workspace build and platform-only check without GTK pass. Run 33981411327 verified the CLI schema/collector checkpoint completely; later heartbeat and GL changes await their own CI evidence.

Removed ten more obsolete tests after inspecting their runtime dependencies and checking callers: AppleScript keystroke injection, AppKit drag routing, the two NSSplitView/Bonsplit underflow variants, CoreGraphics overlay/tab-transfer click scenarios, the missing macOS nightly workflow, NSView portal hosting, the absent drop-overlay animation probe and Info.plist/Sentry-framework version scanning. Portable nested-split attachment scenarios remain for adaptation. Replaced the old Xcode-discovery version test with a Linux scenario that verifies both executables' short/long version flags without display/socket services or application-state writes; CI invokes it explicitly.

Removed eleven newly orphaned macOS drag-gate/Bonsplit methods from the v1 test client after checking remaining callers. The v2 Bonsplit helper remains because a retained geometry scenario still uses it; that scenario requires a separate behavior audit. Python syntax checks and whitespace validation pass; executable verification stays in CI.

Replaced the pane tree's `/proc` child-process scan with Ghostty PWD actions. The surface registry now owns both pane routing and the latest directory, copies callback data before its native lifetime ends, and removes metadata on teardown. Snapshots use this per-surface value; absent reports fall back only to the explicitly supplied launch directory, never an unrelated child or HOME guess. Shared lookup functions replace duplicated reverse-map access. Registry lifecycle coverage and a two-shell OSC 7/session-snapshot scenario are added for CI. This corrects directory attribution without implementing the deferred shutdown flush or resume hooks.

Diagnostics count realized terminals through that same lifecycle registry, allowing resource reports to distinguish registered surfaces from RSS growth. The directory refactor builds and passes all-target compilation, Clippy correctness/suspicious checks and Python syntax validation. CI run 33981726753 fully verified the earlier heartbeat/encoding checkpoint; directory and later GL changes still require their own completed CI evidence.

The Rust function-comment pass now covers declarations found across owned application, platform and build-script sources (generated bindings and Ghostty excluded). Comments describe callback lifetimes, GTK ownership, persistence, protocol validation and test behavior. This declaration scan is supporting evidence, not proof that every older comment is accurate; semantic review and the other language passes remain open. Removed the no-op restore-time surface-sync traversal and its idle callback. Selected terminal/browser lookup now shares the pane lookup rather than maintaining three recursive searches. Removed constant-only retry/focus-policy tests that did not exercise production behavior; retained executable backoff and dispatch validation coverage.

The Go daemon now separates its native PTY adapter from request dispatch. Documented daemon entry/framing, stream ownership and PTY function contracts; the remaining Go handlers and tests still need their documentation pass. Proxy close now reuses the stream retirement helper. Stream output is encoded directly from the read buffer, eliminating an intermediate allocation, and delivery failure retires the stream rather than leaving its reader running. Added a real pipe/output-failure scenario for CI, alongside existing PTY launch/directory/reaping coverage. Built with Go 1.22.12 to match the module's Go 1.22 requirement; no local tests were run.

The Go test executable also compiles without running. Native PTY review identified no-op deadline methods and an indefinite termination wait. Close now allows 500 ms for termination before killing and reaping a stubborn shell. A shared native dimension validator rejects values outside 1..65535 before spawn or resize, avoiding uint16 wraparound. New executable tests use a signal-ignoring shell and inspect a real PTY grid after invalid resize attempts. Deadline handling remains open; builds and test compilation use Go 1.22.12, while execution stays in CI.

Moved the daemon's existing attachment-size metadata into `sessions.go` and documented its functions. Attach and resize now share validation/update logic while resize still rejects missing attachments; expanded dispatcher coverage verifies that distinction. Removed an unlocked deferred stdout-buffer flush: each frame already flushes under the shared writer mutex, and the extra shutdown flush could race with stream writers. This is a refactor of existing metadata behavior, not implementation of the deferred process/session-resume feature.

Separated remote CLI command mapping from relay transport and authentication, documenting transport contracts and consolidating workspace/surface environment defaults. Go 1.22.12 builds and test compilation pass; runtime validation remains in CI. Review exposed unbounded relay response reads and missing v2 I/O deadlines; these remain explicit transport-hardening work rather than being hidden by the extraction. CI run 33982861225 passed Linux runtime scenarios including native directory reports, diagnostics, memory churn and SSH restoration; its optimized benchmark remains pending.

Relay transport now caps aggregate v1 and single-frame v2 responses at 4 MiB, authentication lines at 64 KiB, and rejects oversized peers without draining them. Unix connection establishment has the same two-second bound as TCP. Legacy exchanges have a fifteen-second total budget while preserving their short multiline idle window; v2 uses thirty seconds, extended for explicit JSON browser waits. New executable coverage exercises size limits, fragmented lines, EOF and stalled writes/reads. Go build and test compilation pass locally without executing tests; CI must validate behavior.

Moved shared Go RPC parameter parsing into `params.go` and documented the remaining proxy handler ownership contracts. Integer parsing now rejects unsigned/native-width overflow, out-of-range floats, infinities and NaN before reporting success; JSON integer conversions share the signed range check. Executable boundary tests cover accepted extrema and rejected values. Go 1.22.12 builds and test compilation pass without local test execution. Duration multiplication overflow and no-op PTY deadlines remain separate outstanding review items.
