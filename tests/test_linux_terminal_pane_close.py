#!/usr/bin/env python3
"""Verify closing a nested terminal pane preserves siblings and reaps exactly its PTY child."""
from pathlib import Path
import re
import tempfile

from linux_app import running_app


with tempfile.TemporaryDirectory(prefix="cmux-terminal-close-") as directory:
    root = Path(directory)
    with running_app(root) as app:
        app.wait_for(lambda: len(app.children()) >= 1, "initial terminal child")
        baseline_children = app.children()
        app.cli("split", "--direction", "horizontal")
        app.cli("split", "--direction", "vertical")
        before = app.surfaces()
        assert len(before) == 3, f"expected three surfaces: {before}"
        selected = next(surface["uuid"] for surface in before if surface["active"])
        app.wait_for(lambda: len(app.children()) == len(baseline_children) + 2, "split children")
        before_children = app.children()
        app.cli("close-surface", selected)
        app.cli("ping")
        after = app.surfaces()
        assert {surface["uuid"] for surface in after} == {
            surface["uuid"] for surface in before if surface["uuid"] != selected
        }, f"closing a pane changed surviving identities: {after}"
        app.wait_for(lambda: len(app.children()) == len(before_children) - 1, "closed PTY child")
        assert app.children() < before_children, "pane close replaced an unrelated child"
        assert baseline_children <= app.children(), "pane close terminated the original terminal"
    log = (root / "app.log").read_text()
    assert not re.search(r"gtk_paned_set_(start|end)_child: assertion|segmentation fault|core dumped", log)
    print("terminal pane close preserved siblings and reaped one PTY")
