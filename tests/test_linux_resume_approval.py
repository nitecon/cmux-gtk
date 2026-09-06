#!/usr/bin/env python3
"""Approve through GTK, restart an actual PTY, then revoke and prove the next launch stays inert."""
import json
from pathlib import Path
import shlex
import subprocess
import tempfile

from linux_app import running_app
from process_support import stop_process


def window(app, name=None):
    """Find a visible window owned by this fixture, optionally matching its exact title."""
    arguments = ["xdotool", "search", "--all", "--onlyvisible", "--pid", str(app.process.pid)]
    if name:
        arguments.extend(["--name", "^" + name + "$"])
    result = subprocess.run(arguments, capture_output=True, text=True, timeout=5)
    return result.stdout.split()[-1] if result.returncode == 0 and result.stdout.split() else None


def key(target, chord):
    """Deliver a keyboard action through the window manager, without socket approval shortcuts."""
    subprocess.check_call(["xdotool", "windowfocus", "--sync", target, "key", "--clearmodifiers", chord], timeout=10)


def review(app, chord):
    """Open the production preferences panel and activate its approval or revocation mnemonic."""
    key(window(app), "ctrl+comma")
    app.wait_for(lambda: window(app, "Preferences"), "resume review panel")
    dialog = window(app, "Preferences")
    key(dialog, chord)
    return dialog


def quit_app(app):
    """Quit normally and retire only this exited process's socket before the next restart."""
    key(window(app), "ctrl+q")
    assert app.process.wait(timeout=15) == 0
    app.socket_path.unlink(missing_ok=True)


def main():
    """Exercise UI authority, literal launch context, changed-binding rejection and durable revocation."""
    with tempfile.TemporaryDirectory(prefix="cmux-approval-") as directory:
        root = Path(directory)
        output = root / "automatic-result"
        command = "printf '%s\\n' \"$PWD\" \"$CMUX_SURFACE_ID\" \"$PROJECT\" > " + shlex.quote(str(output))
        wm = subprocess.Popen(["openbox"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            with running_app(root) as app:
                app.wait_for(lambda: bool(app.surfaces()), "terminal")
                surface = next(row["uuid"] for row in app.surfaces() if row["active"])
                params = {"surface_id": surface, "command": command, "cwd": str(root),
                          "environment": {"PROJECT": "literal $HOME and 'quotes'"}}
                app.cli("raw", "surface.resume.set", "--params", json.dumps(params))
                def approved():
                    """Read the backend's decision for the exact current binding."""
                    return json.loads(app.cli("surface", "resume", "show", "--surface", surface, "--json"))["auto_resume"]
                assert not approved() and not output.exists()
                dialog = review(app, "alt+a")
                app.wait_for(approved, "UI approval")
                assert not output.exists(), "approving must not execute in a live terminal"
                key(dialog, "Escape")
                app.wait_for(lambda: window(app, "Preferences") is None, "review close")
                quit_app(app)
            with running_app(root) as app:
                app.wait_for(output.exists, "automatic command after restart")
                app.wait_for(lambda: output.read_text().splitlines() == [str(root), surface, params["environment"]["PROJECT"]],
                             "approved command context")
                assert approved()
                for change in ({"cwd": "/tmp"}, {"environment": {"PROJECT": "different"}}, {"command": "true"}):
                    app.cli("raw", "surface.resume.set", "--params", json.dumps(dict(params, **change)))
                    assert not approved(), "a changed launch retained automatic authority"
                app.cli("raw", "surface.resume.set", "--params", json.dumps(params))
                assert approved()
                dialog = review(app, "alt+r")
                app.wait_for(lambda: not approved(), "approval revocation")
                key(dialog, "Escape")
                app.wait_for(lambda: window(app, "Preferences") is None, "review close")
                quit_app(app)
            output.unlink()
            with running_app(root) as app:
                app.wait_for(lambda: bool(json.loads(app.cli("read-text", "--id", surface, "--json"))["text"].strip()),
                             "unapproved terminal shell")
                assert not approved() and not output.exists()
        finally:
            stop_process(wm)
    print("GTK approvals survived restart, matched exact launch inputs, and revoked durably")


if __name__ == "__main__":
    main()
