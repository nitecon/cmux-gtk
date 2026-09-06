# Workspace attention and desktop bells

The GTK port tracks attention per workspace when a background terminal rings its bell. The sidebar marks the workspace; desktop delivery uses the Linux notification service through `notify-send`, subject to focus and rate-limit policy.

Read attention without moving focus:

```bash
cmux list-notifications --json
```

The response lists workspace UUID, name and `has_attention`. It is a workspace-state snapshot, not a notification inbox with message bodies or read/unread entries.

Clear a workspace's attention without selecting it:

```bash
cmux clear-notification <workspace-uuid>
```

Desktop delivery has bounded concurrency and execution time. A successful helper exit does not establish that the desktop displayed a notification. See [Observability](../Docs/Observability.md) for delivery outcomes, resource bounds and the native bell benchmark.

Agent-authored notifications, an inbox, pane highlighting, notification-click routing and resume hooks remain in the separately deferred agent-capability task. The inherited macOS `cmux notify` and AppleScript examples did not describe commands implemented by this GTK port and have been removed.
