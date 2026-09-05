#!/usr/bin/env python3
"""Collect resource trends and correlated CLI timings from a running cmux instance."""

import argparse
from datetime import datetime, timezone
import json
import math
import os
from pathlib import Path
import platform
import re
import subprocess
import time


def sample(binary, socket):
    """Capture a read-only snapshot and its correlation ID, retaining bounded failures."""
    command = [binary, "--json", "--verbose"]
    if socket:
        command.extend(["--socket", socket])
    started = time.perf_counter_ns()
    try:
        result = subprocess.run(command + ["diagnostics"], capture_output=True,
                                text=True, timeout=10)
    except subprocess.TimeoutExpired:
        return {"error": "command_timeout"}
    except OSError as error:
        return {"error": "command_unavailable", "errno": error.errno}
    record = {"round_trip_us": (time.perf_counter_ns() - started) / 1000,
              "exit_code": result.returncode}
    trace = re.search(r"trace_id=([0-9a-f-]{36})", result.stderr)
    if trace:
        record["trace_id"] = trace.group(1)
    if result.returncode:
        record["error"] = "command_failed"
        return record
    try:
        record["snapshot"] = json.loads(result.stdout)
    except json.JSONDecodeError:
        record["error"] = "invalid_response"
    return record


def cpu_percent(previous, current):
    """Estimate interval CPU usage from adjacent successful samples of the same process.

    One fully occupied CPU is 100%; missing/regressing counters or nonpositive
    time intervals return None. Sampling and CLI overhead remain in the interval.
    """
    if "error" in previous or "error" in current:
        return None
    before, after = previous.get("snapshot"), current.get("snapshot")
    if not isinstance(before, dict) or not isinstance(after, dict):
        return None
    pid = before.get("pid")
    if type(pid) is not int or pid <= 0 or type(after.get("pid")) is not int or after["pid"] != pid:
        return None
    counters = []
    for snapshot in (before, after):
        resources = snapshot.get("resources")
        if not isinstance(resources, dict):
            return None
        values = [resources.get(key) for key in ("cpu_user_us", "cpu_system_us")]
        if any(type(value) is not int or value < 0 for value in values):
            return None
        counters.append(values)
    if any(end < start for start, end in zip(*counters)):
        return None
    start, end = previous.get("elapsed_seconds"), current.get("elapsed_seconds")
    if not all(type(value) in (int, float) and math.isfinite(value) for value in (start, end)):
        return None
    elapsed = end - start
    if elapsed <= 0:
        return None
    return (sum(counters[1]) - sum(counters[0])) / (elapsed * 1_000_000) * 100


def collect(binary, socket, samples, interval):
    """Retain a bounded time series without reading terminal content or application files."""
    report = {
        "schema": 1,
        "collected_at": datetime.now(timezone.utc).isoformat(),
        "host": {"system": platform.system(), "release": platform.release(),
                 "machine": platform.machine()},
        "requested_samples": samples, "interval_seconds": interval, "samples": [],
    }
    started = time.monotonic()
    for index in range(samples):
        if index:
            time.sleep(interval)
        record = sample(binary, socket)
        record["elapsed_seconds"] = time.monotonic() - started
        record["cpu_percent"] = cpu_percent(report["samples"][-1], record) if index else None
        report["samples"].append(record)
    return report


def main():
    """Validate collection bounds and create a private report without overwriting files."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default="cmux")
    parser.add_argument("--socket", help="override normal cmux socket discovery")
    parser.add_argument("--samples", type=int, default=12)
    parser.add_argument("--interval", type=float, default=5)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if not 1 <= args.samples <= 120 or not 0.01 <= args.interval <= 60:
        parser.error("samples must be 1..120 and interval must be 0.01..60 seconds")
    try:
        descriptor = os.open(args.output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    except OSError as error:
        parser.error(f"cannot create report: {error}")
    with os.fdopen(descriptor, "w") as output:
        report = collect(args.binary, args.socket, args.samples, args.interval)
        json.dump(report, output, indent=2)
        output.write("\n")
    failures = sum("error" in sample for sample in report["samples"])
    print(f"wrote {args.output}: {args.samples} samples, {failures} failed")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
