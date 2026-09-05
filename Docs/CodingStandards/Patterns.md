# Gateway pattern review

Reviewed latest active gateway patterns on 2026-09-05. Retrieve full entries by slug with `agent-tools patterns get`. These are guidance adapted to this desktop application, not a reason to introduce unrelated web infrastructure.

| Pattern | Applicable guidance | Project adaptation |
| --- | --- | --- |
| `rust-code-style` | rustfmt, small modules, narrow visibility, simple dispatch | User explicitly requires function documentation; this overrides the pattern's default of no comments. |
| `rust-unsafe-code` | Safe wrappers, specific SAFETY invariants, explicit unsafe operations | Audit owned FFI; generated bindings retain upstream conventions. |
| `rust-concurrency-async` | Bounded queues, cancellation, no blocking I/O or sync guards across awaits | GTK owns its main context; Tokio owns asynchronous transport. No additional runtime. |
| `rust-memory-ownership` | Borrow first, transfer ownership, avoid copies in hot loops | Add buffer pools or dependencies only when measurements justify them. |
| `rust-workspace-architecture` | Acyclic dependencies and documented library APIs | Extract real component boundaries; retain package version/release policy and avoid speculative crate proliferation. |
| `rust-dependencies` | Deliberate additions, minimal features and committed binary lockfile | Preview decoding uses `image` with only JPEG/PNG enabled for explicit resource limits; GTK still owns texture presentation. |
| `rust-performance-awareness` | Optimized benchmarks, before/after data, hardware/workload metadata | CI runs tests and benchmarks; avoid arbitrary latency thresholds on shared runners. |
| `go-observability-otel` | Correlation across process and asynchronous boundaries | Preserve CLI/socket/SSH protocol compatibility; no unrelated HTTP middleware, database or compulsory telemetry server. |
| `go-logging-zerolog` | Structured fields, operation context, main-owned process exit | Choose one coherent logging implementation when instrumenting the daemon; keep diagnostics off protocol stdout. |

Searches found no applicable Python, C or Zig language patterns and no shell coding pattern. Their standards use primary language/tool documentation. The gateway's ndesign/axum/chi web patterns do not apply to GTK.

KISS remains the controlling design rule: measure first, centralize repeated behavior, and do not import an entire service stack to obtain correlation IDs and bounded diagnostic output.
