#!/usr/bin/env python3
"""Exercise non-focus browser startup and delayed completion against a live GTK application."""
from contextlib import contextmanager
import json
import os
from pathlib import Path
import shlex
import shutil
import signal
import subprocess
import tempfile

from linux_app import running_app
from process_support import stop_process


def command(app, *arguments):
    """Build the isolated production CLI invocation used by synchronous and pending requests."""
    binary_dir = Path(app.environment.get("CMUX_BIN_DIR", "target/debug"))
    return [str(binary_dir / "cmux"), "--socket", str(app.socket_path), *arguments]


@contextmanager
def pending_open(app, browser_dir):
    """Hold one real daemon navigation until released, always reaping its CLI on failure."""
    pause = browser_dir / "pause-navigate"
    waiting = browser_dir / "navigate-waiting"
    waiting.unlink(missing_ok=True)
    pause.touch()
    process = None
    try:
        process = subprocess.Popen(command(app, "browser", "open", "https://example.test/delayed"),
                                   env=app.environment, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        app.wait_for(waiting.exists, "daemon navigation pause", timeout=5)
        yield process
    finally:
        pause.unlink(missing_ok=True)
        if process is not None:
            stop_process(process)
            process.stdout.close()
            process.stderr.close()


def finish_open(process, browser_dir, success):
    """Release the daemon and check the actual CLI result with a bounded wait."""
    (browser_dir / "pause-navigate").unlink(missing_ok=True)
    output, error = process.communicate(timeout=10)
    assert (process.returncode == 0) == success, (output, error)
    return json.loads(output) if success else error


def main():
    """Verify GTK responsiveness, physical keyboard focus, workspace targeting and stale-result rejection."""
    with tempfile.TemporaryDirectory(prefix="cmux-browser-lifecycle-") as directory:
        root = Path(directory)
        browser_dir = root / "browser"
        browser_dir.mkdir()
        mock = root / "agent-browser"
        shutil.copyfile(Path(__file__).parent / "fixtures/mock_agent_browser.py", mock)
        mock.chmod(0o700)
        try:
            with running_app(root, {"CMUX_AGENT_BROWSER": str(mock), "AGENT_BROWSER_SOCKET_DIR": str(browser_dir), "SHELL": "/bin/bash"}) as app:
                app.wait_for(lambda: bool(app.children()), "initial terminal child")
                first = app.surfaces()[0]
                source, terminal = first["workspace_uuid"], first["uuid"]
                app.cli("focus-surface", terminal)
                result = json.loads(app.cli("browser", "open", "https://example.test/initial"))
                assert "surface_ref" in result, result
                assert {item["uuid"] for item in app.surfaces() if item["active"]} == {terminal}
                window = subprocess.check_output(["xdotool", "search", "--sync", "--onlyvisible", "--pid", str(app.process.pid)], text=True, timeout=10).split()[-1]
                marker = root / "keyboard-owner"
                text = "printf '%s' \"$CMUX_SURFACE_ID\" > " + shlex.quote(str(marker))
                subprocess.run(["xdotool", "windowfocus", window], check=True, timeout=3)
                subprocess.run(["xdotool", "type", "--clearmodifiers", "--delay", "1", "--", text], check=True, timeout=5)
                subprocess.run(["xdotool", "key", "--clearmodifiers", "Return"], check=True, timeout=3)
                app.wait_for(lambda: marker.exists() and marker.read_text() == terminal, "keyboard input in original terminal")
                assert marker.read_text() == terminal

                with pending_open(app, browser_dir) as pending:
                    subprocess.run(command(app, "ping"), env=app.environment, check=True, capture_output=True, timeout=2)
                    target = json.loads(app.cli("new-workspace", "--json"))["uuid"]
                    before = app.surfaces()
                    finish_open(pending, browser_dir, True)
                    assert app.surfaces() == before, "browser completion changed the new workspace"
                    assert json.loads(app.cli("current-workspace", "--json"))["uuid"] == target

                app.cli("select-workspace", source)
                with pending_open(app, browser_dir) as pending:
                    app.cli("select-workspace", target)
                    app.cli("close-workspace", source)
                    before = app.surfaces()
                    error = finish_open(pending, browser_dir, False)
                    assert "closed during browser startup" in error, error
                    assert app.surfaces() == before, "stale completion mutated another workspace"
                app.cli("ping")
        finally:
            pid_file = browser_dir / "mock.pid"
            if pid_file.exists():
                try:
                    os.kill(int(pid_file.read_text().strip()), signal.SIGTERM)
                except (ProcessLookupError, FileNotFoundError):
                    pass
    print("browser startup preserved focus, GTK responsiveness and workspace ownership")


if __name__ == "__main__":
    main()
