#!/usr/bin/env python3
"""Verify sibling terminal close selection and background-pane focus through Linux GTK/CLI."""
from pathlib import Path
import subprocess
import tempfile

from linux_app import running_app


def active(app):
    """Require one selected surface across the active workspace and return its UUID."""
    selected = [row["uuid"] for row in app.surfaces() if row["active"]]
    assert len(selected) == 1, selected
    return selected[0]


with tempfile.TemporaryDirectory(prefix="cmux-tab-selection-") as directory:
    with running_app(Path(directory)) as app:
        app.wait_for(lambda: len(app.children()) == 1, "initial shell")
        windows = subprocess.check_output(
            ["xdotool", "search", "--onlyvisible", "--pid", str(app.process.pid)],
            text=True, timeout=10,
        ).split()
        assert windows, "application has no X11 window"
        for count in range(2, 5):
            subprocess.check_call(
                ["xdotool", "windowfocus", windows[-1], "key", "--clearmodifiers", "ctrl+t"],
                timeout=10,
            )
            app.wait_for(lambda: len(app.children()) == count, "new sibling terminal")
        ids = [row["uuid"] for row in app.surfaces()]
        assert len(ids) == 4, ids
        app.cli("focus-surface", ids[1])
        app.cli("close-surface", ids[1])
        assert active(app) == ids[2], "middle close must select the next tab"
        app.cli("close-surface", ids[0])
        assert active(app) == ids[2], "earlier close must preserve selected UUID"
        app.cli("focus-surface", ids[3])
        app.cli("close-surface", ids[3])
        assert active(app) == ids[2], "last close must select the previous tab"
        app.wait_for(lambda: len(app.children()) == 1, "closed sibling PTYs")

        subprocess.check_call(
            ["xdotool", "windowfocus", windows[-1], "key", "--clearmodifiers", "ctrl+t"],
            timeout=10,
        )
        app.wait_for(lambda: len(app.children()) == 2, "background close sibling")
        background = active(app)
        app.cli("split", "--direction", "horizontal")
        foreground = active(app)
        assert foreground != background
        app.cli("close-surface", background)
        assert active(app) == foreground, "closing a background tab stole pane focus"
        assert {row["uuid"] for row in app.surfaces()} == {ids[2], foreground}
        app.wait_for(lambda: len(app.children()) == 2, "background tab PTY cleanup")
        # Three panes distinguish the active pane from the fallback sibling
        # selected by tree collapse when the background pane disappears.
        app.cli("split", "--direction", "vertical")
        final_focus = active(app)
        assert final_focus not in {ids[2], foreground}
        app.cli("close-surface", ids[2])
        assert active(app) == final_focus, "removing a background pane stole focus"
        assert {row["uuid"] for row in app.surfaces()} == {foreground, final_focus}
        app.wait_for(lambda: len(app.children()) == 2, "background pane PTY cleanup")
    print("sibling and pane closure preserve selection and background focus")
