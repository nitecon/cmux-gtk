"""Private artifact ownership and latency summaries shared by native benchmark fixtures."""
from contextlib import contextmanager
import json
import math
import os
import platform
import statistics
import subprocess


@contextmanager
def artifact(output, report):
    """Write caller evidence once, retaining failure categories without exception payloads.

    The caller marks success only after its workload and process cleanup finish.
    Existing artifacts are never replaced; abrupt process termination may prevent writing.
    """
    report.update(schema=1, status="failed",
                  host={"platform": platform.platform(), "machine": platform.machine()},
                  revision=subprocess.check_output(["git", "rev-parse", "HEAD"], text=True, timeout=10).strip())
    output.parent.mkdir(parents=True, exist_ok=True)
    descriptor = os.open(output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
        try:
            yield report
        except BaseException as error:
            report.update(status="failed", error_kind=type(error).__name__)
            raise
        finally:
            stream.write(json.dumps(report, indent=2) + "\n")


def summarize_us(samples):
    """Summarize finite nonnegative latency samples using median and nearest-rank percentiles."""
    if not samples or any(not math.isfinite(value) or value < 0 for value in samples):
        raise ValueError("latency samples must be nonempty, finite and nonnegative")
    ordered = sorted(samples)
    return {"median": statistics.median(ordered),
            "p95": ordered[math.ceil(len(ordered) * 0.95) - 1],
            "p99": ordered[math.ceil(len(ordered) * 0.99) - 1]}


def resource_delta(before, after, elapsed_seconds):
    """Require usable same-process counters and compute approximate one-CPU usage and resource changes."""
    if (type(before.get("pid")) is not int or before["pid"] <= 0
            or before["pid"] != after.get("pid")
            or not math.isfinite(elapsed_seconds) or elapsed_seconds <= 0):
        raise ValueError("invalid process resource interval")
    names = ("cpu_user_us", "cpu_system_us", "rss_kib", "threads", "file_descriptors")
    values = [snapshot.get("resources", {}) for snapshot in (before, after)]
    if any(type(row.get(name)) is not int or row[name] < 0 for row in values for name in names):
        raise ValueError("resource counters unavailable")
    delta = {name: values[1][name] - values[0][name] for name in names}
    if delta["cpu_user_us"] < 0 or delta["cpu_system_us"] < 0:
        raise ValueError("CPU counters regressed")
    delta["cpu_percent"] = (delta["cpu_user_us"] + delta["cpu_system_us"]) / (elapsed_seconds * 10000)
    return delta
