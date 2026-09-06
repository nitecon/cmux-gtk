#!/usr/bin/env python3
"""Verify immediate GTK quit saves current workspaces before native surface teardown."""
import json
from pathlib import Path
import subprocess
import tempfile

from linux_app import running_app
from process_support import stop_process


def main():
    """Mutate and quit by keyboard/window manager, then reopen without visiting background workspaces."""
    with tempfile.TemporaryDirectory(prefix="cmux-session-quit-") as directory:
        root = Path(directory)
        wm = subprocess.Popen(["openbox"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        expected = None
        binding_surface = None
        saved_binding = None
        try:
            for cycle in range(3):
                with running_app(root) as app:
                    app.cli("ping")
                    if expected is not None:
                        restored = json.loads(app.cli("list-workspaces", "--json"))["workspaces"]
                        assert [(row["uuid"], row["name"]) for row in restored] == expected
                        assert json.loads(app.cli("current-workspace", "--json"))["uuid"] == expected[1][0]
                        binding = json.loads(app.cli("surface", "resume", "show", "--surface", binding_surface, "--json"))
                        assert binding["resume_binding"] == saved_binding
                        assert not (root / "must-not-execute").exists(), "registration executed a stored command"
                    else:
                        app.cli("new-workspace", "--name", "middle")
                        app.cli("new-workspace", "--name", "background")
                    rows = json.loads(app.cli("list-workspaces", "--json"))["workspaces"]
                    app.cli("select-workspace", rows[1]["uuid"])
                    windows = subprocess.check_output(
                        ["xdotool", "search", "--onlyvisible", "--pid", str(app.process.pid)],
                        text=True, timeout=10,
                    ).split()
                    assert windows
                    subprocess.check_call(["xdotool", "windowfocus", "--sync", windows[-1]], timeout=10)
                    binding_surface = next(row["uuid"] for row in app.surfaces()
                                           if row["workspace_uuid"] == rows[2]["uuid"])
                    registered = json.loads(app.cli(
                        "surface", "resume", "set", "--surface", binding_surface,
                        "--shell", "touch " + str(root / "must-not-execute"),
                        "--kind", "custom", "--checkpoint", f"checkpoint-{cycle}", "--json",
                    ))
                    saved_binding = registered["resume_binding"]
                    try:
                        app.cli("surface", "resume", "clear", "--surface", binding_surface,
                                "--checkpoint", "stale-checkpoint")
                    except subprocess.CalledProcessError:
                        pass
                    else:
                        raise AssertionError("stale checkpoint cleared a newer binding")
                    unchanged = json.loads(app.cli("surface", "resume", "show", "--surface", binding_surface, "--json"))
                    assert unchanged["resume_binding"] == saved_binding
                    assert json.loads(app.cli("current-workspace", "--json"))["uuid"] == rows[1]["uuid"]
                    name = f"last-mutation-{cycle}"
                    app.cli("rename-workspace", rows[2]["uuid"], name)
                    # No debounce sleep: quit immediately after the mutation is acknowledged.
                    if cycle % 2:
                        subprocess.check_call(["wmctrl", "-ic", hex(int(windows[-1]))], timeout=10)
                    else:
                        subprocess.check_call(
                            ["xdotool", "key", "--clearmodifiers", "ctrl+q"], timeout=10,
                        )
                    assert app.process.wait(timeout=15) == 0
                    saved = json.loads((root / "data/cmux/session.json").read_text())
                    expected = [(row["uuid"], row["name"]) for row in rows]
                    expected[2] = (rows[2]["uuid"], name)
                    assert [(row["uuid"], row["name"]) for row in saved["workspaces"]] == expected
                    assert saved["active_index"] == 1
                    # The owned process has exited; remove its stale discovery endpoint
                    # so the next launch cannot mistake it for listener readiness.
                    app.socket_path.unlink(missing_ok=True)
            with running_app(root) as app:
                app.cli("ping")
                cleared = json.loads(app.cli("surface", "resume", "clear", "--surface", binding_surface,
                                             "--checkpoint", saved_binding["checkpoint_id"], "--json"))
                assert cleared["resume_binding"] is None
        finally:
            stop_process(wm)
    print("immediate quit persisted final state through repeated background-workspace restores")


if __name__ == "__main__":
    main()
