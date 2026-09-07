# Upstream capability parity

Active goal, started September 6, 2026. This replaces the earlier decision to defer the capability work; it does not reopen the completed structural refactor. The existing [monthly review](../docs/research/upstream-2026-09.md) supplies release chronology. GitHub still reports [v0.64.22](https://github.com/manaflow-ai/cmux/releases/tag/v0.64.22), published August 3, as latest stable. Current-main capability discovery is pinned to [e36b8e8632a414e2982185f8dae4002a98be2b53](https://github.com/manaflow-ai/cmux/tree/e36b8e8632a414e2982185f8dae4002a98be2b53); main-only capabilities must not be represented as shipped in an earlier month.

## Delivery and evidence matrix

Every row requires implementation and executable CI evidence before completion. Existing functionality is a starting point, not proof of complete parity. Linux adaptations preserve useful behavior through GTK and the platform library. Native macOS implementation details are not copied. No release tag precedes local user validation.

| Period / source | Capability | Required work / acceptance | Status / authoritative evidence |
| --- | --- | --- | --- |
| March | SSH workspaces, automatic forwarding | Discover and forward remote listeners; browser localhost/subresource requests must originate through the selected remote workspace; validate split/tab inheritance and route changes. | **Implemented; cumulative verification pending.** See [remote listener discovery](#remote-pty-listener-discovery), [remote browser launch](#remote-browser-launch-integration), [same-port isolation](#same-port-remote-workspace-isolation-coverage), and [reconnect](#remote-browser-reconnect-scenario). |
| March / May | cmux.json project actions and launch configuration | Project configuration schema, command palette/actions and CLI/socket configuration; explicit one-time remote initial command and script semantics. | **Implemented; cumulative verification pending.** See [resolution](#project-configuration-resolution-foundation), [workspace execution](#default-project-workspace-execution), [custom layouts](#custom-project-workspace-layouts), and [reviewed palette](#project-command-palette-and-confirmation). |
| April | Listening ports | Attribute listeners to owned terminal descendants, clear on exit, distinguish local/remote provenance; expose sidebar and API. | **Implemented; cumulative verification pending.** See [local attribution](#local-workspace-listening-ports), [remote discovery](#remote-pty-listener-discovery), and [retirement coverage](#forwarded-service-retirement-coverage). |
| May | Reliable session / agent restoration | Final snapshot before teardown, serialized durable quit save, immediate mutation/quit/reopen test; bounded scrollback across untouched background workspaces; manual prior-session restore; per-surface resume bindings and automatic/manual controls. | **Implemented and previously verified.** See [final-save checkpoint](#first-implementation-checkpoint), [scrollback replay](#session-scrollback-replay-checkpoint), [previous-session recovery](#previous-session-startup-recovery-checkpoint), and [approval](#exact-command-approval-checkpoint). |
| May / current catalog | Agent hooks | Native IDs, environment-aware resume and setup integration for every advertised agent; custom bindings, stale protection, notification routing, lifecycle traces and restore benchmark. | **Partial; Actions pending for newest providers.** Claude/Codex/shared JSON/OpenCode/Cursor/Pi/Amp/Rovo checkpoints cover the pinned 13. OMP, Campfire, Kiro and Antigravity are implemented below. Current-upstream Hermes Agent and Kimi remain. |
| May | Copy-on-select / workspace order and colors | Copy-on-select, PRIMARY middle-click paste, generic copy/paste shortcuts, batch reorder/configuration and keyboard identity/focus controls. | **Verified.** Existing native clipboard and workspace scenarios passed cumulative [Actions run 33951541718](https://github.com/nitecon/cmux-gtk/actions/runs/33951541718); later [batch order](#batch-workspace-ordering-checkpoint) preserves the same identity contract. |
| May–June / README | Notification inbox / highlighting | Per-surface inbox, OSC9/99/777 including chunk completion, notify CLI/socket, read/clear/dismiss, unread navigation, focused suppression and exact click targeting; retained history and bounded data. | **Verified through the core cumulative checkpoint.** See [inbox](#notification-inbox-protocol-checkpoint), [OSC](#native-osc-delivery-checkpoint), [desktop actions](#desktop-message-delivery-checkpoint), and [Actions run 34048253282](https://github.com/nitecon/cmux-gtk/actions/runs/34048253282). |
| February baseline / June | Sidebar metadata and project/diff views | Branch/PR/MR, status/progress/markdown metadata, description and latest notification; automatic Git updates during foreground work; project/diff views and API with bounded worker I/O. | **Partial; diff implementation awaiting Actions.** Metadata, markdown, Git polling, ports and latest notification are implemented ([metadata](#agent-sidebar-metadata-checkpoint), [Git provenance](#git-tracking-and-provenance-checkpoint)). Patch/unstaged/staged/branch and per-agent-turn diff surfaces now have bounded self-contained browser views and native placement ([diff surfaces](#agent-accessible-diff-surface-checkpoint)); durable review comments and the dedicated project view remain. |
| June | Collapsible workspace groups | Persistent group membership/order/color/collapse, unread aggregation, keyboard/CLI/UI operations preserving workspace identity. | **Implemented; cumulative verification pending.** See [persistent workspace groups](#persistent-workspace-groups). |
| July | Workspace/surface reorder shortcuts | Batch and keyboard reorder, pane/surface move and drag topology with correct ownership, focus and remote routing. | **Partial; implementation awaiting Actions.** Workspace moves, stable tab reorder, cross-pane/cross-workspace transfer, browser route rebinding, pane-center/edge and sidebar pointer drops, and placeholder-free directional split now have native boundaries and fixtures ([workspace moves](#workspace-move-shortcuts-checkpoint), [surface topology](#surface-move-and-drag-topology-checkpoint)). Direct pointer interaction evidence and final-surface remote bridge transfer remain. |
| July–August | Mosh / remote resume | Implement resilient remote transport and remote resume; explicitly verify replacement/EOF semantics. | **Partial.** Mosh, Mosh-tmux, capability probing, fallback and persistence are implemented ([Mosh transport](#mosh-interactive-transport)); remote agent resume execution and final EOF policy evidence remain. |
| Current README / preserved contracts | Agent browser access | Complete target matrix; nearest-right opening, local files, independent state, history, screenshots, remote routing and profiles with bounded ownership. | **Implemented; cumulative profile verification pending.** See [local/history](#real-local-document-and-history-verification-checkpoint), [independent ownership](#independent-browser-surfaces--implementation-awaiting-actions), [remote origin](#isolated-browser-origin-actions-coverage), and [profiles](#browser-profile-reuse). |
| Current README | Agent teamwork | Native teammate split workflow, metadata and notification wiring without an unrelated terminal multiplexer dependency. | **Implemented; cumulative verification pending.** See [native Claude Code teams](#native-claude-code-teams). |
| Current README | Window/session fidelity | Audit multi-window controls and restore semantics; implement applicable behavior and native visual lifecycle tests. | **Open.** One-window geometry and topology are durable; upstream multi-window creation/routing/restore contracts still require a GTK design and executable evidence. |
| July–August research | Mobile/Iroh/TUI | Assess public contracts and Linux applicability explicitly; record desktop-side requirements rather than silently dropping platform-specific UI. | **Assessed.** [Platform applicability](PlatformApplicability.md) excludes Apple UI while preserving the authenticated Linux-host and protocol migration requirements. No unsupported compatibility is claimed. |

## Implementation order

1. Session final-save boundary, then persisted resume bindings/hooks and restore fidelity.
2. Agent notification/highlight and sidebar metadata primitives with CLI/socket parity.
3. Browser targeting/navigation and remote discovery/forwarding.
4. Project actions, groups/reordering, remaining remote/agent workflows and platform adaptations.
5. Full contract audit, optimized restore/resume and feature workloads, cumulative CI and local handoff.

The saved gateway task contains specific behavior contracts recovered during legacy cleanup; [RefactorAudit](RefactorAudit.md) retains their provenance. Restore tests must include at least three workspaces, an initially selected middle workspace, and repeated restarts without activating background terminals. No process checkpointing is claimed for arbitrary programs: persisted agent IDs and explicit resume commands recreate supported durable sessions.

## First implementation checkpoint

Normal close-request and application shutdown now freeze the final live layout, preventing teardown callbacks from publishing a degraded replacement. A finish signal interrupts the 500-ms debounce; the same worker completes older writes before saving and syncing the latest snapshot and parent directory. The composition root joins it before runtime shutdown. Save failures remain explicit. This does not checkpoint processes or make forced termination durable. Unit failure/coalescing cases and native immediate-quit/reopen coverage are added to CI; runtime verification is pending.

## Resume metadata checkpoint

CLI/socket manual per-terminal `surface.resume.set/show/clear` supports exact UUID targeting across workspaces, bounded command/environment data and checkpoint-conditional clearing. Pane snapshots persist the binding; sibling/split creation does not copy it. Registration does not execute commands. Automatic resume and provider hooks remain incomplete and automatic requests fail explicitly until that policy is implemented. The native quit/restart scenario verifies binding persistence, stale-checkpoint rejection and nonexecution while preserving selection; runtime verification is pending.

Contract research used pinned upstream `ControlSurfaceResumeSetInputs.swift`, `ControlSurfaceResumeBinding.swift` and `WorkspaceSurfaceResumeBinding.swift`. This checkpoint is additive progress, not complete upstream resume compatibility.

## Manual resume execution checkpoint

`cmux restore [--checkpoint ID]` now launches a saved command by replacing the calling CLI inside its owning local terminal. The CLI requires matching terminal identity and a TTY, validates checkpoint/location and applies literal environment/cwd values through a prepared process command. Remote bindings fail on this local path; automatic resume and remote execution still require implementation. Ghostty receives each local terminal's actual surface UUID and socket discovery values before realization, including restored and sibling/split terminals. Registration alone still never executes. Secret-like environment overrides and CMUX routing overrides are removed before storage and launch; explicit null environments are accepted. Native execution and sanitization scenarios are added to Actions; runtime evidence is pending.

## First provider hook checkpoint

Claude SessionStart/SessionEnd installation and ingestion are implemented against the [provider hook reference](https://code.claude.com/docs/en/hooks). Setup merges only owned handlers while preserving existing configuration and synchronizes the atomic replacement. It rejects malformed/oversized/symlink settings rather than replacing them. Native payloads are bounded and validate event/session/surface context; the resume command preserves the exact native ID, working directory and known configuration directories. Conditional end cleanup cannot remove another checkpoint. Outside-cmux implicit hooks skip; explicit socket failures remain errors.

CI executes generated handlers against GTK with a provider discovery/argv shim and checks repeat installation, malformed-config preservation, session capture, literal argv and conditional cleanup. This is executable protocol integration, not a live external Claude session. Other providers, per-turn notifications and automatic trust/approval remain required. Quit-save source e3dd562a passed full Actions 34043857902; later resume/hook changes await cumulative verification.

## Exact-command approval checkpoint

Preferences (Ctrl+,) now displays the selected local terminal's command, absolute directory and environment overrides. Explicit approval signs those exact execution inputs using HMAC-SHA256; the private key stays outside the session snapshot. The existing serialized session writer saves up to 128 approvals. Altered or unauthenticated records cannot authorize execution. Changing the binding invalidates its match; revoking approvals preserves manual bindings.

Restored local terminals schedule approved commands through `cmux restore --automatic`, which rechecks the current binding's approval before replacing itself. After command exit or failure, the restored terminal opens an interactive shell. Registration and approval do not execute in a live terminal. Remote launches retain their existing transport and cannot use this local path. This is exact-command approval, not yet upstream command-prefix policy or trusted-provider auto-resume. Those requirements, per-provider toggles and detailed launch/restore benchmarks remain open.

Actions coverage includes UI approval, native PTY restart with literal launch context, changed-input rejection, revocation across restart, signed-record tampering and private-key file validation. Local compile/lint checks pass; runtime evidence for this checkpoint is pending. Manual resume through source 10d47083 passed full Actions 34044712611.

## Literal prefix approval checkpoint

The review panel now also accepts an editable command prefix made of complete initial shell arguments. It signs the prefix and policy mode along with the reviewed command, exact directory and environment. A changed native session ID can match that prefix; a changed executable, directory or environment cannot. Prefix matching rejects shell expansion, redirection, pipelines, command separators and malformed quoting. Literal quoted arguments and empty arguments remain distinct. Exact approvals retain their prior signatures and can still cover explicitly reviewed shell scripts.

The contract follows the pinned upstream `SurfaceResumeApprovalRecord` and `SurfaceResumeCommandCanonicalizer` in [SessionPersistence.swift](https://github.com/manaflow-ai/cmux/blob/e36b8e8632a414e2982185f8dae4002a98be2b53/Sources/SessionPersistence.swift). Tests cover signed-prefix integrity and restart with a new native ID through GTK review. Runtime evidence is pending. Trusted-provider evidence, per-agent auto-resume controls and a full saved-approval editor remain outstanding; prefix support alone does not complete agent restoration.

## Notification inbox protocol checkpoint

`cmux notify` now creates a bounded message with stable notification, workspace and surface identities. `cmux notifications` exposes list, clear, mark-read, dismiss, open and jump-to-unread. Socket aliases support explicit surface/workspace delivery; invalid or conflicting targets never fall back to the selected terminal. Ordinary delivery and history mutations preserve focus; only open/jump select the message's exact terminal. Focused-terminal delivery records a read message.

The session snapshot retains up to 256 messages and 1 MiB of accounted content/identity data, evicting read history before the oldest unread message. Titles, subtitles and bodies have separate byte limits. Unread records drive pane rings and sidebar attention separately from terminal BEL state. The protocol `notifications` array now contains message records; the prior workspace attention rows remain under `workspace_attention`, with the optimized BEL benchmark adapted to that field. Creation diagnostics record identity, focused suppression and eviction count without message text.

Native CI covers background-pane delivery, exact navigation, read/dismiss behavior, malformed/stale/conflicting targets, normal-quit persistence and flood retention. Runtime verification is pending. The live GTK inbox panel, OSC9/99/777 payload/chunk handling, per-turn agent hook delivery, desktop message content/click routing and complete caller-target resolution remain outstanding. Protocol research uses pinned upstream `ControlCommandCoordinator+Notification.swift`; this checkpoint does not claim the full notification row complete. Claude hook source 044d5d85 passed full Actions 34045159272.

## Live inbox panel checkpoint

Notifications is available in the View menu and through Ctrl+Shift+I; Ctrl+Shift+U jumps to the latest unread target. The nonmodal panel updates its unread count and retained messages through a coalesced watch signal, with no polling timer or unbounded update queue. Its listener and action handlers hold weak model/widget references; close aborts the listener. Plain-text message rows support exact-terminal opening and dismissal, with bulk mark-read and clear controls. Successful navigation closes the panel and returns to the target terminal. CI exercises live arrival/removal counts and panel navigation; runtime verification is pending.

Approval run 34046349555 exposed a test readiness race on the third restart: text was read before Ghostty initialization after its first resize. The fixture now waits for surface health before reading. The earlier exact approval/restart/revocation stages passed in that run; prefix verification had not yet executed, so cumulative approval evidence remains pending.

## Native OSC delivery checkpoint

Each terminal now owns a bounded parser on the existing native raw-output callback, installed before the IO thread starts. This path covers local PTYs and manually supplied remote output before Ghostty's desktop-only truncation/rate limits. OSC9 and OSC777 notifications, plus OSC99 title/body chunks with native IDs, completion and base64 fields, enter the same bounded inbox event queue. Completed messages retain the originating sibling surface UUID; the native presentation callback is acknowledged without duplicate delivery.

Framing survives arbitrary read boundaries, ignores unrelated control strings and ConEmu operations, caps a frame at 16 KiB, and retains at most eight incomplete messages per terminal with 60-second idle expiry. Oversized/discarded chunks cannot complete an earlier partial message. Normal notification field limits still apply. Terminal teardown frees the parser only after Ghostty has joined its IO thread. Diagnostics expose observed output bytes, parser time, accepted/rejected queue events and oversized frames without terminal content.

Native CI emits rapid OSC9/777 messages and fragmented OSC99 from an inactive sibling, checks full body preservation, incomplete-message suppression, oversized-frame recovery and stable focus. Runtime evidence is pending. OSC99 query/replacement/activation-report semantics and desktop click delivery remain outstanding; the implemented subset follows the [kitty notification protocol](https://sw.kovidgoyal.net/kitty/desktop-notifications/).

## Claude turn notification checkpoint

The hook installer now merges Stop and Notification handlers as well as SessionStart/SessionEnd. Stop forwards the provider's final response text; Notification forwards its title, message and notification type. Both require the native event/session/surface context, route through the shared inbox API, and preserve the resume binding and current focus. Long messages use visible UTF-8-safe truncation within inbox limits. The integration follows the [Claude hook reference](https://code.claude.com/docs/en/hooks) and does not read transcript files or make hook decisions that continue/block the agent.

The installed-handler CI scenario now checks background notification routing, literal response text, Unicode truncation, unchanged focus and unchanged resume metadata. Runtime verification remains pending. This completes the first provider's turn-to-inbox wiring in source; other providers and the remaining resume/notification policy requirements stay on the matrix.


## Desktop message delivery checkpoint

Background messages now send escaped plain content through the bounded Linux notify-send service adapter. Its default action returns to the original inbox message and exact sibling terminal; dismissed or closed targets cannot redirect focus. Focused delivery remains suppressed, and desktop failure or overload preserves inbox history. Four helpers share admission with BEL delivery; actionable helpers have a 15-second deadline and bounded captured output.

Actions 34047279827 passed the complete approval and live-inbox suite at ab9bbb07, including optimized benchmarks. Native OSC and Claude turn hooks await the subsequent cumulative run. The new desktop fixture exercises actual GTK routing with a controlled notify-send executable: escaped payload, background focus preservation, exact sibling activation and stale-message rejection. It validates the helper contract, not a physical desktop daemon. Desktop runtime verification is pending; advanced OSC99, caller resolution and other matrix rows remain open.


## Caller attribution checkpoint

`notification.create_for_caller` and `notification.clear` with `caller=true` now share upstream-style attribution: stable surface identity, hard explicit workspace scope, ambient workspace rehoming and unique current local native TTY evidence. Caller failures do not borrow the focused terminal. A known workspace with no proven terminal produces a workspace-only record (`surface_id: null`), retained across quit; opening it selects the workspace and preserves its selected sibling. Workspace-only records contribute sidebar unread attention but no arbitrary pane ring.

The existing Ghostty TTY getter supplies live evidence and its allocation is released synchronously; remote workspace PTYs are excluded. This checkpoint covers socket caller selectors and real local TTYs. CLI automatic caller provenance, fresh shell-reported nested-multiplexer TTYs and remote relay rewriting remain outstanding. Native CI now covers real PTY lookup, UUID precedence, explicit/ambient scope, failed attribution, scoped clear, workspace-only navigation and persistence. Runtime verification is pending.


## CLI caller identity checkpoint

Plain `cmux notify` now sends inherited CMUX surface/workspace hints as ambient caller evidence and discovers a TTY from its standard descriptors through the platform library. Explicit `--workspace` or `--surface` uses direct targeting without importing conflicting ambient selectors. `cmux notifications clear --caller` shares caller attribution; its flags conflict with explicit clear scopes. No subprocess or file scan is used for TTY discovery.

The caller CI scenario now invokes the real CLI from an inactive terminal with CMUX identity variables removed, and from a captured-output process with a deliberately stale ambient workspace hint. It checks native TTY fallback, stable surface rehoming, explicit override and caller-scoped clear. Runtime evidence remains pending. Fresh nested shell TTY reporting and remote relay identity rewriting remain open.


## Unread navigation checkpoint

Jump-to-unread now skips records whose saved workspace or terminal is closed, matching the upstream notification navigation coordinator's search for an openable record. Explicit message open still rejects a closed target without redirecting. If no retained unread message is reachable, the API returns opened=false and preserves focus/history. Workspace-only opens resolve the workspace independently of active terminal availability. Native inbox CI adds closed-target rejection, older reachable-message navigation and an all-stale no-op case. Runtime verification remains pending; upstream's separate non-message workspace/window unread fallback remains part of the broader attention/window matrix.


## Cumulative verification at 61266ee5

[Actions 34048253282](https://github.com/nitecon/cmux-gtk/actions/runs/34048253282) passed every job for native OSC intake and Claude Stop/Notification hooks, including the installed-handler/native-PTY integration scenarios and optimized memory/redraw benchmarks. This verifies the earlier source checkpoints at cb29ae1b/92534b35 plus the ordinary-output fast path. Desktop message delivery, caller identity, nullable workspace-only messages and stale-target navigation were added after this revision; their runtime evidence remains pending.


## Bounded scrollback capture checkpoint

`cmux read-scrollback --id UUID --json` / `surface.read_scrollback` now captures up to 2,000 recent rows as VT within 256 KiB through the existing vendored native screen-tail API. Native formatting reduces the suffix to fit its fixed scratch bound and preserves rendered styles, wide characters and compressed history. The allocation is released before returning; capture neither changes selection/viewport nor selects a background workspace. The new native CI fixture checks offscreen styled history, bounds and inactive-workspace focus/viewport preservation. Runtime evidence is pending.

Session replay is not yet implemented by this checkpoint. The native capture API is suitable for it, but capture must be combined with total retained-session limits, a preserved cache for never-initialized background terminals, a pre-shell replay path and replay-completion tracking. Ghostty initial_input writes to the child and must not be used to replay output. Upstream's replay-file/start/end boundary lifecycle provides the relevant pattern; its raw control sequences must not be blindly trusted from edited session files.


## Replay text normalization checkpoint

Native scrollback capture now removes theme-setting OSC and other command channels before returning VT. The shared bounded filter retains printable Unicode, CR/LF/tab and complete numeric SGR styling, brackets output with resets, and budgets those resets within the 256-KiB limit. Hyperlink OSC metadata is omitted; visible link text remains. This prevents saved history from changing the current theme, clipboard, window title or notification state when replay is connected. Unit cases cover style/Unicode retention, control-channel suppression, incomplete frames and exact byte boundaries; execution is deferred to Actions. Session persistence and replay lifecycle are still outstanding.


## Session scrollback replay checkpoint

Session snapshots now retain per-terminal styled VT history under a 256-KiB per-terminal and 16-MiB aggregate budget. Loading normalizes history before GTK restoration. Background surfaces retain pending history in their widget-owned cache until initialization, so quitting again without opening them preserves it. Layout-only API reads do not capture history.

A small native entry point seeds normalized history after renderer initialization and before the I/O thread begins processing child output. It borrows the bytes only during construction, preserving the existing shell/startup/resume and manual remote launch configuration. No replay file, child-stdin injection or asynchronous completion guess is used. The pending cache is removed after successful synchronous creation. Session replay diagnostics record byte count, duration and outcome without history text.

Local native ReleaseFast build, Rust binaries and strict clippy pass. Actions adds a three-launch GTK scenario: generate styled history, quit with another workspace selected, quit again without opening the source, then open it and verify retained history precedes actual fresh shell output. Runtime verification is pending. This does not restore arbitrary running processes; full agent/provider policy, manual previous-session restore and the rest of the parity matrix remain open.

The replay entry point is pinned to nitecon/ghostty commit `86d20c5d1`, reachable on `cmux-linux-session-scrollback`; the submodule URL now uses that fork so clean CI/developer checkouts can fetch the dependency. Existing upstream-derived native behavior is preserved.

Actions 34049479302 passed desktop click routing but failed caller attribution because explicit terminal read/input UUIDs were still resolved only in the selected workspace. The shared terminal resolver now searches all workspaces for an explicit UUID, preserving selected-workspace defaults only when identity is omitted. The caller and background scrollback fixtures cover this correction; cumulative verification is pending.


## Restored working-directory checkpoint

Plain local terminals now restore their last reported directory ahead of the workspace's original launch directory. Explicit startup-command and remote-workspace restoration retain workspace launch precedence. Each deferred GLArea keeps its launch directory for snapshots taken before native initialization; this prevents an untouched background terminal losing its directory during another quit. The three-launch history fixture now starts a workspace in one directory, changes to a second directory with an OSC7 report, and verifies the actual shell directory after an intermediate unopened quit. Runtime verification is pending.


## Previous-session startup recovery checkpoint

Before GTK activation on a normal launch, a valid nonempty current snapshot is archived atomically as `session.previous.json`, then the file and parent directory are synced. Live autosaves only update `session.json`. After quitting cmux, `cmux-app --restore-previous-session` explicitly loads the backup through the same validation/restoration path and leaves that recovery source unchanged. An invalid or missing backup fails without starting a fresh session or replacing current saved state. Backup-write failures are diagnosed and do not prevent ordinary startup.

The Actions recovery fixture creates a workspace, closes it during the next run, restores it with the flag, checks stable identities and backup independence, then checks invalid-backup failure preserves the current snapshot. Local clippy and fixture syntax pass; runtime evidence is pending. Upstream's in-app reopen into additional windows and unclean-launch backup preservation remain dependent on the broader window/session lifecycle work and are not claimed complete here.


## Unclean-launch recovery checkpoint

Startup now writes a durable owner-token `session.running` marker. An existing marker preserves the previous backup instead of rotating current state over it. Only a successful final durable session save retires this launch's marker; forced termination, panic and failed saves leave recovery evidence. The recovery CI fixture now waits for a workspace-closing autosave, kills its owned process, launches normally and verifies the earlier backup survives before explicit recovery. Runtime verification remains pending.

Actions 34050046754 failed at the closed-surface notification case: explicit `surface.close` still searched only the selected workspace. Both explicit close and focus now locate the owning workspace by surface identity. Close preserves the selected workspace; focus deliberately selects the owner. Existing notification and cross-workspace fixtures cover the corrected close path; further cumulative evidence is pending.

Pending history now uses shared immutable text across session state and uninitialized widgets, removing repeated full-buffer copies during unopened-background snapshots while keeping the same JSON format and history limits. New session_scrollback diagnostic counters expose capture cost, bytes, errors, pending reuse and budget skips; native CI checks actual capture metrics. Runtime performance evidence remains pending. Live-history caching is deliberately not inferred from PTY position because native non-output actions can also change history.

## Agent sidebar metadata checkpoint

Keyed plain-text status entries and labeled progress now have CLI commands, bounded workspace-owned state, GTK rendering and session persistence. `set-status`, `clear-status`, `list-status`, `set-progress` and `clear-progress` accept an explicit workspace or the caller's `CMUX_WORKSPACE_ID` without changing focus. Status priority controls display order; validated colors and GTK theme icons are supported. New keys fail at the 32-entry limit without evicting existing data. Rendering escapes styled values and confines labels to one line.

These JSON `sidebar.*` methods adapt the pinned upstream legacy `set_status` / `set_progress` commands from `ControlCommandCoordinator+SidebarMetadataV1.swift`. Markdown, clickable URLs, panel provenance, SF Symbols translation and legacy v1 wire compatibility remain outstanding, as do automatic Git/PR/ports and project/diff views. Actions `test_linux_sidebar_metadata.py` covers actual CLI mutations, inactive workspace focus, limits, persistence and clears; runtime evidence is pending. Local strict clippy passes.


## Inline Markdown and status link checkpoint

`set-status --format markdown` now renders inline emphasis, strikethrough, code and HTTP(S) links with a CommonMark parser. Raw HTML is escaped, images retain alt text without fetching resources, and block boundaries collapse to spaces in the sidebar row. `--url` / `--link` adds a whole-row destination, taking precedence over inline links. GTK handles explicit link activation through its normal URI launcher. Destinations are bounded to 2048 bytes and restricted to HTTP(S), matching pinned upstream status URL validation. Existing session entries default to plain text.

The implementation uses the [pulldown-cmark event API](https://docs.rs/pulldown-cmark/0.13.4/pulldown_cmark/enum.Event.html) to generate escaped GTK markup rather than rendering HTML. Actions unit coverage checks nested formatting, escaping, rejected schemes and link precedence; the native CLI/restart fixture now covers Markdown and URL persistence. Strict clippy and fixture syntax pass locally; runtime and actual desktop link activation remain unverified. Multiline metadata blocks and panel ownership remain open.


## Multiline metadata block checkpoint

`report-meta-block`, `clear-meta-block` and `list-meta-blocks` now manage separate persistent summaries, with eight blocks of at most 8 KiB per workspace. GTK renders priority-ordered collapsible summaries with bounded-height scrolling; live updates preserve expansion state. Markdown headings, paragraphs, list markers, code and inline styles retain multiline structure, with escaped HTML and no remote image downloads. CLI/JSON preserve literal Markdown bytes; the upstream legacy parser's backslash expansion is not applied to this JSON adapter.

Actions coverage extends the real sidebar fixture with block replacement at capacity, rejected extra/empty/oversized blocks, restart retention and removal. Rust coverage checks multiline structure and loaded bounds. Runtime verification remains pending; panel provenance, rich project views and full legacy wire compatibility remain outstanding.


## Batch workspace ordering checkpoint

`cmux reorder-workspaces --order UUID,UUID --dry-run` and `workspace.reorder_many` now validate the entire request before moving anything and return a plan with original/final indices. Listed workspaces precede unlisted workspaces, whose relative order remains unchanged, matching upstream's unpinned-workspace planner. Duplicate, malformed and unknown IDs fail atomically. Applying a batch moves the existing workspace, split engine and GTK row together, preserves active identity and publishes one session snapshot; dry runs publish none. Responses include changed-item events only for applied batches.

Actions `test_linux_workspace_reorder_many.py` covers three workspaces with the middle selected, dry-run nonmutation, invalid-batch atomicity, retained focus and quit/restart ordering. Strict clippy and fixture syntax pass locally; runtime verification is pending. Pin/group ordering, multiwindow routing and upstream short reference/index normalization remain gaps; this checkpoint supports stable UUIDs in the existing single-window model.

## Cumulative CI and directory-report fixture correction

[Actions 34051272228](https://github.com/nitecon/cmux-gtk/actions/runs/34051272228), at `b9b6a62c`, passed notification history/exact targeting, desktop actions, caller attribution, native scrollback capture, immediate quit-save, manual resume, Claude hooks, signed automatic restart, clipboard, terminal churn and SSH/script restoration. The run failed the scrollback restart fixture at its final shell-CWD assertion; this is not a full green run and does not verify newer sidebar or batch-order commits.

The fixture emitted `file:///path` in OSC 7, but the pinned native `stream_handler.zig` requires a local hostname and rejects a missing host before recording PWD. The fixture now reports `file://<local-hostname>/path` and asserts the exact saved terminal CWD after each durable quit, before checking the final shell. This separates initial native directory reporting, uninitialized-background snapshot retention and actual launch behavior. Python syntax and diff checks pass locally; the corrected runtime scenario remains pending in Actions.


## Workspace move shortcuts checkpoint

Ctrl+Shift+Page Up / Page Down now move the selected workspace through the same model operation used by drag/drop and single-workspace CLI reorder. At either boundary the action is a no-op, preserving terminal focus and identity. `[shortcuts]` accepts `move_workspace_up` and `move_workspace_down` GTK accelerator overrides. The native batch-order fixture now also sends actual keyboard events and verifies direction, nonwrapping boundaries and an additional normal restart. Strict clippy, Python syntax and diff checks pass locally; Actions runtime evidence remains pending.

## Automatic local Git metadata checkpoint

Workspace rows now show local branch and dirty state, and workspace-list responses include a transient `git` observation. One bounded background Git process at a time visits workspaces round-robin, using the selected terminal's reported directory or explicit launch directory. Results are discarded after a directory change or workspace closure; applying them preserves focus and does not save sessions or overwrite agent statuses. Remote workspaces are excluded until remote discovery is implemented.

The parser uses Git's documented [porcelain-v2 status format](https://git-scm.com/docs/git-status#_porcelain_format_version_2). Execution, stdout/stderr and cleanup waits are bounded; inherited Git routing/config environment overrides and fsmonitor hooks are disabled. Diagnostics correlate workspace identity and probe duration/outcome without file paths or branch content. Actions coverage uses real repository commits, branch changes, background edits and terminal directory changes. Strict clippy and fixture syntax pass locally; runtime evidence is pending. PR/MR association, remote Git and project/diff views remain incomplete.

## Status color CLI correction

[Actions 34052123324](https://github.com/nitecon/cmux-gtk/actions/runs/34052123324), at `102a9eba`, failed the sidebar fixture when setting the first unstyled status after a styled one. The root CLI's global `color` argument supplied its `auto` default to the status subcommand's same-named optional text color. The server correctly rejected that non-hex styling value. Output color now defaults in the formatter instead of the argument parser, so omitted status colors remain absent. Explicit output-color modes and explicit status hex colors retain their syntax. Parser regression coverage is added alongside the existing real CLI fixture; strict clippy passes locally, runtime verification pending.


## Git tracking and provenance checkpoint

Local Git observations now include optional upstream, ahead/behind counts, HEAD object ID and the observed directory. The same bounded porcelain-v2 probe provides these fields; it performs no fetch. Missing tracking information remains null. Sidebar rows show positive ahead/behind counts and a short commit ID for detached checkouts, with the directory in a tooltip. Actions coverage adds a real local upstream and ahead commit; parser coverage checks tracking headers and malformed counters. Strict clippy and Python syntax pass locally, runtime pending. Remote discovery and PR/MR/project views remain outstanding.


## Browser address normalization checkpoint

CLI open/goto, RPC open and the GTK address entry now share normalization. `about:blank`, `data:` and `blob:` addresses remain intact; absolute Unix paths become correctly escaped `file://` URLs. This fixes the previous HTTPS prefix applied to hostless browser schemes and local paths. The native lifecycle fixture checks delivered navigation addresses and focus using its owned browser service fixture. Strict clippy and Python syntax pass locally; runtime verification is pending.

This does not complete local-document parity. Real local rendering, file subresource behavior and per-surface browser state remain open. The external service documents a separate [local-file access option](https://agent-browser.dev/commands); address normalization does not alter its JavaScript file-access policy.


## Real local-document and history verification checkpoint

The optimized real-browser Actions benchmark now opens a local HTML document through `cmux browser goto` using a filename containing spaces, `#` and `?`. It verifies the exact encoded URL, a relative classic script, stylesheet computed color, DOM snapshot and a subsequent preview frame, then navigates back to the HTTP page and forward to the document without stealing terminal focus. The existing artifact records local-document operation latency and a separate outcome. This uses pinned agent-browser/Chromium; it is added coverage awaiting execution, not evidence of a passing scenario. Cross-file JavaScript fetch access and independent multi-surface browser history remain unverified.


## Explicit browser workspace routing correction

`browser open --workspace UUID` previously forwarded the local workspace selector to the external service while attaching the GTK pane to the active workspace. The target is now validated and resolved locally before daemon startup, removed from external service parameters, and retained across asynchronous completion. Malformed/nonexistent targets fail without creating a pane; closing a target during startup retains the existing stale-result rejection. Native lifecycle coverage opens into a third background workspace and checks unchanged active terminal identity, plus invalid-target nonmutation. Strict clippy and Python syntax pass locally; runtime pending. This corrects pane placement, not the remaining shared-browser page-state limitation.

## Independent browser ownership audit

The source audit confirms that browser panes still share one manager/page and that mapped-tab selection reopens URLs. Backend IDs in the browser reference map are not reliable GTK surface identities. [Independent browser surface ownership](BrowserSurfaceOwnership.md) records the concrete replacement and its multi-surface DOM/history/preview, cancellation and total-resource verification gates. This is an implementation plan; it does not mark browser parity complete.

### Independent browser surfaces — implementation awaiting Actions

Browser panes now own private daemon sessions keyed by their real GTK surface UUID. Creating another page preserves existing DOM and history; callbacks, agent commands, frame streams and shutdown route to that owner. Stale references fail instead of falling through to another page. Saved browser panes initialize lazily when mapped through serialized restore admission; close includes suspended panes. RPC URL metadata updates the owning saved address. Retired mapped-tab URL reopening has been removed.

The real Chromium Actions benchmark now opens two pages, independently mutates their DOM, navigates one through local documents and history, closes the other, and checks that stale commands fail while the survivor remains usable. Runtime verification is pending. Remaining browser gaps include agent-triggered background initialization of suspended surfaces, complete page-driven URL reconciliation, total browser-process memory accounting and resident-page bounds. This checkpoint does not complete browser or overall upstream parity.

Browser workspace cleanup now retires each owned daemon before removing its GTK tree, independently of delayed widget destruction. The lifecycle Actions fixture checks that closing a background workspace removes its daemon while preserving other workspaces' daemons, and that closing the originating workspace during pending startup eventually leaves no orphan session. Verification pending in Actions.

Browser close now shares the model operation between the keyboard shortcut and RPC. Daemon retirement follows successful GTK tab removal; a rejected final-workspace-surface close preserves its page and reports `close_failed`. The Actions reentrant-close fixture checks the surviving surface identity, connected state and a subsequent daemon exchange. Closing all is sequential: successful earlier closes remain applied if a final surface is rejected. Runtime verification pending.

Agent commands now initialize suspended saved browser surfaces on demand through the same startup worker used by mapped tabs, then dispatch to the captured UUID without changing selection. Startup cancellation preserves the pane reference for a retry and retires the unfinished session. Restore admission is bounded to fifteen seconds. Connected and streaming sessions both report connected in browser listings. The existing hidden-browser Actions fixture now exercises background agent initialization with terminal selection preserved. Runtime verification pending; aggregate process accounting and full page URL reconciliation remain open.

The real-browser benchmark now includes bounded daemon/Chromium process-tree RSS, PSS and private-memory samples, a warmed workload PSS delta, two-session footprint and post-close exit checks for observed process identities. This closes the benchmark's previous GTK-only visibility gap for sampled descendants. Runtime evidence, longer OOM workloads, resident-page policy and production browser-process diagnostics remain open.

Browser locations now reconcile page-driven navigation through a window-owned, single-worker round-robin refresh of initialized sessions. Address editing and stale-session results are protected; suspended pages remain unstarted. UI navigation completion also persists when its pane becomes hidden. Real-browser Actions coverage changes a background page using the History API and requires the exact surface URL in live metadata and the durable session, with terminal focus preserved. Runtime verification pending; refresh is eventual and does not promise capturing a change immediately before quit.

Browser address reconciliation and RPC URL application now recognize focus in GTK Entry's delegated text child, preserving active address editing. A dedicated headless GTK Actions test verifies focus acquisition and release across two entries; runtime evidence pending.

### Listener attribution foundation

The Linux platform service now reads bounded TCP/IPv6 listener tables and intersects socket inodes with descriptors owned by explicitly supplied PID/start-time identities. It checks identity before and after scanning, excludes non-LISTEN sockets and reused/exited processes, and reports permission or size failures explicitly. A real-socket platform test verifies attribution, PID-reuse rejection and disappearance after close in Actions. This is the collector foundation, not completed port discovery: terminal descendant registration, worker scheduling, sidebar/API publication, remote collection and automatic forwarding remain to implement. Upstream behavior was inspected in pinned `Sources/PortScanner.swift`, which attributes ports per panel and batches scans.

The listener platform service also discovers bounded current process trees across all spawning threads, qualifies each child by parent and start time, and rejects changed root identity. A real child-process Actions test covers discovery and root reuse rejection. Historical ownership of detached children is not inferred; terminal root registration and application publication remain outstanding.

### Local workspace listening ports

Local workspaces now show unique listening port numbers in the sidebar and expose attributed records through `list-workspaces --json`. A single blocking worker matches current application descendants against native terminal controlling-TTY devices and intersects socket ownership, publishing surface UUID, address, port, PID and local provenance. Unknown/failed scans return null, successful empty scans return an empty list. Changes preserve focus and are not persisted. Closed or changed terminal layouts discard stale results. Diagnostics record scan duration, outcome and count.

The new Actions fixture launches a real terminal child server, observes its port from a different selected workspace, excludes an unrelated server and requires removal after exit. Runtime verification pending. Detached processes that lose controlling-TTY ancestry, shared network-table batching, dedicated port commands, remote collection and automatic forwarding remain open. Blocking procfs work has cardinality/size bounds but cannot be forcibly interrupted after starting.

`cmux ports` and `ports.list` now expose the latest attributed listener snapshot with workspace/surface filters, exact scope validation and no focus changes. The native listener fixture checks filtered records and conflicting-scope rejection. Runtime verification pending.

Listener observation now batches qualified processes and reads TCP tables once per network namespace per scan. Open namespace handles keep cached identities stable; cache limits are 16 namespaces and 16,384 listener entries each. Per-process descriptor matching and before/after identity checks remain in place. Existing platform and native listener Actions fixtures cover attribution through the batched path; runtime verification pending.

### Remote PTY listener discovery

The remote daemon now exposes `ports.list` for a registered PTY stream. Linux collection follows the owned shell's current descendants, qualifies PID/start-time identities and intersects listening socket inodes with process descriptors. It returns bounded records with remote provenance and rejects stale, missing or non-PTY streams. A Go Actions fixture starts a real server inside the PTY, excludes an unrelated caller listener and checks retired-stream rejection. Desktop polling/publication and automatic forwarding remain to implement; the daemon endpoint alone does not complete remote workspace parity.

Cumulative Actions [34055394097](https://github.com/nitecon/cmux-gtk/actions/runs/34055394097) passed for `c859c5fd`, including the independent-browser, background restoration, location persistence, recovery and optimized benchmark checks present at that commit. Later port changes still require cumulative verification.

Remote workspace listener observations now flow from the SSH routing task into stream-qualified bridge state and then the shared sidebar/ports API. Polling is sequential per connection and uses existing request bounds/tracing. Reconnect and context removal clear cached identities; GTK verifies the live context, stream and connection before applying remote-provenance records. Disconnection clears published ports. The real SSH fixture checks background remote discovery and service-exit removal. Runtime verification pending; forwarding and browser remote-origin routing remain open.

Automatic forwarding implementation is governed by [RemoteForwarding](RemoteForwarding.md): connection-owned listeners and clients, bounded separate proxy queues that cannot stall RPC responses, cleanup of cancelled remote opens, endpoint identity across reconnects, and full browser subresource routing gates. Forwarding remains unimplemented; this contract records the transport and verification requirements before mutation.

Initial automatic remote forwarding now uses the existing SSH RPC transport, with bounded per-client queues separate from terminal routes, connection-owned listeners/tasks and nullable `forwarded_local_port` in port records. The real SSH fixture transfers a multi-chunk payload through the published loopback endpoint. Runtime evidence and remaining forwarding gates in [RemoteForwarding](RemoteForwarding.md), including half-close/reconnect/overload and browser subresources, remain outstanding.

Forwarding cleanup now preserves existing routes on duplicate stream responses, closes capacity-rejected new streams, stops clients on listener termination and removes the matching published endpoint. Completed listener registrations are eligible for retry. Route ownership/overload tests are added for Actions; runtime evidence remains pending.

Forwarded clients can now send request EOF without losing the remote response through `proxy.shutdown_write`. The real SSH fixture sends a request, shuts down its write direction and requires a multi-chunk reply from a server that waits for EOF. Go coverage also rejects non-TCP streams. Runtime verification pending; reverse half-close and remaining forwarding gates stay open.

Forwarding diagnostics now report active listener/client tasks, byte counters with explicit acknowledgement/completion semantics, rejected data/client admission and requested closes. The SSH fixture checks exact transfer deltas and cleanup gauges. Runtime verification pending; counters do not establish remote-close acknowledgement.

Normal forwarding completion now awaits remote `proxy.close` acknowledgement, with cancellation/failure retaining bounded queued cleanup. Diagnostics expose confirmed and failed closes; the SSH fixture checks acknowledgement after transfer. Runtime verification pending.

Opt-in proxy subscriptions now support remote-to-local half-close without retiring the remaining write direction. Reserved termination capacity preserves EOF ordering under a full data queue; forwarding waits for both clean transfer directions. Real-TCP Go and bounded-route Rust tests are added for Actions. Runtime and full SSH reverse-half-close evidence remain pending.

### Reverse TCP half-close integration coverage

The real SSH workspace fixture now waits for remote response FIN before sending a multi-chunk request. It verifies the remote payload, exact directional byte counters, confirmed stream retirement, zero remaining client tasks and preserved background workspace selection. Python and embedded server syntax validation pass; runtime verification belongs to Actions. Actions run 34056967638 passed for its earlier source revision; it does not verify this new scenario. Full remote forwarding and browser-origin parity remain open.

### Remote capability negotiation

Hello now negotiates listener discovery and bidirectional half-close support per connection. Older terminal-capable daemons stay usable, with unsupported discovery/forwarding disabled until daemon upgrade and reconnect. Handshake diagnostics report both feature decisions. Added transport coverage for full/partial/missing capability sets; strict workspace Clippy passes, Actions execution pending. Reconnect/collision/overload integration evidence and full browser remote-origin routing remain open.

### Forwarded service retirement coverage

The real SSH fixture now retires a remote listening socket while its accepted client remains open, requiring discovery-driven EOF at both peers, confirmed close, zero owned forwarding tasks and preserved workspace selection. It records retirement latency and checks the fallback port differs from the occupied preferred port. Python syntax validation passes; Actions runtime execution remains pending. This does not establish multi-workspace collision or reconnect coverage.

### Browser remote-origin design evidence

Inspected the shared browser launch path, agent-browser public proxy flags and Chromium proxy semantics. The [remote forwarding plan](RemoteForwarding.md#browser-origin-implementation-plan) now specifies a workspace-owned stable SOCKS endpoint, generation-bound stream admission, explicit loopback proxying, remote DNS and no local fallback. Separate namespace/subresource/WebSocket/reconnect tests are required. This is design progress; remote browser routing is not implemented yet.

### Remote browser transport foundation

Added a workspace-retained SOCKS5 endpoint, bounded generation-qualified admission and shared remote TCP transfer ownership. Protocol coverage preserves bytes following CONNECT and rejects unsupported authentication. Strict Clippy passes; Actions runtime verification is pending. Browser startup configuration and end-to-end remote origin verification remain unimplemented.

### Remote browser launch integration

UI, RPC and restored browser managers now capture their owning workspace proxy, retain its bridge through shutdown and pass explicit SOCKS/loopback settings after bounded readiness. Added child-process argument coverage; strict Clippy passes, runtime verification pending. Actions 34057511902 passed at 77ab1590 for the earlier half-close implementation. This does not verify the later SOCKS transport or browser wiring. Full remote web-origin and reconnect evidence remain open.

### Isolated browser-origin Actions coverage

Added a real SSH network-namespace mode with a same-port local decoy and real Chromium assertions for redirects, relative/absolute scripts, loopback fetches, WebSockets and background workspace selection. New Actions step writes remote-browser-release.json. Syntax checks pass; runtime results are pending. This does not yet establish remote-only DNS, multi-workspace same-port isolation or reconnect behavior.

### Same-port remote workspace isolation coverage

Extended the real remote browser fixture to two namespaces serving different script identities at the same port. It checks both page identities, first-page renavigation, distinct automatically forwarded local endpoints, preserved background selection and no local decoy traffic. Syntax validation passes; runtime verification remains pending Actions.

### Remote browser reconnect scenario

Added workspace connection/proxy observations and a real SSH reconnect scenario that retains browser DOM/session identity and its stable proxy port, rejects requests before service restart without local fallback, and restores resource loading after a new-generation PTY subscription. Second-workspace content remains independent. Strict Clippy and syntax checks pass; runtime evidence is pending Actions.

### Remote-only browser hostname coverage

The isolated SSH browser test now supplies a hostname only through a private remote hosts-file mount. Both workspace browsers must resolve it and receive their distinct remote page identity. Syntax checks pass; runtime evidence remains pending Actions.

### SOCKS handshake overload coverage

Added bounded handshake gauges/outcome counters and workspace-qualified timing events. Actions coverage fills all sixteen slots, rejects the next client, observes deadline cleanup and verifies subsequent browser navigation. Strict Clippy and syntax checks pass; execution remains pending. Full transferred-data overload remains a separate gate.

### Project configuration resolution foundation

Added [project action resolution](ProjectActions.md) and offline `cmux project-actions --directory PATH`, preserving global/nearest-project precedence and winning source paths under bounded regular-file reads. Upstream source resolves an ambiguity in its dogfood documentation: actions use nearest local plus global fallback; hierarchy lookup is separate for hooks. Typed validation, execution, palette and remote config remain open. Strict Clippy passes; runtime verification is pending Actions.

### Typed project action intent

Read-only action inspection now reports typed intent/target and rejects malformed command/agent fields, targets, presentation booleans and unknown action types. Workspace layouts and builtin capability resolution remain incomplete, along with execution and palette integration. Strict Clippy passes; Actions runtime pending. Earlier SOCKS transport foundation ce95b481 passed full Actions run 34058349025.

### Project builtin identity validation

Builtin actions now resolve upstream aliases into explicit canonical identities and reject unknown names. Recognized but unimplemented platform features remain represented; no execution or parity completion is implied. Strict Clippy passes; runtime tests await Actions.

### Typed project workspace layouts

Added bounded recursive pane/split validation, typed surface kinds, launch fields and environment checks. Upstream split defaults/clamping are preserved; invalid topology is rejected before launch integration. Strict Clippy passes; runtime Actions verification is pending. Workspace execution, project renderer, colors and restart semantics remain outstanding.

### Workspace-scoped project action discovery

Added project.actions.list and project-actions --workspace, resolving captured terminal CWD on bounded workers and reusing local directory selection with Git metadata. The background-workspace Actions fixture checks CWD changes, no execution and focus preservation. Strict Clippy/syntax checks pass; runtime pending. Remote reads and execution/palette integration remain open.

### Project action review identity

Action inspection now returns a versioned fingerprint binding full definition, source, ID and captured directory for stale-review rejection during execution. No authorization or execution is added by the digest. Strict Clippy passes; change-invalidation tests await Actions.

### Explicit project command execution

Local project.actions.run / project-run re-reads the inspected action and rejects stale fingerprint, CWD or selected-surface context before submission. Commands support selected sibling tabs in the captured directory or literal input into the current terminal. Success reports submission only; explicit requests authorize execution, while listing and fingerprint possession do not. Persistent trust, palette, other action families, remote configuration and workspace layout execution remain open. Actions coverage checks stale rejection, new-tab CWD and current-terminal identity. Strict Clippy and syntax checks pass; runtime pending.

Actions run 34059233345 failed in remote-browser fixture cleanup because browser close requires --surface. The fixture argument is corrected; complete isolated-origin/reconnect/overload runtime verification awaits a green rerun.

### Project terminal builtin execution

Project actions can now invoke newTerminal, splitRight and splitDown using existing tab/pane ownership. Fingerprint/context checks precede mutation, and the selected new surface is returned and saved. Actions fixture covers creation, identity, focus and cleanup for all three; strict Clippy and syntax pass, runtime pending. Other builtin families and full action/palette integration remain open.

### Project workspace builtin execution

cmux.newWorkspace now applies the reviewed project directory through existing local-workspace creation, sidebar handlers and session persistence. Run results and traces distinguish source from destination workspace IDs. Actions coverage checks identity, directory and selected surface; strict Clippy and syntax pass, runtime pending. Full workspace layout intents and other builtin families remain open.

### Project browser builtin execution

cmux.newBrowser now reuses browser startup/cancellation/session wiring, rejects a changed target surface during startup and selects the resulting browser only for explicit project execution. CLI project-run allows thirty seconds. Mock-browser Actions coverage checks target selection and independent daemon retirement; Clippy/AST pass, runtime pending. Palette, agent/workspace intents and remaining builtins remain open.

### Project browser stale-context coverage

The browser lifecycle fixture now holds a project browser startup while the target pane changes. It requires explicit rejection, no added browser/focus mutation, and cleanup of only the rejected daemon. Python syntax passes; Actions runtime evidence remains pending.

### Project agent execution

Agent intents now share reviewed command execution and target behavior, with upstream provider aliases and shell-argument trimming. Custom bounded CLI names work through the terminal shell. Tests cover alias/argument semantics and live custom-agent output in the project directory; runtime pending Actions. Provider hooks and resume integration remain separate open matrix items.

### Named workspace command discovery

Config resolution now exposes bounded named commands with local precedence, source and typed definition. Workspace-command action review identities bind referenced command content and source to detect indirect edits. Tests cover local/global precedence, first duplicate and reference invalidation; runtime pending Actions. Workspace layout launch and restart behavior remain open.

### Named command schema validation

Named definitions now require exactly one workspace or command and validate restart values independently of action inference. Action-only fields cannot redirect intent; duplicate definitions are validated before precedence. Clippy passes; schema tests await Actions. Layout/restart execution remains open.

### Native project launch environment boundary

Added local configured surface launch with owned environment strings, inherited/explicit/managed identity precedence and CMUX_ override filtering. SplitEngine passes workspace overrides to new terminals without process-global mutation. Native Actions fixture checks literal values and protected surface identity; Clippy passes, runtime pending. Project layout application and persistence remain open.

### Workspace launch environment persistence

Session snapshots now retain explicit workspace environment overrides and restore them into local terminals and future tabs. Shared bounded validation rejects malformed saved values; old sessions default empty. Tests cover round-trip and compatibility; runtime pending Actions. Project layout launch remains open.

### Restored launch environment end-to-end coverage

The multi-restart fixture now checks explicit environment retention through an unopened workspace, actual restored-shell values and a later split terminal. Both must receive their own cmux identities despite a forged saved override. Existing history/style/CWD checks remain. Python syntax passes; runtime pending Actions.

### Shared project and session layout depth

Project validation and session restoration share a bounded eight-level Linux cap. The inspected upstream source has no explicit layout-depth limit. Actions runs34062084578,34063064981 and34063972634 proved that16 nested GtkPaned splits and17 live terminal surfaces can open the socket yet leave the GTK main thread unable to answer readiness within ten seconds; allowing constrained descendants below natural size did not resolve it. Linux therefore rejects deeper trees before constructing GTK widgets and falls back to a single terminal during session restore. Startup coverage exercises the accepted boundary with eight nested splits and nine retained panes/surfaces; runtime proof is pending. Full project layout application is covered below.

### Custom project workspace layouts

Inline workspace and named workspaceCommand actions now create their validated pane trees directly, without allocating a placeholder terminal. Horizontal and vertical splits retain bounded ratios; panes retain terminal and browser tabs plus stable identities and the requested focused surface. Worker preparation canonicalizes every terminal directory before GTK mutation. Workspace setup is combined with the first terminal command once, while later terminals receive only their own command. Workspace and surface environments merge at native launch and persist through session snapshots under the same bounded validator. Restored-style browser tabs use the shared lazy startup and cancellation ownership. Project surfaces remain an explicit Linux error until a native renderer exists. Actions fixtures exercise mixed terminal topology, focus, commands, setup/environment, browser navigation and daemon cleanup; runtime verification is pending.

### Named workspace restart policies

Named workspace commands retain upstream `new`, `ignore`, `recreate` and `confirm` policy values. Name collision behavior matches the inspected upstream executor: `new` creates another workspace, `ignore` selects the existing workspace without launching, and `recreate` builds the replacement before closing the old workspace. This ordering avoids destroying a working workspace when replacement construction fails. `confirm` returns `confirmation_required` when a matching name exists and launches normally without a collision. The palette and CLI decision paths are covered below. Actions coverage checks stable identity/count for ignore and identity replacement with constant count for recreate; runtime verification is pending.

### Project command palette and confirmation

The GTK header, hamburger menu and Ctrl+Shift+P now open a searchable project-action palette for the active local workspace. It calls the shared bounded resolver and reviewed action runner directly, excludes metadata-only and `palette: false` entries, renders configuration text as plain text, and holds only weak GTK/model references while asynchronous work is pending. Enter executes the first visible search result. An action that declares `confirm: true`, or a colliding workspace with restart `confirm`, returns `confirmation_required`; the palette presents the complete bounded reviewed definition, including a referenced named command and its source, then retries with the same workspace/action/fingerprint identity only after acceptance. The CLI exposes the same explicit decision through `project-run --confirm`. Unit coverage checks palette filtering; the native project fixture opens/searches/executes the actual palette and checks fail-closed versus confirmed replacement identity/count. Runtime verification is pending.

### Restore topology preflight

Session reconstruction now checks the full tree depth before allocating any GTK widgets or scheduling ratio callbacks. An invalid late branch previously caused earlier branches to be constructed before fallback. Boundary tests cover the shared maximum and a too-deep late branch; Clippy passes, runtime pending Actions. This avoids partial allocation for rejected trees and is not evidence that the reported OOM is resolved.

### Default project workspace execution

Inline and named workspace actions now create configured workspaces with reviewed name/relative-or-absolute cwd/RGB color/environment. Validation and path resolution run off GTK; first terminal receives overrides before realization, with normal persistence/sidebar wiring. Actions coverage exercises both forms and child values. Layout/setup and non-new restart policies remain explicit errors pending implementation; full parity remains open.

### Workspace setup input and cumulative verification

Default-layout setup now delivers once as first-terminal input after native initialization, consumes pending text and excludes it from future tabs/session restore. Inline/named fixtures require setup output using configured env. Runtime pending for setup.

Actions run34061210213 succeeded at11ed9692, verifying project command/terminal/workspace/browser builtin paths at that revision and isolated remote-browser routing/reconnect/overload. Downloaded benchmark artifact reports passed remote-browser workload and zero local-decoy requests. Later changes are not covered by this green result.

### Persistent workspace groups

Workspace groups now have stable UUIDs, bounded names, optional validated colors, collapse state and ordered session persistence. Workspace membership is stored by group UUID and is independent of GTK row position. Group headers display member and aggregate unread counts; collapse hides member rows while keeping their terminal trees live. Header clicks toggle collapse, and each workspace context menu can create a group from that workspace, assign it to an existing group or make it ungrouped. CLI/socket operations list, create, update, assign and delete groups without focus changes; assignment validates every UUID before mutation and deletion retains members as ungrouped workspaces. Group mutation diagnostics contain only operation, IDs and counts. Native Actions coverage exercises atomic validation and graceful restart persistence; runtime verification is pending.

### Mosh interactive transport

Remote workspace management and terminal transport are now separate. `cmux ssh DESTINATION --transport mosh` and `cmux mosh` keep the existing SSH daemon lane for browser routing, ports and management while launching Mosh inside the interactive Ghostty PTY; `cmux mosh-tmux` attaches a validated named tmux session. The GTK SSH dialog exposes the same terminal choice. A generated POSIX command requires a local Mosh client with `--experimental-remote-ip`, probes `mosh-server` over the bounded SSH configuration and uses proxy address resolution for SSH aliases. Missing or incompatible support reports its stage and replaces itself with direct SSH. Transport/profile/session intent persists and restores before terminal realization, with `mosh://` sidebar locations and content-free transport diagnostics. Executable builder coverage uses fake SSH and Mosh programs in Actions; full remote UDP integration remains environment-dependent and runtime verification is pending.

### Browser profile reuse

Current upstream imports authenticated browser data into named WebKit profiles. The Linux browser backend is the independently versioned `agent-browser`, so GTK now exposes its native Chrome profile contract through `cmux browser open URL --profile NAME_OR_PATH` and project browser surfaces' `profile` field. The validated selector is passed as a distinct process argument, removed from daemon navigation payloads, retained per surface, shown by `browser list`, and persisted through session restoration. Omitting it retains the isolated ephemeral browser context. This provides authenticated profile reuse without copying browser cookie databases or credentials through cmux; source-browser discovery and any decryption remain owned by agent-browser. Unit coverage verifies exact process arguments and session round trips, while the GTK lifecycle fixture verifies an explicit profile reaches the real public startup boundary. Actions runtime verification is pending.

### Native Claude Code teams

The Linux CLI now exposes `cmux claude-teams [CLAUDE_ARGS...]`. It resolves and replaces itself with the real Claude executable, preserves argument boundaries, enables the provider's agent-teams mode and supplies an auto teammate mode plus a named-teammate hint only when the caller did not already choose those options. The launcher requires an originating cmux terminal identity and installs a single owner-only, atomically replaced tmux compatibility shim in the runtime directory. Exact trust opt-in before `--` is the only input that sets Claude's sandbox marker.

Claude's public teammate flow translates `display-message`, `split-window`, `select-layout main-vertical`, `resize-pane`, `list-panes`, `respawn-pane` and `kill-pane` into bounded socket calls. Pane tokens are opaque stable surface UUIDs. Explicit split targets resolve across workspaces, the first teammate opens beside the leader, and later teammates stack in the agent column. New terminals receive cmux surface identities, so the existing Claude hooks bind session resume and notifications to the exact teammate pane. The Actions fixture launches a fake Claude through a real terminal, creates three teammates through the private shim, executes work in the final pane, and verifies the four native surface identities and managed environment. Runtime evidence is pending.

### Codex lifecycle hooks

Codex joins Claude as a native resumable provider. `cmux hooks setup codex` merges owned `SessionStart`, `Stop` and `SessionEnd` command handlers into the documented `$CODEX_HOME/hooks.json` schema, preserving unrelated groups and refusing malformed, oversized or symlink configuration. An omitted provider installs each detected supported provider. Hook payloads are bounded to 64 KiB and require the official `hook_event_name`, native `session_id`, absolute working directory and exact originating cmux surface. Start records `codex resume SESSION_ID` plus `CODEX_HOME`, Stop routes bounded summary text to that surface's inbox, and End clears only the matching checkpoint. Actions runtime verification is pending.

### Shared JSON agent lifecycle hooks

The nested JSON provider registry extends the same bounded exact-surface lifecycle to Grok, Gemini, GitHub Copilot, CodeBuddy, Factory Droid and Qoder. Each provider retains its upstream config location, executable name, event names and native resume argv. Setup is idempotent, preserves unrelated matcher groups, installs only detected providers when no name is supplied and rejects malformed, oversized or symlink configuration. Payload intake accepts the provider catalog's snake-case/camel-case scalar names and fixed known envelopes without recursive or unbounded traversal. Grok's turn-boundary `SessionEnd` is deliberately not installed as destructive cleanup, preserving its durable binding. One Actions fixture installs every registry entry twice, executes every start/stop/true-end flow through live GTK, invokes every saved provider command and checks exact pane notifications. Pi, Amp, Cursor and Rovo Dev remain separate-format work.

### OpenCode lifecycle plugin

OpenCode uses its native JavaScript plugin API rather than pretending to support a JSON hook table. Setup atomically installs an owner-generated ESM plugin under the configured OpenCode `plugins` directory and adds its relative path to `opencode.json` without removing user fields or other plugins. The bounded plugin watches created/updated/idle/deleted session events, submits only native identity and working-directory metadata to the exact inherited cmux surface, and uses a five-second synchronous child deadline. The shared handler persists `opencode --session SESSION_ID`, routes idle attention and clears a true deletion/archive checkpoint. Actions imports and executes the installed plugin with Node against live GTK, verifies byte-stable reinstall and user plugin preservation, invokes the saved provider command, and observes exact notification and cleanup behavior.

### Cursor flat lifecycle hooks

Cursor Agent uses its native flat `.cursor/hooks.json` contract. Setup preserves unrelated fields and handlers, writes schema version 1, and idempotently installs cmux-owned `beforeSubmitPrompt` and `stop` commands with the same size, symlink, atomic-write and durable-sync boundaries as the other providers. The bounded handler accepts Cursor's conversation identity and working directory aliases, persists `cursor-agent --resume SESSION_ID` on the exact originating surface, and routes stop messages into that surface's inbox. Cursor exposes no durable session-end event, so stop retains the binding. Actions installs twice, executes both generated handlers against live GTK, executes the saved provider command and verifies exact notification targeting.

### Pi lifecycle extension

Pi uses its native auto-discovered TypeScript extension under `${PI_CODING_AGENT_DIR:-~/.pi/agent}/extensions`, not a fabricated hooks table. Setup writes only an owner-marked `cmux-session.ts`, refuses an unowned file or symlink, and atomically syncs an idempotent generated module. Its bounded, detached five-second dispatch registers Pi's `session_start`, `before_agent_start`, `agent_end`, and `session_shutdown` callbacks, forwarding only the native session identity, current directory and bounded completion reason through the inherited exact surface. The prompt callback records the per-turn Git baseline. The shared handler stores `pi --session SESSION_ID`, publishes completion attention and conditionally clears the matching checkpoint on shutdown. Actions imports the generated ESM without Pi dependencies, invokes all callbacks against live GTK and executes the exact saved provider command.

### Amp lifecycle plugin

Amp uses its experimental native TypeScript plugin API under `~/.config/amp/plugins`. Setup owns only a marker-bearing `cmux-session.ts`, refuses an unowned file or symlink, and atomically syncs repeatable content. The generated dependency-free module registers `session.start` and `agent.end`, resolves the event, context, or root thread identity in that order, and dispatches through the inherited exact surface with a five-second child deadline. Start stores `amp threads continue SESSION_ID`; completion publishes attention while deliberately retaining the durable thread binding because Amp exposes no terminal session-end callback. Actions imports and invokes both native callbacks against live GTK and executes the exact continuation argv.

### Rovo Dev YAML lifecycle hooks

Rovo Dev uses the native `~/.rovodev/config.yml` `eventHooks.events` schema. The structure-aware line merger preserves unrelated YAML and handlers, removes only a complete cmux marker block, supports existing `eventHooks`/`events` parents and atomically syncs the bounded result. It installs `on_tool_permission`, `on_complete`, and `on_error`. Because those payloads omit the durable ID, the handler scans at most 256 regular session directories under `CMUX_ROVODEV_SESSIONS_DIR`, configured `sessions.persistenceDir`, or the default directory; reads at most 64 KiB of metadata per candidate; matches the canonical working directory; and chooses the newest metadata/context timestamp. It stores `acli rovodev run --restore SESSION_ID`, publishes exact-surface completion attention and retains the binding. Actions verifies idempotent YAML preservation, unrelated/newer-session rejection, exact argv and live GTK notification routing.

### OMP and Campfire lifecycle extensions

Current-upstream OMP and Campfire use Pi-compatible extension APIs but distinct configuration roots and ownership rules. Linux installs owner-marked modules in OMP's resolved `PI_CODING_AGENT_DIR` or `PI_CONFIG_DIR` agent root and Campfire's `CAMPFIRE_CODING_AGENT_DIR` or default agent root. Both capture native session identity, prompt boundaries and completion attention and restore with `--session`. OMP pins the first top-level session, ignores nested session events and adopts explicit session switch/branch events. Campfire accepts only the explicit host role so join capability URLs cannot become restore commands; its bounded observer bridge publishes join, permission and relay attention summaries without prompt or invite content. The Actions fixture executes the generated modules through Node and verifies idempotence, exact argv and GTK routing.

### Kiro and Antigravity lifecycle hooks

Kiro's generated custom-agent JSON keeps user metadata and explicit tool selection while installing direct `command`/`timeout_ms` entries for `agentSpawn`, `userPromptSubmit` and `stop`; new files receive the required name, description and usable tool defaults. `KIRO_HOME` correctly resolves through its `agents` child. The saved command is `kiro-cli chat --resume-id SESSION_ID`. Antigravity preserves every user top-level hook group and replaces only its owned `cmux` group in `~/.gemini/config/hooks.json`, using direct typed command entries and ten-second timeouts for start, pre-invocation, stop and notification. It resumes through `agy --conversation SESSION_ID` and treats its turn-level end semantics as non-destructive. A combined Actions fixture verifies both native schemas, idempotence, prompt baselines, exact resume argv and surface attention.

### Surface move and drag topology checkpoint

`surface.reorder`, `surface.move` and `surface.drag_to_split` now transfer stable terminal and browser widgets inside a workspace without restarting their native process or changing their UUID. An emptied source pane collapses through the existing split tree, and directional split movement inserts the existing tab directly without a placeholder terminal. Native GtkNotebook pointer reordering synchronizes the model and requests a durable session snapshot instead of leaving visual and restored order inconsistent. Each mutation records bounded identity, pane, position and focus fields without terminal or browser content.

The Actions scenario reorders a restored terminal/browser tab pair, rejects an invalid destination atomically, moves the live browser across panes, exercises its daemon, turns the same browser into a directional split, and restarts cmux to verify the three-pane topology and unique browser identity. It then transfers the live browser into another workspace without focus theft, moves a sole local terminal while removing its emptied source workspace, and restarts again to verify both identities and ownership. Notification records follow a cross-workspace surface. A browser whose local/remote proxy route changes retires only its old daemon and lazily restores the retained URL/profile through the destination route; same-route managers remain live.

Each tab label now publishes its stable surface UUID as a native GTK move drag. A pane-center drop resolves the current tab position and transfers or reorders it, the nearest outer quarter creates a left/right/up/down split, and a workspace sidebar-row drop targets that workspace's focused pane. Drop actions resolve ownership at delivery time and log bounded rejection reasons. Direct pointer interaction still needs executable GTK coverage. Moving the final surface out of a remote workspace is rejected because the current SSH reconnect task is workspace-owned; transferring that bridge lifetime without interrupting the terminal remains required. This checkpoint therefore advances but does not close the July matrix row.

### Agent-accessible diff surface checkpoint

`cmux diff` now accepts a regular unified patch file or piped UTF-8 input and Git `unstaged`, `staged`, or merge-base `branch` sources. It writes an owner-only, self-contained viewer under the persistent cmux data directory, opens it with the existing independently owned browser surface, and moves that stable surface into a new pane immediately to the right of the caller. Default opening restores the previously focused terminal; `--focus` selects the viewer. The resulting surface retains the normal `cmux browser` snapshot, evaluation, click, screenshot and navigation controls and survives session restoration as a file URL.

The Linux viewer uses a small embedded DOM renderer rather than importing upstream's macOS WKWebView scheme bridge and 34 MiB generated web bundle. It exposes a file list, searchable content, split/unified controls and 4,000-line paging so one large patch cannot create an unbounded DOM. Input is capped at 32 MiB. Git writes to owner-only temporary files, is polled for output overflow, and is killed after 30 seconds; generated pages retain at most 64 files or 256 MiB. Patch JSON escapes script delimiters before embedding. The initial Actions fixture checks FIFO rejection, injection escaping, Git collection, focus/topology, browser identity and restart. A second fixture uses the pinned real Chromium integration to inspect and operate the rendered DOM through cmux's browser API.

Prompt lifecycle hooks now record the current tracked, staged and untracked repository state in a temporary private Git index and update a per-surface latest ref plus a hashed provider-session ref. `cmux diff --last-turn` snapshots the current state the same way and compares the trees, leaving the user's index and working tree untouched. Baseline capture shares one four-second hook deadline, rejects more than 10,000 changed paths or 256 MiB of current changed regular files, and remains best effort so a non-Git directory or oversized repository cannot break the agent hook. A missing baseline opens an empty view.

This checkpoint remains incomplete parity evidence until Actions passes. Durable line review comments and the separate dedicated project view also remain.
