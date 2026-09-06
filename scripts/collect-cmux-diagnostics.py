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
import stat
import subprocess
import time

LOG_TAIL_BYTES = 512 * 1024
LOG_RECORD_BYTES = 64 * 1024
LOG_RECORD_COUNT = 2048


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


def idle_evidence(report, settle_seconds, revision):
    """Annotate collected idle samples, retaining raw evidence when runtime or CPU validation fails.

    This measures a caller-controlled quiet application, including its diagnostic
    sampling overhead. It cannot detect all external input or prove inactivity.
    """
    report.update(workload="idle_resources", status="failed", revision=revision,
                  settle_seconds=settle_seconds,
                  includes="Application background work and diagnostic sampling; child process CPU excluded")
    try:
        samples = report["samples"]
        if len(samples) < 2 or len(samples) != report["requested_samples"]:
            raise ValueError("incomplete sample series")
        if any("error" in sample for sample in samples):
            raise ValueError("failed resource sample")
        first = samples[0]["snapshot"]
        terminals = first["terminals"]["registered"]
        if type(terminals) is not int or terminals < 0:
            raise ValueError("invalid terminal count")
        for sample in samples:
            snapshot = sample["snapshot"]
            if snapshot["build_profile"] != "release" or snapshot["pid"] != first["pid"]:
                raise ValueError("optimized process identity changed")
            count = snapshot["terminals"]["registered"]
            if type(count) is not int or count != terminals:
                raise ValueError("terminal count changed")
        for previous, current in zip(samples, samples[1:]):
            if cpu_percent(previous, current) is None:
                raise ValueError("CPU interval unavailable")
        report["cpu_percent"] = cpu_percent(samples[0], samples[-1])
        report["observed_seconds"] = samples[-1]["elapsed_seconds"] - samples[0]["elapsed_seconds"]
        report["status"] = "passed"
    except (ValueError, KeyError, TypeError) as error:
        report["failure"] = {"phase": "idle_validation", "error_kind": type(error).__name__}
    return report


def log_tail(path, pids):
    """Read a bounded regular-file tail, retaining complete diagnostic records for sampled processes.

    Rotation and concurrent writes can omit records; this is incident context,
    not a transactional log export. Paths and invalid raw lines are not retained.
    """
    result = {"records": [], "discarded": 0, "other_process": 0, "truncated": False}
    try:
        descriptor = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_CLOEXEC | os.O_NOFOLLOW)
        with os.fdopen(descriptor, "rb") as source:
            metadata = os.fstat(source.fileno())
            if not stat.S_ISREG(metadata.st_mode):
                raise ValueError("not a regular file")
            offset = max(0, metadata.st_size - LOG_TAIL_BYTES)
            source.seek(offset)
            content = source.read(min(metadata.st_size, LOG_TAIL_BYTES))
            result["truncated"] = offset > 0
        if offset:
            _, _, content = content.partition(b"\n")
            result["discarded"] += 1
        lines = content.split(b"\n")
        if lines[-1]:
            result["discarded"] += 1
        for line in lines[:-1]:
            try:
                if len(line) + 1 > LOG_RECORD_BYTES:
                    raise ValueError("record too large")
                record = json.loads(line)
                if (not isinstance(record, dict) or type(record.get("schema")) is not int
                        or record["schema"] != 1 or type(record.get("pid")) is not int
                        or not isinstance(record.get("event"), str)
                        or not isinstance(record.get("fields"), dict)):
                    raise ValueError("invalid diagnostic envelope")
                if record["pid"] not in pids:
                    result["other_process"] += 1
                    continue
                json.dumps(record, allow_nan=False)
                result["records"].append(record)
            except (ValueError, UnicodeError, RecursionError):
                result["discarded"] += 1
        if len(result["records"]) > LOG_RECORD_COUNT:
            result["discarded"] += len(result["records"]) - LOG_RECORD_COUNT
            result["records"] = result["records"][-LOG_RECORD_COUNT:]
            result["truncated"] = True
        result["status"] = "collected"
    except FileNotFoundError:
        result.update(status="missing")
    except (OSError, ValueError) as error:
        result.update(status="failed", error_kind=type(error).__name__)
    return result


def collect_logs(path, report):
    """Attach explicit active/previous log tails, filtered to successful sampled process identities."""
    pids = {sample["snapshot"]["pid"] for sample in report["samples"]
            if "error" not in sample and isinstance(sample.get("snapshot"), dict)
            and type(sample["snapshot"].get("pid")) is int and sample["snapshot"]["pid"] > 0}
    if not pids:
        return {"status": "failed", "error_kind": "no_process_identity"}
    previous = log_tail(Path(str(path) + ".1"), pids)
    active = log_tail(path, pids)
    failed = active["status"] != "collected" or previous["status"] == "failed"
    return {"status": "failed" if failed else "collected", "active": active, "previous": previous,
            "max_bytes_per_file": LOG_TAIL_BYTES, "max_records_per_file": LOG_RECORD_COUNT}


def main():
    """Validate collection bounds and create a private report without overwriting files."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default="cmux")
    parser.add_argument("--socket", help="override normal cmux socket discovery")
    parser.add_argument("--samples", type=int, default=12)
    parser.add_argument("--interval", type=float, default=5)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--log", type=Path, help="include bounded active and .1 diagnostic log tails")
    parser.add_argument("--idle-benchmark", action="store_true", help="require optimized idle resource evidence")
    parser.add_argument("--settle", type=float, default=10, help="idle benchmark settling seconds (default: 10)")
    args = parser.parse_args()
    if not 1 <= args.samples <= 120 or not 0.01 <= args.interval <= 60:
        parser.error("samples must be 1..120 and interval must be 0.01..60 seconds")
    if not 0 <= args.settle <= 60 or (args.idle_benchmark and args.samples < 2):
        parser.error("settle must be 0..60 seconds; idle benchmarks require at least two samples")
    revision = None
    if args.idle_benchmark:
        try:
            revision = subprocess.run(["git", "rev-parse", "HEAD"], capture_output=True,
                                      text=True, check=True, timeout=10).stdout.strip()
        except (OSError, subprocess.SubprocessError) as error:
            parser.error(f"cannot identify benchmark revision: {type(error).__name__}")
    try:
        descriptor = os.open(args.output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    except OSError as error:
        parser.error(f"cannot create report: {error}")
    with os.fdopen(descriptor, "w") as output:
        if args.idle_benchmark:
            time.sleep(args.settle)
        report = collect(args.binary, args.socket, args.samples, args.interval)
        if args.idle_benchmark:
            idle_evidence(report, args.settle, revision)
        if args.log:
            report["logs"] = collect_logs(args.log, report)
        json.dump(report, output, indent=2)
        output.write("\n")
    failures = sum("error" in sample for sample in report["samples"])
    print(f"wrote {args.output}: {args.samples} samples, {failures} failed, "
          f"status={report.get('status', 'failed' if failures else 'collected')}, "
          f"logs={report.get('logs', {}).get('status', 'not_requested')}")
    return 1 if failures or report.get("status") == "failed" or report.get("logs", {}).get("status") == "failed" else 0


if __name__ == "__main__":
    raise SystemExit(main())
