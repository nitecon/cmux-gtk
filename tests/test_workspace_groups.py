#!/usr/bin/env python3
"""Exercise persistent group identity, membership and collapse through the production CLI."""
import json
from pathlib import Path
import tempfile

from linux_app import running_app


def records(app):
    return json.loads(app.cli("list-workspaces", "--json"))["workspaces"]


def groups(app):
    return json.loads(app.cli("list-workspace-groups", "--json"))["groups"]


with tempfile.TemporaryDirectory(prefix="cmux-workspace-groups-") as directory:
    root = Path(directory)
    with running_app(root) as app:
        app.cli("new-workspace")
        app.cli("new-workspace")
        workspace_ids = [row["uuid"] for row in records(app)]
        created = json.loads(app.cli(
            "create-workspace-group", "Backend", "--color", "#285943", "--json"
        ))
        group_id = created["id"]
        second_group = json.loads(app.cli("create-workspace-group", "Frontend", "--json"))["id"]
        app.cli("update-workspace-group", second_group, "--position", "0")
        assert [group["id"] for group in groups(app)] == [second_group, group_id]
        app.cli(
            "assign-workspace-group", "--group", group_id,
            "--workspaces", ",".join(workspace_ids[:2]),
        )
        current = groups(app)
        backend = next(group for group in current if group["id"] == group_id)
        assert backend["workspace_ids"] == workspace_ids[:2]
        assert backend["color"] == "#285943"
        assert [row["group_id"] for row in records(app)] == [group_id, group_id, None]
        app.cli("update-workspace-group", group_id, "--collapsed", "true")
        assert next(group for group in groups(app) if group["id"] == group_id)["collapsed"] is True

        # Validation is atomic: an unknown workspace cannot partially change membership.
        unknown = "00000000-0000-0000-0000-000000000001"
        try:
            app.cli(
                "assign-workspace-group", "--group", group_id,
                "--workspaces", f"{workspace_ids[2]},{unknown}",
            )
            raise AssertionError("invalid assignment unexpectedly succeeded")
        except Exception as error:
            assert "workspace not found" in getattr(error, "output", "") or getattr(error, "returncode", 0) != 0
        assert records(app)[2]["group_id"] is None

    # Graceful quit flushes group metadata through the sole session writer.
    with running_app(root) as app:
        restored = groups(app)
        assert [group["id"] for group in restored] == [second_group, group_id]
        backend = restored[1]
        assert backend["collapsed"] is True
        assert backend["workspace_ids"] == workspace_ids[:2]
        app.cli("delete-workspace-group", group_id)
        assert [group["id"] for group in groups(app)] == [second_group]
        assert all(row["group_id"] is None for row in records(app))
