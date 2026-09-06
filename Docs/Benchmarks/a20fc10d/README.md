# Optimized native input and SSH lifecycle baseline

Unmodified artifacts from [successful CI run 34011183573](https://github.com/nitecon/cmux-gtk/actions/runs/34011183573), revision `a20fc10dd0218d1e39ba017273e0f9b5fd98fd42`. [Checksums](SHA256SUMS) identify the archived files. Both workloads identify release-profile owned application processes. Software rendering used Mesa llvmpipe (LLVM 20.1.2), OpenGL 4.5 / Mesa 25.2.8.

## Background input and native bell attention

[Raw report](attention-input-release.json): five warmup cycles, twenty measured cycles, 64 BEL bytes per burst. Values below are calculated from the unchanged raw samples using median and nearest-rank percentiles.

| Boundary | Median µs | p95 µs | p99 µs |
| --- | ---: | ---: | ---: |
| CLI input through child acknowledgement/readback | 5,112.6805 | 5,207.888 | 11,546.401 |
| Bell input through observed app attention | 108,845.5115 | 115,009.923 | 116,062.789 |

The 100-ms polling interval contributes heavily to bell latency. This measures application buffer/attention processing, not compositor or desktop notification presentation. Across 2.402 seconds, RSS changed from 271,792 to 271,828 KiB; descriptors stayed at 46 and threads at 29. This short workload does not establish sustained OOM behavior.

## Real SSH lifecycle

[Raw report](ssh-workspaces-release.json): real loopback OpenSSH and owned Go daemon, with a guarded initial upload failure and no warmup. Two application launches reached socket visibility in 201,044 and 200,770 µs. These are polling-based socket-readiness observations, not terminal readiness.

| Phase | Prompt-readiness µs | Input to complete remote marker µs |
| --- | ---: | ---: |
| Initial remote workspace | 2,038,776.924 | 40,191.046 |
| Remote split | 286,047.635 | 37,180.407 |
| Second same-host workspace | 268,987.992 | 36,721.540 |
| Original workspace still live | 13,638.475 | 25,072.869 |
| Restored workspace after restart | 849,924.484 | 39,293.744 |
| Restored second workspace | 262,754.273 | 36,726.149 |

The report retains per-operation before/after resource snapshots for the two distinct application PIDs. Cleanup did not require the kill fallback. These heterogeneous lifecycle phases include CLI/xdotool startup and polling; they are not a steady-state network latency distribution. Session restoration starts fresh shells, not resumed processes.
