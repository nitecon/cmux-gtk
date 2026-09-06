#!/usr/bin/env python3
"""Exercise bounded native VT history capture from an inactive terminal without viewport mutation."""
import json
from pathlib import Path
import shlex
import tempfile

from linux_app import running_app


def main():
    """Retain offscreen styled rows while leaving the selected workspace and source viewport intact."""
    with tempfile.TemporaryDirectory(prefix="cmux-scrollback-") as directory:
        root = Path(directory)
        script = root / "output.py"
        script.write_text("import os\nfor i in range(300):\n    os.write(1, ('\\x1b[31mHISTORY-%04d-界\\x1b[0m\\r\\n' % i).encode())\n")
        with running_app(root) as app:
            app.wait_for(lambda: bool(app.surfaces()), "initial terminal")
            source = next(row["uuid"] for row in app.surfaces() if row["active"])
            app.cli("send-text", "--id", source, "python3 " + shlex.quote(str(script)))
            app.cli("send-key", "--id", source, "\r")

            def capture():
                """Read bounded VT through the public CLI and exact target resolver."""
                return json.loads(app.cli("read-scrollback", "--id", source, "--json"))["text"]

            app.wait_for(lambda: "HISTORY-0299" in capture(), "terminal output fully applied")
            app.cli("new-workspace", "--name", "capture observer")
            selected = next(row["uuid"] for row in app.surfaces() if row["active"])
            before = json.loads(app.cli("read-text", "--id", source, "--json"))["text"]
            text = capture()
            after = json.loads(app.cli("read-text", "--id", source, "--json"))["text"]
            assert "HISTORY-0000" in text and "HISTORY-0299" in text and "界" in text
            assert "\x1b[" in text, "native capture lost styled VT representation"
            assert "\x1b]" not in text, "history retained terminal OSC commands"
            assert text.startswith("\x1b[0m") and text.endswith("\x1b[0m")
            assert len(text.encode()) <= 256 * 1024
            assert "HISTORY-0000" not in before, "fixture did not create offscreen history"
            assert before == after, "history capture changed the viewport"
            assert next(row["uuid"] for row in app.surfaces() if row["active"]) == selected
    print("bounded native styled history preserved inactive target, focus and viewport")


if __name__ == "__main__":
    main()
