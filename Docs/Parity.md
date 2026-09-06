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
