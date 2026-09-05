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
    if report.get("schema") != 1 or report.get("workload") != "sequential_cli_ping":
        raise ValueError("only schema-1 sequential_cli_ping reports are supported")
    if report.get("status") != "passed":
        raise ValueError("failed or partial benchmark reports cannot establish a comparison")
    samples = report["latency_us"]["samples"]
    count = report["iterations"]
    if type(count) is not int or not 1 <= count <= 100000:
        raise ValueError("invalid iteration count")
    if not isinstance(samples, list) or len(samples) != count or report["completed_iterations"] != count:
        raise ValueError("sample count does not match completed workload")
    if any(type(value) not in (int, float) or not math.isfinite(value) or value <= 0 for value in samples):
        raise ValueError("latency samples must be finite positive numbers")
    return sorted(samples)


def comparison_settings(report):
    """Require matching recorded workload/runtime metadata; this cannot establish identical hardware."""
    before = report["before"]
    after = report["after"]
    if before["pid"] != after["pid"]:
        raise ValueError("application changed during benchmark")
    names = ("build_profile", "gtk_version", "requested_backend")
    settings = {name: before[name] for name in names}
    if any(value is None for value in settings.values()):
        raise ValueError("runtime metadata is incomplete")
    if settings["build_profile"] != "release":
        raise ValueError("comparison requires optimized application builds")
    settings.update(host=report["host"], iterations=report["iterations"], warmup=report["warmup"],
                    terminals=before["terminals"]["registered"], includes=report["includes"])
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
    except (OSError, ValueError, KeyError, TypeError) as error:
        parser.error(f"cannot compare reports: {error}")
    print(json.dumps(report, indent=2, allow_nan=False))


if __name__ == "__main__":
    main()
