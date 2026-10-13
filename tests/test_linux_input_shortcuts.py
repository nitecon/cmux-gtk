#!/usr/bin/env python3
"""Actions-only native terminal keys: observe exact bytes at the foreground process."""
import json
from pathlib import Path
import shlex
import subprocess
import tempfile

from linux_app import running_app


def send_shortcut(shortcut):
    """Keep modifiers held until the main key is released, without xdotool key's chord cleanup order."""
    keys = shortcut.split("+")
    arguments = ["xdotool"]
    for key in keys:
        arguments.extend(("keydown", key))
    for key in reversed(keys):
        arguments.extend(("keyup", key))
    subprocess.run(arguments, check=True, timeout=10)


def main():
    """Require plain arrows and literal text to reach the native process without a separate input editor."""
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
                """Inspect native process output without repairing focus."""
                return rpc("surface.read_text", id=surface)

            app.wait_for(lambda: bool(screen()["text"].strip()), "initial shell")
            rpc("surface.send_text", id=surface, text=shlex.join([str(executable), str(log)]))
            rpc("surface.send_key", id=surface, key="\r")
            app.wait_for(lambda: "CMUX key recorder ready" in screen()["text"], "foreground recorder")
            assert screen()["input"] is None
            windows = subprocess.check_output([
                "xdotool", "search", "--onlyvisible", "--pid", str(app.process.pid),
            ], text=True, timeout=10).split()
            assert windows
            subprocess.run(["xdotool", "windowfocus", "--sync", windows[-1]], check=True, timeout=10)

            # Plain arrows go directly to the application; no local editor can consume them.
            for key, final in (("Left", b"D"), ("Right", b"C"), ("Up", b"A"), ("Down", b"B")):
                start = log.stat().st_size
                send_shortcut(key)
                presses = (b"\x1b[" + final, b"\x1b[1;1:1" + final, b"\x1b[1:1" + final)
                releases = (b"\x1b[1;1:3" + final, b"\x1b[1:3" + final)
                def delivered():
                    data = log.read_bytes()[start:]
                    return any(press in data for press in presses) and any(release in data for release in releases)
                try:
                    app.wait_for(delivered, "native " + key + " press/release")
                except AssertionError:
                    print("Received key bytes:", repr(log.read_bytes()[start:]))
                    raise
            rpc("surface.send_text", id=surface, text="DIRECT_NATIVE_DRAFT")
            app.wait_for(lambda: b"DIRECT_NATIVE_DRAFT" in log.read_bytes(), "literal input goes directly to the process without Enter")
            assert screen()["input"] is None
            print("PASS: native arrows, key releases and direct process-owned input")


if __name__ == "__main__":
    main()
