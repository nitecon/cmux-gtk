#!/usr/bin/env python3
"""Verify workspace close selection through the Linux CLI in an isolated application."""
import json
from pathlib import Path
import tempfile

from linux_app import running_app


def workspaces(app):
    """Return ordered workspace records from the production JSON CLI."""
    return json.loads(app.cli("list-workspaces", "--json"))["workspaces"]


def selected(app):
    """Require exactly one selected workspace and return its stable UUID."""
    active = [row["uuid"] for row in workspaces(app) if row["selected"]]
    assert len(active) == 1, active
    return active[0]


with tempfile.TemporaryDirectory(prefix="cmux-close-selection-") as directory:
    with running_app(Path(directory)) as app:
        for _ in range(3):
            app.cli("new-workspace")
        rows = workspaces(app)
        assert len(rows) == 4, rows
        ids = [row["uuid"] for row in rows]
        app.cli("select-workspace", ids[1])
        assert selected(app) == ids[1]
        app.cli("close-workspace", ids[1])
        assert selected(app) == ids[2], "closing middle must select the next workspace"
        assert [row["uuid"] for row in workspaces(app)] == [ids[0], ids[2], ids[3]]

        app.cli("close-workspace", ids[0])
        assert selected(app) == ids[2], "closing an earlier row must preserve selected identity"
        app.cli("select-workspace", ids[3])
        app.cli("close-workspace", ids[3])
        assert selected(app) == ids[2], "closing last must select the previous workspace"

        app.cli("new-workspace")
        extra = next(row["uuid"] for row in workspaces(app) if row["uuid"] != ids[2])
        app.cli("select-workspace", ids[2])
        app.cli("close-workspace", extra)
        assert selected(app) == ids[2], "closing another workspace must preserve selection"
        app.wait_for(lambda: len(app.children()) == 1, "closed workspace PTY cleanup")
    print("workspace closure preserves selection and reaps removed PTYs")
