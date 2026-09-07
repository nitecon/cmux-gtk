#!/usr/bin/env python3
"""Surface reorder, pane transfer and drag-to-split preserve live identity and restart topology."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

from linux_app import running_app


def panes(app):
    return json.loads(app.cli("list-panes", "--json"))["panes"]


def pane_for(records, surface):
    return next(record for record in records if surface in record["surface_ids"])


def wait_for_browser(app, surface):
    """Wait for a restored or rerouted browser daemon to accept commands."""
    binary = Path(app.environment.get("CMUX_BIN_DIR", "target/debug")) / "cmux"

    def ready():
        try:
            result = subprocess.run(
                [str(binary), "--socket", str(app.socket_path), "browser", "eval",
                 surface, "document.title"],
                env=app.environment, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                timeout=1, check=False,
            )
        except subprocess.TimeoutExpired:
            return False
        return result.returncode == 0

    app.wait_for(ready, f"browser {surface} command readiness")


def saved_workspace_for(root, surface):
    """Return the durable workspace containing a surface, or None during a pending save."""
    try:
        session = json.loads((root / "data/cmux/session.json").read_text())
    except (FileNotFoundError, json.JSONDecodeError):
        return None

    def contains(node):
        if "surfaces" in node:
            return any(row.get("surface_uuid") == surface for row in node["surfaces"])
        return contains(node.get("start", {})) or contains(node.get("end", {}))

    return next((row["uuid"] for row in session["workspaces"] if contains(row["layout"])), None)


with tempfile.TemporaryDirectory(prefix="cmux-surface-move-") as directory:
    root = Path(directory)
    browser_dir = root / "browser"
    browser_dir.mkdir()
    (root / "data/cmux").mkdir(parents=True)
    fixtures = Path(__file__).parent / "fixtures"
    shutil.copyfile(fixtures / "session_terminal_over_browser.json", root / "data/cmux/session.json")
    mock_browser = root / "agent-browser"
    shutil.copyfile(fixtures / "mock_agent_browser.py", mock_browser)
    mock_browser.chmod(0o700)
    environment = {
        "AGENT_BROWSER_SOCKET_DIR": str(browser_dir),
        "CMUX_AGENT_BROWSER": str(mock_browser),
    }
    browser = "20000000-0000-4000-8000-000000000002"
    terminal = "30000000-0000-4000-8000-000000000003"

    with running_app(root, environment) as app:
        original = panes(app)
        assert len(original) == 1 and original[0]["surface_ids"] == [browser, terminal], original
        original_pane = original[0]["id"]

        app.cli("reorder-surface", terminal, "0")
        reordered = panes(app)
        assert reordered[0]["surface_ids"] == [terminal, browser], reordered

        app.cli("focus-surface", terminal)
        app.cli("split", "--direction", "horizontal")
        after_split = panes(app)
        assert len(after_split) == 2, after_split
        destination = next(record for record in after_split if record["id"] != original_pane)
        destination_pane = destination["id"]

        before_invalid = panes(app)
        invalid = subprocess.run(
            [str(Path(app.environment.get("CMUX_BIN_DIR", "target/debug")) / "cmux"),
             "--socket", str(app.socket_path), "move-surface", browser,
             "--pane", "pane:999999999"],
            env=app.environment, capture_output=True, text=True, timeout=15,
        )
        assert invalid.returncode != 0 and panes(app) == before_invalid

        app.cli("move-surface", browser, "--pane", destination_pane, "--position", "0")
        moved = panes(app)
        assert pane_for(moved, browser)["id"] == destination_pane, moved
        assert pane_for(moved, browser)["surface_ids"][0] == browser, moved
        wait_for_browser(app, browser)

        app.cli(
            "drag-surface-to-split", browser,
            "--pane", original_pane,
            "--direction", "down",
        )
        split_moved = panes(app)
        assert len(split_moved) == 3, split_moved
        browser_pane = pane_for(split_moved, browser)
        assert browser_pane["surface_ids"] == [browser], split_moved
        assert browser_pane["focused"] is True, split_moved
        assert sum(record["surface_ids"].count(browser) for record in split_moved) == 1
        wait_for_browser(app, browser)
        app.wait_for(
            lambda: len(json.loads((root / "data/cmux/session.json").read_text())["workspaces"][0]["layout"].get("start", {})) > 0,
            "moved split session snapshot",
        )

    with running_app(root, environment) as app:
        restored = panes(app)
        assert len(restored) == 3, restored
        assert pane_for(restored, browser)["surface_ids"] == [browser], restored
        assert sum(record["surface_ids"].count(browser) for record in restored) == 1
        wait_for_browser(app, browser)

        original_workspace = next(
            row["workspace_uuid"] for row in app.surfaces() if row["uuid"] == browser
        )
        destination_workspace = json.loads(app.cli("new-workspace", "--json"))["uuid"]
        destination_pane = next(
            record["id"] for record in panes(app)
            if record["workspace_uuid"] == destination_workspace
        )
        app.cli("select-workspace", original_workspace)
        app.cli(
            "notify", "--workspace", original_workspace,
            "--surface", browser, "--body", "moves with browser",
        )
        move_result = json.loads(app.cli(
            "move-surface", browser,
            "--workspace", destination_workspace,
            "--no-focus", "--json",
        ))
        assert move_result["workspace_id"] == destination_workspace
        assert move_result["pane"] == destination_pane
        assert move_result["browser_route_restarted"] is False
        assert json.loads(app.cli("current-workspace", "--json"))["uuid"] == original_workspace
        assert pane_for(panes(app), browser)["workspace_uuid"] == destination_workspace
        notifications = json.loads(app.cli("notifications", "list", "--json"))["notifications"]
        moved_notice = next(row for row in notifications if row["surface_id"] == browser)
        assert moved_notice["workspace_id"] == destination_workspace, moved_notice
        wait_for_browser(app, browser)

        # Moving the only surface out of a local workspace removes that empty workspace
        # without destroying the transferred PTY.
        solo_workspace = json.loads(app.cli("new-workspace", "--json"))["uuid"]
        solo_surface = next(
            row["uuid"] for row in app.surfaces()
            if row["workspace_uuid"] == solo_workspace
        )
        app.cli("select-workspace", destination_workspace)
        app.cli("move-surface", solo_surface, "--workspace", destination_workspace, "--no-focus")
        workspaces = json.loads(app.cli("list-workspaces", "--json"))["workspaces"]
        assert solo_workspace not in {row["uuid"] for row in workspaces}, workspaces
        assert pane_for(panes(app), solo_surface)["workspace_uuid"] == destination_workspace
        app.cli("send-text", "printf surface-transfer-alive", "--id", solo_surface)
        app.wait_for(
            lambda: saved_workspace_for(root, browser) == destination_workspace
            and saved_workspace_for(root, solo_surface) == destination_workspace,
            "cross-workspace surface session snapshot",
        )

    with running_app(root, environment) as app:
        workspaces = json.loads(app.cli("list-workspaces", "--json"))["workspaces"]
        assert solo_workspace not in {row["uuid"] for row in workspaces}, workspaces
        restored = panes(app)
        assert pane_for(restored, browser)["workspace_uuid"] == destination_workspace
        assert pane_for(restored, solo_surface)["workspace_uuid"] == destination_workspace
        wait_for_browser(app, browser)

    print("surface reorder, pane move and drag-to-split preserved identity across restart")
