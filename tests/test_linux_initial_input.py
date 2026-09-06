#!/usr/bin/env python3
"""Verify first and newly selected terminals accept physical keys before submission.

Buffer readback and executed output are observable PTY behavior; this fixture does
not claim compositor presentation or sample screenshots that could force redraw.
"""
import json
from pathlib import Path
import subprocess
import tempfile

from linux_app import running_app
from test_multi_workspace_focus import selected_surface


def main():
    """Type without surface focus repair, require the unsubmitted buffer, then actual child output."""
    with tempfile.TemporaryDirectory(prefix="cmux-initial-input-") as directory:
        with running_app(Path(directory)) as app:
            app.wait_for(lambda: len(app.children()) == 1, "initial shell")
            windows = subprocess.check_output(
                ["xdotool", "search", "--onlyvisible", "--pid", str(app.process.pid)],
                text=True, timeout=10,
            ).split()
            assert windows
            subprocess.check_call(["xdotool", "windowfocus", "--sync", windows[-1]], timeout=10)
            for index in range(2):
                if index:
                    app.cli("new-workspace")
                    app.wait_for(lambda: len(app.children()) == 2, "new workspace shell")
                surface = selected_surface(app)

                def text():
                    """Decode the selected terminal viewport without focusing or refreshing it."""
                    return json.loads(app.cli("read-text", "--id", surface, "--json"))["text"]

                app.wait_for(lambda: bool(text().strip()), "initial prompt output")
                command = "printf 'CMUX_EXEC_%s\\n' " + str(index)
                expected = f"CMUX_EXEC_{index}"
                subprocess.check_call(
                    ["xdotool", "type", "--clearmodifiers", "--delay", "1", "--", command], timeout=10,
                )
                app.wait_for(lambda: command in text(), "unsubmitted typed command")
                assert expected not in text().splitlines(), "command executed before Enter"
                assert selected_surface(app) == surface
                subprocess.check_call(["xdotool", "key", "--clearmodifiers", "Return"], timeout=10)
                app.wait_for(lambda: expected in text().splitlines(), "executed shell output")
                assert selected_surface(app) == surface
    print("first and newly selected terminals accepted physical input before Enter")


if __name__ == "__main__":
    main()
