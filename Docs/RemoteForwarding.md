# Remote listener forwarding

This is the implementation contract for the outstanding remote-forwarding parity row. Listener discovery and an initial automatic forwarding implementation are present; runtime verification and the full gates below remain outstanding. Reuse the established workspace SSH transport and the daemon's `proxy.open`, `proxy.stream.subscribe`, `proxy.write`, and `proxy.close` operations. Do not create a second SSH process for every listener.

## Ownership

A forwarding supervisor belongs to one `run_proxy_routing` connection generation. It consumes validated current `SshBridge.listeners` observations. Maintain at most sixteen advertised local listeners per connection and sixteen accepted client streams in total. Local binds use IPv4 loopback only, prefer the remote port when available, and otherwise request an ephemeral local port. Publish the actual endpoint with remote address, port and originating workspace/surface identity. Two workspaces exposing the same remote port must remain distinguishable.

Each listener owns its accept loop and clients. Service disappearance stops accepting immediately and asks clients to close. Connection teardown cancels the supervisor and all client work; closing SSH stdin/transport causes the owned daemon to close every registered stream. Clear endpoint publication before a new connection generation is advertised. Never reuse a prior connection's forwarded endpoint as proof of current routing.

A client opens a remote proxy stream with the discovered destination, installs response routing before subscribing, and copies bytes in both directions. Map unspecified remote binds to the same address family's loopback destination. Preserve explicit remote bind addresses. Validation permits only destinations drawn from the current owned-listener observation, rather than arbitrary client-supplied proxy targets.

## Reader and backpressure contract

The shared SSH stdout reader carries terminal output, proxy data and RPC replies. It must not await a forwarded client's network write or a full client queue: a client may itself be waiting for an RPC reply later in that same reader. Route proxy data through per-client queues bounded to sixteen 32-KiB chunks. A queue overflow retires that client stream with an explicit diagnostic; do not silently discard bytes and continue a corrupted connection. Terminal output retains its existing backpressure behavior.

Keep client data routes separate from terminal `stream_to_pane` routes. Remove a route on EOF/error before notifying the consumer. Bound encoded proxy frames before decoding; the owned Go pump emits 32-KiB chunks. Client writes use the existing bounded correlated RPC transport. Connection count limits plus fixed copy buffers bound aggregate retained data.

## Cancellation and errors

`proxy.open` can mutate the daemon even when a local caller stops waiting. Listener removal must signal cancellation and let an in-flight open finish within the existing RPC deadline, then close any returned stream. Do not abort an open and forget a possible remote stream while retaining the same connection indefinitely. Once a stream ID is known, one owner issues its bounded close and removes local routing. Full connection teardown may abort that owner because daemon teardown is then authoritative cleanup.

Validate response stream IDs and generation before publishing success. Unknown, duplicate or replaced identities fail closed. No GTK borrow or bridge mutex may span network I/O. Listener/client tasks are tracked and reaped; no unbounded detached task or close-future registry.

## Browser routing

Forwarded endpoints support tools and ordinary local clients. Browser workspace-origin parity additionally requires all page subresources, redirects and WebSockets to follow the selected remote workspace; rewriting only the initial localhost URL cannot satisfy that requirement. Extend each surface-owned browser startup with a workspace-owned proxy configuration or a complete request routing adapter. Audit localhost bypass rules explicitly and preserve independent browser daemon identity. Do not mark browser remote-origin parity complete on evidence from a single rewritten document request.

## Verification gates

Actions must exercise a real remote HTTP server through the established SSH daemon, response bodies larger than one proxy chunk, simultaneous local clients and unrelated terminal input. Two workspaces must expose the same remote port with independent endpoint identity. Verify listener removal, active-client closure, delayed-open cancellation, reconnect invalidation, bounded overload and absence of retained daemon streams. Record setup latency, byte counts, failures and active listener/client counts without payloads. Browser tests must include remote-only subresources and a WebSocket, not merely a document title.

The desktop and remote sides already share bounded RPC identity/tracing; use these existing boundaries for forwarding diagnostics. Current sources are `src/ssh/tunnel.rs`, `src/ssh/bridge.rs`, `src/ssh/writer.rs`, `daemon/remote/cmd/cmuxd-remote/streams.go` and the [ports gateway context](../.agent/api/cmux-ports.yaml).

## Initial implementation checkpoint

`src/ssh/forward.rs` implements connection-owned discovery reconciliation, sixteen listeners/clients, at most thirty-two active or retiring listener tasks, bounded proxy queues and loopback endpoint publication. Port records expose nullable `forwarded_local_port`. Client retirement uses the established bounded outbound control channel for remote close; full connection teardown remains the authoritative cleanup fallback. An in-flight open settles before service-stop cancellation is applied. The real SSH fixture now transfers a payload larger than one proxy chunk through the published endpoint.

Runtime verification is pending. Remaining gaps include TCP half-close fidelity, multiple-workspace collision/reconnect and overload workload evidence, aggregate byte/active counters, and full browser origin/subresource routing. These are required follow-ups rather than claims covered by the initial fixture.

Forwarding route registration now rejects duplicates atomically without changing the existing owner. Capacity rejection closes the newly opened remote stream. Listener termination signals its client stop channel and removes only its matching endpoint before draining pending opens. The supervisor removes completed listener registrations and can recreate them while the service remains advertised. A route test verifies duplicate-owner preservation and explicit overload retirement; runtime verification pending.

Client-to-server half-close now uses daemon `proxy.shutdown_write`: local request EOF sends TCP FIN while retaining the response reader. The daemon accepts registered TCP connections only and leaves stream ownership intact. A real TCP Go test and the multi-chunk SSH fixture require response delivery after request EOF. Reverse-direction half-close behavior and compatibility with older daemons remain separate verification work; the existing daemon stream pump still retires a connection on remote read EOF.

Normal client completion and subscription failure now await an idempotent `proxy.close` response through the bounded request transport. Only `closed: true` confirms retirement. The owner remains live during that await, so cancellation or failure still queues the existing close fallback; connection teardown remains authoritative cleanup for interrupted transport. Diagnostics distinguish requested, confirmed and failed closes. The SSH fixture requires a confirmed close and no close failure after the transferred response; runtime evidence pending.

Forwarding subscriptions now opt into `half_close: true`. For TCP streams the daemon emits `proxy.stream.read_eof` on clean read EOF and retains the writable connection until explicit close. Existing subscriptions keep their previous full-retirement behavior. The desktop forwards FIN to the local response direction and waits for both transfer directions to finish, while errors or cancellation retire the connection. Each route reserves one termination slot in addition to sixteen data slots, preserving ordered EOF/error delivery when data capacity is full. Go real-TCP coverage verifies late writes after remote FIN; a Rust route test verifies EOF ordering behind sixteen chunks. The real SSH fixture also reads an early response through FIN before sending a multi-chunk request, checks the remote payload, acknowledged byte counts, confirmed close and unchanged workspace selection. Runtime verification of that new scenario and older-daemon capability negotiation remain outstanding.
