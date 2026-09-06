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
