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
        assert json.loads(app.cli("browser", "eval", browser, "document.title"))["success"] is True

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
        assert json.loads(app.cli("browser", "eval", browser, "document.title"))["success"] is True
        app.wait_for(
            lambda: len(json.loads((root / "data/cmux/session.json").read_text())["workspaces"][0]["layout"].get("start", {})) > 0,
            "moved split session snapshot",
        )

    with running_app(root, environment) as app:
        restored = panes(app)
        assert len(restored) == 3, restored
        assert pane_for(restored, browser)["surface_ids"] == [browser], restored
        assert sum(record["surface_ids"].count(browser) for record in restored) == 1
        assert json.loads(app.cli("browser", "eval", browser, "document.title"))["success"] is True

    print("surface reorder, pane move and drag-to-split preserved identity across restart")
