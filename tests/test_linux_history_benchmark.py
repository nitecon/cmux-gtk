#!/usr/bin/env python3
"""Compare real GTK history-key processing before and after twenty workspace launches."""

import json
import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import time

from benchmark_support import artifact, summarize_us, resource_delta
from linux_app import running_app
from test_multi_workspace_focus import selected_surface


RC = '''HISTFILE=/dev/null
HISTSIZE=2000
HISTCONTROL=
PROMPT_COMMAND=
PS1='CMUX_HISTORY> '
set -o emacs
history -c
for ((i=1; i<=120; i++)); do
    printf -v entry ': CMUX_HISTORY_%04d' "$i"
    history -s "$entry"
done
'''


def measure(root, report):
    """Seed deterministic Bash history, measure physical keys, churn workspaces and repeat in place."""
    rc = root / "history.rc"
    rc.write_text(RC)
    with running_app(root, {"CMUX_BIN_DIR": "target/release", "SHELL": "/bin/bash"}) as app:
        app.wait_for(lambda: len(app.children()) == 1, "initial shell")
        surface = selected_surface(app)
        workspace = json.loads(app.cli("current-workspace", "--json"))["uuid"]
        app.wait_for(lambda: json.loads(app.cli("health", "--id", surface, "--json"))["alive"], "terminal realization")
        app.cli("send-text", "exec /bin/bash --noprofile --rcfile " + shlex.quote(str(rc)) + " -i", "--id", surface)
        app.cli("send-key", "\r", "--id", surface)

        def line_is(expected):
            """Require the final prompt's current buffer, avoiding matches in earlier terminal output."""
            text = app.cli("read-text", "--id", surface)
            return "CMUX_HISTORY>" in text and text.rsplit("CMUX_HISTORY>", 1)[1].strip() == expected

        def resources():
            """Record the same optimized application with known first-context metadata."""
            value = json.loads(app.cli("diagnostics", "--json"))
            assert value["pid"] == app.process.pid and value["build_profile"] == "release"
            assert value["first_opengl_context"] is not None
            return value

        app.wait_for(lambda: line_is(""), "seeded history prompt")
        windows = subprocess.check_output(["xdotool", "search", "--onlyvisible", "--pid", str(app.process.pid)],
                                          text=True, timeout=10).split()
        assert windows
        subprocess.check_call(["xdotool", "windowfocus", "--sync", windows[-1]], timeout=10)
        for phase in ("baseline", "after_workspace_churn"):
            if phase == "after_workspace_churn":
                for count in range(20):
                    app.cli("new-workspace")
                    app.wait_for(lambda: len(app.children()) == count + 2, "new workspace child")
                app.cli("select-workspace", workspace)
                assert selected_surface(app) == surface
                app.wait_for(lambda: line_is(""), "original history prompt after churn")
            evidence = {"phase": phase, "before": resources(), "samples": [], "after": None}
            report["phases"].append(evidence)
            started = time.monotonic()
            position = 121
            for key in ["Up"] * 90 + ["Down"] * 90:
                position += -1 if key == "Up" else 1
                expected = f": CMUX_HISTORY_{position:04d}" if position <= 120 else ""
                tick = time.perf_counter_ns()
                subprocess.check_call(["xdotool", "key", "--clearmodifiers", key], timeout=10)
                submitted = time.perf_counter_ns()
                app.wait_for(lambda: line_is(expected), "history buffer update")
                observed = time.perf_counter_ns()
                evidence["samples"].append({"key": key, "submission_us": (submitted - tick) / 1000,
                                            "buffer_observed_us": (observed - tick) / 1000})
            evidence["after"] = resources()
            evidence["elapsed_seconds"] = time.monotonic() - started
            evidence["resource_delta"] = resource_delta(evidence["before"], evidence["after"], evidence["elapsed_seconds"])
            evidence["latency_us"] = {kind: summarize_us([row[kind] for row in evidence["samples"]])
                                      for kind in ("submission_us", "buffer_observed_us")}
            assert selected_surface(app) == surface
        assert len(app.children()) == 21
    report["status"] = "passed"


def main():
    """Write private raw before/after workload evidence, retaining samples on ordinary failures."""
    output = Path(os.environ.get("CMUX_HISTORY_REPORT", "target/benchmarks/history-churn-release.json"))
    report = {"schema": 1, "workload": "history_navigation_before_after_workspace_churn", "status": "failed",
              "history_entries": 120, "added_workspaces": 20, "keys_per_phase": 180,
              "key_sequence": "90 Up then 90 Down", "warmup": 0, "poll_interval_seconds": 0.1,
              "includes": "X11 key submission, GTK/Ghostty/readline processing and CLI viewport polling; compositor presentation is not measured",
              "phases": []}
    with artifact(output, report):
        with tempfile.TemporaryDirectory(prefix="cmux-history-") as directory:
            measure(Path(directory), report)
    print("optimized history navigation verified before and after twenty workspace launches")


if __name__ == "__main__":
    main()
