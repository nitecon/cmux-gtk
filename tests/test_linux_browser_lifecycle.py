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
def pending_open(app, browser_dir, *arguments):
    """Hold one real daemon navigation until released, always reaping its CLI on failure."""
    pause = browser_dir / "pause-navigate"
    waiting = browser_dir / "navigate-waiting"
    waiting.unlink(missing_ok=True)
    pause.touch()
    process = None
    try:
        process = subprocess.Popen(command(app, *(arguments or ("browser", "open", "https://example.test/delayed"))),
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

                for address, expected in (("about:blank", "about:blank"),
                                          (str(root / "page #?.html"), (root / "page #?.html").as_uri())):
                    app.cli("browser", "open", address)
                    assert json.loads((browser_dir / "last-navigation.json").read_text())["url"] == expected
                    assert {item["uuid"] for item in app.surfaces() if item["active"]} == {terminal}

                with pending_open(app, browser_dir) as pending:
                    subprocess.run(command(app, "ping"), env=app.environment, check=True, capture_output=True, timeout=2)
                    target = json.loads(app.cli("new-workspace", "--json"))["uuid"]
                    before = app.surfaces()
                    finish_open(pending, browser_dir, True)
                    assert [row for row in app.surfaces() if row["workspace_uuid"] == target] == [row for row in before if row["workspace_uuid"] == target], "browser completion changed the new workspace"
                    assert json.loads(app.cli("current-workspace", "--json"))["uuid"] == target

                for invalid in (str(uuid.uuid4()), 17):
                    before = app.surfaces()
                    try:
                        app.cli("raw", "browser.open", "--params", json.dumps({"url": "about:blank", "workspace": invalid}))
                    except subprocess.CalledProcessError:
                        pass
                    else:
                        raise AssertionError("invalid explicit browser workspace was accepted")
                    assert app.surfaces() == before
                # Target a third workspace while the observer stays selected.
                third = json.loads(app.cli("new-workspace", "--name", "explicit browser target", "--json"))["uuid"]
                app.cli("select-workspace", target)
                active_before = {item["uuid"] for item in app.surfaces() if item["active"]}
                previous_daemons = set(browser_dir.glob("*.pid"))
                app.cli("browser", "open", "about:blank", "--workspace", third)
                third_daemons = set(browser_dir.glob("*.pid")) - previous_daemons
                assert len(third_daemons) == 1, third_daemons
                third_surfaces = [item for item in app.surfaces() if item["workspace_uuid"] == third]
                assert len(third_surfaces) == 2, third_surfaces
                assert {item["uuid"] for item in app.surfaces() if item["active"]} == active_before
                app.cli("close-workspace", third)
                app.wait_for(lambda: all(not pid.exists() for pid in third_daemons), "closed workspace browser daemon exit")
                assert all(pid.exists() for pid in previous_daemons), "closing one workspace retired another browser"
                (root / "cmux.json").write_text(json.dumps({"actions": {"fixture.browser": {"builtin": "cmux.newBrowser"}}}))
                project = json.loads(app.cli("new-workspace", "--cwd", str(root), "--json"))["uuid"]
                reviewed = json.loads(app.cli("project-actions", "--workspace", project, "--json"))
                source_terminal = next(row["uuid"] for row in app.surfaces() if row["active"])
                before_daemons = set(browser_dir.glob("*.pid"))
                with pending_open(app, browser_dir, "project-run", "fixture.browser", "--workspace", project,
                        "--fingerprint", reviewed["config"]["actions"]["fixture.browser"]["fingerprint"], "--json") as pending:
                    app.cli("split", "--direction", "horizontal")
                    changed_terminal = next(row["uuid"] for row in app.surfaces() if row["active"])
                    app.cli("select-workspace", target)
                    before_surfaces = app.surfaces()
                    error = finish_open(pending, browser_dir, False)
                    assert "changed" in error.lower(), error
                    assert app.surfaces() == before_surfaces, "stale project browser changed layout or focus"
                app.wait_for(lambda: set(browser_dir.glob("*.pid")) == before_daemons,
                             "stale project browser daemon retirement")
                app.cli("close-surface", changed_terminal)
                app.cli("select-workspace", project)
                app.cli("focus-surface", source_terminal)
                app.cli("select-workspace", target)
                before_daemons = set(browser_dir.glob("*.pid"))
                created = json.loads(app.cli("project-run", "fixture.browser", "--workspace", project,
                    "--fingerprint", reviewed["config"]["actions"]["fixture.browser"]["fingerprint"], "--json", timeout=35))
                assert created["status"] == "submitted" and created["workspace_id"] == project
                assert {row["uuid"] for row in app.surfaces() if row["active"]} == {created["surface_id"]}
                assert json.loads(app.cli("current-workspace", "--json"))["uuid"] == project
                owned = set(browser_dir.glob("*.pid")) - before_daemons
                assert len(owned) == 1
                app.cli("close-workspace", project)
                app.wait_for(lambda: all(not pid.exists() for pid in owned), "project browser daemon exit")
                assert all(pid.exists() for pid in before_daemons)
                app.cli("select-workspace", target)
                app.cli("select-workspace", source)
                with pending_open(app, browser_dir) as pending:
                    app.cli("select-workspace", target)
                    app.cli("close-workspace", source)
                    before = app.surfaces()
                    error = finish_open(pending, browser_dir, False)
                    assert "closed during browser startup" in error, error
                    assert app.surfaces() == before, "stale completion mutated another workspace"
                app.wait_for(lambda: not list(browser_dir.glob("*.pid")), "closed workspace and cancelled startup daemon exit")
                app.cli("ping")
        finally:
            for pid_file in browser_dir.glob("*.pid"):
                try:
                    os.kill(int(pid_file.read_text().strip()), signal.SIGTERM)
                except (ProcessLookupError, FileNotFoundError):
                    pass
    print("browser startup preserved focus, GTK responsiveness and workspace ownership")


if __name__ == "__main__":
    main()
