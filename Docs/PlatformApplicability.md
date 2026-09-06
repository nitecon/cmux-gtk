# Mobile, Iroh, and cmux-tui applicability

This assessment compares cmux-linux with upstream main at
[`93f58f8d6d3d513fad0cd1645fa214a35741e56d`](https://github.com/manaflow-ai/cmux/tree/93f58f8d6d3d513fad0cd1645fa214a35741e56d).
It records the Linux boundary explicitly so platform-specific work is not silently counted as parity.

## Mobile client and host

Upstream mobile is an iOS SwiftUI client paired with a desktop host. Its public contract includes authenticated device registration, expiring pair grants, mobile workspace and notification synchronization, terminal render-grid streaming, browser/artifact lanes, reconnect cursors, and Apple-specific lifecycle and Keychain custody. The phone UI, SwiftUI rendering, iOS background behavior, Simulator streaming, Bonjour declarations, and Apple Keychain implementation do not apply to a GTK Linux desktop executable.

The desktop host role is portable in principle, but it is not an isolated terminal feature. Upstream authorization depends on its Stack account/device registry, signed same-account grants, EndpointID generation, relay policy, revocation, and mobile protocol negotiation. cmux-linux has none of those account or mobile-client contracts. Exposing a superficially compatible listener without that authority would create an unauthenticated substitute, so this repository will not claim mobile-host parity by adding an open socket or copying Apple-only RPC names.

If cmux-linux later adopts the upstream mobile ecosystem, the required work is a separately versioned Linux host service implementing the existing authenticated mobile protocol and render-grid data plane. It must interoperate with the released client, retain bounded per-peer lanes and reconnect cursors, use Linux secret storage with equivalent device-only semantics, and include revoke/expiry and end-to-end transport tests. That program requires a real mobile client and control-plane test environment; it is not a prerequisite for GTK desktop capability parity.

Sources: upstream [mobile state sync v2](https://github.com/manaflow-ai/cmux/blob/93f58f8d6d3d513fad0cd1645fa214a35741e56d/docs/mobile-state-sync-v2.md), [iOS mobile plan](https://github.com/manaflow-ai/cmux/blob/93f58f8d6d3d513fad0cd1645fa214a35741e56d/docs/ios-swift-mobile-plan.md), and [Iroh transport architecture](https://github.com/manaflow-ai/cmux/blob/93f58f8d6d3d513fad0cd1645fa214a35741e56d/docs/iroh-app-transport-architecture.md).

## Iroh transport

Upstream uses Iroh as the encrypted peer transport for the mobile application. Endpoint IDs authenticate keys, while separately signed cmux grants authorize an account and capability scope. Relay URLs, direct addresses, Bonjour records, and VPN addresses are reachability hints only. The design explicitly rejects using network location as authority and does not expose arbitrary private-network resources.

Iroh is therefore a transport choice rather than a standalone desktop feature. The current Linux SSH and Mosh workspace paths already provide authenticated remote terminals, reverse CLI relay, remote listener discovery, browser routing, reconnect, and bounded diagnostics. Replacing those working paths with Iroh would not by itself add an upstream user capability. An Iroh lane becomes applicable only with the authenticated mobile host described above or with adoption of upstream's independently versioned cmux-tui remote protocol. At that point, Linux should use the native Rust implementation and preserve EndpointID plus grant admission; it must not use a relay URL or endpoint hint as authorization.

## cmux-tui

`cmux-tui` is a separate cross-platform product and protocol stack on upstream main. It owns terminals and sessions, exposes the public `cmux.protocol/2` resource API, and separately negotiates private mux protocol 12 and authenticated remote protocol 5. Upstream explicitly says those protocols do not share envelopes, IDs, capabilities, or version numbers. Its cloud daemon is also intended to replace upstream's older Go remote daemon, with structured styled-grid snapshots and reconnect cursors instead of raw-byte replay.

Embedding or vendoring that application into cmux-linux would duplicate the GTK terminal owner, session model, CLI, and provider system. It is not a missing GTK view. The applicable user outcomes are already represented here by the native GTK terminal, v2 Unix-socket API, persisted layout/scrollback, SSH/Mosh workspaces, and the authenticated reverse CLI relay. Those protocols remain intentionally distinct; cmux-linux does not advertise `cmux.protocol/2`, mux protocol 12, or remote protocol 5 compatibility.

There is one relevant future migration: replace `cmuxd-remote` only if cmux-linux adopts upstream's cmux-tui daemon as an external negotiated backend. That requires protocol-5 enrollment, generation changes, styled snapshot reconciliation, cursor-based lane replay, binary provenance, and compatibility tests. Until that complete boundary exists, the Go daemon remains the smaller Linux SSH helper and its limitations stay documented in [the remote daemon contract](../daemon/remote/README.md).

Sources: upstream [programmability contracts](https://github.com/manaflow-ai/cmux/blob/93f58f8d6d3d513fad0cd1645fa214a35741e56d/cmux-tui/spec/README.md), [remote daemon protocol](https://github.com/manaflow-ai/cmux/blob/93f58f8d6d3d513fad0cd1645fa214a35741e56d/cmux-tui/spec/remote-daemon.md), and [cloud daemon design](https://github.com/manaflow-ai/cmux/blob/93f58f8d6d3d513fad0cd1645fa214a35741e56d/docs/cloud-cmux-tui-daemon.md).

## Parity decision

The iOS UI and Apple host plumbing are platform-specific and excluded from the Linux desktop deliverable. Iroh and cmux-tui remain explicit future integration boundaries, with the authentication, protocol, and verification requirements above. No compatibility is claimed today. This closes the assessment row while preserving the desktop-side work needed if either ecosystem becomes a product requirement.
