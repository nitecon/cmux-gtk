#!/usr/bin/env python3
"""Verify styled history survives normal quit and an unopened background workspace across two restarts."""
import json
import socket
from pathlib import Path
import shlex
import subprocess
import tempfile

from linux_app import running_app
from process_support import stop_process
from test_linux_resume_approval import quit_app


def assert_saved_directory(root, surface_id, expected):
    """Locate the exact terminal snapshot after durable quit and diagnose directory loss at its boundary."""
    session = json.loads((root / "data/cmux/session.json").read_text())

    def terminals(node):
        """Walk saved layout objects without relying on a particular split depth."""
        if isinstance(node, dict):
            if node.get("surface_uuid") == surface_id and "cwd" in node:
                yield node
            for value in node.values():
                yield from terminals(value)
        elif isinstance(node, list):
            for value in node:
                yield from terminals(value)

    matches = list(terminals(session))
    assert len(matches) == 1, f"saved terminal identity missing or duplicated: {surface_id}"
    assert matches[0]["cwd"] == str(expected), f"saved directory: {matches[0]['cwd']!r}, expected {str(expected)!r}"


def main():
    """Replay before fresh child output while retaining untouched background history and workspace focus."""
    with tempfile.TemporaryDirectory(prefix="cmux-history-restore-") as directory:
        root = Path(directory)
        changed = root / "changed-directory"
        changed.mkdir()
        script = root / "history.py"
        script.write_text("import os\nfor i in range(120):\n    os.write(1, ('\\x1b[32mRESTORED-%04d-界\\x1b[0m\\r\\n' % i).encode())\n")
        wm = subprocess.Popen(["openbox"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            with running_app(root) as app:
                app.wait_for(lambda: bool(app.surfaces()), "initial terminal")
                app.cli("new-workspace", "--name", "history source", "--cwd", str(root))
                source = next(row for row in app.surfaces() if row["active"])
                app.wait_for(lambda: json.loads(app.cli("health", "--id", source["uuid"], "--json"))["alive"],
                             "source native initialization")

                def capture():
                    """Observe native terminal history without changing its focus."""
                    return json.loads(app.cli("read-scrollback", "--id", source["uuid"], "--json"))["text"]

                directory_uri = "file://" + socket.gethostname() + str(changed)
                app.cli("send-text", "--id", source["uuid"], "cd " + shlex.quote(str(changed)) + "; printf '\\033]7;%s\\007' " + shlex.quote(directory_uri) + "; python3 " + shlex.quote(str(script)))
                app.cli("send-key", "--id", source["uuid"], "\r")
                app.wait_for(lambda: "RESTORED-0119" in capture(), "original history")
                app.cli("new-workspace", "--name", "keep selected")
                selected = next(row["uuid"] for row in app.surfaces() if row["active"])
                quit_app(app)
                assert_saved_directory(root, source["uuid"], changed)
            with running_app(root) as app:
                app.wait_for(lambda: bool(app.surfaces()), "restored selected terminal")
                assert next(row["uuid"] for row in app.surfaces() if row["active"]) == selected
                # Intentionally never select or read the source before the next normal quit.
                quit_app(app)
                assert_saved_directory(root, source["uuid"], changed)
            with running_app(root) as app:
                app.cli("select-workspace", source["workspace_uuid"])

                def restored():
                    """Wait for deferred native initialization without treating initial unavailability as success."""
                    try:
                        return "RESTORED-0119" in capture()
                    except subprocess.CalledProcessError:
                        return False

                app.wait_for(restored, "unopened history replay")
                cwd_file = root / "replayed-cwd"
                app.cli("send-text", "--id", source["uuid"], "pwd > " + shlex.quote(str(cwd_file)))
                app.cli("send-key", "--id", source["uuid"], "\r")
                app.wait_for(lambda: cwd_file.exists() and cwd_file.stat().st_size, "restored shell directory")
                assert cwd_file.read_text().strip() == str(changed), f"restored shell directory: {cwd_file.read_text()!r}"

                text = capture()
                assert "RESTORED-0000" in text and "界" in text
                assert "\x1b[" in text and "\x1b]" not in text
                app.cli("send-text", "--id", source["uuid"], "printf 'FRESH-%s\\n' AFTER-REPLAY")
                app.cli("send-key", "--id", source["uuid"], "\r")
                app.wait_for(lambda: "FRESH-AFTER-REPLAY" in capture(), "fresh shell output")
                text = capture()
                assert text.index("RESTORED-0119") < text.index("FRESH-AFTER-REPLAY")
                assert len(text.encode()) <= 256 * 1024
        finally:
            stop_process(wm)
    print("styled history and unopened background cache survived normal quit/reopen before fresh shell output")


if __name__ == "__main__":
    main()
