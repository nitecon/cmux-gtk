#!/usr/bin/env python3
"""Closing a terminal above a browser tab must preserve the browser and keep GTK responsive."""
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile

from linux_app import running_app


with tempfile.TemporaryDirectory(prefix="cmux-tab-close-") as directory:
    root = Path(directory)
    browser_dir = root / "browser"
    browser_dir.mkdir()
    (root / "data/cmux").mkdir(parents=True)
    fixtures = Path(__file__).parent / "fixtures"
    shutil.copyfile(fixtures / "session_terminal_over_browser.json", root / "data/cmux/session.json")
    mock_browser = root / "agent-browser"
    shutil.copyfile(fixtures / "mock_agent_browser.py", mock_browser)
    mock_browser.chmod(0o700)
    diagnostic_log = root / "cmux.log"
    browser_id = "20000000-0000-4000-8000-000000000002"
    terminal_id = "30000000-0000-4000-8000-000000000003"

    def recorded(message):
        """Inspect bounded diagnostic records while allowing the writer to create its log asynchronously."""
        if not diagnostic_log.exists():
            return False
        with diagnostic_log.open() as log:
            return message in log.read(1024 * 1024)

    try:
        with running_app(root, {
            "AGENT_BROWSER_SOCKET_DIR": str(browser_dir),
            "CMUX_AGENT_BROWSER": str(mock_browser), "CMUX_LOG": str(diagnostic_log),
        }) as app:
            # A hidden saved browser initializes for an agent command without selecting it.
            before = {surface["uuid"] for surface in app.surfaces() if surface["active"]}
            assert before == {terminal_id}, before

            def browser_ready():
                result = subprocess.run(
                    ["target/debug/cmux", "--socket", str(app.socket_path), "browser", "eval",
                     browser_id, "document.title"],
                    env=app.environment, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                    timeout=2, check=False,
                )
                return result.returncode == 0

            app.wait_for(browser_ready, "browser command readiness")
            assert {surface["uuid"] for surface in app.surfaces() if surface["active"]} == before
            app.wait_for(lambda: recorded(f"browser tab wiring complete uuid={browser_id}"), "browser wiring")
            panes = json.loads(app.cli("list-panes", "--json"))["panes"]
            assert len(panes) == 1, panes
            pane_id = panes[0]["id"]
            assert pane_id in app.cli("list-panes")
            assert panes[0]["surface_ids"] == [browser_id, terminal_id]
            workspace = json.loads(app.cli("current-workspace", "--json"))
            assert workspace["pane_count"] == 1 and workspace["surface_count"] == 2
            assert workspace["id"] == workspace["uuid"]
            assert workspace["title"] == workspace["name"]
            assert "(1 pane)" in app.cli("list-workspaces")
            assert workspace["id"] in app.cli("current-workspace")
            for target in [browser_id, terminal_id] * 3:
                app.cli("focus-surface", target)
                selected = {surface["uuid"] for surface in app.surfaces() if surface["active"]}
                assert selected == {target}, selected
                app.cli("focus-pane", pane_id)
                snapshot = json.loads(app.cli("list-panes", "--json"))["panes"]
                assert snapshot[0]["id"] == pane_id
                assert snapshot[0]["active_surface_uuid"] == target

            original_workspace = app.surfaces()[0]["workspace_uuid"]
            app.cli("focus-surface", browser_id)
            temporary_workspace = json.loads(app.cli("new-workspace", "--json"))["uuid"]
            app.cli("select-workspace", original_workspace)
            assert {surface["uuid"] for surface in app.surfaces() if surface["active"]} == {browser_id}
            app.cli("close-workspace", temporary_workspace)
            app.cli("focus-surface", terminal_id)
            before_invalid = app.surfaces()
            try:
                app.cli("focus-surface", "00000000-0000-4000-8000-000000000000")
            except subprocess.CalledProcessError:
                pass
            else:
                raise AssertionError("unknown surface focus unexpectedly succeeded")
            assert app.surfaces() == before_invalid, "failed focus changed selection"
            app.cli("close-surface", terminal_id)
            app.cli("ping")
            app.wait_for(lambda: recorded(f"surface-tab closed uuid={terminal_id}"), "terminal closure record")
            assert {surface["uuid"] for surface in app.surfaces()} == {browser_id}
            app.wait_for(lambda: not app.children(), "original terminal process exit")
            for _ in range(10):
                app.cli("split", "--direction", "horizontal")
                surfaces = app.surfaces()
                assert len(surfaces) == 2, surfaces
                new_terminal = next(surface["uuid"] for surface in surfaces if surface["active"])
                assert new_terminal != browser_id
                app.wait_for(lambda: len(app.children()) == 1, "new terminal process")
                for target in [browser_id, new_terminal] * 5:
                    app.cli("focus-surface", target)
                    selected = {surface["uuid"] for surface in app.surfaces() if surface["active"]}
                    assert selected == {target}, selected
                app.cli("close-surface", new_terminal)
                app.wait_for(lambda: not app.children(), "split terminal process exit")
                assert {surface["uuid"] for surface in app.surfaces()} == {browser_id}
                app.cli("ping")
            # The last browser cannot be removed under the workspace final-surface policy.
            # Rejection must preserve both the visible tab and its live daemon reference.
            rejected = subprocess.run(
                [str(Path(app.environment.get("CMUX_BIN_DIR", "target/debug")) / "cmux"),
                 "--socket", str(app.socket_path), "browser", "close", "--surface", browser_id],
                env=app.environment, capture_output=True, text=True, timeout=15,
            )
            assert rejected.returncode != 0 and "cannot close the final surface" in rejected.stderr
            browsers = json.loads(app.cli("browser", "list"))["surfaces"]
            assert len(browsers) == 1 and browsers[0]["uuid"] == browser_id
            assert browsers[0]["status"] == "connected", browsers
            assert json.loads(app.cli("browser", "eval", browser_id, "document.title"))["success"] is True
            assert {surface["uuid"] for surface in app.surfaces()} == {browser_id}
            assert not recorded("PANIC")
    finally:
        # The mock is a detached child of its CLI, not of this fixture. Normal cmux
        # shutdown sends close; retain explicit signalling for failed startup paths.
        for pid_file in browser_dir.glob("*.pid"):
            try:
                os.kill(int(pid_file.read_text().strip()), signal.SIGTERM)
            except (ProcessLookupError, FileNotFoundError):
                pass
    print("terminal tab close preserved browser selection and kept the application responsive")
