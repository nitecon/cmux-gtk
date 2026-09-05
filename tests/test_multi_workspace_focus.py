#!/usr/bin/env python3
"""Verify GTK keyboard routing after workspace switches using per-shell identity markers."""
import json
from pathlib import Path
import shlex
import subprocess
import tempfile

from linux_app import running_app


def selected_surface(app):
    """Require exactly one selected surface and return its UUID across all workspaces."""
    selected = [row["uuid"] for row in app.surfaces() if row["active"]]
    assert len(selected) == 1, selected
    return selected[0]


def seed_shell_identity(app, surface, marker):
    """Set an identity in the target shell and await acknowledgement through the real PTY."""
    command = f"CMUX_FOCUS_PROBE={shlex.quote(surface)}; printf ready > {shlex.quote(str(marker))}"
    app.cli("send-text", command, "--id", surface)
    app.cli("send-key", "\r", "--id", surface)
    app.wait_for(lambda: marker.exists() and marker.read_text() == "ready", "shell identity setup")


def assert_keyboard_destination(app, expected, marker):
    """Type through X11 into the focused GTK window and verify the receiving shell identity.

    This deliberately does not focus a surface or send targeted socket text. The
    marker comes from shell-local state, so a correct UI highlight alone cannot pass.
    """
    assert selected_surface(app) == expected, "workspace switch changed selected surface"
    command = f"printf '%s' \"$CMUX_FOCUS_PROBE\" > {shlex.quote(str(marker))}"
    subprocess.check_call(["xdotool", "type", "--clearmodifiers", "--delay", "1", command], timeout=10)
    subprocess.check_call(["xdotool", "key", "--clearmodifiers", "Return"], timeout=10)
    app.wait_for(lambda: marker.exists() and bool(marker.read_text()), "keyboard input marker")
    assert marker.read_text() == expected, f"keyboard input reached {marker.read_text()} instead of {expected}"
    assert selected_surface(app) == expected, "typing changed selected surface"


def main():
    """Cycle three split workspaces, preserve each selected terminal and reap all owned processes."""
    with tempfile.TemporaryDirectory(prefix="cmux-workspace-focus-") as directory:
        root = Path(directory)
        with running_app(root) as app:
            app.wait_for(lambda: len(app.children()) == 1, "initial shell")
            workspaces = [json.loads(app.cli("current-workspace", "--json"))["uuid"]]
            for _ in range(2):
                workspaces.append(json.loads(app.cli("new-workspace", "--json"))["uuid"])
            app.wait_for(lambda: len(app.children()) == 3, "workspace shells")
            terminals = {}
            for index, workspace in enumerate(workspaces):
                app.cli("select-workspace", workspace)
                app.cli("split", "--direction", "horizontal" if index % 2 == 0 else "vertical")
                app.wait_for(lambda: len(app.children()) == 4 + index, "split shell")
                ids = [row["uuid"] for row in app.surfaces() if row["workspace_uuid"] == workspace]
                assert len(ids) == 2, ids
                terminals[workspace] = ids
                for surface in ids:
                    seed_shell_identity(app, surface, root / f"ready-{surface}")

            windows = subprocess.check_output(
                ["xdotool", "search", "--onlyvisible", "--pid", str(app.process.pid)],
                text=True, timeout=10,
            ).split()
            assert windows, "application has no X11 window"
            subprocess.check_call(["xdotool", "windowfocus", "--sync", windows[-1]], timeout=10)

            for round_number in range(2):
                # Give every workspace a selected pane, then exercise only workspace switches.
                for workspace in workspaces:
                    app.cli("select-workspace", workspace)
                    app.cli("focus-surface", terminals[workspace][round_number])
                for _ in range(5):
                    for workspace in reversed(workspaces):
                        app.cli("select-workspace", workspace)
                for index, workspace in enumerate(workspaces):
                    app.cli("select-workspace", workspace)
                    assert_keyboard_destination(
                        app, terminals[workspace][round_number], root / f"typed-{round_number}-{index}",
                    )
            assert len(app.children()) == 6, "workspace switching changed terminal process ownership"
    print("GTK keyboard input reached the selected terminal after repeated workspace switching")


if __name__ == "__main__":
    main()
