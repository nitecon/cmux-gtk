#!/usr/bin/env python3
"""Exercise bounded workspace metadata through the real CLI and normal restart."""
import json
from pathlib import Path
import subprocess
import tempfile
import uuid

from linux_app import running_app
from process_support import stop_process
from test_linux_resume_approval import quit_app


def main():
    """Update an inactive workspace without focus changes, reject bad input and retain state."""
    with tempfile.TemporaryDirectory(prefix="cmux-sidebar-") as directory:
        root = Path(directory)
        wm = subprocess.Popen(["openbox"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            with running_app(root) as app:
                app.wait_for(lambda: bool(app.surfaces()), "initial surface")
                initial = next(row for row in app.surfaces() if row["active"])
                app.cli("new-workspace", "--name", "metadata")
                target = next(row for row in app.surfaces() if row["active"])["workspace_uuid"]
                app.cli("select-workspace", initial["workspace_uuid"])

                def metadata():
                    """Fetch the background workspace without changing focus."""
                    return json.loads(app.cli("list-status", "--workspace", target, "--json"))

                app.cli("set-status", "agent", "literal <b>text</b> $HOME", "--icon", "dialog-information-symbolic",
                        "--color", "#123456", "--priority", "9", "--tab", target)
                app.cli("set-progress", "0.4", "--label", "Compiling", "--workspace", target)
                state = metadata()
                assert state["statuses"]["agent"] == {"value": "literal <b>text</b> $HOME",
                                                       "icon": "dialog-information-symbolic", "color": "#123456", "priority": 9, "format": "plain", "url": None}
                assert state["progress"] == {"value": 0.4, "label": "Compiling"}
                for index in range(31):
                    app.cli("set-status", f"key{index}", str(index), "--workspace", target)
                app.cli("set-status", "agent", "**updated** [details](https://example.com)",
                        "--format", "markdown", "--url", "https://example.com/status", "--workspace", target)
                saved = metadata()
                assert saved["statuses"]["agent"]["format"] == "markdown"
                assert saved["statuses"]["agent"]["url"] == "https://example.com/status"
                for method, fields in (
                    ("sidebar.set_status", {"key": "overflow", "value": "rejected"}),
                    ("sidebar.set_status", {"key": "agent", "value": "x" * 1025}),
                    ("sidebar.set_status", {"key": "agent", "value": "x", "color": "red'/>"}),
                    ("sidebar.set_status", {"key": "agent", "value": "x", "format": "html"}),
                    ("sidebar.set_status", {"key": "agent", "value": "x", "url": "file:///tmp/status"}),
                    ("sidebar.set_progress", {"value": 0.5, "label": "x" * 513}),
                    ("sidebar.metadata", {"workspace_id": str(uuid.uuid4())}),
                    ("sidebar.metadata", {"workspace_id": 42}),
                ):
                    try:
                        app.cli("raw", method, "--params", json.dumps({"workspace_id": target, **fields}))
                    except subprocess.CalledProcessError:
                        pass
                    else:
                        raise AssertionError(f"invalid metadata accepted: {method}")
                assert metadata() == saved
                assert next(row["uuid"] for row in app.surfaces() if row["active"]) == initial["uuid"]
                quit_app(app)
            with running_app(root) as app:
                assert metadata() == saved, "metadata changed across normal quit/restart"
                app.cli("clear-status", "agent", "--workspace", target)
                assert "agent" not in metadata()["statuses"]
                app.cli("set-progress", "2", "--workspace", target)
                assert metadata()["progress"]["value"] == 1.0
                app.cli("clear-progress", "--workspace", target)
                assert metadata()["progress"] is None
                assert next(row["uuid"] for row in app.surfaces() if row["active"]) == initial["uuid"]
        finally:
            stop_process(wm)
    print("sidebar status/progress bounds, focus and persistence passed")


if __name__ == "__main__":
    main()
