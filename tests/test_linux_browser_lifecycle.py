#!/usr/bin/env python3
"""Exercise non-focus browser startup and delayed completion against a live GTK application."""
from contextlib import contextmanager
import json
import os
import re
from pathlib import Path
import shlex
import shutil
import signal
import subprocess
import tempfile
import uuid

from linux_app import running_app
from process_support import linux_process_belongs_to, stop_process


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
            with running_app(root, {"CMUX_AGENT_BROWSER": str(mock), "AGENT_BROWSER_SOCKET_DIR": str(browser_dir), "SHELL": "/bin/bash", "CMUX_LOG": str(root / "events.jsonl")}) as app:
                app.wait_for(lambda: bool(app.children()), "initial terminal child")
                terminal_children = app.children()
                first = app.surfaces()[0]
                source, terminal = first["workspace_uuid"], first["uuid"]
                app.cli("focus-surface", terminal)
                opened = subprocess.run(command(app, "--verbose", "browser", "open", "https://example.test/initial"),
                                        env=app.environment, capture_output=True, text=True, check=True, timeout=15)
                result = json.loads(opened.stdout)
                trace = re.search(r"trace_id=([0-9a-f-]+)", opened.stderr).group(1)

                def stream_correlated():
                    """Observe CLI, metadata and WebSocket connection records sharing the caller trace."""
                    try:
                        with (root / "events.jsonl").open() as log:
                            records = [json.loads(line) for line in log.read(1024 * 1024).splitlines()]
                    except (FileNotFoundError, json.JSONDecodeError):
                        return False
                    links = [record["fields"] for record in records
                             if record["event"] == "browser.transport.request"
                             and record["fields"].get("parent_trace_id") == trace]
                    if len(links) < 2:
                        return False
                    completed = {record["fields"]["trace_id"]: record["fields"] for record in records
                                 if record["event"] == "browser.activity.complete"
                                 and record["fields"].get("parent_trace_id") == trace}
                    for link in links:
                        if link["trace_id"] not in completed:
                            return False
                        assert link["request_id"].startswith("cmux-")
                        uuid.UUID(link["request_id"][5:])
                        assert completed[link["trace_id"]]["outcome"] == "success"
                        assert completed[link["trace_id"]]["duration_us"] >= 0
                    records = [record for record in records if record["fields"].get("trace_id") == trace]
                    stages = {record["fields"].get("stage") for record in records
                              if record["event"] == "browser.activity.complete"}
                    connects = [record["fields"] for record in records if record["event"] == "browser.stream.connect"]
                    if not {"rpc_startup", "stream_metadata"} <= stages or not connects:
                        return False
                    assert connects[0]["duration_ms"] >= 0
                    assert connects[0]["outcome"] in {"success", "error", "timeout"}
                    return True

                app.wait_for(stream_correlated, "correlated browser stream attachment")
                assert "surface_ref" in result, result
                assert {item["uuid"] for item in app.surfaces() if item["active"]} == {terminal}
                window = subprocess.check_output(["xdotool", "search", "--sync", "--onlyvisible", "--pid", str(app.process.pid)], text=True, timeout=10).split()[-1]
                marker = root / "keyboard-owner"
                text = "printf '%s' \"$$\" > " + shlex.quote(str(marker))
                subprocess.run(["xdotool", "windowfocus", window], check=True, timeout=3)
                subprocess.run(["xdotool", "type", "--clearmodifiers", "--delay", "1", "--", text], check=True, timeout=5)
                subprocess.run(["xdotool", "key", "--clearmodifiers", "Return"], check=True, timeout=3)
                app.wait_for(lambda: marker.exists() and linux_process_belongs_to(marker.read_text(), terminal_children),
                             "keyboard input in original terminal")

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
