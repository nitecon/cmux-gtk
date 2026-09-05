#!/usr/bin/env python3
"""Verify literal Ctrl+C delivery through cmux to an unfocused native terminal."""
from pathlib import Path
import shlex
import tempfile

from linux_app import running_app


PROBE = '''import os
from pathlib import Path
import signal
import sys

ready, result = map(Path, sys.argv[1:])

def interrupted(signum, frame):
    """Record the signal received by the terminal child and exit normally."""
    result.write_text(str(signum))
    raise SystemExit(0)

signal.signal(signal.SIGINT, interrupted)
signal.alarm(30)
ready.write_text(str(os.getpid()))
while True:
    signal.pause()
'''


with tempfile.TemporaryDirectory(prefix="cmux-sigint-") as directory:
    root = Path(directory)
    probe = root / "probe.py"
    probe.write_text(PROBE)
    ready, result = root / "ready", root / "result"
    with running_app(root) as app:
        app.wait_for(lambda: len(app.children()) == 1, "initial terminal child")
        target = app.surfaces()[0]["uuid"]
        app.cli("split", "--direction", "horizontal")
        app.wait_for(lambda: len(app.children()) == 2, "foreground terminal child")
        before = app.surfaces()
        foreground = next(row["uuid"] for row in before if row["active"])
        assert foreground != target
        command = "exec python3 " + " ".join(shlex.quote(str(path)) for path in (probe, ready, result))
        app.cli("send-text", command, "--id", target)
        app.cli("send-key", "\r", "--id", target)
        app.wait_for(lambda: ready.exists() and ready.read_text().strip().isdigit(), "SIGINT probe readiness")
        child = ready.read_text().strip()
        app.cli("send-key", "\x03", "--id", target)
        app.wait_for(lambda: result.exists() and result.read_text() == "2", "SIGINT received through terminal input")
        app.wait_for(lambda: not Path(f"/proc/{child}").exists(), "SIGINT child reaped")
        assert app.surfaces() == before, "signal delivery changed selected surface or layout"
        app.cli("close-surface", target)
        app.wait_for(lambda: len(app.children()) == 1, "closed probe terminal cleanup")
        assert [row["uuid"] for row in app.surfaces()] == [foreground]
    print("literal Ctrl+C reached the unfocused terminal child without focus mutation")
