#!/usr/bin/env python3
"""Measure optimized CLI-to-application round trips against an existing instance."""

import argparse
import json
import math
import os
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
    """Retain successful samples and phase/error metadata even when a workload fails."""
    samples = []
    report = {
        "schema": 1, "workload": "sequential_cli_ping", "status": "failed",
        "includes": "CLI process startup, socket dispatch, GTK execution and response",
        "iterations": iterations, "warmup": warmup, "completed_iterations": 0,
        "host": {"platform": platform.platform(), "machine": platform.machine()},
        "before": None, "after": None,
    }
    started = None
    elapsed = None
    phase = "warmup"
    try:
        for _ in range(warmup):
            if not call(binary, socket, "ping").get("pong"):
                raise ValueError("warmup ping did not reach the application")
        phase = "initial_diagnostics"
        report["before"] = call(binary, socket, "diagnostics")
        if type(report["before"].get("pid")) is not int or report["before"]["pid"] <= 0:
            raise ValueError("diagnostics omitted a valid process ID")
        if report["before"].get("build_profile") != "release":
            raise ValueError("benchmark requires an optimized cmux-app build")
        phase = "measurement"
        started = time.monotonic()
        for _ in range(iterations):
            tick = time.perf_counter_ns()
            result = call(binary, socket, "ping")
            duration = (time.perf_counter_ns() - tick) / 1000
            if not result.get("pong"):
                raise ValueError("ping did not reach the application")
            samples.append(duration)
        elapsed = time.monotonic() - started
        phase = "final_diagnostics"
        report["after"] = call(binary, socket, "diagnostics")
        if report["after"].get("pid") != report["before"].get("pid"):
            raise ValueError("application process changed during measurement")
        report["status"] = "passed"
    except Exception as error:
        # Keep terminal output, command arguments and server messages out of artifacts.
        report["failure"] = {"phase": phase, "error_kind": type(error).__name__}
    finally:
        if started is not None and elapsed is None:
            elapsed = time.monotonic() - started
        report["completed_iterations"] = len(samples)
        report["elapsed_seconds"] = elapsed
        report["operations_per_second"] = len(samples) / elapsed if elapsed else None
        report["latency_us"] = {
            "median": statistics.median(samples) if samples else None,
            "p95": percentile(samples, 0.95) if samples else None,
            "p99": percentile(samples, 0.99) if samples else None,
            "samples": samples,
        }
    return report


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
    revision = subprocess.run(["git", "rev-parse", "HEAD"], capture_output=True, text=True,
                              check=True, timeout=10)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    descriptor = os.open(args.output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, "w", encoding="utf-8") as output:
        report = measure(args.binary, args.socket, args.iterations, args.warmup)
        report["revision"] = revision.stdout.strip()
        output.write(json.dumps(report, indent=2) + "\n")
    print(f"wrote {args.output}: {report['status']}, {report['completed_iterations']} completed pings")
    if report["status"] != "passed":
        raise SystemExit(1)


if __name__ == "__main__":
    main()
