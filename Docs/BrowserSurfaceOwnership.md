# Independent browser surfaces

Implementation plan for the active parity goal, based on the September 6, 2026 source audit. This document describes required changes, not implemented behavior. The existing external browser adapter and bounded worker contracts remain in force.

## Confirmed gaps

`AppState.browser_manager` owns one daemon session for all browser surfaces. `wire_browser_tab` replaces that manager's frame reader when another picture is wired. `MappedNavigation` opens the newly mapped tab's saved URL, which recreates page state instead of selecting an independent page. Thus two visible browser panes cannot each retain their own DOM, history and preview stream.

Socket startup records the daemon's `id` or `surface_id`, falling back to `unknown`, in `browser_surface_refs`. These values are not the browser UUIDs in `SplitEngine.browser_tabs()`. `resolve_surface_ref` accepts arbitrary direct strings without checking the live pane tree. Passing such a value to the daemon is not proof that a requested GTK surface received the operation.

## Ownership change

Keep `BrowserManager` as the concrete owner of one external session and its navigation, input and stream tasks. Move application ownership to a map keyed by the existing browser surface UUID. Allocate the UUID before asynchronous startup and retain a provisional entry until the corresponding pane is installed. Each completion must match surface UUID, owning workspace UUID and manager session identity. Failed or cancelled startups retire their provisional entry and close any launched session using the existing bounded shutdown drain.

Use the same creation path for UI open, socket open and restored panes. A restored browser surface uses its saved UUID and URL. Starting another surface must create another manager instead of calling open on a manager belonging to a different pane. Restore admission must be bounded; defer unopened background surfaces where possible rather than launching every saved browser at once. The public browser-reference map is a convenience index into live GTK UUIDs, never a map of external daemon IDs.

Each widget callback must resolve its captured surface UUID to its manager. That includes keyboard releases, pointer motion, clicks, wheel, URL entry, history buttons, resize, DevTools snapshot and preview delivery. Remove map-time URL reopening: showing an existing surface resumes its view without navigating. If a widget is closed or replaced, its callback must fail without falling back to the currently selected surface.

Socket commands validate explicit UUID/short-ref targets against live browser surfaces before obtaining the matching manager. Omitted targets follow an explicit documented selection rule. Browser list returns owning workspace and stable surface identity. A single-surface close retires only that manager and its short refs; workspace close retires its browser surfaces; application quit drains all remaining closes. The existing post-GTK bounded shutdown join remains the final owner of close tasks.

Independent browser sessions can increase Chromium memory use. Bound startup concurrency and measure total application plus owned browser process resources. Record admitted, pending, cancelled and closed sessions without page contents. Do not call a manager count or an application-only RSS sample proof of total browser memory bounds.

## Verification gates

1. Open two pages through the real cmux CLI in one workspace and two workspaces. Keep the terminal selected. Verify distinct stable UUIDs and exact workspace placement.
2. Mutate a DOM counter and input in each page, navigate one page twice, switch pane/workspace selection repeatedly and verify both DOM states and independent back/forward history.
3. Render two simultaneously visible previews with distinct content. Verify input and resize target the intended page; closing either leaves the other usable.
4. Reject unknown UUIDs, stale short refs and closed-surface operations without sending a command to another browser. Cancel or close a workspace during delayed startup and verify owned processes/tasks are retired.
5. Quit/reopen multiple browser surfaces, retaining UUIDs and URLs while clearly distinguishing reconstructed pages from live DOM restoration. Repeated startup/close churn must return manager, task, descriptor and owned-process counts to baseline.
6. Run optimized workloads with the pinned external browser, retain raw resource/latency artifacts, and provide the user's local validation command only after the full parity goal passes its gates.

The first implementation step is the UUID-keyed owner and shared creation/retirement path. Callback migration and socket routing must land with executable multi-surface coverage before independent browser parity is claimed.
