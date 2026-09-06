# Optimized history, browser and paced-attention baseline

Unmodified JSON artifacts from successful [Actions run 34013445662](https://github.com/nitecon/cmux-gtk/actions/runs/34013445662), checkout `9ee61500bf51c6ea5af24cea6ce729b5c357ec69`. SHA256SUMS records the archived bytes. Runtime tests executed in CI only.

| Workload | Observation |
| --- | --- |
| History before churn | 180 physical keys (90 Up, 90 Down), observed buffer median 33.840 ms, p95 36.134 ms, p99 36.784 ms. |
| History after twenty added workspaces | Same original terminal and sequence: median 33.817 ms, p95 36.045 ms, p99 37.958 ms. |
| Actual Chromium | agent-browser 0.31.1, HeadlessChrome 152.0.0.0; cold open 9.200 s. Five warmup and ten measured fill/click/snapshot/evaluate cycles. |
| Browser DOM median | Fill 5.170 ms; click 15.480 ms; snapshot 6.588 ms; evaluate 3.713 ms. |
| Browser app resources during measured cycles | RSS +632 KiB; descriptors and threads unchanged; app CPU 24.20% of one CPU over 0.331 s. External browser CPU is excluded. |
| Paced native attention | Twenty measured cycles after five warmups, 64 BEL bytes per burst, minimum gap 110 ms after observed attention. Input median 5.595 ms; bell median 116.090 ms. |

History timing includes xdotool submission, GTK/Ghostty/readline processing and CLI viewport polling. The observed distributions are similar in this single controlled run; they do not establish a general performance bound or a leak fix. No compositor presentation latency was measured. Resource windows and all raw samples are retained.

The browser fixture required actual DOM changes, snapshots, a six-second conditional wait with concurrent responsive cmux ping, preview textures assigned by GTK, focus preservation, rejected-evaluation trace correlation and owned daemon closure. Assigned textures do not prove pixels reached the compositor. Ten samples give weak tail estimates; p95 and p99 select the same maximum sample here.

The attention fixture includes 100-ms polling. Its new 110-ms inter-burst pacing respects native bell suppression and contributes to overall elapsed/resource measurements, while staying outside per-operation latency. It is a different workload from the earlier unpaced a20fc10d baseline and should not be compared as identical. Desktop-notification presentation is not measured.

Host, build and first OpenGL context metadata remain in the reports. These shared-runner software-rendered measurements are not physical-GPU or long-duration OOM evidence. Later source changes require their own verification.
