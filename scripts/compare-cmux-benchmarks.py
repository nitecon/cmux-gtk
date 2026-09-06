#!/usr/bin/env python3
"""Compare completed optimized ping or terminal-churn reports without running a workload."""
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


def validated_label(value):
    """Accept unknown or bounded nonempty UTF-8 driver/CPU labels without control characters."""
    if value is not None and (
            not isinstance(value, str) or not value.strip()
            or len(value.encode("utf-8")) > 256
            or any(ord(character) < 32 or 127 <= ord(character) <= 159 for character in value)):
        raise ValueError("hardware label must be bounded nonempty text or unknown")
    return value


def runtime_settings(before, after):
    """Validate stable process, build and driver metadata for two snapshots."""
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
    software = before.get("libgl_software_override")
    if software is not None and type(software) is not bool:
        raise ValueError("software rendering override must be boolean or unknown")
    final_software = after.get("libgl_software_override")
    if (final_software is not None and type(final_software) is not bool) or final_software != software:
        raise ValueError("software rendering override changed during benchmark")
    settings["libgl_software_override"] = software
    cpu_model = validated_label(before.get("cpu_model"))
    if after.get("cpu_model") != cpu_model:
        raise ValueError("CPU model changed during benchmark")
    settings["cpu_model"] = cpu_model
    renderer = before.get("first_opengl_context")
    if renderer is not None:
        if not isinstance(renderer, dict) or set(renderer) != {"vendor", "renderer", "version"}:
            raise ValueError("OpenGL context metadata is invalid")
        for value in renderer.values():
            validated_label(value)
    if after.get("first_opengl_context") != renderer:
        raise ValueError("OpenGL context metadata changed during benchmark")
    settings["first_opengl_context"] = renderer
    return settings


def comparison_settings(report):
    """Require matching ping workload metadata; matching labels cannot prove identical hardware."""
    before, after = report["before"], report["after"]
    settings = runtime_settings(before, after)
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
    if isinstance(baseline.get("workload"), dict) or isinstance(candidate.get("workload"), dict):
        return compare_memory_reports(baseline, candidate)
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


def memory_evidence(report):
    """Validate the maintained churn sequence and calculate observed post-warmup resource changes."""
    workload = {"interactive_sibling_tabs": 3, "split_close_cycles": 45, "child_eof_cycles": 9,
                "split_right_shortcut": "<Ctrl><Alt>d", "redraw_iterations": 1800,
                "redraw_target_hz": 30, "window_pixels": [1800, 1000]}
    if (type(report.get("schema")) is not int or report["schema"] != 1
            or report.get("status") != "passed" or report.get("shutdown_forced") is not False):
        raise ValueError("memory comparison requires complete schema-1 evidence with graceful shutdown")
    if json.dumps(report.get("workload"), sort_keys=True) != json.dumps(workload, sort_keys=True):
        raise ValueError("unsupported terminal churn workload")
    samples = report["samples"]
    if (not isinstance(samples, list) or not 34 <= len(samples) <= 200
            or any(not isinstance(sample, dict) for sample in samples)):
        raise ValueError("memory sample count is invalid")
    expected = [("baseline", 0), ("interactive_tabs", 3)]
    live_split = samples[2].get("phase") == "split_live"
    if live_split:
        expected.append(("split_live", 1))
    for cycle in range(45):
        if cycle % 5 == 0:
            expected.append(("child_eof", cycle + 1))
        if cycle in (9, 24, 44):
            expected.append(("split_close", cycle + 1))
    redraw_count = len(samples) - len(expected)
    if redraw_count < 20:
        raise ValueError("insufficient sustained redraw evidence")
    expected.extend(("redraw", index + 1) for index in range(redraw_count))
    first = samples[0]["snapshot"]
    settings = runtime_settings(first, first)
    if (report["build_profile"] != settings["build_profile"]
            or report["backend"] != settings["requested_backend"]
            or report["software_rendering"] is not True
            or settings["libgl_software_override"] is False):
        raise ValueError("memory launch metadata does not match the measured application")
    host = report["host"]
    if not isinstance(host, dict) or any(
            not isinstance(host.get(name), str) or not host[name].strip()
            for name in ("system", "release", "machine")):
        raise ValueError("memory host metadata is incomplete")
    previous_time = -1
    previous_cpu = None
    split_rss, redraw = [], []
    for sample, (phase, iteration) in zip(samples, expected):
        if (sample["phase"] != phase or type(sample["iteration"]) is not int
                or sample["iteration"] != iteration):
            raise ValueError("memory phase sequence is incomplete or changed")
        elapsed = sample["elapsed_seconds"]
        if (type(elapsed) not in (int, float) or not math.isfinite(elapsed)
                or elapsed < 0 or elapsed <= previous_time):
            raise ValueError("memory sample times must increase monotonically")
        previous_time = elapsed
        snapshot = sample["snapshot"]
        if runtime_settings(first, snapshot) != settings:
            raise ValueError("memory runtime metadata changed")
        terminals = snapshot["terminals"]["registered"]
        if type(terminals) is not int or terminals != (2 if phase in ("split_live", "child_eof") else 1):
            raise ValueError("memory terminal lifecycle evidence is inconsistent")
        resources = snapshot["resources"]
        for name in ("rss_kib", "file_descriptors", "threads", "cpu_user_us", "cpu_system_us"):
            if type(resources[name]) is not int or resources[name] < 0:
                raise ValueError("memory resource counters must be nonnegative integers")
        if resources["rss_kib"] == 0:
            raise ValueError("resident memory measurement is missing")
        cpu = (resources["cpu_user_us"], resources["cpu_system_us"])
        if previous_cpu is not None and any(new < old for new, old in zip(cpu, previous_cpu)):
            raise ValueError("memory CPU counters regressed")
        previous_cpu = cpu
        if phase == "split_close":
            split_rss.append(resources["rss_kib"])
        if phase == "redraw":
            redraw.append(sample)
    rss = [sample["snapshot"]["resources"]["rss_kib"] for sample in redraw]
    initial = redraw[0]["snapshot"]["resources"]
    final = redraw[-1]["snapshot"]["resources"]
    duration = redraw[-1]["elapsed_seconds"] - redraw[0]["elapsed_seconds"]
    metrics = {
        "split_close_rss_growth_kib": split_rss[-1] - split_rss[0],
        "redraw_rss_growth_kib": max(rss[-10:]) - min(rss[:10]),
        "redraw_final_rss_median_kib": statistics.median(rss[-10:]),
        "redraw_cpu_percent": sum(final[name] - initial[name] for name in
                                  ("cpu_user_us", "cpu_system_us")) / (duration * 10000),
        "redraw_fd_growth": final["file_descriptors"] - initial["file_descriptors"],
        "redraw_thread_growth": final["threads"] - initial["threads"],
    }
    if any(not math.isfinite(value) for value in metrics.values()):
        raise ValueError("memory comparison exceeds the finite numeric range")
    settings.update(host=host, workload=workload, live_split_sample=live_split)
    return settings, metrics, {"redraw_samples": len(redraw), "redraw_observed_seconds": duration}


def compare_memory_reports(baseline, candidate):
    """Report resource deltas for compatible optimized churn runs without inferring a leak or threshold."""
    settings, old, old_window = memory_evidence(baseline)
    candidate_settings, new, new_window = memory_evidence(candidate)
    if settings != candidate_settings:
        raise ValueError("recorded memory workload or runtime settings differ")
    metrics = {name: {"baseline": value, "candidate": new[name], "delta": new[name] - value}
               for name, value in old.items()}
    if any(not math.isfinite(metric["delta"]) for metric in metrics.values()):
        raise ValueError("memory comparison exceeds the finite numeric range")
    return {"schema": 1, "workload": "terminal_churn", "matched_settings": settings,
            "baseline_revision": baseline.get("revision"), "candidate_revision": candidate.get("revision"),
            "baseline_window": old_window, "candidate_window": new_window, "resources": metrics,
            "interpretation": "Observed process resources include allocator caches and sampling overhead. Deltas do not establish a leak, statistical significance or identical hardware. Redraw windows may differ; output iterations are not presented frames."}


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
