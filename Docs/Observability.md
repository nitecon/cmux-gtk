# Observability and benchmarks

Status: required implementation scope for the active refactor. Existing diagnostics write lifecycle strings and panic backtraces; that is insufficient to claim end-to-end visibility.

Implemented first stage: `cmux diagnostics --json` returns procfs resource samples and writer health. `cmux --verbose ping` prints a trace UUID and client round-trip time; JSONL records at the normal diagnostic path correlate that UUID with GTK queue wait, dispatch duration and response outcome. The GTK dispatch event means the handler returned, not necessarily that an asynchronous browser operation completed. Completion is separately recorded when the response arrives. SSH/browser internal propagation remains pending.

The diagnostic worker has 128 queue slots, a 64 KiB record limit, and two files of up to 8 MiB for newly written logs (active file plus `.1`). Queue overflow drops records and increments the snapshot counter. Five-second resource sampling runs off GTK. Startup also trims oversized logs inherited from earlier versions, retaining complete trailing records within the same cap. Existing unstructured stderr output is still being audited; this change does not claim all output is structured or bounded.

## Operation correlation

Trace an operation from CLI or UI entry through socket validation, queue wait, GTK dispatch and completion. Where it calls browser or SSH services, carry the correlation through that request and report transport versus execution time separately. Use stable operation names, request/trace IDs, workspace/surface identifiers, monotonic durations and explicit success/error/cancelled outcomes. Do not use user command text or URLs as metric labels.

Structured records identify schema version, build version, process, component and event. Keep protocol stdout machine-readable: diagnostics go to a separate stream or file. Instrument actual completion rather than declaring success when a message is enqueued. Document boundaries where an external service cannot propagate a trace.

## Resource and performance coverage

Measure resident memory, threads, descriptors, live terminals, remote PTYs, browser frames received/rendered/dropped, render timing, event-loop delay, queue depth/backpressure and session-save duration. Sampling must be bounded and low overhead. Log transitions/errors; aggregate frequent events instead of writing every frame or keystroke. Diagnostic output needs bounded retention and an observable dropped-record counter.

Provide a discoverable CLI diagnostic snapshot and a repeatable collection workflow for issue reports. Exclude terminal content, clipboard contents, secrets and full environment dumps. Configuration controls belong in persisted application settings with explicit overrides where needed.

## Benchmark evidence

The initial executable benchmark is `scripts/benchmark-cmux.py`. CI builds optimized binaries, launches an isolated application through the diagnostics fixture, warms ten CLI calls and measures 100 sequential pings. The revision-named artifact contains every latency sample, median/p95/p99, throughput, process resources before/after, GTK version, requested display backend and host metadata. No arbitrary latency gate is applied. This measures CLI startup plus transport/GTK response; sustained rendering, workspace churn and SSH/browser benchmark scenarios remain to be added.

Run repeatable optimized workloads in CI for startup, command round trips, workspace/split churn, sustained redraw and representative SSH/browser operations. Record revision, build profile, hardware/runner, GTK/backend, workload, warmup and iteration counts. Report throughput, duration percentiles and memory before/after or slope. Preserve raw machine-readable results as artifacts so regressions can be compared across revisions.

Distinguish software-rendered CI measurements from physical GPU/Wayland measurements. Establish baselines before adding performance gates. Functional lifecycle bounds can be asserted; shared-runner timing thresholds need evidence to avoid flaky tests. Demonstrate correlation and bounded diagnostic overhead through executable tests, including failure/cancellation paths.

The applicable gateway performance and logging principles are recorded in [Patterns](CodingStandards/Patterns.md). This scope does not require deploying a collector or external monitoring service to use cmux.
