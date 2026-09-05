#!/usr/bin/env python3
"""Closing a terminal above a browser tab must defer reentrant mapping and preserve the browser."""
import os
from pathlib import Path
import shutil
import signal
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
            app.wait_for(lambda: recorded(f"browser tab wiring complete uuid={browser_id}"), "browser wiring")
            app.cli("close-surface", terminal_id)
            app.cli("ping")
            app.wait_for(lambda: recorded("browser map deferred while application state is busy"), "deferred browser mapping")
            app.wait_for(lambda: recorded(f"surface-tab closed uuid={terminal_id}"), "terminal closure record")
            assert {surface["uuid"] for surface in app.surfaces()} == {browser_id}
            assert not recorded("PANIC")
    finally:
        # The mock is a detached child of its CLI, not of this fixture. Normal cmux
        # shutdown sends close; retain explicit signalling for failed startup paths.
        pid_file = browser_dir / "mock.pid"
        if pid_file.exists():
            try:
                os.kill(int(pid_file.read_text().strip()), signal.SIGTERM)
            except (ProcessLookupError, FileNotFoundError):
                pass
    print("terminal tab close deferred browser mapping and preserved the browser surface")
