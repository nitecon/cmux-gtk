#!/usr/bin/env python3
"""Verify closing a nested terminal pane preserves siblings and reaps exactly its PTY child."""
from pathlib import Path
import re
import subprocess
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
        # Target the first pane while the last pane is focused. Traversal order must
        # place the new split beside the requested pane, not beside the focused one.
        survivors = [surface["uuid"] for surface in after]
        app.cli("focus-surface", survivors[-1])
        app.cli("split", "--id", survivors[0], "--direction", "horizontal")
        targeted = app.surfaces()
        new_id = next(surface["uuid"] for surface in targeted if surface["active"])
        assert new_id not in survivors
        assert [surface["uuid"] for surface in targeted] == [survivors[0], new_id, survivors[1]]
        app.wait_for(lambda: len(app.children()) == len(before_children), "targeted split child")
        try:
            app.cli("split", "--id", "00000000-0000-4000-8000-000000000000")
        except subprocess.CalledProcessError:
            pass
        else:
            raise AssertionError("unknown split target unexpectedly succeeded")
        assert app.surfaces() == targeted, "failed split changed layout or selection"
        app.cli("close-surface", new_id)
        app.wait_for(lambda: len(app.children()) == len(before_children) - 1, "targeted split cleanup")
    log = (root / "app.log").read_text()
    assert not re.search(r"gtk_paned_set_(start|end)_child: assertion|segmentation fault|core dumped", log)
    print("terminal pane close preserved siblings and reaped one PTY")
