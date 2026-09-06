# Components

These boundaries describe the current implementation. Requirement and verification evidence is tracked in [RefactorAudit](RefactorAudit.md).

| Component | Responsibility | Current source |
| --- | --- | --- |
| Desktop composition | Construct GTK application, connect events, own application lifetime | `src/main.rs`, header/menu/dialog modules |
| Workspace model | Workspace identity, launch settings, order and attention | `src/workspace.rs`, `src/app_state.rs` |
| Pane tree | Splits, sibling tabs, selection and surface lifecycle; saved layouts rebuild through a dedicated GTK restoration module | `src/split_engine.rs`, `src/split_engine/{restore,recovery}.rs` |
| Terminal adapter | Ghostty configuration, input, clipboard and rendering through its C ABI; surface registry owns routing and last-known directories through teardown; events.rs bounds deferred native actions; text.rs owns paste/typed input, non-selecting viewport capture and native buffer release; inherited.rs owns allocated split/tab configuration directories | `src/ghostty` |
| Browser adapter | Optional browser daemon, commands, previews and navigation; ui.rs owns GTK tab wiring and action orchestration | `src/browser.rs`, `src/browser/{ui,input,input_queue,mapped,cli,discovery,transport,frames,pixels,stream,motion,metrics}.rs` |
| Command interface | CLI argument schema, discovery, transport and output; completion generation imports the schema alone | `src/cli`; Python clients share `scripts/cmux_socket_discovery.py` and `scripts/cmux_socket_transport.py` |
| Socket service | Listener/connection lifetimes, peer admission, worker validation, bounded GTK command bridge and correlated responses | `src/socket/{mod,admission,auth,framing,dispatch,commands,handlers,response}.rs`; dispatcher scenarios in `dispatch_tests.rs` |
| Diagnostics | Bounded structured logs, request correlation, resource snapshots, collection and optimized benchmarks | `src/diagnostics`, `src/diagnostics.rs`, `scripts/collect-cmux-diagnostics.py`, `scripts/benchmark-cmux.py`, `scripts/compare-cmux-benchmarks.py` |
| Session/configuration | Serializable state, compatibility, shared immutable snapshots, coalesced serialized writer, buffered session serialization/loading with strict UTF-8 validation and atomic writes | `src/session.rs`, `src/config.rs`, `src/ssh_hosts.rs` |
| Remote transport | SSH deployment, reconnect and stream ownership; Go PTY adapter owns native child launch, I/O, resize and teardown; session module manages attachment-size metadata; relay transport owns authentication, bounded response framing and socket deadlines | `src/ssh`, `daemon/remote/cmd/cmuxd-remote/{main,params,pty,sessions,streams,relay_transport}.go` |
| Linux services | XDG locations, control-socket discovery with bounded markers and one-candidate debug scanning, OS-native PATH candidate discovery, bounded UTF-8 metadata reads, atomic file replacement, private directory/file and release-executable permissions, peer credentials, bounded synchronous local connection retries and I/O setup, process resources, installation ownership, desktop notifications and optional GTK/X11 placement and OpenGL callbacks | `crates/cmux-platform` |
| Distribution | Dependency setup, linking, packages and release publishing; scripts/release-version.sh owns literal root-package version lookup | `build.rs`, `scripts`, `packaging`, `.github/workflows` |

The Linux library exposes small typed functions, with no dependency on workspace state. CLI use must not require GTK initialization. GTK-specific services can be an optional feature. Keep Unix transport and Linux kernel details at this boundary. A future platform provides equivalent services; the current refactor does not implement unsupported platforms.

Share repeated behavior at its owning component. UI and socket callers should invoke the same workspace operations; configuration files should use the same path and atomic-write helpers; credential tests must call the production authentication function. Do not duplicate those rules in adapters or tests.

`src/bounded_json.rs` owns size-limited JSON-line encoding shared by CLI requests, socket responses and diagnostic records. It has no GTK or platform dependency; each caller supplies its own byte budget.

Ordered selection after removal is shared by workspace rows and sibling tabs in `src/selection.rs`: preserve the surviving selected identity, otherwise select the replacement at the same slot and fall back at the end. The helper is independent of GTK; callers own widget/model synchronization.

Socket startup receives a runtime handle and command sender, not application state. Worker validation and typed command construction live in `dispatch.rs`; `handlers.rs` owns GTK/model mutation. Both use `response.rs` to preserve the common success/error envelope and request identity. Command-specific fields are added before transport serialization.

`src/task.rs` owns the abort-on-drop companion-task guard shared by SSH tunnel operations and browser result delivery. Aborting requests cancellation; paths requiring cleanup completion still await their task. Browser widget guards also disconnect weak GTK notifications when result delivery is abandoned.

`src/line_reader.rs` owns asynchronous UTF-8 line framing shared by local socket admission and SSH daemon stdout: idle waiting is separate from a started-frame deadline and caller-selected byte cap. Consumers retire failed readers because partial bytes have been consumed. SSH stderr is a separate fixed-buffer drain with a per-connection diagnostic prefix cap and connection-owned cancellation.

`src/task.rs` owns companion-task abort guards and the shared direct-child reap policy. SSH requests a normal-exit grace period after routing ends; failed browser CLI operations request immediate termination. Launchers retain kill-on-drop ownership, while the helper bounds the reap wait and returns status/forced-termination evidence.

`src/ssh/writer.rs` serializes every SSH request (hello, PTY creation/subscription and terminal input/control) through one deadline-aware writer. A persistent retirement signal ties partial-write failure or cancellation to the routing lifetime. The adapter caps encoded frames with the existing bounded JSON serializer; no second writer actor or queue is introduced.

`src/ssh/handshake.rs` validates daemon identity and required capabilities under one hello-exchange deadline before terminal routing starts. The same buffered reader passes into routing so prefetched response/event bytes are not discarded.

SSH session creation and subscription share `request_remote` for response-slot registration before writing, bounded reply waiting, success/identity validation and cancellation cleanup. Known remote stream closure requests share `SshBridge::request_close`, including failed subscription setup.

`src/ssh/outbound.rs` owns bounded GTK-to-SSH admission: a 64-slot FIFO, 16-KiB raw input chunks, whole-callback slot reservation and persistent queue failure. Callback producers never wait for transport. A new sender/receiver generation is published only after old terminal stream targets are cleared.

`cmux-platform::local_socket` exports the concrete Linux blocking stream and optional Tokio stream/listener types, along with bounded blocking connection setup. CLI and browser/control socket callers import their native transports from this component. The `async-io` feature enables Tokio networking independently of GTK; these exports add no wrapper state, buffering or alternate protocol. Framing, admission, authentication policy and application connection lifetimes remain with their existing owners. A future platform still needs an equivalent transport implementation; the type exports do not create Windows support. Direct libc use is confined to the platform crate, so the desktop package no longer declares an unused libc dependency.


`src/notification.rs` owns desktop-delivery admission and diagnostics: at most four active tasks, no waiting queue, five-second execution and shared five-second kill/reap cleanup. The platform library only constructs the `notify-send` command. GTK owns attention and the existing per-workspace five-second rate limit; overload skips desktop delivery without clearing attention. Runtime shutdown cancels remaining commands through kill-on-drop. Process success does not prove desktop presentation.


`src/task.rs::run_output` centralizes concurrent stdout/stderr draining, byte budgets, execution deadlines and kill/reap cleanup for browser CLI commands and updater version preflight. Callers retain their own protocol decoding and cleanup diagnostics. The CLI binary compiles this GTK-independent helper directly; the completion generator still compiles only the argument schema.

Direct-binary updates stream downloads to the private staging directory while hashing a 64-KiB buffer, verify SHA-256 before extraction, and then reuse existing staged executable checks and ordered replacement. Release JSON is capped at one MiB and checksum metadata at four KiB. Version preflight has four-KiB stdout/stderr caps and five-second execution plus existing five-second cleanup. Archive disk usage still scales with the release; this is bounded-memory streaming, not an archive-size or decompression-time quota. Package-owned installation policy is unchanged.


`src/browser_timeout.rs` shares explicit wait budgets between the CLI and browser transport, keeping response margins outside the requested browser wait. Ordinary exchange deadlines and startup CLI allowance remain explicit at their callers.

SSH task setup for RPC, dialog and restore shares `AppState::start_ssh`. The helper retains the bridge, records launch identity and schedules the owned lifecycle handle; every reconnect attempt keeps the original launch parent. Model creation and UI row wiring remain with their callers.

The maintained Python JSON client now exposes bounded raw method calls and basic system queries. The prompt diagnostic and stdio/protocol fixtures supply current wire fields explicitly; unused upstream window, workspace, browser, notification and metadata convenience wrappers have been removed. The minimal older line client remains solely for discovery/transport compatibility checks and raw manual forwarding, not as a GTK command catalog. Both share socket discovery and bounded transport helpers.
