#!/usr/bin/env python3
"""Exercise real Ghostty terminal cleanup and measure bounded RSS after warmup."""
import json
import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import time


def eventually(check):
    for _ in range(100):
        if check():
            return
        time.sleep(0.1)
    raise AssertionError("terminal state did not converge")


with tempfile.TemporaryDirectory(prefix="cmux-memory-") as directory:
    root = Path(directory)
    env = dict(os.environ, XDG_DATA_HOME=str(root / "data"),
               XDG_CONFIG_HOME=str(root / "config"), XDG_STATE_HOME=str(root / "state"),
               XDG_RUNTIME_DIR=str(root / "runtime"), GDK_BACKEND="x11",
               LIBGL_ALWAYS_SOFTWARE="1", CMUX_NO_UPDATE="1")
    (root / "runtime").mkdir(mode=0o700)
    socket = root / "runtime/cmux/cmux.sock"
    log = (root / "app.log").open("w+")
    app = subprocess.Popen(["target/debug/cmux-app"], env=env, stdout=log, stderr=log)

    def cli(*args):
        return subprocess.check_output(["target/debug/cmux", "--socket", str(socket), *args], env=env, text=True)

    def rss():
        status = Path(f"/proc/{app.pid}/status").read_text()
        return int(next(line.split()[1] for line in status.splitlines() if line.startswith("VmRSS:")))

    def children():
        return set(Path(f"/proc/{app.pid}/task/{app.pid}/children").read_text().split())

    try:
        eventually(socket.exists)
        eventually(lambda: len(children()) >= 1)
        baseline_children = children()
        samples = []
        for cycle in range(45):
            cli("split", "--direction", "horizontal")
            eventually(lambda: len(children()) == len(baseline_children) + 1)
            surfaces = json.loads(cli("list-surfaces", "--json"))["surfaces"]
            selected = next(s["uuid"] for s in surfaces if s["active"])
            cli("close-surface", selected)
            eventually(lambda: children() == baseline_children)
            if cycle in (9, 24, 44):
                time.sleep(0.5)
                samples.append(rss())
            assert app.poll() is None
        # Software GL allocators keep warm caches. Detect continuing substantial
        # growth, rather than requiring RSS to return to an unrealistically exact value.
        assert samples[-1] - samples[0] < 96 * 1024, f"RSS grew after warmup: {samples} KiB"
        assert len(json.loads(cli("list-surfaces", "--json"))["surfaces"]) == 1
        print(f"45 split/close cycles reaped every PTY; RSS samples: {samples} KiB")

        # The reported OOM happened without pane churn. Exercise thousands of
        # large frames as well, keeping terminal history bounded by repainting
        # the same screen. A frame-sized leak becomes visible within seconds.
        windows = subprocess.check_output(["xdotool", "search", "--pid", str(app.pid)], text=True).split()
        assert windows, "application has no X11 window"
        subprocess.check_call(["xdotool", "windowsize", windows[-1], "1800", "1000"])
        marker = root / "render-complete"
        program = ("import sys,time,pathlib; "
                   "[(sys.stdout.write('\\x1b[H' + (str(i%10)*160+'\\r\\n')*50), "
                   "sys.stdout.flush(),time.sleep(1/30)) for i in range(1800)]; "
                   f"pathlib.Path({str(marker)!r}).touch()")
        cli("send-text", "python3 -u -c " + shlex.quote(program) + "\n")
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
            time.sleep(1)
        assert len(render_samples) >= 20, "sustained output did not run long enough"
        assert max(render_samples[-10:]) - min(render_samples[:10]) < 128 * 1024, f"render RSS kept growing: {render_samples} KiB"
        print(f"1800 large terminal redraws; RSS samples: {render_samples} KiB")
    except BaseException:
        log.flush()
        log.seek(0)
        print(log.read())
        raise
    finally:
        app.terminate()
        app.wait(timeout=10)
        log.close()
