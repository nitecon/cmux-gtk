#!/usr/bin/env python3
"""Verify styled history survives normal quit and an unopened background workspace across two restarts."""
import json
from pathlib import Path
import shlex
import subprocess
import tempfile

from linux_app import running_app
from process_support import stop_process
from test_linux_resume_approval import quit_app


def main():
    """Replay before fresh child output while retaining untouched background history and workspace focus."""
    with tempfile.TemporaryDirectory(prefix="cmux-history-restore-") as directory:
        root = Path(directory)
        script = root / "history.py"
        script.write_text("import os\nfor i in range(120):\n    os.write(1, ('\\x1b[32mRESTORED-%04d-界\\x1b[0m\\r\\n' % i).encode())\n")
        wm = subprocess.Popen(["openbox"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            with running_app(root) as app:
                app.wait_for(lambda: bool(app.surfaces()), "initial terminal")
                source = next(row for row in app.surfaces() if row["active"])

                def capture():
                    """Observe native terminal history without changing its focus."""
                    return json.loads(app.cli("read-scrollback", "--id", source["uuid"], "--json"))["text"]

                app.cli("send-text", "--id", source["uuid"], "python3 " + shlex.quote(str(script)))
                app.cli("send-key", "--id", source["uuid"], "\r")
                app.wait_for(lambda: "RESTORED-0119" in capture(), "original history")
                app.cli("new-workspace", "--name", "keep selected")
                selected = next(row["uuid"] for row in app.surfaces() if row["active"])
                quit_app(app)
            with running_app(root) as app:
                app.wait_for(lambda: bool(app.surfaces()), "restored selected terminal")
                assert next(row["uuid"] for row in app.surfaces() if row["active"]) == selected
                # Intentionally never select or read the source before the next normal quit.
                quit_app(app)
            with running_app(root) as app:
                app.cli("select-workspace", source["workspace_uuid"])

                def restored():
                    """Wait for deferred native initialization without treating initial unavailability as success."""
                    try:
                        return "RESTORED-0119" in capture()
                    except subprocess.CalledProcessError:
                        return False

                app.wait_for(restored, "unopened history replay")
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
