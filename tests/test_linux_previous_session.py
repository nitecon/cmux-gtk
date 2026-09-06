#!/usr/bin/env python3
"""Recover a workspace closed during a later run from the separate startup backup."""
import json
from pathlib import Path
import subprocess
import tempfile

from linux_app import running_app
from process_support import stop_process
from test_linux_resume_approval import quit_app


def main():
    """Normal autosaves cannot replace the previous snapshot; explicit recovery leaves its source intact."""
    with tempfile.TemporaryDirectory(prefix="cmux-previous-") as directory:
        root = Path(directory)
        backup = root / "data/cmux/session.previous.json"
        wm = subprocess.Popen(["openbox"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            with running_app(root) as app:
                app.cli("new-workspace", "--name", "Recover this")
                recovered = next(row for row in app.surfaces() if row["active"])
                quit_app(app)
            with running_app(root) as app:
                assert backup.exists()
                original = backup.read_bytes()
                app.cli("close-workspace", recovered["workspace_uuid"])
                assert all(row["workspace_uuid"] != recovered["workspace_uuid"] for row in app.surfaces())
                quit_app(app)
                assert backup.read_bytes() == original, "live saves overwrote recovery source"
            with running_app(root, extra_arguments=["--restore-previous-session"]) as app:
                assert any(row["uuid"] == recovered["uuid"] for row in app.surfaces())
                app.cli("new-workspace", "--name", "After recovery")
                quit_app(app)
                assert backup.read_bytes() == original, "recovery replaced its own source"
                saved = json.loads((root / "data/cmux/session.json").read_text())
                assert any(row["uuid"] == recovered["workspace_uuid"] for row in saved["workspaces"])
            current = (root / "data/cmux/session.json").read_bytes()
            backup.write_text("invalid backup")
            binary = Path(app.environment.get("CMUX_BIN_DIR", "target/debug")) / "cmux-app"
            failed = subprocess.run([str(binary), "--restore-previous-session"], env=app.environment,
                                    capture_output=True, text=True, timeout=15)
            assert failed.returncode != 0 and "no valid previous session" in failed.stderr
            assert (root / "data/cmux/session.json").read_bytes() == current

        finally:
            stop_process(wm)
    print("previous session remained independent of autosaves and explicit recovery retained workspace identity")


if __name__ == "__main__":
    main()
