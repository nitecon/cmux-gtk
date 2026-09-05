# Upstream cmux review: March 5–September 5, 2026

Reviewed September 5, 2026 against upstream GitHub release records and this Rust/GTK implementation. The latest stable release returned by GitHub was **v0.64.22, published August 3**. This report does not treat unreleased `main` changes as shipped September features. Release publication dates provide the timeline; individual PRs may have merged earlier.

## What upstream added

| Period | Evidence | Relevance to Linux |
| --- | --- | --- |
| March | [v0.63.0](https://github.com/manaflow-ai/cmux/releases/tag/v0.63.0) introduced SSH remote workspaces with automatic port forwarding ([#1296](https://github.com/manaflow-ai/cmux/pull/1296)), custom commands in cmux.json, and scrollback/focus fixes. | Strong fit: reusable remote sessions and explicit workspace launch commands. GTK already contains an SSH daemon/tunnel foundation, but initial creation alone is insufficient: splits and restore must preserve the remote destination. |
| April | [v0.63.2](https://github.com/manaflow-ai/cmux/releases/tag/v0.63.2) included remote listening-port detection. | Useful follow-up once remote lifecycle and bounded output handling are reliable. |
| May | [v0.64.0](https://github.com/manaflow-ai/cmux/releases/tag/v0.64.0) expanded workspace configuration/actions, selected-row colors, agent restoration and notification reliability. [v0.64.4](https://github.com/manaflow-ai/cmux/releases/tag/v0.64.4) restored SSH descriptors on relaunch. [v0.64.10](https://github.com/manaflow-ai/cmux/releases/tag/v0.64.10) added copy-on-select and batch workspace reorder. | Bring over behavior and persistence concepts using GTK/Ghostty APIs. Preserve existing Linux configuration rather than importing the macOS settings implementation. |
| June | [v0.64.11](https://github.com/manaflow-ai/cmux/releases/tag/v0.64.11) introduced collapsible workspace groups with ordering, colors and unread indicators; it also expanded project/diff views and notification UI. | Individual workspace ordering/colors first. Groups and project views are a separate UI project after this functional pass. |
| July–August | [v0.64.20](https://github.com/manaflow-ai/cmux/releases/tag/v0.64.20) added workspace/surface reorder shortcuts. [v0.64.21](https://github.com/manaflow-ai/cmux/releases/tag/v0.64.21) included Mosh transport, remote resume behavior, connection-state fixes, and work on iOS/Iroh/TUI. [v0.64.22](https://github.com/manaflow-ai/cmux/releases/tag/v0.64.22) fixed SSH startup-script handling. | Reorder controls and remote correctness apply directly. Mosh, mobile pairing and a separate TUI would each require their own transport/product scope. |

Colors and sidebar metadata were already present around the start of the review window: [v0.61.0](https://github.com/manaflow-ai/cmux/releases/tag/v0.61.0), published February 25, introduced workspace color schemes, PR metadata and a first session-persistence pass. Those are useful prior art, not new six-month additions.

## Implementation priorities

1. Linux clipboard semantics: Ctrl+Shift+C/V for the standard clipboard; selection ownership and middle-click for PRIMARY. Ghostty already supplies selection behavior. Correct the GTK embedding's request routing rather than duplicating terminal selection logic.
2. Named local, script and SSH workspaces with saved launch descriptors. Keep working directories native to the surface/remote PTY configuration. New terminal tabs and splits must inherit launch context.
3. Location subtitles with full-path tooltips; `/first/…/basename` for deep local paths and `ssh://target/path` remotely. Preserve user names independently of locations.
4. Stable workspace reordering and background colors, persisted across restarts. Moving a workspace must move its split engine with it and preserve active identity/focus.
5. Memory correctness before visual polish: bounded/coalesced replaceable updates and explicit cleanup of terminal, browser and remote resources.

## What to consider next

Remote port discovery/forwarding, richer remote connection errors, workspace groups, and an explicit launch-profile/configuration API offer concrete value. Add them only with end-to-end Linux coverage. Defer native macOS window/portal work, iOS/Iroh pairing, a new TUI, and wholesale browser/UI redesign: those implementations do not transfer directly to GTK and are not required for this pass.

## Memory investigation

Upstream release notes mention its own leaks, including [#4555](https://github.com/manaflow-ai/cmux/pull/4555) and [#9000](https://github.com/manaflow-ai/cmux/pull/9000). Those are not evidence that this Rust/GTK port has the same cause.

The local audit found unbounded browser JPEG and session-snapshot queues, retained browser stream consumers, and strong widget references in callbacks that can retain closed GTK trees. Corrections and regression coverage must be evaluated against this port. A green functional test alone cannot establish that the user's reported OOM is fully resolved; memory/churn evidence is also required.

The host kernel log confirms that cmux-app 0.1.7 was killed on September 5 at 00:38 EDT with 178,521,208 KiB anonymous RSS (about 170 GiB), roughly five minutes after startup. Its log showed four restored workspaces, one initialized 2375×1216 terminal, and continuous redraw wakeups. There was no browser initialization or split/unrealize activity in that interval. This is evidence against attributing the entire incident to browser queues or closed panes.

The installed GTK 4.22.4 contains an independently confirmed [GtkGLArea texture ownership leak](https://github.com/GNOME/gtk/commit/7ff233c7ff2a9949ffd28c9ff55500e1b7578e5e), also [reported against Ghostty](https://github.com/ghostty-org/ghostty/discussions/12888). The port disables the affected dmabuf path on audited GTK 4.16–4.22.4 versions and matches native Ghostty's desktop-GL compositor setup. This is a compatibility workaround, not proof that the observed OOM has the same cause. CI covers split/close RSS and 1,800 large terminal redraws; software X11 coverage cannot establish hardware Wayland driver behavior. Startup diagnostics include GTK version and renderer flags for follow-up reports.
