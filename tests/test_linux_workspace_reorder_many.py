#!/usr/bin/env python3
"""Verify atomic batch reordering, dry-run plans, native identities and restart order."""
import json
from pathlib import Path
import subprocess
import tempfile
import uuid

from linux_app import running_app
from process_support import stop_process
from test_linux_resume_approval import quit_app


def main():
    """Reject entire invalid batches and move existing terminals without changing active identity."""
    with tempfile.TemporaryDirectory(prefix="cmux-reorder-") as directory:
        root = Path(directory)
        wm = subprocess.Popen(["openbox"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            with running_app(root) as app:
                app.wait_for(lambda: bool(app.surfaces()), "initial surface")
                ids = [next(row for row in app.surfaces() if row["active"])["workspace_uuid"]]
                for name in ("second", "third"):
                    app.cli("new-workspace", "--name", name)
                    ids.append(next(row for row in app.surfaces() if row["active"])["workspace_uuid"])
                app.cli("select-workspace", ids[1])
                active = next(row["uuid"] for row in app.surfaces() if row["active"])

                def order():
                    """Read native workspace order without changing selection."""
                    return [row["uuid"] for row in json.loads(app.cli("list-workspaces", "--json"))["workspaces"]]

                preview = json.loads(app.cli("reorder-workspaces", "--order", ids[2], "--dry-run", "--json"))
                assert preview["dry_run"] and preview["events"] == []
                assert [item["workspace_id"] for item in preview["plan"]] == [ids[2], ids[0], ids[1]]
                assert order() == ids
                for requested in ([ids[2], ids[2]], [ids[2], str(uuid.uuid4())], []):
                    try:
                        app.cli("raw", "workspace.reorder_many", "--params", json.dumps({"workspace_ids": requested}))
                    except subprocess.CalledProcessError:
                        pass
                    else:
                        raise AssertionError("invalid batch accepted")
                    assert order() == ids
                result = json.loads(app.cli("reorder-workspaces", "--order", ids[2], "--json"))
                assert result["plan"] == preview["plan"] and len(result["events"]) == 3
                assert order() == [ids[2], ids[0], ids[1]]
                assert next(row["uuid"] for row in app.surfaces() if row["active"]) == active
                quit_app(app)
            with running_app(root) as app:
                assert order() == [ids[2], ids[0], ids[1]]
                assert next(row["uuid"] for row in app.surfaces() if row["active"]) == active
                app.cli("reorder-workspaces", "--order", ",".join(ids))
                assert order() == ids
        finally:
            stop_process(wm)
    print("batch reorder atomic validation, dry-run, focus and persistence passed")


if __name__ == "__main__":
    main()
