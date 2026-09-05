# CI baseline: e08b976f

Source: [Actions run 33992451759](https://github.com/nitecon/cmux-gtk/actions/runs/33992451759), successful, commit `e08b976fde0847086b3e54e945a68dd7fa86a380`.
Raw files are copied from artifact `cmux-benchmarks-e08b976fde0847086b3e54e945a68dd7fa86a380` (artifact ID 9977221061).

| Workload | Configuration | Observation |
| --- | --- | --- |
| Sequential CLI ping | Release, 10 warmups, 100 measured process launches, two registered terminals | Median 1657.496 µs; p95 1737.285 µs; p99 1811.637 µs |
| Terminal churn | Debug, three interactive sibling tabs, 45 split/close cycles, nine EOF cycles | First/last sampled post-warmup RSS 354636/360820 KiB; final one registered terminal |
| Sustained redraw | Debug, X11 software rendering, 1800 iterations targeting 30 Hz, 1800×1000 window | 46 sampled RSS values: first 376200 KiB, last/max 376228 KiB |

Linux x86_64, kernel 6.17.0-1022-azure, GTK 4.14.5. The release report records host glibc 2.39; this is a CI measurement, not release-package ABI validation. Churn shutdown did not require forced killing.

The ping measurement includes CLI process startup, socket dispatch, GTK execution and response. Debug churn and release ping are distinct experiments. Compare like workloads and builds; do not interpret these as real-browser, remote-session, long-running idle or universal leak results. Startup and churn retain more RSS than the initial cold sample (242564 KiB). The stable redraw interval does not explain all retained allocations or establish an OOM root cause. Later commits need their own verification.

See [Observability](../../Observability.md) for remaining benchmark requirements.
