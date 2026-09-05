# Refactor audit

Status: in progress. This document is a completion checklist, not a claim that the migration is finished.

## Current requirement status

This table supersedes the historical checkpoint notes below. Inventory refreshed from `58471397` plus the removal of the absent Claude Teams launcher test group on 2026-09-05. Counts include tracked, existing owned files and exclude the Ghostty submodule; this is not a completion claim.

| Requirement | Current evidence | Remaining work |
| --- | --- | --- |
| Identify owned/required stacks | Architecture lists languages, roles and version sources. Tracked owned source has 70 Rust, 12 Go and one C file; no Swift, Objective-C `.m` or Zig files outside Ghostty. | Continue distinguishing runtime dependencies from retained upstream test tooling. |
| Remove unnecessary legacy artifacts | Website, copied native headers/stubs, duplicate desktop asset and multiple absent-Swift/AppleScript tests removed. Complete Ghostty submodule preserved. | Audit remaining legacy protocol/debug tests and historical planning material before removing or adapting them. |
| Document every owned function | Both Python clients and five maintained Python scripts have function docstrings; earlier Rust/Go declaration passes are recorded below. | Recursive Python AST scan finds 295 undocumented declarations among 558 functions in 68 `tests` files, and 659 among 853 functions in 91 `tests_v2` files. All 17 functions in the five maintained `scripts` Python files have docstrings. Audit unsupported scenarios before documenting them. Continue semantic review of existing contracts and embedded script helpers. |
| Language standards and architecture | All seven linked standards files exist: Rust, Go, Python, Shell, C, Zig and Configuration. Architecture links Components, Observability and gateway adaptations. | Keep contracts aligned as ownership boundaries change. |
| Concise agent instructions and symlink | Root AGENTS.md is six bullets, 42 whitespace-delimited words; CLAUDE.md is a symlink to AGENTS.md. | Preserve these constraints during further edits. |
| Linux component library | `cmux-platform` exports paths, filesystem, installation, notification, peer and process services, with optional GTK window/OpenGL modules and no workspace-model dependency. Headless compilation passes. | Native transport/process discovery callers still need boundary review; this does not establish complete platform isolation. |
| KISS and shared behavior | Shared pane closure, persistence, browser transport/decoding, Go stream lifecycle and Python discovery/transport/reference helpers have replaced duplication. | Review remaining large desktop/pane modules and synchronous browser startup/UI paths; finish cancellation and queue-bound audits. |
| End-to-end observability and benchmarks | Bounded logs, CLI/GTK correlation, resource/CPU counters, browser metrics, session-write timing, ping/churn artifacts and issue snapshots implemented. | SSH/external-service correlation, remaining browser paths, workload-specific rendering/notification benchmarks, report comparison and diagnostic-log collection remain incomplete. |
| Executable verification | Run 33991217679 at `9caf439a` passed Go, unit tests, browser-tab mapping and SSH restore. Terminal churn failed at the strict sibling-tab keyboard marker check, before reaching EOF or rendering workloads. | Resolve the input/PID marker failure using the added diagnostics; verify EOF, sustained rendering, optimized benchmarks and all subsequent changes in completed cumulative CI. No local tests are permitted. |

Session standby/resume hooks and additional agent-facing feature parity remain a separately deferred task. Their deferral does not waive any active refactor or observability requirement. A release tag is not appropriate while the active scope remains unfinished.

## Historical implementation checkpoints

The following notes preserve findings and verification at the time of each change. Their "remaining", "next stage" and CI-pending statements are historical; use the table above and current CI state for decisions.

### Initial inventory

The initial tracked-source inventory contained 41 Rust files, four Go files and three C files outside Ghostty; no owned Swift, Objective-C or Zig source. The application binaries are Rust. `build.rs` builds the X11 C bridge and links the Ghostty archive. At that initial checkpoint, a separate website CI job typechecked the inherited standalone `web` tree; both were subsequently removed.

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

Proxy open and write now share checked millisecond-to-duration parsing. Omitted timeouts retain ten-second/eight-second defaults and zero retains explicit unlimited behavior. Explicit negative, malformed, fractional or overflowing values now return `invalid_params` rather than silently falling back, disabling a deadline or wrapping its duration. Parser and RPC dispatcher cases compile for CI; no tests ran locally. This resolves the proxy duration multiplication item; PTY deadline implementation remains open.

PTY deadline methods now delegate to a pollable nonblocking master file rather than returning success without enforcement. Startup duplicates and registers the descriptor with Go polling, closes the original after transfer, and reaps the child on preparation failure. Resize uses `SyscallConn.Control` so native ioctl access preserves nonblocking mode. Native CI coverage exercises idle reads, a full raw input queue, resize and close cancellation. Go 1.22.12 build and test compilation pass; runtime validation remains pending. The implementation follows the [PTY library nonblocking guidance](https://github.com/creack/pty) and [Go file deadline contract](https://pkg.go.dev/os#File.SetDeadline), checked against the pinned local sources.

Separated daemon stream handlers and connection ownership into `streams.go`, leaving entry/framing/dispatch in `main.go`. TCP and PTY launch now use one registration helper, resize uses the shared lookup, and stream pumps retire through deferred cleanup across all exits. Expected native file closure is suppressed alongside network closure instead of producing a spurious error event. Go 1.22.12 build and test compilation pass; existing stream lifecycle and output-failure scenarios remain the executable CI coverage.

Completed the Go function-comment pass across production sources and test helpers. Comments identify tested behavior, fixture ownership, output capture limitations and synthetic connection semantics. A declaration scan finds no Go function lacking an adjacent comment; this supports navigation coverage and does not prove all tests or older code are correct. Python, shell and native-source documentation review remains outstanding.

Removed the unused root `cmux-Bridging-Header.h` and copied `ghostty.h`. Reference inspection confirms the active Rust build generates bindings from `ghostty/zig-out/include/ghostty.h`; remaining root-header mentions are historical planning records, not build inputs. The complete Ghostty submodule and its header remain intact. The only owned C implementation is the documented X11 positioning bridge in `cmux-platform`. Workspace binaries build after removal; the existing GTK X11 deprecation warnings remain. Historical planning records still require their broader relevance audit.

The maintained Python prompt probe now documents its functions, shares the existing Linux-aware client socket discovery and uses monotonic readiness timers. Cleanup starts immediately after workspace creation, covering selection/readiness failures that previously leaked the temporary workspace. Invalid counts and nonfinite timing arguments are rejected before creating a workspace. Added a CI-only behavioral scenario for selection failure, cleanup and original-workspace restoration; local validation was syntax compilation only. Larger Python client/test and shell audits remain pending.

Documented the named shell functions in maintained setup, release-version, remote-asset and browser-resolution scripts. Fixed an unquoted repo-local Zig executable invocation so toolchain reuse also works when the checkout path contains spaces. Bash syntax validation passes without executing setup or release actions. CI remote-daemon tests at e89d88cb passed, covering the accumulated response bounds, numeric and timeout validation, pollable PTY deadlines and stream ownership changes; that run's Linux job is still active.

Removed `tests_v2/test_update_timing.py`: it only parsed timing constants from the absent `Sources/Update/UpdateTiming.swift`, had no callers, and exercised no Linux behavior. Remaining AppleScript-dependent runtime candidates must be adapted or retired only after checking their Linux behavior and replacement coverage:

| Candidate | Required follow-up |
| --- | --- |
| Removed `tests_v2/test_ctrl_enter_keybind.py` | Replaced by configured Ctrl+Enter plus real X11 input in the isolated GTK clipboard/input fixture. |
| Removed `tests/test_cmd_option_t_close_other_tabs_in_pane.py` | Mac-only Cmd+Option+T and Cmd+D confirmation have no equivalent implemented Linux close-other-tabs action. This is an upstream feature gap, not retained runnable coverage. |
| Removed `tests{,_v2}/test_cpu_notifications.py` | Removed macOS-only workload and unreliable sampling; Linux notification CPU benchmark remains outstanding. |
| `tests/test_session_restore_unfocused_workspace_{relaunch,multi_window}_cycle.py` | Audit current metadata restoration coverage separately from the deferred session-resume feature. |

The browser-panel stability tests mention AppleScript only to state that it is unnecessary; that keyword does not justify removal. These remaining scenarios are not claimed as Linux coverage merely because they remain tracked.

Added retained diagnostic snapshots to the real Linux memory-churn/redraw scenario and its existing CI artifact upload. Reports identify the debug/software-rendering workload, phase, revision and partial failure status; they are not labeled optimized render benchmarks. Documented scenario helpers and added kill/reap fallback when normal fixture shutdown times out. Python syntax compilation and diff checks pass; the real scenario remains CI-only.

Instrumented the shared session-save function with serialization and filesystem timing, byte/workspace counts and categorized failures, excluding paths and payloads. Corrected the schema-version documentation. The real diagnostics CI scenario now triggers a workspace mutation and checks its save measurements. All Rust targets compile; Python syntax and diff checks pass without local test execution.

Extracted browser Unix socket exchange into `src/browser/transport.rs`. Responses are limited to 4 MiB and individual reads/writes have five-second timeouts; real socket-pair CI tests cover valid framing and oversized peers. All Rust targets compile. This remains synchronous: connection establishment, public CLI subprocesses and daemon readiness waits still need the off-GTK transport refactor. Per-I/O timeouts are not an overall deadline against a slowly trickling peer.

Browser frame delivery now transfers immutable `glib::Bytes` through the existing latest-value channel, eliminating the full JPEG copy on GTK consumption. Stream cancellation is centralized and also runs when BrowserManager is dropped, so task ownership does not depend on an explicit shutdown call. Corrected stale mpsc/idle-callback documentation. All Rust targets compile; browser runtime and rendering measurements remain outstanding, so no quantified memory improvement is claimed.

Added aggregate browser-preview metrics to diagnostics: active task ownership, incoming JPEG counts/bytes, texture assignments and decode failures. Instrumentation records no frame content and avoids per-frame log writes. All Rust targets compile. A real browser stream fixture is still required to validate lifecycle counters and rendering behavior end to end.

CI run 33984532821 at e89d88cb completed successfully, including Go transport/PTY tests, Linux runtime scenarios and the optimized benchmark. Newer browser and diagnostic changes remain queued. Added an actual connected-but-silent browser socket case to verify read timeout behavior; all-target compilation passes without local execution.

Extracted optional browser executable discovery from preview management into `src/browser/discovery.rs`; agent-browser and Chromium now share PATH traversal. Replaced the custom timestamp/thread hash request identity with the existing UUID implementation. Discovery still defers executable permission errors to process launch, and blocking browser process/connection work still requires the off-GTK refactor. All Rust targets compile after extraction.

Generic browser RPC proxying now resolves surface references on GTK, drops the AppState borrow, and performs Unix connect/write/read asynchronously on Tokio. Exchanges have a five-second overall deadline and sixteen active slots with immediate overload rejection. Sync/UI and public CLI browser paths remain to migrate. A real Unix-listener test covers fragmented async responses; all Rust targets compile, with execution delegated to CI.

Generic async browser exchanges now stop when their response receiver is dropped, releasing socket and concurrency capacity through future cancellation. This does not undo a browser-side action already received. A real Unix socket scenario checks EOF after aborting an in-flight exchange. All Rust targets compile; runtime execution remains in CI.

Synchronous and asynchronous browser exchanges now share response-size and JSON validation in one parser, keeping protocol checks aligned during the remaining caller migration. The existing real socket tests cover both transport modes. All Rust targets compile; runtime CI is pending.

Preview WebSocket messages and individual frames now have explicit 8 MiB limits. Frame parsing uses a typed borrowed envelope instead of a copied JSON value tree, retaining support for escaped JSON strings. Added executable decoding cases for valid, escaped, missing and invalid payloads. The compressed-message bound does not constrain decoded texture dimensions; that and real stream validation remain pending.

Browser stream connection setup now has a five-second overall WebSocket handshake deadline, so a listening peer cannot retain startup indefinitely. Bounded diagnostic transition records distinguish connection success, failure and timeout without exposing the endpoint. The existing stream task guard releases its active count on each outcome. Rust all-target compilation passes; runtime stream coverage remains pending.

Extracted preview WebSocket receipt and weak-widget GTK delivery into `src/browser/stream.rs`. The reader now observes channel closure alongside incoming messages, releasing an idle WebSocket when its consumer disappears. Added a real loopback WebSocket scenario that delivers a frame and verifies reader completion after receiver removal. All Rust targets compile; tests remain CI-only. Texture decoding and actual GTK presentation still require broader browser runtime coverage.

GTK preview delivery now listens for picture destruction alongside new frames, closing the channel when an idle picture disappears. The destruction notification is disconnected when delivery ends normally, avoiding accumulated callbacks. Added a display-backed CI test that drops the picture while keeping its frame sender alive and verifies receiver cleanup. All Rust targets compile; no local tests ran.

Generic browser RPC dispatch now carries the observed trace identity into async completion records, including receiver cancellation, without logging parameters or response payloads. All-target compilation passes. The external daemon and remaining synchronous browser paths still lack internal trace propagation.

Moved browser pointer-motion forwarding into its own async component. It now waits out the throttle window and sends the newest retained coordinates, instead of dropping an initial/final motion event unless another event arrives. Socket exchanges reuse bounded async transport under a one-second overall budget; no blocking worker is spawned. Added a real Unix-listener case for a lone initial motion event. All Rust targets compile; runtime checks remain CI-only. Keyboard/click ordering and public CLI UI operations still need their separate threading migration.

Exposed async browser transport admission in diagnostic snapshots using the existing semaphore as the active-work source, plus an overload rejection counter. No duplicate active-work bookkeeping or per-input logging was added. Rust all-target compilation passes; CI runtime validation remains pending.

Repeated all-target Clippy correctness/suspicious checks after the browser refactor; they pass. Removed redundant native pointer casts, unit bindings and boolean matches, used the shared range idiom, simplified socket discovery sorting and avoided a full iterator scan when extracting an SSH host suffix. Remaining non-gating warnings concern larger function signatures, enum naming, a close-path match and test-module placement. No local tests ran.

## Shared surface and empty-pane closure

Keyboard, socket and native terminal-close callers now share `SplitEngine::close_surface_and_empty_pane`. The helper removes a tab and closes its pane when empty, returning the final-pane case to the caller. Keyboard confirmation, socket refusal to remove the final pane and native workspace-close policy remain with their existing callers. Keyboard closure of an empty pane now also schedules session persistence through the successful-close branch. Workspace-wide Cargo checking passes; runtime pane/tab-close verification runs in CI.

## Bounded preview pixels and background decoding

Replaced GTK-thread image decoding with the browser pixels component, using `image` without default features and only JPEG/PNG codecs. It supplies strict header limits, a pixel budget, checked output allocation and a best-effort codec allocation budget rather than maintaining a custom image-header parser. Two semaphore permits bound blocking decodes globally and remain owned by running work after delivery cancellation. GTK receives shared RGBA memory textures. Added real PNG/JPEG decode tests, oversized-header rejection, and texture-assignment coverage in the existing display-isolated CI test. Added aggregated decode timing and overload counters. Workspace checks and Clippy gates pass; new runtime coverage awaits CI. No local tests were run.

## Linux keybinding coverage replaces AppleScript

The isolated Ghostty/GTK input fixture now loads `ctrl+enter=text:\r`, types a shell command through xdotool, checks that typing alone has not executed it, then asserts Ctrl+Enter executes in the focused terminal. It does not read the user's config or skip for missing Accessibility permissions. Removed its old AppleScript test and the unimplemented macOS close-other-tabs shortcut test. Both were outside Linux CI; the new binding assertion runs in the existing Xvfb clipboard/input job. No local tests were run.

## Shared Linux discovery for Python clients

Both retained Python protocol clients now import one documented discovery helper instead of separate macOS Application Support and tagged-bundle search chains. It follows Rust CLI candidate precedence: CMUX_SOCKET, CMUX_SOCKET_PATH, standard XDG socket, runtime marker, debug socket, newest tagged debug socket. Missing candidates return the standard XDG socket so Python clients can wait for startup; Rust currently reports no discovered socket. Relative XDG_RUNTIME_DIR is ignored consistently with the platform library. Marker reads are bounded and malformed text or disappearing candidates do not break discovery. Tests use temporary Unix sockets for override/runtime/marker precedence and reject oversized, invalid and stale markers; CI executes them. Python syntax compilation and diff checks pass locally; no local tests run. The subsequently audited notification scripts and their v1 bundle-ID helper have been removed.

## Shared Python connection lifecycle

The v1/v2 clients now share documented Unix connection setup with monotonic startup budgets, timeouts set before connect, retries limited to missing/refused endpoints and failed-attempt socket cleanup. Existing client startup/I/O timeout values are preserved. Closing clears buffered response state before reconnecting. The v2 constructor now resolves discovery at construction time, fixing its stale import-time default. Added CI socket-exchange, missing-endpoint deadline and invalid-budget tests. Syntax compilation and diff checks pass; runtime tests remain CI-only. Response framing, UTF-8 chunk handling and aggregate response limits are still pending transport work.

## Bounded Python response framing

The shared transport now reads at most 4 MiB plus one overflow-detection byte, uses monotonic total read deadlines and decodes UTF-8 only after assembling response bytes. V2 retains subsequent coalesced lines in a byte buffer and requires a newline; V1 preserves its multiline response protocol with a 100 ms post-newline idle boundary or EOF. Read/framing failures close the client connection and discard partial state. Real socket-pair CI tests cover fragmented Unicode, coalesced lines, oversized replies, silent-peer timeout, multiline responses and premature v2 EOF. Local validation was syntax compilation and diff checking only. Request serialization/send budgeting and semantic v2 response validation remain separate work.

## Removed duplicated macOS notification CPU tests

Deleted the v1/v2 notification CPU scripts: their process discovery targets macOS app bundles, their popover scenario requires SwiftUI/AppleScript, and they suppress notification-command failures while converting missing CPU samples to zero. Neither ran in Linux CI. Removed the last client bundle-ID/suffix helpers after confirming these were their only external caller. A Linux notification burst/idle benchmark with positive operation assertions and interval CPU accounting remains required observability work; existing memory-churn evidence does not cover that workload. Notification-pane implementation remains in the separately deferred agent-capability task. Python client syntax and diff validation pass; no runtime tests were run locally.

## Process CPU accounting in Linux services

Added documented, checked getrusage CPU accounting to cmux-platform process resources and exposed cumulative user/kernel microseconds in diagnostic snapshots. The FFI is contained in the platform component with explicit initialization invariants; malformed or unavailable values remain absent. Existing benchmark artifacts and diagnostic collectors retain these fields without another sampling implementation. CI covers time conversion, live nondecreasing accounting and CLI exposure. Workspace Cargo checking, Python syntax compilation and diff validation pass; no local tests executed.

## Python convenience-client function documentation

Documented the remaining 95 v2 client functions after reviewing their implementations, including identifier resolution, tuple return shapes, focus policy, target exclusivity, transport errors and upstream debug hooks. Both retained Python clients now have docstrings for every declared function in the syntax-tree audit. This is documentation evidence, not proof of server support: the module explicitly identifies upstream wrappers as capability-dependent. Debug-method compatibility and broader legacy-test portability still require audit. Syntax compilation and diff checks pass; no tests were added or run for this documentation-only change.

## Shared Python reference resolution

Consolidated workspace, pane and surface selector parsing and numeric-index lookup into one documented v2 client helper. Thin typed wrappers retain existing callers and current-selection differences: no workspace is an error; no focused pane/surface remains optional. Explicit UUIDs and typed references avoid RPC lookup, while indexes retain optional workspace scope. Added executable client tests with controlled server responses for identifiers, index resolution, missing selections and wrong-kind references. CI runs these with Python transport tests. Syntax compilation and diff checks pass; no local tests run.

## Python v2 response envelope ownership

Added one documented envelope validator for object shape, integer request-ID equality, boolean success state and structured server errors. Python boolean IDs no longer compare equal to request integer 1. Malformed JSON/envelopes and failed writes now close the connection and clear buffered state; correctly framed server errors preserve the connection for the next numbered request. CI socket-pair tests exercise invalid shapes/IDs and a server error followed by success. Syntax and diff checks pass; no local tests executed.

## Benchmark failure evidence

Ping benchmarking now retains completed samples and initial diagnostics when ordinary workload operations fail, records phase/error category without command output, rejects invalid warmup responses and process changes, and exits unsuccessfully after writing a failed report. Output creation is exclusive and mode 0600. CI behavior tests cover partial measurement failure, empty warmup failure, success and process replacement. Python syntax and diff checks pass locally; no local tests run.

## Linux CPU trends replace obsolete stack sampling

Removed tests/test_cpu_usage.py and its tests_v2 counterpart, whose macOS app matching, sample command and SwiftUI stack regexes do not describe this GTK process. The existing diagnostic collector now calculates adjacent-sample CPU usage from platform counters, preserving unknown values for unavailable/reset samples and process changes. CI verifies interval arithmetic, multi-core percentages, zero usage and invalid samples. This does not claim coverage of the old SwiftUI stack patterns or establish an optimized idle benchmark. Syntax and diff checks pass; no local tests run.

## Maintained Linux fixture contracts

Documented the remaining fifteen helpers in the Linux window-state and SSH/workspace-launch scenarios, including owned process lifetimes, X11 discovery, persisted snapshots, bounded CLI calls and remote-directory proof. A syntax-tree audit now finds no undocumented functions in tests/test_linux*.py. Broader retained Python test documentation remains incomplete. These were documentation-only edits; syntax compilation and diff checking passed, with no local test execution.

## Bounded Linux fixture child cleanup

Window and workspace integration fixtures now share a documented terminate/wait/kill/reap helper for directly owned children. Nested cleanup ensures application failure still reaches window-manager or SSH cleanup and closes log handles. Workspace restart assertions continue to fail when graceful application shutdown requires killing. The helper does not claim to stop entire process groups or privileged SSH daemons; those remain explicitly owned by the fixture. CI tests launch a TERM-ignoring child to verify forced reaping and verify already-exited children. Syntax and diff checks pass; no local tests run. CI run 33986935543 completed successfully at d958b692, including bounded browser image decoding and Ctrl+Enter coverage.

## Shared cleanup across maintained Linux scenarios

Diagnostics, memory-churn and surface-directory scenarios now use the same bounded child cleanup as window/workspace fixtures. Diagnostics and directory scenarios preserve failure on forced shutdown while ensuring the child is reaped; churn retains its existing kill-fallback policy and records shutdown_forced in its report. This removes duplicated teardown logic without changing workload thresholds. Python syntax and diff checks pass; execution and helper behavior tests remain in CI.

## Bounded Rust CLI response reads

Replaced unbounded BufRead::read_line in the Rust CLI with a documented newline reader capped at 4 MiB and one monotonic read deadline. It assembles bytes before JSON decoding, rejects premature EOF and leaves subsequent buffered replies intact. Per-read socket timeouts are reduced to the remaining budget, preventing continuous partial input from resetting the full timeout. CI Unix socket-pair tests cover byte-sized UTF-8 reads, coalesced lines, exact limits, overflow, idle peers and truncated replies. Workspace all-target checking and diff validation pass; no local tests run. Connection establishment, write budgeting and semantic response validation remain separate transport audit items.

## Rust CLI envelope validation and retirement

The CLI now validates response IDs, boolean ok fields and structured server errors in one documented decoder. Transport/framing/semantic protocol failures shut down and retire the connection, preventing buffered data from being reused as another request's response. Correctly framed server errors preserve the connection. Request-ID increment uses checked arithmetic. CI socket-pair tests cover malformed envelopes and a server error followed by a successful numbered response. Workspace all-target checking and diff validation pass; no local tests run.

## Shared Rust CLI exchange budget

CLI request writes now use a documented partial-write loop with decreasing socket timeouts. A shared remaining-budget helper carries the operation budget from request preparation through writing and response reading, rather than granting a fresh full timeout at each phase. Interrupted writes retry within the same budget; transport errors continue to retire the connection. CI socket-pair coverage verifies exact bytes and a live non-reading peer. Workspace all-target compilation and diff checks pass; no local tests run. Unix connection establishment and outbound serialization allocation remain open audit items.

## Shared bounded JSON encoding

Extracted the existing diagnostic JSON-line encoder into a neutral module shared by desktop and CLI compilation. CLI requests now stop serialization at 4 MiB including the newline, accounting for JSON escaping before socket writes. Oversized local requests leave the connection usable because no bytes were sent. Existing caller-owned JSON values remain outside the encoder budget. Moved encoding behavior tests with the helper and added a real socket-pair test proving no partial request is emitted and a later valid request succeeds. Workspace all-target checking and diff validation pass; no local tests run.

## Server request framing boundary

Extracted socket request framing from connection dispatch into a documented module. It limits incoming lines to 4 MiB, applies a ten-second completion deadline after first bytes arrive, accepts CRLF and rejects incomplete EOF/invalid UTF-8 before JSON parsing or GTK dispatch. Idle connections remain supported. Rejections emit bounded structured error categories. CI Unix socket-pair tests cover byte-fragmented Unicode, coalesced lines, exact bounds, overflow, stalled partial requests and truncated EOF. Workspace all-target checking and diff validation pass; no local tests run.

## Bounded server response delivery

The framing component now owns response writes as well as request reads. It caps serialized response bodies at 4 MiB and applies one ten-second deadline across body/newline writes. Errors close the connection and record a structured delivery failure. Added a bounded in-memory transport CI test for exact framing and a non-reading peer; all-target Cargo checking and diff validation pass. No local tests run. Handler-side response allocation and failure trace correlation remain outstanding.

## Socket admission ownership

Added a 64-slot authenticated connection gate with immediate overload rejection. Handler tasks own semaphore permits through idle/read/dispatch/write phases, and normal exit or cancellation releases them. Diagnostics expose capacity, active handlers and rejected connections without per-rejection log traffic. CI tests cover saturation, reuse, task cancellation and live diagnostic fields. Workspace all-target checking, Python syntax and diff validation pass; no local tests run. The limit is an initial resource bound, not a measured optimal capacity or complete GTK queue bound.

## Request payload ownership during dispatch

Socket dispatch now consumes its raw request string, takes ID/method/parameter fields from parsed JSON, and drops unused envelope/parameter storage before waiting for execution. Generic browser commands move their full parameter value into the command instead of retaining a cloned tree; diagnostics requests discard unused params before worker sampling. This reduces simultaneous retained representations without changing wire behavior. Existing request-validation coverage compiles with the owned input signature. All-target Cargo checking and diff validation pass; runtime verification remains CI-only.

## Transport error and module style cleanup

CLI error variants now use Connection/Command/Protocol within CliError, share one message-formatting arm and implement the standard Error trait. Binary exit-code routing is preserved. SSH cleanup guards precede the test module, removing the remaining module-order warning. All-target Clippy correctness/suspicious gates pass; remaining Rust style findings are the two large constructor signatures, plus native GTK deprecation warnings. No local tests run.

## Shared restored-terminal construction

Saved-tree restoration now borrows one immutable context containing GTK/native launch dependencies. A single terminal constructor handles legacy leaves, sibling terminal tabs and empty-pane fallback, preserving UUIDs, directory precedence, remote/startup-command precedence and context-menu wiring. The recursive walk passes context plus tree/depth/ID state rather than repeating eight arguments. No new restore/resume behavior or ownership framework was introduced. Existing Linux launch/session scenarios provide runtime coverage in CI; workspace all-target compilation and diff validation pass locally, with no local test execution.

### Pane restoration module

Moved saved-layout reconstruction and its borrowed launch context into `src/split_engine/restore.rs`. The pane engine retains interactive operations and shared surface constructors; the child module retains existing UUID, focus, directory/command precedence and depth-limit behavior. This is component separation of existing restoration, not implementation of the deferred session-resume feature task. Updated the component map and recorded the applicable Rust module guidance. `cargo check --workspace --all-targets` passed; executable restoration checks remain in GitHub Actions. Existing native GTK/X11 deprecation warnings remain.

### Remove obsolete pane-construction dependencies

Removed the unused GTK Application parameter from terminal construction and the corresponding retained application handle from SplitEngine and restoration context. Removed the unused initial surface-cell constructor argument; the terminal widget still owns the native surface cell and lifecycle callbacks. Corrected documentation that claimed the engine stored that cell or initialized the Ghostty application. Local, remote and restored workspace creation now share the existing sidebar row builder, replacing two duplicate construction blocks. Updated the executable clipboard fixture for the narrower constructor API. Validation: all-target Cargo check; runtime coverage remains in Actions.

### Retire upstream portal close-race scenario

Removed `tests_v2/test_split_cmd_shift_d_ctrl_d_no_portal_orphans.py`: it requires macOS portal counters (`debug.portal.stats`), `selectedPanels` layout records and `surface.close.childExited` / `surface.lifecycle.deinit` / `ws.term.visible` log formats absent from the owned GTK implementation. Its `ctrl-d` socket request also reaches a current single-character-only `SurfaceSendKey` handler, so the scenario cannot exercise its stated workload here. No CI workflow references the removed file.

Existing Linux coverage in `tests/test_linux_memory_churn.py` repeatedly closes surfaces through the CLI, verifies child PTYs are reaped and checks warm-cache RSS growth; CI also exercises workspace widget release. These checks do not prove the original transient close-to-visible invariant. Remaining coverage requirement: a native GTK input scenario must send actual Ctrl-D, observe child-exit-driven pane removal, and verify the closed surface cannot be rebound or regain visibility during deferred callbacks. Implement executable lifecycle evidence before claiming that race covered. Multi-key socket input support remains an agent capability gap; this cleanup does not implement the deferred agent-feature task.

### Native EOF lifecycle coverage

The Linux churn fixture now includes nine real GTK Ctrl-D exits among its 45 split/close cycles. Each selected terminal executes a canonical-input Python reader in place of the shell; a file marker establishes readiness before xdotool sends Ctrl-D. The fixture verifies the child is reaped before explicitly closing the surface, then checks baseline child ownership and existing warmup RSS bounds. Diagnostic artifacts distinguish child_eof samples and record the workload count. This covers native input and exited-child teardown without pretending the logging-only Ghostty close callback removes panes automatically. Automatic pane removal and transient visibility coverage remain incomplete. Python compilation and diff checks passed locally; runtime validation is CI-only.

### Filesystem permission boundary

Moved socket-directory, socket-file and updater-executable access policies into documented cmux-platform filesystem functions. Private directories are created with restrictive creation modes and existing final directories are corrected to 0700; file/socket restriction and executable staging use 0600 and 0755 respectively. Socket startup now stops on required permission failures instead of continuing after a chmod warning. Helpers retain ordinary symlink-following filesystem semantics; this does not claim adversarial path-race protection. Added executable filesystem tests for existing-directory tightening, file access transitions and missing-path errors. All-target Cargo check passed; tests run only in Actions.

### Shared packaging validation

All three package validators now source documented argument-based reporting from packaging/scripts/validation.sh. DEB checks no longer use eval: dpkg listing/control output is cached in temporary files and checks invoke validators and grep directly. RPM negative checks no longer interpolate paths into bash command strings. A shared absence check distinguishes no match from input errors. Added a main-CI shell behavior scenario for literal arguments, paths containing spaces, counters, negative matching and missing-file failures. Shell syntax checks and git diff --check passed; behavior tests remain CI-only. Existing package-format assertions remain release-workflow checks.

### Shared release version lookup

DEB/RPM builders and the version-bump script now use the documented package_version helper in scripts/release-version.sh. It selects the root [package] table, tolerates whitespace/comments used in literal declarations, and rejects missing, inherited or non-release versions instead of selecting an unrelated dependency version. The helper intentionally supports the repository literal X.Y.Z convention rather than claiming to parse arbitrary TOML. Package builders preserve their CMUX_VERSION override. Added main-CI executable shell scenarios for workspace/dependency versions preceding/following the root, paths with spaces, absent files and unsupported declarations. Shell syntax and diff checks passed locally; no tests ran locally.

### Retire legacy icon identifiers

Renamed the three tracked icon source files from com.cmux_lx.terminal.png to io.cmux.App.png without changing their bytes. Updated generation, DEB/RPM builders, source validation and release archive assembly together; installed icon paths remain io.cmux.App.png. AppStream developer ID now follows the listed nitecon maintainer as io.github.nitecon. Owned packaging and workflow sources no longer reference the old identifier. Shell syntax and diff checks passed; package validation remains in the release workflow.

### Browser daemon ownership and metadata bounds

BrowserManager now generates a private session identity instead of every application instance using the shared cmux daemon. Command sockets, stream advertisements, public CLI calls and shutdown derive from that same identity. This prevents cross-instance navigation/closure, but does not implement crashed-daemon discovery or cleanup. Stream-port file reads retain at most 65 bytes and reject advertisements above 64 bytes, invalid UTF-8, zero and out-of-range ports without echoing file contents. Added endpoint-isolation and real-file boundary/error tests for CI. All-target Cargo check passed. Synchronous startup, public CLI output collection and GTK navigation remain unfinished refactor work.

### Asynchronous browser history navigation

Back/forward/reload now execute their public CLI command and URL refresh on Tokio with one admitted history operation per browser manager. Each subprocess has a fifteen-second deadline, 4 MiB stdout and 64 KiB stderr budgets; pipes drain concurrently, errors kill/reap the direct child and task cancellation uses kill-on-drop. The helper shares CLI envelope decoding with remaining synchronous callers and omits raw pipe contents from errors. GTK only applies results to a surviving mapped entry whose address has not changed; widget destruction aborts the operation without retaining AppState. Added executable subprocess tests for dual-pipe output, nonzero exit status, timeout, overflow and cancellation/reaping. All-target Cargo check passed; tests run in CI only. Daemon startup, other public CLI operations and ordering against those synchronous operations remain incomplete; direct-child cancellation does not claim descendant daemon cleanup.

### Browser navigation trace coverage

Added a shared browser activity guard for correlated GTK history-operation and CLI-stage begin/completion records. Drop records cancellation; fixed outcomes identify subprocess timeout, output overflow, I/O failure, protocol/command failure and stale widget results. Admission records identify overlap rejection. Records use existing bounded diagnostics and contain no URLs, arguments or process output. All-target compilation passed. Actual browser trace integration and external-daemon propagation remain unverified/incomplete; subprocess tests cover the underlying execution lifecycle in CI.

### History worker regression and EOF fixture correction

Added a controlled executable browser CLI scenario that exercises production history navigation: session/argument routing, ordered history+URL commands, overlap rejection with no child invocation, nonzero-exit admission recovery and dropping an unpolled operation. All-target compilation passed; execution remains in CI.

CI run 33989291802 failed the new EOF churn scenario because the default application Ctrl-D binding creates a split; the log showed an additional pane, not a child-exit failure. The isolated fixture now overrides split_right to Ctrl-Alt-D before application startup so Ctrl-D reaches canonical terminal input, and records that override in its artifact. Default product shortcuts are unchanged. Python syntax and diff checks pass; the corrected scenario still needs a successful CI run.

### Shared asynchronous URL navigation

The Go button and Enter handler now share address normalization and async viewport/open submission. History and URL operations use one manager admission slot, ordered bounded public CLI subprocesses and one GTK completion/cancellation helper. Failed viewport sizing stops the sequence; absent/zero dimensions skip sizing. The controlled executable fixture now covers literal URL arguments with spaces/shell syntax, viewport/open/get ordering, cross-operation overlap rejection and early termination after viewport failure. All-target Cargo check passed; runtime assertions remain CI-only. Startup, mapping, resize and input/devtools synchronous paths remain to migrate.

### Browser-manager cancellation ownership

BrowserManager now owns a navigation stop signal as well as admission. Shutdown and Drop share stop_navigation: close admission and notify admitted futures, which stop their command sequence and drop any active kill-on-drop child future. A stopped manager cannot launch delayed navigation after teardown. Added executable coverage for an unpolled admitted operation after manager drop and a running real child that must be cancelled/reaped before its fifteen-second command deadline. All-target compilation passed; CI execution is pending. This controls application-owned CLI children, not detached browser-daemon descendants.

### Browser UI component boundary

Moved browser tab creation/restoration, widget wiring, navigation completion and CDP key translation out of shortcuts.rs into src/browser/ui.rs. The shortcut module retains workspace/pane actions and re-exports its existing browser entry points, preserving callers. The browser UI module documents GTK/AppState ownership separately from sibling worker transport/CLI/stream modules. Mapping, initial viewport, startup and input/devtools synchronous paths remain visible refactor targets; this structural move does not claim those paths asynchronous. All-target compilation and diff checks validate the move locally; behavior verification remains CI-only.

### Browser input coordinate boundary

Extracted CDP key/modifier conversion and picture coordinate translation into src/browser/input.rs. Click and hover now share a contained-image transform; scroll shares viewport-size resolution. The previous independent width/height scales treated preview content as stretched, producing incorrect input positions when the image was letterboxed. Preview construction now explicitly selects Contain, matching the [GTK Picture default](https://docs.gtk.org/gtk4/property.Picture.content-fit.html); the transform accounts for centered padding and ignores points outside the image. Added pure behavior tests for wide/tall aspect ratios, image boundaries, zero allocation and non-finite input. All-target Cargo check passed; tests run in CI. Keyboard/click/scroll transport still has synchronous callers and remains incomplete.

### Shared browser keyboard envelopes

Key press/release callbacks now call one documented keyboard_event translator for CDP key/code, modifiers and text inclusion. Presses use Unicode character count instead of UTF-8 byte length, preserving text for characters such as é. Releases, named navigation keys and Control shortcuts omit text. Added executable translator tests covering non-ASCII presses/releases, shortcut modifiers, arrows and space. All-target Cargo check and diff checks passed; tests execute in Actions. This fixes direct key-event encoding, not IME composition or physical-key layout fidelity, and leaves synchronous transport migration open.

### CI-driven accelerator and browser fixture corrections

Run 33989812669 showed the configured Ctrl-D override still triggered a split because GIO registered hard-coded defaults separately from capture-phase ShortcutMap. Menu accelerators now resolve from that same map, including collision/default handling; fixed nonconfigurable actions retain their bindings. The existing isolated EOF churn scenario exercises the end-to-end override. Browser-tab startup failed because mock_agent_browser.py still created cmux.sock after production switched to private session identities. The documented mock now carries --session through daemon spawning, socket and stream paths, and closes probe/read handles deterministically. All-target Cargo check and Python syntax checks passed; corrected runtime verification remains pending.

### Retire absent AppKit health/focus checks

Removed both duplicate nested_split_no_detach_during_update scenarios. Their health polling expects a surfaces array containing in_window, while the owned surface.health response is a single alive/has_attention object. The v2 helper turns the absent array into an empty list, and the scenario only counts explicit false values, allowing absent evidence to pass. Neither script validates GTK attachment or visibility. Removed test_omnibar_focus_cpu.py, whose commands target absent macOS webview/focus simulation APIs, whose process fallback searches app bundles and whose ps errors become 0% CPU; its failure diagnostic invokes macOS sample. No maintained workflow or script references these files.

Coverage still required: native nested-split visibility/attachment across reparenting and bounded CPU after repeated address-entry focus. Existing GTK widget/PTY lifecycle tests and interval CPU diagnostics cover related resource behavior, not these visual/focus invariants. Retiring these scenarios does not count their intent as verified. Diff checks passed; no runtime behavior was changed or tested locally.

### Replace permissive sibling-tab focus scenarios

Removed the duplicate v1/v2 new_tab_interactive_after_splits scripts, which relied on upstream focus/readback simulation APIs and could fall back to direct socket input, then only warn when a command-side-effect marker never appeared. The Linux churn fixture now builds two nested splits and creates three sibling terminal tabs through actual Ctrl-T input. It verifies the new surface is selected, types a command through GTK with no focus-repair or direct-send fallback, and requires the marker to contain the newly spawned terminal child PID. Each tab and split is closed and children must return to baseline. Artifact metadata records the workload and a resource sample. This verifies input routing/command execution; it does not claim pixel-level redraw correctness. Python syntax/diff checks passed; runtime confirmation remains pending in Actions.

### Owned shell function documentation pass

A tracked-file declaration scan outside Ghostty found three remaining shell functions without preceding contracts: two Linux fixture cleanup traps and the pane-close child counter. Added comments documenting ownership, global inputs, error propagation and the existing unbounded shutdown wait. All conventional shell function declarations found by this scan now have adjacent comments; this is declaration evidence, not a claim that all shell lifecycle behavior or embedded code has been reviewed. The two shell fixtures still need bounded process teardown consistent with maintained Python fixture cleanup. Shell syntax and diff checks passed; no tests ran locally.

### Shared Linux application fixture lifetime

Replaced test_linux_terminal_pane_close.sh with a Python scenario using tests/linux_app.py. The shared fixture owns isolated XDG paths, bounded CLI calls, monotonic condition waits, thread-aware child discovery and process_support termination/reaping; failure-log output is capped at 64 KiB. The pane-close scenario retains real CLI split/close and native assertion checks while strengthening verification to preserve exact surviving surface identities and the original terminal child. Main CI now invokes the Python scenario. Python syntax and diff checks passed; executable verification remains CI-only. The browser-map shell fixture still needs migration to the same bounded ownership path.

### Browser-map fixture uses bounded application cleanup

Replaced the reentrant browser-map shell scenario with Python using the same running_app helper as pane close. It retains actual terminal close, responsive ping, deferred-mapping and panic checks, and additionally requires the browser UUID to be the sole surviving surface. Diagnostic polling reads at most 1 MiB. The session-aware browser mock now exits and removes its endpoint/metadata after a close command; the fixture retains explicit signalling for failed-startup paths because the detached mock is not its direct child. Both maintained shell application fixtures now use shared bounded app termination/reaping. Python syntax and diff checks passed; behavior runs only in CI.


### Shared fixture polling deadlines

`tests/process_support.py::wait_until` centralizes polling used by window restore,
SSH/script launch, terminal churn and the shared Linux application fixture.
Monotonic elapsed time includes unsuccessful predicate work; sleeps are limited
to the remaining budget. Exceptions propagate without retries. The application
wrapper also checks whether its owned process has exited. Predicates cannot be
preempted, so their blocking operations still require independent deadlines.
The existing CI helper suite covers budget exhaustion, eventual success and
predicate failures. Local validation was Python syntax parsing and diff checks;
runtime verification remains in GitHub Actions.


### Browser-only pane split and obsolete WebKit stability suites

Removed identical `tests/test_browser_panel_stability.py` and
`tests_v2/test_browser_panel_stability.py`: both require WKWebView first-responder
commands absent from the GTK server. They also suppress surface cleanup failures.
The maintained GTK browser mapping fixture now repeats terminal split creation,
explicit surface focus changes, terminal closure and child-process cleanup while
preserving the browser surface. It uses the existing local browser mock; it proves
application selection and lifecycle behavior, not rendered Chromium focus or
keyboard delivery. Those require separate real-browser integration coverage.

This exposed a product inconsistency: a browser-only pane could create a terminal
tab but could not split, because split creation required a native terminal to
inherit from. Split and sibling-tab paths now share terminal widget construction,
workspace launch settings and context-menu attachment. Native inheritance remains
optional; browser-only splits use workspace configuration. Local workspace binary
checking passed; executable regression verification remains in GitHub Actions.


### Surface focus selects the requested sibling tab

The socket `surface.focus` handler previously found a surface's containing pane
and focused whichever tab was already selected there. `SplitEngine::focus_surface`
now owns complete selection: resolve the existing notebook page, release model
borrows before GTK callbacks, select that page and focus its owning pane. Missing
identities fail without changing selection. The GTK browser lifecycle fixture
covers alternating terminal/browser siblings, cross-pane selection and missing
identity behavior through the public CLI. Local binary checking and Python syntax
parsing passed; runtime coverage is pending CI.

Actions run 33990901144 failed the strict new-terminal keyboard scenario before
reaching memory churn: the shell PID marker did not match a newly created direct
child. This does not establish an OOM or input-routing root cause. The fixture now
preserves bounded, fixture-owned failure evidence (expected/current PIDs, selected
surface and whether the marker exists with a numeric PID) in its benchmark artifact
and log. The original strict input assertion remains unchanged.


### Workspace-owned native focus restoration

`SplitEngine::focus_active_surface` now resolves its selected terminal through the
pane tree and shares GTK focus routing with `grab_active_focus`. It no longer
scans the process-global raw-widget registry for the first `active-pane` CSS
class, calls GTK/Ghostty while retaining those registry locks, or queues redraws
for every unrelated terminal. Browser selection retains the URL-entry fallback;
only the selected realized terminal receives a render request. The current GTK
pane-close/browser lifecycle fixtures exercise this restoration path. Local
workspace binary checking passed, with existing deprecated X11 bridge warnings.

The separate divider-drag recovery helper still scans global registries and
schedules follow-up rendering; it remains a refactor target requiring resize and
focus regression coverage. This change does not claim to resolve the pending
interactive-tab CI failure or all focus routing.


### Divider recovery belongs to the affected pane subtree

Moved divider gesture discovery and recovery to `src/split_engine/recovery.rs`.
Shared discovery replaces the duplicated Paned/child controller wiring. All GTK
callbacks use local closures and weak divider references. Recovery traverses only
mapped descendant terminal widgets, excluding hidden sibling tabs and unrelated
workspaces, and releases registry lookup locks before native calls. Follow-up
paint passes recheck owner mapping and never retain a detached divider. The global
native mailbox tick remains necessary for Ghostty's asynchronous resize pipeline.
The position-notification fallback still coalesces per idle, not per completed
gesture; this compatibility fallback is not a full drag lifecycle signal.

A new explicit Xvfb CI step exercises mapping, selected-terminal GTK focus and
owner release with pending recovery callbacks. It does not establish native pixel
correctness or real pointer-divider gesture behavior. Local binary and test-target
compilation passed; no tests ran locally. Full runtime verification remains pending.


### Terminal-local resize and unlocked render dispatch

A terminal resize now queues drawing only for its own mapped GLArea after the
synchronous native size update and mailbox tick. Other terminals receive their own
resize signals or targeted native render actions. This removes the per-resize
application-wide widget scan. Native render dispatch now releases the mapping lock
before inspecting or scheduling GTK widgets, avoiding the old nested global locks.
Surface-specific requests allocate no snapshot; application-wide requests copy
registered widget identities before dispatch. Mapping lifetime and GTK-thread
preconditions are documented beside the conversion helper.

Local workspace binary checking and diff validation passed. Existing headless
terminal lifecycle, clipboard and sustained rendering CI scenarios provide runtime
coverage, which is not yet confirmed for this commit. No benchmark improvement or
OOM resolution is claimed from compilation alone.


### Retired redundant global terminal state

Removed the now write-only `GL_AREA_REGISTRY`, its `GtkGLAreaPtr` raw-pointer
wrapper and unsafe Send/Sync implementations. Render and focus dispatch no longer
consume this duplicate list. Also removed the unused `SURFACE_PTR` last-created
terminal global: clipboard completion already belongs to the requesting widget's
native surface cell. Initialization and teardown no longer maintain either global.
The clipboard regression still starts two real terminals and checks that standard
and primary paste reach only the requesting one; its artificial write to the
obsolete global was removed. The live GLArea-to-native mapping and surface metadata
registry retain their distinct routing and diagnostics responsibilities.

Workspace binary and test-target compilation passed. No local tests ran; runtime
clipboard/lifecycle verification remains in GitHub Actions.


### Bounded native event handoff

Replaced separate pending-bit/last-pane atomic pairs for bells and new-terminal
actions with `src/ghostty/events.rs`. The former representation silently overwrote
other panes and repeated terminal requests between GTK timer ticks. The shared
queue preserves accepted event order, coalesces redundant bell attention, bounds
retained events and GTK batch size, and reports overflow through structured
diagnostics. Queue locks are released before model mutation; closed-pane requests
are ignored by existing ownership lookup. Session persistence is scheduled once
per batch that creates terminals.

CI unit cases cover multi-pane bells, repeated terminal requests, FIFO ordering,
capacity rejection, bounded draining and restored admission. Workspace binary and
test-target compilation passed locally; executable verification remains pending.
This refactors existing attention handling, without starting the deferred agent
notification or session-resume feature task.


### Native callback contracts and target decoding

Documented the module boundary and explicit safety contracts for wakeup, action
and close callbacks, including worker-to-GTK handoff, tagged-union validity and
borrowed directory payload lifetime. Action dispatch now decodes the surface target
once and shares deferred pane-action routing between bells and terminal creation.
The close-request log and declaration now accurately state that shell exit leaves
the tab open until explicit teardown, removing the misleading future AppState
promise. This does not implement automatic EOF tab closure or session resume.

Rust formatting, workspace binary compilation and diff checks passed. Runtime
validation remains in GitHub Actions.


### Removed wrapper suites with absent production targets

Deleted `tests/test_open_wrapper.py`, `tests/test_claude_wrapper_hooks.py` and
`tests/test_shell_zdotdir_wrapper.py`. Their targets were exclusively the absent
`Resources/bin/open`, `Resources/bin/claude` and `Resources/shell-integration`
wrappers. The first two cannot load their production target; the zsh suite returned
success after reporting the wrapper missing. No workflow or remaining source
references these suite filenames. No shipped wrapper was removed by this change.
Future agent hook/resume behavior remains in the separate deferred task and must
receive tests against its eventual Linux implementation.

The requirement matrix now reports current recursive AST counts and the verified
failure boundary of run 33991217679. Parsing the retained Python files exposed two
existing invalid-escape warnings in the PR polling fixture; syntax still parses.
No runtime tests were executed locally; diff validation passed.


### Removed missing-target shell integration suites

Deleted five tests that skip missing `Resources/shell-integration` scripts and
return success: PR polling (#1138), shell-integration disablement (#734), bundled
zsh integration precedence, scrollback color replay and minimal-PATH scrollback
replay. The polling harness skips both shells and still prints PASS. None of these
suite filenames is referenced by a remaining workflow or source file. No production
integration or Ghostty submodule content was removed.

Their useful requirements remain distinct from current coverage: native Ghostty
shell configuration must be tested against shipped Ghostty resources; future
session replay needs byte-preserving output and cleanup tests against its actual
Linux implementation. PR metadata polling needs a current implementation and
failure/recovery scenario before any coverage claim. Scrollback/resume work stays
deferred. The maintained prompt-probe cleanup fixture targets an existing script
and remains. Removing the stale PR fixture also removes its invalid Python escape
warnings; no escaping workaround was added to an absent-target test.

Diff validation passed; no runtime tests were run locally.


### Removed absent Claude Teams launcher test group

Removed five `tests/test_cli_claude_teams_*.py` suites and their sole shared
`claude_teams_test_utils.py` helper. The current Rust command schema and dispatch
have no `claude-teams`, `__tmux-compat` or external-subcommand fallback. These tests
only invoke that absent launcher, testing environment injection, help forwarding,
wrapper avoidance, existing shim reuse and tmux teammate sequences. The helper's
fallback also instructed users to run the removed `scripts/reload.sh`. Its only
callers were the deleted suites; no workflow or other source references the group.

No Claude executable, installed user configuration or production API was changed.
If a future agent launcher is selected for the deferred parity work, requirements
include argument/environment forwarding, preserving explicit socket selection,
avoiding recursive wrapper invocation and testing teammate pane operations through
the actual Linux implementation. The deleted upstream suites are not current
coverage of those requirements. Diff validation passed; no local runtime tests ran.


### CI evidence: terminal ancestry and GTK release timing

Run 33991708007 produced the previously missing input evidence: iteration zero
wrote a numeric shell marker (`12081`), while the newly observed direct child was
`12078`. Input therefore executed, but the marker check's direct-child equality
was too restrictive for a launcher plus interactive shell. The fixture now walks
at most 64 live Linux ancestors and requires the marker PID to belong to the new
terminal's process tree. It still sends real X11 keyboard input with no focus
repair or socket-input fallback. CI helper coverage checks real child ancestry,
unrelated roots, invalid identities and reaped children. The new ancestry result
must pass CI before claiming correct routing for the failing scenario.

The same run passed divider mapping/focus checks but failed immediate weak-owner
destruction. The test now closes and releases its GTK window, child snapshots and
notebook references, then drives GTK with a three-second destruction deadline.
This checks eventual release and rejects persistent callback cycles; it does not
claim that no transient GTK reference exists immediately after detachment.
Local Rust test-target compilation, Python syntax parsing and diff checks passed.
No local tests ran; corrected runtime results remain pending.


### Shared Linux child-process snapshots

`tests/process_support.py::linux_child_pids` now owns the per-thread `/proc`
traversal shared by the application lifecycle and memory churn fixtures. It
returns PID strings, tolerates thread disappearance, and propagates other file
read errors. The snapshot remains observational rather than atomic. A CI helper
case launches a real child from a worker thread, verifies discovery and checks
its absence after explicit termination/reaping. This guards against accidentally
switching back to the main thread's children file and missing Ghostty's IO-thread
children. Python syntax parsing and diff checks passed; runtime verification is
left to GitHub Actions.


### Explicit surface split targets

The socket split handler no longer discards its optional surface ID. It uses the
shared pane-tree surface selector before splitting, so a requested sibling tab or
pane supplies the active target in the current workspace. An unknown target
returns `not_found` without creating a pane or changing selection. Omitted targets
retain active-pane behavior. Cross-workspace target switching is not implied.

The terminal lifecycle CI scenario focuses the last pane, splits the first by ID,
and verifies the new surface's traversal position, selected identity and child
creation/cleanup. It also checks unchanged layout and selection after an invalid
ID. Workspace binary checking, Python syntax parsing and diff validation passed;
runtime results remain pending GitHub Actions.


### Shared terminal input and truthful delivery errors

Text and literal-key socket commands now share terminal resolution and native
input delivery. Missing, unrealized or nonterminal targets return `not_found`,
embedded NUL text returns `invalid_params`, and unsupported named key combinations
return `not_supported` instead of silent success. A single Unicode scalar is
accepted by the literal-key path; the old byte-length check discarded non-ASCII
characters. CLI help describes this implemented scope. Native calls run after
releasing the model borrow and retain their input allocation through completion.
The GTK fixture checks invalid-target and unsupported-key failure without selection
changes. Local binary compilation, Python parsing and diff checks passed.

`surface.read_text` remains an empty-result stub, despite native APIs existing in
the current Ghostty header. Replacing it needs bounded extraction and executable
read/focus coverage. Full named-key dispatch also remains unfinished; explicit
rejection is not claimed as implementation of that capability.


### Bounded terminal viewport reads replace the empty-result stub

`surface.read_text` now resolves the requested terminal without changing focus and
calls `src/ghostty/text.rs` for viewport extraction. It uses the existing native
scrollbar and byte-bounded clipboard-text APIs, preserving selection and scroll
position. Native formatting limits output to 256 KiB and selected-cell work to
65,536 cells; oversized or failed captures return `read_failed`, not empty success.
A guard frees every successful native allocation, including validation failures.
Worst-case JSON escaping remains under the socket's four-MiB response limit.
Output uses Ghostty clipboard trimming/codepoint settings and contains plain text.
The separate scrollbar/read calls are a best-effort snapshot under concurrent
output, not an atomic terminal checkpoint. No Ghostty submodule change was needed.

Shared terminal resolution now serves text input and reads. The GTK CI fixture
prints a marker through targeted input, reads it from an unfocused terminal,
checks absence in another terminal, preserves selection and rejects a missing
read target. CLI help documents viewport scope. Local binary/test-target checking,
Python syntax parsing and diff validation passed; native runtime results remain
pending CI. This is current terminal inspection, not deferred session replay.


### Terminal identity for refresh and availability

Refresh now resolves an explicit surface UUID to that terminal's GLArea instead
of refreshing the selected sibling in its containing pane. Native pointer lookup
shares this widget resolver. An unknown/nonterminal refresh target returns
`not_found`; no-target refresh uses the model's active pane directly. Health with
an omitted target now checks for a native terminal just like explicit-target
health, rather than unconditionally returning alive when a workspace exists.
CLI help describes native terminal availability: this is not shell-process or
external-browser health. Pane attention remains a separate returned property.

The GTK fixture exercises unfocused refresh, known/missing terminal availability,
unknown refresh rejection and unchanged selection across these operations. Local
binary/test-target compilation, Python syntax parsing and diff validation passed;
runtime verification remains pending CI. Pixel-level proof for refreshing a
hidden sibling is still broader than this protocol regression's assertions.


### Pane listings preserve the product hierarchy

`pane.list` now returns one row per split pane instead of aliasing the surface
list. Pane snapshots group ordered surface IDs and the notebook's selected tab.
The new `id` is `pane:N`, valid for the current application lifetime; the legacy
`uuid` remains the selected surface UUID. `pane.focus` accepts the new reference
or a legacy surface UUID and preserves the selected tab. CLI formatting displays
full pane references and the focused marker, without byte-slicing Unicode IDs.

The existing GTK mixed-tab fixture checks one pane with two surfaces, stable pane
identity across tab changes and pane focus preserving the selected tab. Local
binary/test-target compilation, Python parsing and diff validation passed; runtime
verification remains pending. Session-persistent pane IDs are not implied.

Published `.agent/api/cmux-terminal-commands.yaml` after validation as gateway
Documentation `01a07372-b5a2-7890-a7af-751bd3b5391a`. It records the current pane,
input, read, health and refresh contracts and remaining named-key/browser-health
limitations. Publication is complete for this API context change.


### CLI identity formatting matches protocol records

Pane and surface lists now share a documented identity formatter. It reads the
current active/UUID fields with legacy focused/ID aliases, prefers explicit active
state, and displays full identities for reuse in commands. This fixes surface
list output showing `unknown` and omitting active markers when consuming the
actual GTK server response. It also removes unsafe byte-index abbreviation for
non-ASCII legacy IDs. JSON output remains the original structured response.

CI unit cases cover current surface records, full UUIDs, ANSI selection markers,
JSON preservation, long pane references, Unicode IDs and empty lists. The GTK
terminal fixture checks human-readable output against its real JSON identities.
Rust formatting, binary/test-target compilation, Python syntax and diff checks
passed locally; runtime results remain pending CI.


### Workspace switches share selected-surface focus ownership

Removed the sidebar's extra global-registry focus assignment after workspace
selection. `AppState::switch_to_index` now calls the pane tree's shared GTK/native
focus restoration for every caller. Selected-surface lookup no longer falls back
to an arbitrary hidden terminal when a browser tab is selected. The registry's
now-unused first-terminal query and a duplicate recursive native lookup were
removed; explicit terminal inheritance still uses its separate intentional lookup.

The mixed-tab GTK fixture switches through a temporary workspace while a browser
is selected, verifies browser selection on return, then cleans up the temporary
workspace. It does not directly inspect Ghostty's private focused flag. Local
binary/test-target compilation, Python parsing and diff checks passed; runtime
results remain pending CI.


### Workspace metadata shares real layout counts

Workspace listing and current-workspace responses now use one record builder,
including compatible id/uuid and title/name aliases. Counts derive from pane
snapshots: two sibling tabs in one pane count as one pane and two surfaces. Missing
engine metadata is null rather than a fabricated zero, and CLI formatting labels
that state unavailable. This fixes current-workspace output showing an unknown
identity and list-workspaces reporting zero panes despite a populated layout.

The mixed-tab GTK fixture verifies hierarchy counts, naming/identity aliases and
human-readable CLI output. A formatter case covers known and unavailable counts.
Local binary/test-target compilation, Python syntax and diff checks passed.
Updated API context was validated and republished to gateway Documentation
`01a07372-b5a2-7890-a7af-751bd3b5391a`.

CI run 33992451759 at e08b976f confirms successful divider lifetime, terminal churn
(including strict keyboard ancestry, EOF and sustained rendering), and SSH/script
restore scenarios. Optimized benchmark build was still running when inspected.
This verifies the earlier fixture corrections, not changes after that commit or
the full refactor objective.


### Terminal adapter ownership and verified baseline artifacts

Literal input conversion and native delivery now live in `ghostty/text.rs` beside
bounded viewport reads, with explicit pointer-lifetime contracts. Socket handling
keeps target resolution and protocol error translation. This extraction preserves
input behavior; local binary/test-target compilation and diff checks passed.

Run 33992451759 at e08b976f completed successfully, including the optimized ping
benchmark. Raw ping and memory-churn JSON artifacts are preserved under
`Docs/Benchmarks/e08b976f`, with provenance and scope limits in its README. Release
ping measured median 1.657496 ms and p95 1.737285 ms. Debug post-warmup churn RSS
samples went from 354636 to 360820 KiB, and the sampled redraw interval from 376200
to 376228 KiB. These are workload-specific baselines, not universal leak-freedom
or verification of later commits. No local runtime tests were executed.


### Guarded CLI benchmark comparison

Added `scripts/compare-cmux-benchmarks.py` for completed CLI ping evidence. File
size, sample validity, workload completion and recorded environment compatibility
are checked before recalculating latency summaries. Output includes revisions,
matched settings and delta/percentage values without claiming significance or
inventing thresholds. CI tests use the preserved e08b976f report, simulate slower
raw samples with stale summaries, and reject partial, mismatched and nonfinite
inputs. Added the test to the existing Python tooling CI step. Local Python syntax
and diff checks passed; no tests or benchmark workloads ran locally.

Comparison of memory, browser, remote and other workloads remains incomplete,
as does richer hardware/renderer metadata for fully controlled comparisons.

### Typed input versus paste (2026-09-05)

Actions run 33993016898 failed the unfocused terminal read scenario: the command was delivered through Ghostty's paste API, including its attempted Enter. Native API inspection confirms that this path uses bracketed paste, while ghostty_surface_text_input delivers committed typed input. The one-character send-key implementation now uses a documented typed-input adapter, preserving target resolution and focus. The CI scenario pastes a command with a literal escaped newline format and submits a separate carriage return through send-key. Named-key translation remains unfinished. Cargo check and git diff --check pass; cargo fmt --all -- --check exposes existing formatting drift in multiple owned modules, which remains to be normalized. Executable verification is delegated to Actions. The comparison-tool run 33993392449 was still running when inspected; no cumulative green result is claimed.

### Rust formatting normalization (2026-09-05)

Applied workspace rustfmt to the five owned modules with accumulated formatting drift: main, socket commands/handlers, split engine and SSH dialog. No Ghostty files or user image changes were staged. Workspace formatting and whitespace checks now pass, as does cargo check --workspace --bins (existing deprecated X11 API warnings remain). No local executable tests were run. Actions run 33993392449 remains live; typed-input follow-up 33993716499 was pending when inspected. The full legacy/documentation/architecture audit and cumulative runtime verification remain incomplete.

### Benchmark evidence validation (2026-09-05)

Strengthened the CLI comparison boundary: schema and completion counts reject boolean aliases; metadata must describe a host, workload and stable positive application PID; runtime build/backend/GTK metadata and terminal counts must remain stable within each report. Warmup and terminal counts require nonnegative integers. Non-finite derived metrics and serialization errors now fail through the CLI error path before stdout output. Added executable tests for equally malformed report pairs, partial completion aliases, overflow and oversized report files. Python AST parsing and git diff --check pass; tests remain CI-only. Run 33993392449 is confirmed live at workspace checking, with its Go job successful; newer input/formatting and comparison changes are not yet cumulatively verified.

### Inherited test and status cleanup (2026-09-05)

Removed tests/test_shell_histfile_ghostty_zdotdir_regression.py: its only application target is the absent Resources/shell-integration/.zshenv wrapper, and absence returns success before exercising anything. The owned Rust implementation does not inject CMUX_ORIGINAL_ZDOTDIR; Ghostty remains a complete submodule. User shell-history preservation remains a valid launch requirement, but this stacked macOS wrapper scenario did not test Linux behavior.

Removed tests_v2/test_browser_api_unsupported_matrix.py: it asserts WKWebView-specific not_supported responses for viewport, geolocation, network, tracing and input methods, which is the wrong contract for the external agent-browser backend. Its broader capability list remains in the historical browser port specification and retained browser suites; accurate discovery and explicit unsupported errors remain requirements for the deferred parity audit. No Linux CI workflow invoked either removed suite.

Replaced root PROJECTS.md and TODO.md inherited Sparkle/Swift/Bonsplit completion claims with current documentation entry points and explicitly scoped remaining work. The original log/checklist remain in Git history; the detailed browser port specification is now clearly marked historical. Retained Linux-relevant themes include shell history, background-agent targeting, identity/focus semantics, remote/browser lifetimes and visible error handling. No session-resume or parity feature implementation was started. Whitespace validation passes; this removal/documentation checkpoint needs no new runtime tests and does not establish full legacy-audit completion.
