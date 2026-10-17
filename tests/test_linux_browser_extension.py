#!/usr/bin/env python3
"""Verify a shared real browser, extension onboarding, physical typing and persistent login context."""
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
from threading import Thread

from linux_app import running_app


class Handler(SimpleHTTPRequestHandler):
    """Serve only the owned fixture without access-log noise."""

    def log_message(self, format, *args):
        """Suppress ordinary HTTP access output."""


def main():
    """Require page interaction and extension fill to share one persistent browser profile in GTK."""
    binary = shutil.which("agent-browser")
    assert binary, "agent-browser must be installed"
    browsers = sorted((Path.home() / ".agent-browser/browsers").glob("chrome-*/chrome"))
    assert browsers, "agent-browser install must provide Chrome for Testing for unpacked extensions"
    with tempfile.TemporaryDirectory(prefix="cmux-browser-extension-") as directory:
        root = Path(directory)
        (root / "index.html").write_text('<!doctype html><title>Shared login fixture</title>'
                                        '<label>Demo username <input id="name"></label>'
                                        '<script>document.addEventListener("click", () => {window.previewClicked=true})</script>')
        (root / "agent-browser.json").write_text('{}')
        browser_dir = root / "browser"
        browser_dir.mkdir()
        server = ThreadingHTTPServer(("127.0.0.1", 0), partial(Handler, directory=str(root)))
        serving = Thread(target=server.serve_forever, daemon=True)
        serving.start()
        environment = {
            "CMUX_BIN_DIR": "target/release", "CMUX_AGENT_BROWSER": binary,
            "AGENT_BROWSER_SOCKET_DIR": str(browser_dir),
            "AGENT_BROWSER_CONFIG": str(root / "agent-browser.json"),
            "AGENT_BROWSER_EXECUTABLE_PATH": str(browsers[-1]),
            "AGENT_BROWSER_EXTENSIONS": str(Path("tests/fixtures/browser-extension").resolve()),
            "AGENT_BROWSER_HEADED": "false",
        }
        try:
            with running_app(root, environment) as app:
                url = f"http://127.0.0.1:{server.server_port}/"
                profile = str(root / "profile")
                opened = json.loads(app.cli("browser", "open", url, "--profile", profile, timeout=35))
                surface = opened["surface_ref"]
                launch_hash = opened["data"]["lifecycle"]["effectiveLaunch"]["launchHash"]

                def browser(command, *args):
                    """Operate the same live surface, requiring stable launch configuration."""
                    result = json.loads(app.cli("browser", command, surface, *args))
                    assert result["success"] is True, result
                    assert result["data"]["lifecycle"]["effectiveLaunch"]["launchHash"] == launch_hash
                    return result["data"]

                app.cli("focus-surface", opened["uuid"])
                browser("wait", "--selector", "#extension-fill", "--timeout-ms", "10000")
                app.wait_for(lambda: browser("eval", "document.visibilityState")["result"] == "visible",
                             "cmux page foreground after extension onboarding")
                app.wait_for(lambda: json.loads(app.cli("diagnostics", "--json"))["browser_preview"]["textures_assigned"] > 0,
                             "live GTK picture with extension loaded", timeout=15)
                windows = subprocess.check_output(["xdotool", "search", "--onlyvisible", "--pid", str(app.process.pid)],
                                                  env=app.environment, text=True, timeout=5).splitlines()
                subprocess.run(["xdotool", "windowfocus", "--sync", windows[-1]], env=app.environment, check=True, timeout=5)
                geometry = dict(line.split("=", 1) for line in subprocess.check_output(
                    ["xdotool", "getwindowgeometry", "--shell", windows[-1]],
                    env=app.environment, text=True, timeout=5,
                ).splitlines())
                # Surface selection focuses the URL bar. A user's click in the preview
                # transfers GTK keyboard focus to the page before the agent selects a field.
                subprocess.run(["xdotool", "mousemove", "--window", windows[-1],
                                str(int(geometry["WIDTH"]) * 3 // 4),
                                str(int(geometry["HEIGHT"]) * 3 // 4), "click", "1"],
                               env=app.environment, check=True, timeout=5)
                app.wait_for(lambda: browser("eval", "Boolean(window.previewClicked)")["result"],
                             "physical pointer input in the shared page")
                browser("click", "#name")
                subprocess.run(["xdotool", "type", "--clearmodifiers", "--delay", "15", "shared user"],
                               env=app.environment, check=True, timeout=5)
                app.wait_for(lambda: browser("eval", "document.querySelector('#name').value")["result"] == "shared user",
                             "physical user input in the agent's page")
                browser("click", "#extension-fill")
                assert browser("eval", "document.querySelector('#name').value")["result"] == "extension supplied"
                browser("eval", "localStorage.setItem('cmux-profile', 'retained')")
                listed = json.loads(app.cli("browser", "list"))["surfaces"]
                assert len(listed) == 1 and listed[0]["profile"] == profile
                session = listed[0]["session"]
                app.cli("browser", "close", "--surface", surface)
                app.wait_for(lambda: not (browser_dir / f"{session}.pid").exists(), "profile browser shutdown", timeout=15)
                reopened = json.loads(app.cli("browser", "open", url, "--profile", profile, timeout=35))
                surface = reopened["surface_ref"]
                launch_hash = reopened["data"]["lifecycle"]["effectiveLaunch"]["launchHash"]
                assert browser("eval", "localStorage.getItem('cmux-profile')")["result"] == "retained"
        finally:
            server.shutdown()
            serving.join(timeout=5)
            server.server_close()
    print("shared browser preserved physical user input, extension fill, foreground page and persistent profile")


if __name__ == "__main__":
    main()
