#!/usr/bin/env python3
"""Actions-only physical composer shortcuts: observe exact bytes at the foreground process."""
import json
from pathlib import Path
import shlex
import subprocess
import tempfile

from linux_app import running_app


def main():
    """Require immediate press/release delivery without submission, draft edits or window actions."""
    with tempfile.TemporaryDirectory(prefix="cmux-input-keys-") as directory:
        root = Path(directory)
        executable = root / "input-keys"
        log = root / "keys.bin"
        subprocess.run([
            "cc", "-Wall", "-Wextra", "-O2",
            str(Path(__file__).parent / "fixtures/input_keys.c"), "-o", str(executable),
        ], check=True, timeout=20)
        with running_app(root, {"SHELL": "/bin/bash"}) as app:
            surface = app.surfaces()[0]["uuid"]

            def rpc(method, **params):
                """Read or mutate the test-owned surface through the production socket."""
                return json.loads(app.cli("raw", method, "--params", json.dumps(params), "--json"))

            def screen():
                """Inspect process output and composer draft without repairing focus."""
                return rpc("surface.read_text", id=surface)

            app.wait_for(lambda: bool(screen()["text"].strip()), "initial shell")
            rpc("surface.send_text", id=surface, text=shlex.join([str(executable), str(log)]))
            rpc("surface.send_key", id=surface, key="\r")
            try:
                app.wait_for(lambda: "CMUX key recorder ready" in screen()["text"]
                             and screen()["input"]["active"], "foreground recorder and composer")
            except AssertionError:
                print("Recorder startup:", json.dumps(screen()))
                raise
            windows = subprocess.check_output([
                "xdotool", "search", "--onlyvisible", "--pid", str(app.process.pid),
            ], text=True, timeout=10).split()
            assert windows
            subprocess.run(["xdotool", "windowfocus", "--sync", windows[-1]], check=True, timeout=10)

            # Kitty CSI key codes and modifier values must survive GTK and window capture unchanged.
            shortcuts = (
                ("ctrl+Tab", b"9;5", b"u"),
                ("ctrl+space", b"32;5", b"u"),
                ("shift+Left", b"1;2", b"D"),
                ("shift+Right", b"1;2", b"C"),
                ("shift+Home", b"1;2", b"H"),
                ("shift+Tab", b"9;2", b"u"),
                ("ctrl+shift+Tab", b"9;6", b"u"),
                ("ctrl+shift+Left", b"1;6", b"D"),
                ("alt+Left", b"1;3", b"D"),
                ("ctrl+n", b"110;5", b"u"),
                ("ctrl+d", b"100;5", b"u"),
                ("ctrl+shift+t", b"116;6", b"u"),
            )
            for draft in ("", "LOCAL draft"):
                if draft:
                    subprocess.run([
                        "xdotool", "type", "--clearmodifiers", "--delay", "1", "--", draft,
                    ], check=True, timeout=10)
                    app.wait_for(lambda: screen()["input"]["draft"] == draft, "plain Shift text stays local")
                for shortcut, code, final in shortcuts:
                    start = log.stat().st_size
                    subprocess.run([
                        "xdotool", "key", "--clearmodifiers", shortcut,
                    ], check=True, timeout=10)
                    press = b"\x1b[" + code + final
                    release = b"\x1b[" + code + b":3" + final

                    def delivered():
                        """Require both native events in order, before any Enter or draft submission."""
                        data = log.read_bytes()[start:]
                        return press in data and release in data and data.index(press) < data.index(release)

                    try:
                        app.wait_for(delivered, "immediate " + shortcut + " press/release")
                    except AssertionError:
                        print("Received key bytes:", repr(log.read_bytes()[start:]))
                        raise
                    data = log.read_bytes()[start:]
                    assert data.count(press) == data.count(release) == 1, (shortcut, data)
                    assert screen()["input"]["draft"] == draft, shortcut
                    assert len(app.surfaces()) == 1, "composer shortcut triggered a window action"
            assert b"LOCAL draft" not in log.read_bytes(), "unfinished draft leaked to the process"
            # A modifier can be released before its key; release ownership follows the physical key.
            start = log.stat().st_size
            subprocess.run([
                "xdotool", "keydown", "ctrl", "keydown", "Tab", "keyup", "ctrl", "keyup", "Tab",
            ], check=True, timeout=10)
            app.wait_for(lambda: b"\x1b[9;1:3u" in log.read_bytes()[start:], "release after modifier key-up")
            # A further ordinary keystroke proves shortcuts retained the editor's focus and caret.
            subprocess.run(["xdotool", "type", "--clearmodifiers", "--", "!"], check=True, timeout=10)
            app.wait_for(lambda: screen()["input"]["draft"] == "LOCAL draft!", "composer keeps focus and caret")
            print("PASS: process shortcuts preserve modifiers/releases and leave local drafts intact")


if __name__ == "__main__":
    main()
