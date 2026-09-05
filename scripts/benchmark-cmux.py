#!/usr/bin/env python3
"""Measure optimized CLI-to-application round trips against an existing instance."""

import argparse
import json
import math
import platform
from pathlib import Path
import statistics
import subprocess
import time


def call(binary, socket, command):
    """Invoke one read-only CLI operation, bounding transport and process time."""
    result = subprocess.run([binary, "--socket", socket, "--json", command],
                            capture_output=True, text=True, check=True, timeout=10)
    return json.loads(result.stdout)


def percentile(samples, fraction):
    """Return the nearest-rank percentile for a nonempty measured sample set."""
    return sorted(samples)[max(0, math.ceil(len(samples) * fraction) - 1)]


def measure(binary, socket, iterations, warmup):
    """Warm the command path, then retain individual round trips and resource deltas."""
    for _ in range(warmup):
        call(binary, socket, "ping")
    before = call(binary, socket, "diagnostics")
    if before.get("build_profile") != "release":
        raise ValueError("benchmark requires an optimized cmux-app build")
    samples = []
    started = time.monotonic()
    for _ in range(iterations):
        tick = time.perf_counter_ns()
        result = call(binary, socket, "ping")
        samples.append((time.perf_counter_ns() - tick) / 1000)
        if not result.get("pong"):
            raise ValueError("ping did not reach the application")
    elapsed = time.monotonic() - started
    after = call(binary, socket, "diagnostics")
    return {
        "schema": 1, "workload": "sequential_cli_ping",
        "includes": "CLI process startup, socket dispatch, GTK execution and response",
        "iterations": iterations, "warmup": warmup,
        "elapsed_seconds": elapsed, "operations_per_second": iterations / elapsed,
        "latency_us": {"median": statistics.median(samples),
                       "p95": percentile(samples, 0.95), "p99": percentile(samples, 0.99),
                       "samples": samples},
        "host": {"platform": platform.platform(), "machine": platform.machine()},
        "before": before, "after": after,
    }


def main():
    """Validate workload arguments and write a reproducible machine-readable report."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default="target/release/cmux")
    parser.add_argument("--socket", required=True)
    parser.add_argument("--iterations", type=int, default=100)
    parser.add_argument("--warmup", type=int, default=10)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if not 1 <= args.iterations <= 100000 or not 0 <= args.warmup <= 10000:
        parser.error("iterations must be 1..100000 and warmup 0..10000")
    report = measure(args.binary, args.socket, args.iterations, args.warmup)
    revision = subprocess.run(["git", "rev-parse", "HEAD"], capture_output=True, text=True, check=True)
    report["revision"] = revision.stdout.strip()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(f"wrote {args.output}: median={report['latency_us']['median']:.0f} us")


if __name__ == "__main__":
    main()
