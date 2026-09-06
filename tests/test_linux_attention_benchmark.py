#!/usr/bin/env python3
"""Measure optimized background-terminal input and native bell attention through the real CLI."""

import json
import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import time

from benchmark_support import artifact, summarize_us, resource_delta
from linux_app import running_app


PROBE = '''import sys
print("CMUX_PROBE_READY", flush=True)
for line in sys.stdin:
    token = line.strip()
    if token.startswith("bell-"):
        sys.stdout.write("\\a" * 64)
    print("CMUX_ACK:" + token, flush=True)
'''


def measure(root, report):
    """Require actual child output and attention changes while preserving the other pane's focus."""
    probe = root / "probe.py"
    probe.write_text(PROBE)
    with running_app(root, {"CMUX_BIN_DIR": "target/release"}) as app:
        app.wait_for(lambda: len(app.children()) == 1, "initial child")
        target = app.surfaces()[0]["uuid"]
        app.cli("split", "--direction", "horizontal")
        app.wait_for(lambda: len(app.children()) == 2, "foreground child")
        focused = next(row["uuid"] for row in app.surfaces() if row["active"])
        assert focused != target
        app.wait_for(lambda: json.loads(app.cli("health", "--id", target, "--json"))["alive"],
                     "background terminal readiness")
        app.cli("send-text", "exec python3 -u " + shlex.quote(str(probe)), "--id", target)
        app.cli("send-key", "\r", "--id", target)
        app.wait_for(lambda: "CMUX_PROBE_READY" in app.cli("read-text", "--id", target), "probe output")
        workspace = json.loads(app.cli("list-notifications", "--json"))["notifications"][0]["workspace_uuid"]

        def attention():
            """Read this workspace's current attention without selecting it or moving focus."""
            rows = json.loads(app.cli("list-notifications", "--json"))["notifications"]
            return next(row["has_attention"] for row in rows if row["workspace_uuid"] == workspace)

        def snapshot():
            """Require the owned optimized process and captured renderer metadata for every sample."""
            result = json.loads(app.cli("diagnostics", "--json"))
            assert result["pid"] == app.process.pid and result["build_profile"] == "release"
            assert result["first_opengl_context"] is not None
            return result

        for iteration in range(25):
            if iteration == 5:
                report["before"] = snapshot()
                started = time.monotonic()
            row = {}
            for kind in ("input", "bell"):
                token = f"{kind}-{iteration}"
                tick = time.perf_counter_ns()
                app.cli("send-text", token, "--id", target)
                app.cli("send-key", "\r", "--id", target)
                app.wait_for(lambda: "CMUX_ACK:" + token in app.cli("read-text", "--id", target),
                             "executed terminal acknowledgement")
                if kind == "bell":
                    app.wait_for(attention, "native bell attention")
                row[kind + "_us"] = (time.perf_counter_ns() - tick) / 1000
            app.cli("clear-notification", workspace)
            assert not attention(), "clear did not remove attention"
            assert next(row["uuid"] for row in app.surfaces() if row["active"]) == focused
            if iteration >= 5:
                report["samples"].append(row)
        report["after"] = snapshot()
        report["elapsed_seconds"] = time.monotonic() - started
        report["resource_delta"] = resource_delta(report["before"], report["after"], report["elapsed_seconds"])
        report["latency_us"] = {kind: summarize_us([row[kind + "_us"] for row in report["samples"]])
                                for kind in ("input", "bell")}
    report["status"] = "passed"


def main():
    """Preserve raw measurements or partial failure evidence in an exclusive private CI artifact."""
    output = Path(os.environ.get("CMUX_ATTENTION_REPORT", "target/benchmarks/attention-input-release.json"))
    report = {"schema": 1, "workload": "background_terminal_input_and_bell", "status": "failed",
              "warmup": 5, "iterations": 20, "bells_per_burst": 64, "poll_interval_seconds": 0.1,
              "includes": "CLI startup, socket dispatch, PTY input, child output, viewport readback and attention polling; no presentation latency claim",
              "samples": [], "before": None, "after": None}
    with artifact(output, report):
        with tempfile.TemporaryDirectory(prefix="cmux-attention-") as directory:
            measure(Path(directory), report)
    print("optimized terminal input, native bell attention and focus preservation verified")


if __name__ == "__main__":
    main()
