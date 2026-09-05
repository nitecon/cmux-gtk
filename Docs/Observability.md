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

## Memory lifecycle evidence

The CI memory-churn scenario now writes `target/benchmarks/memory-churn.json`, retained by the benchmark artifact upload even when a later step fails. It records diagnostic snapshots at baseline, after 10/25/45 split-close cycles, and roughly once per second during the post-warmup sustained redraw workload. Snapshots provide resource, terminal-registration, GTK-heartbeat and diagnostic-writer counters alongside phase and elapsed time. The report includes revision, host, requested window size, workload settings and pass/failure status without capturing terminal contents. Output is created privately and refuses an existing destination.

This is a **debug-build, software-rendered lifecycle measurement**, distinct from the optimized ping benchmark. The 1,800 redraw iterations count terminal output writes, not verified rendered frames. CLI sampling adds some overhead; use the same workload when comparing reports. Memory thresholds remain the existing functional bounds, not proof that all real-world OOM causes have been resolved.

Existing session persistence emits `session.save` records with workspace count, serialized bytes, serialization/write/total microseconds, outcome and an I/O error category. No workspace paths, names or terminal content are recorded. The measurements cover serialization and atomic file replacement, not the earlier GTK snapshot construction or debounce wait. Existing writer queue and retention limits apply. This instrumentation does not add shutdown flushing or resume hooks.

`browser_preview` snapshots expose active stream tasks, successfully base64-decoded JPEG payload/byte totals, texture assignments and separate base64/texture decode failures. Counters use process-local atomics, sampled independently with no per-frame logs. Active tasks include connection setup and decrement when their task exits or is cancelled. Texture assignment does not prove compositor presentation; received minus assigned is not an exact dropped-frame count because it also includes pending work and decode failures. Browser runtime coverage remains pending.

Generic browser RPC completion now emits `browser.rpc.complete` with the observed CLI trace ID, exchange duration and success/error/receiver-cancelled outcome. Match it to `rpc.gtk.start`, `rpc.gtk.dispatched` and `rpc.complete` to separate dispatch from browser waiting. Duration begins when the Tokio task starts. Requests without an observed wrapper have a null trace ID; runtime teardown may abort a task before it emits completion. This does not propagate tracing inside the external browser daemon or cover the remaining synchronous UI/CLI paths.

`browser_commands` reports asynchronous exchange capacity, currently admitted exchanges and cumulative overload rejections. The active count comes directly from the transport semaphore, so normal completion, timeout and cancellation release the measured capacity through the same ownership path. It includes async motion forwarding as well as generic browser RPCs, but excludes remaining synchronous browser operations. Rejection and active counts are sampled independently.

Preview pixel decoding now runs on at most two blocking workers across the process. Full slots drop incoming decode attempts without queuing them; widget destruction releases its delivery future immediately, while an already-running decoder retains its slot until it finishes. JPEG and PNG are accepted. Headers are limited to 8,192 pixels per edge and 16,777,216 total pixels, with at most 64 MiB for the resulting straight-alpha RGBA buffer. The decoder receives a 128 MiB allocation budget and its output size is checked before allocation. The library documents internal allocation accounting as [best effort](https://docs.rs/image/latest/image/struct.Limits.html); these limits do not cap total process RSS, displayed textures or GPU copies.

`browser_preview.decode_attempts` and `decode_total_us` aggregate completed CPU decode attempts, including failures; `decode_overload_drops` counts admission rejection. Existing `texture_errors` includes invalid images and size-limit rejection. GTK wraps shared RGBA bytes in a memory texture without running the image codec. The two-slot limit also applies to detached decoders after cancellation. These measurements exclude waiting for worker scheduling and compositor presentation.

A notification burst/idle benchmark remains missing. The removed upstream scripts used macOS process matching and SwiftUI popover automation, suppressed command errors, and interpreted missing CPU measurements as zero. A Linux replacement must identify the application from its diagnostics response or owned subprocess, require successful notification operations, and calculate CPU usage from process-time deltas over a monotonic interval. Do not apply the old arbitrary CPU thresholds or count current memory-churn coverage as notification coverage.

`resources.cpu_user_us` and `cpu_system_us` are cumulative process CPU microseconds from [getrusage(RUSAGE_SELF)](https://man7.org/linux/man-pages/man2/getrusage.2.html), summed across application threads and excluding shell, CLI, browser-daemon and remote-daemon child processes. Failure is represented as null, not zero. Existing benchmark and issue-report resource snapshots retain both values. For two samples from the same process, divide the change in their summed CPU microseconds by elapsed monotonic microseconds and multiply by 100; 100% represents one fully occupied CPU and multithreaded usage can exceed it. Sampling and diagnostic handling contribute overhead. This provides accounting for future workload comparisons without introducing an arbitrary CPU regression threshold.

The optimized ping report now records `status`, `completed_iterations` and a failure phase/error category when warmup, measurement or final diagnostics fails. Successfully validated samples and the initial snapshot survive ordinary command failures; empty samples have null percentile values. Partial-run throughput includes time spent waiting for the failed operation and must not be compared as a successful workload score. Warmup must return pong, and before/after snapshots must identify the same process. Reports are created privately with exclusive creation and are not overwritten. Forced process termination or failure to create the output file can still prevent a complete artifact.

The issue-report collector adds `cpu_percent` to each sample after the first, using adjacent cumulative user/kernel counters and elapsed monotonic collection time. Null means unavailable, including a failed sample, changed process, regressing counter or invalid interval. Zero is reserved for a valid interval with no measured CPU increase. Values can exceed 100% because they represent usage relative to one CPU. Collection completion timestamps approximate the resource interval and include sampling overhead; these are triage trends, not precise profiler samples or performance gates. The obsolete macOS `sample`/SwiftUI idle probe was removed; a controlled optimized idle workload remains future benchmark work.
