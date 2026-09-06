# Optimized CI baseline: a897afa9

Source: [successful CI run 34008383930](https://github.com/nitecon/cmux-gtk/actions/runs/34008383930), revision `a897afa9694076363989dfbee5261b200f0b4ffe`. These files are unchanged copies from artifact `cmux-benchmarks-a897afa9694076363989dfbee5261b200f0b4ffe`. The run built optimized binaries and invoked isolated GTK fixtures under X11/Xvfb with the software GL override enabled. First terminal context: Mesa llvmpipe (LLVM 20.1.2, 256 bits), OpenGL 4.5 Core, Mesa 25.2.8. Exact driver, CPU and host labels are retained in the reports.

| Workload | Observed result | Raw evidence |
| --- | --- | --- |
| 100 sequential CLI pings after 10 warmups | Median 1638.6005 µs, p95 1681.795 µs, p99 1687.543 µs | [CLI report](cli-round-trip.json) |
| Idle with 10-second settling and 6 samples | Average 5.0075% of one CPU over 10.0141 seconds | [Idle report](idle-resources.json) |
| 45 split/close cycles including 9 child EOF exits | RSS 353368 KiB at cycle 10, 361408 KiB at cycle 45; growth 8040 KiB | [Optimized churn report](memory-churn-release.json) |
| 1800 terminal output/redraw iterations at requested 30 Hz | 46 post-warmup samples; final RSS 404200 KiB; last 10 maximum minus first 10 minimum 23068 KiB | [Optimized churn report](memory-churn-release.json) |

Across the sampled redraw window (45.2890 seconds), average application CPU was 336.4883% of one core, descriptors changed by 0, and threads by 0. CPU excludes shell/browser/remote children; diagnostic collection contributes overhead. Values are measurements, not newly introduced acceptance thresholds.

The suite passed its coarse OOM and process-lifecycle checks. This short software-rendered run does not prove the absence of a long-running leak, establish physical GPU behavior, or measure compositor presentation latency. Output-loop iterations are not verified presented frames. These observations are not directly comparable to older reports with unknown CPU/driver metadata or debug builds. Later source changes require their own successful CI evidence; this is a preserved baseline, not full-goal completion.
