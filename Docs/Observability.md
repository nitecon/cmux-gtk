# Observability and benchmarks

Status: required implementation scope for the active refactor. Existing diagnostics write lifecycle strings and panic backtraces; that is insufficient to claim end-to-end visibility.

Implemented first stage: `cmux diagnostics --json` returns procfs resource samples and writer health. `cmux --verbose ping` prints a trace UUID and client round-trip time; JSONL records at the normal diagnostic path correlate that UUID with GTK queue wait, dispatch duration and response outcome. The GTK dispatch event means the handler returned, not necessarily that an asynchronous browser operation completed. Completion is separately recorded when the response arrives. SSH/browser internal propagation remains pending.

The diagnostic worker has 128 queue slots, a 64 KiB record limit, and two files of up to 8 MiB for newly written logs (active file plus `.1`). Queue overflow drops records and increments the snapshot counter. Five-second resource sampling runs off GTK. Startup also trims oversized logs inherited from earlier versions, retaining complete trailing records within the same cap. Existing unstructured stderr output is still being audited; this change does not claim all output is structured or bounded.

Formatting stops lifecycle messages at 4 KiB on a UTF-8 boundary and records a truncation flag. JSON serialization stops at the record limit instead of allocating an oversized serialized copy before rejecting it. Already-created caller payloads remain the caller's responsibility.

A one-second GTK heartbeat reports the last and maximum scheduling delay, plus sample age. Its atomics remain readable by background diagnostics during a GTK stall; increasing sample age indicates that the main loop has not serviced the probe. The first sample is explicitly unavailable until GTK ticks. Delay includes OS scheduling and application work, so it is a symptom rather than a root-cause classification. Aggregate RPC counters report in-flight, successful, failed and cancelled operations; a diagnostics request counts itself as in-flight. Counters are process-local and sampled independently, not a transactional accounting snapshot.

## Operation correlation

Trace an operation from CLI or UI entry through socket validation, queue wait, GTK dispatch and completion. Where it calls browser or SSH services, carry the correlation through that request and report transport versus execution time separately. Use stable operation names, request/trace IDs, workspace/surface identifiers, monotonic durations and explicit success/error/cancelled outcomes. Do not use user command text or URLs as metric labels.

Structured records identify schema version, build version, process, component and event. Keep protocol stdout machine-readable: diagnostics go to a separate stream or file. Instrument actual completion rather than declaring success when a message is enqueued. Document boundaries where an external service cannot propagate a trace.

## Resource and performance coverage

Measure resident memory, threads, descriptors, live terminals, remote PTYs, browser frames received/rendered/dropped, render timing, event-loop delay, queue depth/backpressure and session-save duration. Sampling must be bounded and low overhead. Log transitions/errors; aggregate frequent events instead of writing every frame or keystroke. Diagnostic output needs bounded retention and an observable dropped-record counter.

Snapshots now include `terminals.registered`, counted from the same registry that owns native surface routing and directory metadata. This counts realized local and remote terminal surfaces, excludes browser panes and not-yet-realized widgets, and becomes absent (`null`) if the registry lock is poisoned. Registry entries are removed after native teardown; compare this count with RSS and descriptors during churn rather than treating RSS alone as evidence of live-surface leaks.

Provide a discoverable CLI diagnostic snapshot and a repeatable collection workflow for issue reports. Exclude terminal content, clipboard contents, secrets and full environment dumps. Configuration controls belong in persisted application settings with explicit overrides where needed.

## Benchmark evidence

The initial executable benchmark is `scripts/benchmark-cmux.py`. CI builds optimized binaries, launches an isolated application through the diagnostics fixture, warms ten CLI calls and measures 100 sequential pings. The revision-named artifact contains every latency sample, median/p95/p99, throughput, process resources before/after, GTK version, requested display backend and host metadata. No arbitrary latency gate is applied. This measures CLI startup plus transport/GTK response; sustained rendering, workspace churn and SSH/browser benchmark scenarios remain to be added.

The first completed [CI baseline](https://github.com/nitecon/cmux-gtk/actions/runs/33980669281) at `a009f762` measured median 1,382.6 µs, p95 1,944.7 µs and p99 2,013.8 µs across 100 pings (712.3 operations/second). RSS changed from 246,076 to 246,084 KiB; descriptors stayed at 32 and threads at 34. This software-rendered CI sample establishes a reproducible artifact, not a sustained memory-leak or physical-GPU performance verdict.

Run repeatable optimized workloads in CI for startup, command round trips, workspace/split churn, sustained redraw and representative SSH/browser operations. Record revision, build profile, hardware/runner, GTK/backend, workload, warmup and iteration counts. Report throughput, duration percentiles and memory before/after or slope. Preserve raw machine-readable results as artifacts so regressions can be compared across revisions.

Distinguish software-rendered CI measurements from physical GPU/Wayland measurements. Establish baselines before adding performance gates. Functional lifecycle bounds can be asserted; shared-runner timing thresholds need evidence to avoid flaky tests. Demonstrate correlation and bounded diagnostic overhead through executable tests, including failure/cancellation paths.

The applicable gateway performance and logging principles are recorded in [Patterns](CodingStandards/Patterns.md). This scope does not require deploying a collector or external monitoring service to use cmux.

## Collect an issue report

While reproducing an issue, run from a checkout:

```sh
python3 scripts/collect-cmux-diagnostics.py --output cmux-diagnostics.json
```

The collector takes 12 resource snapshots five seconds apart using the installed `cmux`. Use `--binary target/release/cmux` for a local build and `--socket PATH` for a specific instance. Each sample includes CLI round-trip time and a trace UUID that can be matched against `rpc.complete` in the application's diagnostic log. Snapshot collection runs off GTK, so it can diagnose resource pressure even when GTK commands stall; use the ping benchmark to measure GTK dispatch responsiveness.

The report is created with mode 0600 and refuses to overwrite an existing file. It contains process/build metadata, resource samples and logger health; it does not collect application logs, terminal contents, clipboard data, workspace paths or environment dumps. Failures are retained as error categories with exit status rather than raw command output, and the collector exits nonzero if any sample fails. The default collection takes about one minute plus command time. Review the report before attaching it to an issue. Diagnostic log collection and additional workload benchmarks remain pending.
