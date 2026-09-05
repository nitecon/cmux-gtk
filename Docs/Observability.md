# Observability and benchmarks

Status: required implementation scope for the active refactor. Existing diagnostics write lifecycle strings and panic backtraces; that is insufficient to claim end-to-end visibility.

## Operation correlation

Trace an operation from CLI or UI entry through socket validation, queue wait, GTK dispatch and completion. Where it calls browser or SSH services, carry the correlation through that request and report transport versus execution time separately. Use stable operation names, request/trace IDs, workspace/surface identifiers, monotonic durations and explicit success/error/cancelled outcomes. Do not use user command text or URLs as metric labels.

Structured records identify schema version, build version, process, component and event. Keep protocol stdout machine-readable: diagnostics go to a separate stream or file. Instrument actual completion rather than declaring success when a message is enqueued. Document boundaries where an external service cannot propagate a trace.

## Resource and performance coverage

Measure resident memory, threads, descriptors, live terminals, remote PTYs, browser frames received/rendered/dropped, render timing, event-loop delay, queue depth/backpressure and session-save duration. Sampling must be bounded and low overhead. Log transitions/errors; aggregate frequent events instead of writing every frame or keystroke. Diagnostic output needs bounded retention and an observable dropped-record counter.

Provide a discoverable CLI diagnostic snapshot and a repeatable collection workflow for issue reports. Exclude terminal content, clipboard contents, secrets and full environment dumps. Configuration controls belong in persisted application settings with explicit overrides where needed.

## Benchmark evidence

Run repeatable optimized workloads in CI for startup, command round trips, workspace/split churn, sustained redraw and representative SSH/browser operations. Record revision, build profile, hardware/runner, GTK/backend, workload, warmup and iteration counts. Report throughput, duration percentiles and memory before/after or slope. Preserve raw machine-readable results as artifacts so regressions can be compared across revisions.

Distinguish software-rendered CI measurements from physical GPU/Wayland measurements. Establish baselines before adding performance gates. Functional lifecycle bounds can be asserted; shared-runner timing thresholds need evidence to avoid flaky tests. Demonstrate correlation and bounded diagnostic overhead through executable tests, including failure/cancellation paths.

The applicable gateway performance and logging principles are recorded in [Patterns](CodingStandards/Patterns.md). This scope does not require deploying a collector or external monitoring service to use cmux.
