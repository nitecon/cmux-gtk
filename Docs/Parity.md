# Upstream capability parity

Active goal, started September 6, 2026. This replaces the earlier decision to defer the capability work; it does not reopen the completed structural refactor. The existing [monthly review](../docs/research/upstream-2026-09.md) supplies release chronology. GitHub still reports [v0.64.22](https://github.com/manaflow-ai/cmux/releases/tag/v0.64.22), published August 3, as latest stable. Current-main capability discovery is pinned to [e36b8e8632a414e2982185f8dae4002a98be2b53](https://github.com/manaflow-ai/cmux/tree/e36b8e8632a414e2982185f8dae4002a98be2b53); main-only capabilities must not be represented as shipped in an earlier month.

## Delivery and evidence matrix

Every row requires implementation and executable CI evidence before completion. Existing functionality is a starting point, not proof of complete parity. Linux adaptations preserve useful behavior through GTK and the platform library. Native macOS implementation details are not copied. No release tag precedes local user validation.

| Period / source | Capability | Current GTK baseline | Required work / acceptance |
| --- | --- | --- | --- |
| March | SSH workspaces, automatic forwarding | Real SSH daemon, inherited launches, reconnect and restart tested | Discover and forward remote listeners; browser localhost/subresource requests must originate through the selected remote workspace; validate split/tab inheritance and route changes. |
| March / May | cmux.json project actions and launch configuration | Named local/script/SSH launches exist | Project configuration schema, command palette/actions and CLI/socket configuration; explicit one-time remote initial command and script semantics. |
| April | Listening ports | No attributed discovery model | Attribute listeners to owned terminal descendants, clear on exit, distinguish local/remote provenance; expose sidebar and API. |
| May | Reliable session / agent restoration | Layout, identities, names, colors, launch descriptors and URLs; shells restart fresh | Final snapshot before teardown, serialized durable quit save, immediate mutation/quit/reopen test; bounded scrollback across untouched background workspaces; manual prior-session restore; per-surface resume bindings and automatic/manual controls. |
| May / current README | Agent hooks | No persisted native agent sessions | Native IDs, environment-aware resume and setup integration for advertised agents; custom surface resume set/show/clear; stale-binding protection; stop/notification routing; lifecycle traces and restore benchmark. |
| May | Copy-on-select / workspace order and colors | Linux PRIMARY, standard copy/paste, saved order/colors tested | Preserve while extending batch reorder/configuration and keyboard controls; verify selected identity/focus. |
| May–June / README | Notification inbox / highlighting | Workspace BEL attention and bounded desktop helper | Per-surface inbox, OSC9/99/777 including chunk completion, notify CLI/socket, read/clear/dismiss, unread navigation, focused suppression and exact click targeting; retained history and bounded data. |
| February baseline / June | Sidebar metadata and project/diff views | Working directory/location subtitle | Branch/PR/MR, status/progress/markdown metadata, description and latest notification; automatic Git updates during foreground work; project/diff views and API with bounded worker I/O. |
| June | Collapsible workspace groups | Flat workspace list | Persistent group membership/order/color/collapse, unread aggregation, keyboard/CLI/UI operations preserving workspace identity. |
| July | Workspace/surface reorder shortcuts | Individual workspace controls | Batch and keyboard reorder, pane/surface move and drag topology with correct ownership, focus and remote routing. |
| July–August | Mosh / remote resume | SSH only | Implement applicable resilient remote transport and resume behavior; separately verify local replacement/EOF semantics rather than inferring them from retired macOS fixtures. |
| Current README / preserved contracts | Agent browser access | External agent-browser transport, DOM operations, snapshots and preview tested | Audit complete command/target matrix; nearest-right opening, local files, independent surface state, navigation history, screenshots, remote routing, browser profile import; preserve focus and bounded resource ownership. |
| Current README | Agent teamwork | Programmable panes only | Native teammate split workflow, metadata and notification wiring without an unrelated terminal multiplexer dependency. |
| Current README | Window/session fidelity | One GTK window, saved geometry | Audit multi-window controls and restore semantics; implement applicable behavior and native visual lifecycle tests without reusing missing AppKit APIs. |
| July–August research | Mobile/Iroh/TUI | No implementation | Assess public contracts and Linux applicability explicitly. Mobile/macOS UI code is platform-specific; any desktop-side integration requirement must be recorded rather than silently removed from parity. |

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
