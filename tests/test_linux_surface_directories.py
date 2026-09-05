#!/usr/bin/env python3
"""Verify per-terminal OSC 7 directories reach session snapshots without cross-routing."""

import json
from pathlib import Path
from linux_app import running_app
import tempfile
import uuid


def main():
    """Launch two real shells, report distinct changed directories and inspect saved state."""
    with tempfile.TemporaryDirectory(prefix="cmux-directories-") as directory:
        root = Path(directory)
        session_path = root / "data/cmux/session.json"
        session_path.parent.mkdir(parents=True)
        script = root / "startup.sh"
        script.write_text("cd reported\nprintf '\\033]7;file://localhost%s\\007' \"$PWD\"\n"
                          "touch ready\nexec /bin/sh\n")
        workspaces = []
        for name in ("first", "second"):
            launch = root / name
            (launch / "reported").mkdir(parents=True)
            workspaces.append(dict(
                uuid=str(uuid.uuid4()), name=name, working_directory=str(launch),
                startup_script=str(script), active_pane_uuid=None,
                layout=dict(type="Leaf", pane_id=1, surface_uuid=str(uuid.uuid4()),
                            shell="/bin/sh", cwd=""),
            ))
        session_path.write_text(json.dumps(dict(version=3, active_index=0, workspaces=workspaces)))
        with running_app(root, {"CMUX_LOG": str(root / "events.jsonl")}) as app:
            app.wait_for(lambda: (root / "first/reported/ready").exists(), "first shell directory report", timeout=15)
            app.cli("select-workspace", workspaces[1]["uuid"])
            app.wait_for(lambda: (root / "second/reported/ready").exists(), "second shell directory report", timeout=15)

            def saved_directories_match():
                """Trigger the normal save path after Ghostty has processed each report."""
                app.cli("rename-workspace", workspaces[1]["uuid"], "second")
                saved = json.loads(session_path.read_text())
                for workspace in saved["workspaces"]:
                    surfaces = workspace["layout"].get("surfaces", [])
                    expected = str(root / workspace["name"] / "reported")
                    if len(surfaces) != 1 or surfaces[0]["cwd"] != expected:
                        return False
                return len(saved["workspaces"]) == 2

            app.wait_for(saved_directories_match, "isolated persisted terminal directories", timeout=15)
            print("PASS: native directory reports stay isolated across live terminals")


if __name__ == "__main__":
    main()
