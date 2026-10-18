# amux direction

Status: research and product direction, recorded 2026-10-18. This document does
not approve a rewrite, rename, stack selection, or packaging change. The current
implementation remains cmux-gtk; [Architecture](Docs/Architecture.md) and
[Components](Docs/Components.md) describe its actual ownership boundaries.

## Product requirements

amux is the proposed direction for a terminal workspace application on Linux,
Windows, and macOS, with agents able to coordinate work locally and, optionally,
across machines. Preserve workspaces, splits, sibling surface tabs, terminal
input, browser integration, agent lifecycle hooks, session recovery, and the
scriptable interface when evaluating a replacement. Existing capability and
verification records live in [Parity](Docs/Parity.md).

The owner's clarification establishes these requirements:

- Local terminals and local agent coordination work without a gateway.
- An optional, independent gateway provides durable coordination across machines.
- The gateway supports shared tasks, memory, documents, and files.
- Notification fan-out is optional; task state must remain usable without it.
- amux exposes gateway capabilities directly. Installing or invoking agent-tools
  is not required to use them.

Implementation language, frontend toolkit, terminal library, and whether the
gateway is bundled with the application remain proposals.

## Local host and optional gateway

The proposed local background host owns PTYs on Unix and ConPTY sessions on
Windows, agent process identity, local coordination, and session lifecycle. The
frontend attaches to this host. A frontend restart could then preserve running
processes, unlike restoration that relaunches a shell. This behavior requires a
prototype; current session persistence is not evidence that it already exists.

The optional gateway is a separate service boundary. It owns durable shared task
state, claims, comments, completion history, and shared memory, documents, and
files for its configured namespace. Local hosts continue to own their processes
and terminal sessions. A gateway is not required to start the application or to
coordinate agents on one machine.

Use one authority for each task namespace. A local task belongs to local
coordination; a shared task belongs to its gateway. The proposed implementation
must make that distinction visible and avoid maintaining two writable copies of
the same task. Gateway disconnection must not stop local terminals. Shared work
must not acquire competing ownership through automatic offline assignment.

As a storage proposal, a coordinator can use SQLite with atomic claim and event
updates. Clients communicate with the owning service; they do not share its
database file across machines. [SQLite's network guidance](https://sqlite.org/useovernet.html)
explains why a service boundary is preferable to network filesystem access.
Leader election and replicated conflict resolution are not prerequisites for
this direction.

An optional notifier distributes fresh events to connected hosts and agents.
Durable task history remains available through queries even when notification
delivery is disabled or interrupted. Preserve actor/session identity, explicit
input permissions, human input fences, bounded delivery, and protection against
replaying stale notifications into terminals.

## Direct capabilities and agent-tools compatibility

amux needs a first-class interface for connecting to a gateway, inspecting and
updating tasks, claiming and completing work, reading and writing shared memory
and documents, and transferring shared files. These capabilities must be
available through amux's own supported interface without shelling out to
agent-tools. Exact CLI commands, UI placement, API schemas, authentication, and
artifact transfer contracts remain to be designed.

agent-tools can remain an optional compatible client. A future endpoint adapter
should use the same authoritative service and task identities rather than
creating another task board or copying the whole agent-tools implementation.
The earlier research's agent-tools adapter recommendation is therefore an
integration option, not a dependency for core amux functionality.

Gateway deployment may be supplied separately or offered alongside amux. Its
independent service boundary does not settle whether installers bundle it,
whether an existing gateway is reused, or which project owns implementation.
Changes to another repository require a delegated task to that project.

## Stack options from research

The research recommended evaluating a Go background host with a separate GPU
frontend. Go is a candidate, not a requirement; keeping both components in Rust
also deserves comparison against the current code and integration cost.

| Candidate | Reason to evaluate | Evidence still needed |
| --- | --- | --- |
| Rust with GPUI | GPU frontend candidate; [GPUI](https://gpui.rs/) provides the toolkit | Terminal rendering, native input, accessibility, and packaging on all three OSs |
| Go with Gio | Alternative for a Go host/frontend stack; [Gio GPU package](https://pkg.go.dev/gioui.org/gpu) | Terminal widget completeness, native integration, and performance |
| Rust with Slint | Another frontend option from the research | Terminal rendering integration, licensing fit, and native behavior |
| C++ with Qt Quick | Existing scene graph and browser integration options; [Qt rendering architecture](https://doc.qt.io/qt-6/qtquick-visualcanvas-scenegraph.html) | Distribution cost, terminal embedding, and extension compatibility |
| Webview with a terminal frontend | Familiar UI composition and an alternative terminal integration path | Input fidelity, rendering cost, browser boundaries, and process ownership |

For the terminal core, evaluate a pinned adapter around libghostty-vt against the
current embedded Ghostty surface/runtime. Its [public header](https://github.com/ghostty-org/ghostty/blob/main/include/ghostty/vt.h)
describes terminal parsing and state and explicitly warns that the API is
unfinished and unstable. It is not a complete terminal widget: rendering, font
handling, IME, selection, clipboard images, accessibility, and PTY integration
still need implementation or another proven component. The earlier research
identified [Go bindings](https://tangled.org/mitchellh.com/go-libghostty/tree/main)
as another option to validate, not proof of a finished cross-platform terminal.

Keep browser integration behind its own service boundary. A toolkit's browser
support does not establish password-manager, extension, native messaging, or
shared-login compatibility. Those require actual integration evidence.

## Proposed next validation

Only the direction document is authorized by this task. A separately approved
prototype should demonstrate a representative terminal, host detach/reattach,
and local agent delegation on Linux, Windows, and macOS before selecting a stack.
Then demonstrate optional gateway coordination between two machines, including
atomic claims, durable comments/completion, shared artifacts, and reconnect
without stale terminal injection.

Compare typing latency, idle CPU, memory under terminal churn, rendering under
output load, resize, Unicode/IME, selection, clipboard images, accessibility, and
process cleanup. The owner's Windows Codex image-paste check succeeded; Linux
snipping-tool paste under X11 and Wayland remains unverified. Preserve working
native behavior while investigating missing cases.

Run executable verification through GitHub Actions as required by this project.
Prototype results must establish capability parity and identify gaps; neither
this research nor a successful build establishes readiness for a rewrite.
