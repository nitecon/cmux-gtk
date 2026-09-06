#!/usr/bin/env python3
"""Verify bounded message history, read state and exact target navigation through the real CLI."""
import json
from pathlib import Path
import subprocess
import tempfile
import uuid

from linux_app import running_app
from process_support import stop_process
from test_linux_resume_approval import quit_app


def main():
    """Deliver to background panes without focus, navigate deliberately and restore retained history."""
    with tempfile.TemporaryDirectory(prefix="cmux-inbox-") as directory:
        root = Path(directory)
        wm = subprocess.Popen(["openbox"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            with running_app(root) as app:
                app.wait_for(lambda: bool(app.surfaces()), "initial surface")
                initial = next(row for row in app.surfaces() if row["active"])
                app.cli("new-workspace", "--name", "background")
                first = next(row for row in app.surfaces() if row["active"])
                app.cli("split", "--direction", "horizontal")
                second = next(row for row in app.surfaces() if row["active"])
                app.cli("select-workspace", initial["workspace_uuid"])

                def messages():
                    """Read retained messages without selecting their workspace."""
                    return json.loads(app.cli("notifications", "list", "--json"))["notifications"]

                def active():
                    """Read the selected exact terminal identity."""
                    return next(row["uuid"] for row in app.surfaces() if row["active"])

                assert messages() == []
                ids = []
                for surface in (first, second):
                    result = json.loads(app.cli("notify", "--surface", surface["uuid"], "--title", "Agent waiting",
                                                "--subtitle", "Review", "--body", "literal <text> $HOME", "--json"))
                    ids.append(result["id"])
                    assert result["workspace_id"] == first["workspace_uuid"]
                    assert active() == initial["uuid"]
                assert [row["surface_id"] for row in messages()] == [first["uuid"], second["uuid"]]
                assert all(not row["is_read"] for row in messages())
                app.cli("notifications", "mark-read", "--id", ids[0])
                assert [row["is_read"] for row in messages()] == [True, False]
                assert active() == initial["uuid"]
                for params in ({"surface_id": str(uuid.uuid4())}, {"surface_id": first["uuid"], "workspace_id": initial["workspace_uuid"]},
                               {"surface_id": 17}, {"surface_id": first["uuid"], "body": "x" * 8193}):
                    try:
                        app.cli("raw", "notification.create", "--params", json.dumps(params))
                    except subprocess.CalledProcessError:
                        pass
                    else:
                        raise AssertionError("invalid or stale notification target was accepted")
                assert len(messages()) == 2 and active() == initial["uuid"]
                opened = json.loads(app.cli("notifications", "jump-to-unread", "--json"))
                assert opened["id"] == ids[1] and opened["opened"]
                assert active() == second["uuid"]
                assert all(row["is_read"] for row in messages())
                app.cli("notifications", "dismiss", "--id", ids[0])
                assert [row["id"] for row in messages()] == [ids[1]]
                saved = messages()
                quit_app(app)
            with running_app(root) as app:
                assert messages() == saved, "read history was not retained across normal quit"
                app.cli("select-workspace", initial["workspace_uuid"])
                for index in range(150):
                    app.cli("notify", "--surface", first["uuid"], "--title", str(index), "--body", "x" * 8192)
                rows = messages()
                assert len(rows) < 128 and rows[-1]["title"] == "149"
                assert active() == initial["uuid"]
                app.cli("notifications", "clear", "--workspace", first["workspace_uuid"])
                assert messages() == [] and active() == initial["uuid"]
        finally:
            stop_process(wm)
    print("notification targeting, history, read/navigation semantics and retained memory bounds passed")


if __name__ == "__main__":
    main()
