#!/usr/bin/env python3
"""Exercise real Ghostty terminal cleanup and measure bounded RSS after warmup."""
import json
import os
import platform
from pathlib import Path
import shlex
import subprocess
from functools import partial
from process_support import stop_process, wait_until
import tempfile
import time


eventually = partial(wait_until, description="terminal state", timeout=10)

with tempfile.TemporaryDirectory(prefix="cmux-memory-") as directory:
    root = Path(directory)
    env = dict(os.environ, XDG_DATA_HOME=str(root / "data"),
               XDG_CONFIG_HOME=str(root / "config"), XDG_STATE_HOME=str(root / "state"),
               XDG_RUNTIME_DIR=str(root / "runtime"), GDK_BACKEND="x11",
               LIBGL_ALWAYS_SOFTWARE="1", CMUX_NO_UPDATE="1")
    (root / "runtime").mkdir(mode=0o700)
    # Default Ctrl-D splits a pane. Isolate EOF coverage by moving that application
    # shortcut so GTK forwards Ctrl-D to the terminal's canonical input reader.
    (root / "config/cmux").mkdir(parents=True)
    (root / "config/cmux/config.toml").write_text(
        '[shortcuts]\nsplit_right = "<Ctrl><Alt>d"\n'
    )
    socket = root / "runtime/cmux/cmux.sock"
    log = (root / "app.log").open("w+")
    app = subprocess.Popen(["target/debug/cmux-app"], env=env, stdout=log, stderr=log)

    def cli(*args):
        """Invoke the isolated application CLI with a bounded subprocess lifetime."""
        return subprocess.check_output(["target/debug/cmux", "--socket", str(socket), *args], env=env, text=True, timeout=15)

    def rss():
        """Read current resident memory in KiB from the application process."""
        status = Path(f"/proc/{app.pid}/status").read_text()
        return int(next(line.split()[1] for line in status.splitlines() if line.startswith("VmRSS:")))

    def children():
        """Collect child identities across spawning threads, tolerating thread exit races."""
        # Linux exposes children per spawning thread, and Ghostty spawns from
        # its IO thread rather than the GTK thread.
        result = set()
        for path in Path(f"/proc/{app.pid}/task").glob("*/children"):
            try:
                result.update(path.read_text().split())
            except FileNotFoundError:
                pass  # A worker may exit between enumeration and the read.
        return result

    report = {
        "schema": 1, "revision": os.environ.get("GITHUB_SHA"),
        "build_profile": "debug", "backend": "x11", "software_rendering": True,
        "host": {"system": platform.system(), "release": platform.release(),
                 "machine": platform.machine()},
        "workload": {"interactive_sibling_tabs": 3, "split_close_cycles": 45, "child_eof_cycles": 9,
                     "split_right_shortcut": "<Ctrl><Alt>d",
                     "redraw_iterations": 1800,
                     "redraw_target_hz": 30, "window_pixels": [1800, 1000]},
        "status": "running", "samples": [],
    }
    measurement_start = time.monotonic()

    def record_resources(phase, iteration):
        """Retain correlated resource counters without collecting terminal content or paths."""
        snapshot = json.loads(cli("diagnostics", "--json"))
        report["samples"].append({
            "phase": phase, "iteration": iteration,
            "elapsed_seconds": time.monotonic() - measurement_start,
            "snapshot": snapshot,
        })

    try:
        eventually(socket.exists)
        eventually(lambda: len(children()) >= 1)
        windows = subprocess.check_output(
            ["xdotool", "search", "--onlyvisible", "--pid", str(app.pid)],
            text=True, timeout=10,
        ).split()
        assert windows, "application has no X11 window"
        window = windows[-1]
        baseline_children = children()
        record_resources("baseline", 0)
        # New sibling tabs must accept real GTK input without a focus-switch repair.
        split_ids = []
        for _ in range(2):
            cli("split", "--direction", "horizontal")
            eventually(lambda: len(children()) == len(baseline_children) + len(split_ids) + 1)
            listed = json.loads(cli("list-surfaces", "--json"))["surfaces"]
            split_ids.append(next(surface["uuid"] for surface in listed if surface["active"]))
        for iteration in range(3):
            before_children = children()
            before_surfaces = {surface["uuid"] for surface in json.loads(cli("list-surfaces", "--json"))["surfaces"]}
            subprocess.check_call(
                ["xdotool", "windowfocus", window, "key", "--clearmodifiers", "ctrl+t"], timeout=10,
            )
            eventually(lambda: len(children()) == len(before_children) + 1)
            new_children = children() - before_children
            listed = json.loads(cli("list-surfaces", "--json"))["surfaces"]
            selected = next(surface["uuid"] for surface in listed if surface["active"])
            assert selected not in before_surfaces, "new sibling terminal did not become selected"
            marker = root / f"interactive-tab-{iteration}"
            command = "printf '%s' \"$$\" > " + shlex.quote(str(marker))
            subprocess.check_call(
                ["xdotool", "type", "--clearmodifiers", "--delay", "1", "--", command], timeout=10,
            )
            subprocess.check_call(["xdotool", "key", "--clearmodifiers", "Return"], timeout=10)
            try:
                eventually(lambda: marker.exists() and marker.read_text() in new_children)
            except AssertionError:
                # Record fixture-owned evidence only, never terminal contents or shell environment.
                report["interactive_failure"] = {
                    "iteration": iteration, "selected_surface": selected,
                    "expected_child_pids": sorted(new_children),
                    "current_child_pids": sorted(children()),
                    "marker_exists": marker.exists(),
                    "marker_pid": None,
                }
                if marker.exists():
                    with marker.open() as marker_file:
                        value = marker_file.read(32)
                    if value.isdecimal():
                        report["interactive_failure"]["marker_pid"] = value
                print(json.dumps(report["interactive_failure"]))
                raise
            cli("close-surface", selected)
            eventually(lambda: children() == before_children)
        for surface_id in reversed(split_ids):
            cli("close-surface", surface_id)
        eventually(lambda: children() == baseline_children)
        record_resources("interactive_tabs", 3)
        samples = []
        for cycle in range(45):
            cli("split", "--direction", "horizontal")
            eventually(lambda: len(children()) == len(baseline_children) + 1)
            surfaces = json.loads(cli("list-surfaces", "--json"))["surfaces"]
            selected = next(s["uuid"] for s in surfaces if s["active"])
            if cycle % 5 == 0:
                # Replace the interactive shell with a predictable canonical-input
                # reader. Its marker proves readiness before GTK receives Ctrl-D.
                ready = root / f"eof-ready-{cycle}"
                reader = (
                    "import pathlib,sys; "
                    f"pathlib.Path({str(ready)!r}).touch(); sys.stdin.read()"
                )
                cli("send-text", "exec python3 -c " + shlex.quote(reader))
                subprocess.check_call(
                    ["xdotool", "windowfocus", window, "key", "--clearmodifiers", "Return"],
                    timeout=10,
                )
                eventually(ready.exists)
                subprocess.check_call(
                    ["xdotool", "windowfocus", window, "key", "--clearmodifiers", "ctrl+d"],
                    timeout=10,
                )
                eventually(lambda: children() == baseline_children)
                record_resources("child_eof", cycle + 1)
            cli("close-surface", selected)
            eventually(lambda: children() == baseline_children)
            if cycle in (9, 24, 44):
                time.sleep(0.5)
                samples.append(rss())
                record_resources("split_close", cycle + 1)
            assert app.poll() is None
        # Software GL allocators keep warm caches. Detect continuing substantial
        # growth, rather than requiring RSS to return to an unrealistically exact value.
        assert samples[-1] - samples[0] < 96 * 1024, f"RSS grew after warmup: {samples} KiB"
        assert len(json.loads(cli("list-surfaces", "--json"))["surfaces"]) == 1
        print(f"45 split/close cycles including 9 GTK Ctrl-D exits reaped every PTY; RSS samples: {samples} KiB")

        # The reported OOM happened without pane churn. Exercise thousands of
        # large frames as well, keeping terminal history bounded by repainting
        # the same screen. A frame-sized leak becomes visible within seconds.
        subprocess.check_call(["xdotool", "windowsize", window, "1800", "1000"])
        marker = root / "render-complete"
        program = ("import sys,time,pathlib; "
                   "[(sys.stdout.write('\\x1b[H' + (str(i%10)*100+'\\r\\n')*20), "
                   "sys.stdout.flush(),time.sleep(1/30)) for i in range(1800)]; "
                   f"pathlib.Path({str(marker)!r}).touch()")
        cli("send-text", "python3 -u -c " + shlex.quote(program))
        # send-text uses bracketed paste. Enter is a separate keyboard action.
        subprocess.check_call(["xdotool", "windowfocus", window, "key", "--clearmodifiers", "Return"])
        render_samples = []
        deadline = time.monotonic() + 90
        started = time.monotonic()
        while not marker.exists():
            assert app.poll() is None, "application exited during rendering"
            assert time.monotonic() < deadline, "terminal output stalled"
            current = rss()
            assert current < 2 * 1024 * 1024, f"rendering exceeded 2 GiB RSS: {current} KiB"
            if time.monotonic() - started > 15:
                render_samples.append(current)
                record_resources("redraw", len(render_samples))
            time.sleep(1)
        assert len(render_samples) >= 20, "sustained output did not run long enough"
        assert max(render_samples[-10:]) - min(render_samples[:10]) < 128 * 1024, f"render RSS kept growing: {render_samples} KiB"
        print(f"1800 large terminal redraws; RSS samples: {render_samples} KiB")
        report["status"] = "passed"
    except BaseException as error:
        report["status"] = "failed"
        report["error_type"] = type(error).__name__
        log.flush()
        log.seek(0)
        print(log.read())
        raise
    finally:
        try:
            report["shutdown_forced"] = stop_process(app)
        finally:
            log.close()
            destination = os.environ.get("CMUX_CHURN_REPORT")
            if destination:
                report_path = Path(destination)
                report_path.parent.mkdir(parents=True, exist_ok=True)
                descriptor = os.open(report_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
                with os.fdopen(descriptor, "w") as output:
                    json.dump(report, output, indent=2)
                    output.write("\n")
