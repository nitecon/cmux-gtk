#!/usr/bin/env python3
"""Compare completed CLI-ping reports without running an application workload."""
import argparse
import json
import math
from pathlib import Path
import statistics

MAX_REPORT_BYTES = 4 * 1024 * 1024


def load_report(path):
    """Read a size-bounded JSON object; reject oversized input before parsing."""
    with Path(path).open("rb") as source:
        content = source.read(MAX_REPORT_BYTES + 1)
    if len(content) > MAX_REPORT_BYTES:
        raise ValueError("benchmark report exceeds four MiB")
    report = json.loads(content)
    if not isinstance(report, dict):
        raise ValueError("benchmark report must be an object")
    return report


def validated_samples(report):
    """Require complete successful ping evidence and finite positive raw latency measurements."""
    if type(report.get("schema")) is not int or report["schema"] != 1 or report.get("workload") != "sequential_cli_ping":
        raise ValueError("only schema-1 sequential_cli_ping reports are supported")
    if report.get("status") != "passed":
        raise ValueError("failed or partial benchmark reports cannot establish a comparison")
    samples = report["latency_us"]["samples"]
    count = report["iterations"]
    if type(count) is not int or not 1 <= count <= 100000:
        raise ValueError("invalid iteration count")
    if (not isinstance(samples, list) or len(samples) != count
            or type(report["completed_iterations"]) is not int
            or report["completed_iterations"] != count):
        raise ValueError("sample count does not match completed workload")
    if any(type(value) not in (int, float) or not math.isfinite(value) or value <= 0 for value in samples):
        raise ValueError("latency samples must be finite positive numbers")
    return sorted(samples)


def comparison_settings(report):
    """Require matching recorded workload/runtime metadata; this cannot establish identical hardware."""
    before = report["before"]
    after = report["after"]
    if any(type(snapshot["pid"]) is not int or snapshot["pid"] <= 0 for snapshot in (before, after)):
        raise ValueError("application PID must be a positive integer")
    if before["pid"] != after["pid"]:
        raise ValueError("application changed during benchmark")
    names = ("build_profile", "gtk_version", "requested_backend")
    settings = {name: before[name] for name in names}
    if any(not isinstance(value, str) or not value.strip() for value in settings.values()):
        raise ValueError("runtime metadata is incomplete")
    if any(after[name] != settings[name] for name in names):
        raise ValueError("runtime settings changed during benchmark")
    if settings["build_profile"] != "release":
        raise ValueError("comparison requires optimized application builds")
    terminals = before["terminals"]["registered"]
    final_terminals = after["terminals"]["registered"]
    if (type(terminals) is not int or terminals < 0 or type(final_terminals) is not int
            or final_terminals != terminals):
        raise ValueError("terminal count is invalid or changed during benchmark")
    if type(report["warmup"]) is not int or report["warmup"] < 0:
        raise ValueError("warmup must be a nonnegative integer")
    host = report["host"]
    if not isinstance(host, dict) or any(
            not isinstance(host.get(name), str) or not host[name].strip()
            for name in ("platform", "machine")):
        raise ValueError("host metadata is incomplete")
    if not isinstance(report["includes"], str) or not report["includes"].strip():
        raise ValueError("workload description is missing")
    settings.update(host=host, iterations=report["iterations"], warmup=report["warmup"],
                    terminals=terminals, includes=report["includes"])
    return settings


def compare_reports(baseline, candidate):
    """Recalculate median/nearest-rank percentiles and deltas for compatible recorded workloads."""
    old = validated_samples(baseline)
    new = validated_samples(candidate)
    settings = comparison_settings(baseline)
    if settings != comparison_settings(candidate):
        raise ValueError("recorded workload or runtime settings differ")
    metrics = {}
    for name, fraction in [("median", None), ("p95", 0.95), ("p99", 0.99)]:
        old_value = statistics.median(old) if fraction is None else old[math.ceil(len(old) * fraction) - 1]
        new_value = statistics.median(new) if fraction is None else new[math.ceil(len(new) * fraction) - 1]
        metrics[name] = {"baseline_us": old_value, "candidate_us": new_value,
                         "delta_us": new_value - old_value,
                         "change_percent": (new_value / old_value - 1) * 100}
        if not all(math.isfinite(value) for value in metrics[name].values()):
            raise ValueError("latency comparison exceeds the finite numeric range")
    return {"schema": 1, "workload": "sequential_cli_ping",
            "baseline_revision": baseline.get("revision"), "candidate_revision": candidate.get("revision"),
            "matched_settings": settings, "latency": metrics,
            "interpretation": "Positive latency change is slower. Matching recorded metadata does not establish identical hardware or statistical significance."}


def main():
    """Print a comparison as JSON; reject invalid evidence with a nonzero command exit."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", type=Path)
    parser.add_argument("candidate", type=Path)
    args = parser.parse_args()
    try:
        report = compare_reports(load_report(args.baseline), load_report(args.candidate))
        output = json.dumps(report, indent=2, allow_nan=False)
    except (OSError, ValueError, KeyError, TypeError, OverflowError, RecursionError) as error:
        parser.error(f"cannot compare reports: {error}")
    print(output)


if __name__ == "__main__":
    main()
