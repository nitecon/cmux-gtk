#!/usr/bin/env python3
"""Measure cmux browser operations against pinned agent-browser and real Chromium in CI."""

from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import json
import os
import re
from pathlib import Path
import shutil
import subprocess
import tempfile
from threading import Thread
import time

from benchmark_support import artifact, summarize_us, resource_delta
from linux_app import running_app
from browser_process_support import BrowserProcesses
from test_multi_workspace_focus import selected_surface


PAGE = '''<!doctype html><title>cmux browser fixture</title>
<h1>Browser benchmark</h1><label>Name <input id="name"></label>
<button id="increment" onclick="document.querySelector('output').textContent='Count '+(++window.count)">Increment</button>
<output>Count 0</output><script>window.count=0</script>'''


class Handler(SimpleHTTPRequestHandler):
    """Serve only the isolated fixture directory without retaining per-request console logs."""

    def log_message(self, format, *args):
        """Omit normal HTTP access messages from benchmark output."""


def measure(root, report):
    """Exercise actual navigation, DOM fill/click/evaluation, snapshots and streamed GTK textures."""
    (root / "index.html").write_text(PAGE)
    browser_dir = root / "browser"
    browser_dir.mkdir()
    browser_processes = BrowserProcesses(browser_dir)
    binary = shutil.which("agent-browser")
    if binary is None:
        raise RuntimeError("agent-browser is required for the real browser benchmark")
    report["agent_browser_version"] = subprocess.check_output([binary, "--version"], text=True, timeout=10).strip()
    server = ThreadingHTTPServer(("127.0.0.1", 0), partial(Handler, directory=str(root)))
    serving = Thread(target=server.serve_forever, daemon=True)
    serving.start()
    environment = {"CMUX_BIN_DIR": "target/release", "CMUX_AGENT_BROWSER": binary,
                   "AGENT_BROWSER_SOCKET_DIR": str(browser_dir), "CMUX_LOG": str(root / "events.jsonl")}
    try:
        with running_app(root, environment) as app:
            app.wait_for(lambda: bool(app.children()), "initial terminal")
            terminal = selected_surface(app)
            tick = time.perf_counter_ns()
            opened = json.loads(app.cli("browser", "open", f"http://127.0.0.1:{server.server_port}/", timeout=35))
            assert opened["success"] is True
            surface = opened["surface_ref"]
            report["open_us"] = (time.perf_counter_ns() - tick) / 1000

            def browser(*args):
                """Require daemon success and return its data through the real cmux CLI."""
                result = json.loads(app.cli("browser", args[0], surface, *args[1:]))
                assert result["success"] is True, "external browser command failed"
                return result["data"]

            def resources():
                """Read the owned optimized application's resource and preview counters."""
                result = json.loads(app.cli("diagnostics", "--json"))
                assert result["pid"] == app.process.pid and result["build_profile"] == "release"
                assert result["first_opengl_context"] is not None
                return result

            app.wait_for(lambda: resources()["browser_preview"]["textures_assigned"] > 0,
                         "real browser frame assigned to GTK", timeout=15)
            report["browser_user_agent"] = browser("eval", "navigator.userAgent")["result"]
            assert isinstance(report["browser_user_agent"], str) and len(report["browser_user_agent"]) <= 512
            assert browser("wait", "--url-contains", "127.0.0.1", "--timeout-ms", "1000")["waited"] == "function"
            browser("eval", "window.cmuxWaitReady=false; setTimeout(()=>{window.cmuxWaitReady=true},6000); true")
            waiting = subprocess.Popen(["target/release/cmux", "--socket", str(app.socket_path),
                                        "browser", "wait", surface, "--function", "window.cmuxWaitReady === true",
                                        "--timeout-ms", "12000"], env=app.environment,
                                       stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            try:
                assert json.loads(app.cli("ping", "--json"))["pong"]
                output, error = waiting.communicate(timeout=25)
                assert waiting.returncode == 0, error
                assert json.loads(output)["success"] is True
            finally:
                if waiting.poll() is None:
                    waiting.kill()
                    waiting.wait(timeout=5)
                waiting.stdout.close()
                waiting.stderr.close()
            for iteration in range(15):
                if iteration == 5:
                    report["before"] = resources()
                    report["browser_processes_before"] = browser_processes.sample()
                    started = time.monotonic()
                sample = {}
                value = f"fixture-{iteration}"
                for operation, args in (
                    ("fill", ("fill", "#name", value)),
                    ("click", ("click", "#increment")),
                    ("snapshot", ("snapshot", "--max-depth", "8")),
                    ("evaluate", ("eval", "({value: document.querySelector('#name').value, count: window.count})")),
                ):
                    tick = time.perf_counter_ns()
                    result = browser(*args)
                    sample[operation + "_us"] = (time.perf_counter_ns() - tick) / 1000
                    if operation == "snapshot":
                        assert f"Count {iteration + 1}" in result["snapshot"]
                    elif operation == "evaluate":
                        assert result["result"] == {"value": value, "count": iteration + 1}
                assert selected_surface(app) == terminal, "browser automation stole terminal focus"
                if iteration >= 5:
                    report["samples"].append(sample)
                    report.setdefault("browser_process_samples", []).append(browser_processes.sample())
            report["after"] = resources()
            report["browser_processes_after"] = browser_processes.sample()
            assert report["browser_processes_after"]["daemon_count"] == 1
            assert report["browser_processes_after"]["process_count"] > 1
            report["browser_pss_growth_bytes"] = (report["browser_processes_after"]["pss_bytes"]
                                                  - report["browser_processes_before"]["pss_bytes"])
            report["elapsed_seconds"] = time.monotonic() - started
            report["resource_delta"] = resource_delta(report["before"], report["after"], report["elapsed_seconds"])
            report["latency_us"] = {key: summarize_us([row[key] for row in report["samples"]])
                                    for key in report["samples"][0]}
            # A daemon-side evaluation error must reach the CLI as failure, not a successful outer envelope.
            failed = subprocess.run(["target/release/cmux", "--socket", str(app.socket_path), "--verbose", "browser", "eval",
                                     surface, "throw new Error('fixture failure')"], env=app.environment,
                                    capture_output=True, text=True, timeout=15)
            assert failed.returncode != 0
            failure_trace = re.search(r"trace_id=([0-9a-f-]+)", failed.stderr).group(1)

            def failure_correlated():
                """Require the failed external exchange to retain its originating CLI trace."""
                try:
                    with (root / "events.jsonl").open() as log:
                        records = [json.loads(line) for line in log.read(1024 * 1024).splitlines()]
                except (FileNotFoundError, json.JSONDecodeError):
                    return False
                return any(record["event"] == "browser.activity.complete"
                           and record["fields"].get("parent_trace_id") == failure_trace
                           and record["fields"]["stage"] == "daemon_exchange"
                           and record["fields"]["outcome"] == "error" for record in records)

            app.wait_for(failure_correlated, "failed browser exchange correlation")
            assert selected_surface(app) == terminal
            second_open = json.loads(app.cli("browser", "open", f"http://127.0.0.1:{server.server_port}/", timeout=35))
            assert second_open["success"] is True and second_open["uuid"] != opened["uuid"]
            second = second_open["surface_ref"]
            report["two_browser_processes"] = browser_processes.sample()
            assert report["two_browser_processes"]["daemon_count"] == 2

            def other_browser(*args):
                """Route commands to the second real browser surface without changing selection."""
                result = json.loads(app.cli("browser", args[0], second, *args[1:]))
                assert result["success"] is True
                return result["data"]

            assert other_browser("eval", "window.count")["result"] == 0
            other_browser("fill", "#name", "second page")
            other_browser("click", "#increment")
            assert browser("eval", "({value: document.querySelector('#name').value, count: window.count})")["result"] == {"value": "fixture-14", "count": 15}
            assert other_browser("eval", "({value: document.querySelector('#name').value, count: window.count})")["result"] == {"value": "second page", "count": 1}
            assert selected_surface(app) == terminal
            local_page = root / "local page #?.html"
            (root / "local-style.css").write_text("h1 { color: rgb(12, 34, 56); }")
            (root / "local-script.js").write_text("window.localScriptReady = 'relative script loaded';")
            local_page.write_text('<!doctype html><title>Local document</title><link rel="stylesheet" href="local-style.css">'
                                  '<h1>Local document rendered</h1><script src="local-script.js"></script>')
            local_before = resources()["browser_preview"]["textures_assigned"]
            local_started = time.perf_counter_ns()
            browser("goto", str(local_page))
            browser("wait", "--function", "window.localScriptReady === 'relative script loaded'", "--timeout-ms", "3000")
            local = browser("eval", "({url: location.href, title: document.title, color: getComputedStyle(document.querySelector('h1')).color})")["result"]
            assert local == {"url": local_page.as_uri(), "title": "Local document", "color": "rgb(12, 34, 56)"}
            assert "Local document rendered" in browser("snapshot")["snapshot"]
            app.wait_for(lambda: resources()["browser_preview"]["textures_assigned"] > local_before,
                         "local document preview frame")
            report["local_document_us"] = (time.perf_counter_ns() - local_started) / 1000
            browser("back")
            browser("wait", "--url-contains", "127.0.0.1", "--timeout-ms", "3000")
            browser("forward")
            browser("wait", "--function", "document.title === 'Local document'", "--timeout-ms", "3000")
            assert browser("eval", "location.href")["result"] == local_page.as_uri()
            assert selected_surface(app) == terminal, "document/history navigation stole focus"
            report["local_document_and_history"] = "passed"
            assert other_browser("eval", "({value: document.querySelector('#name').value, count: window.count})")["result"] == {"value": "second page", "count": 1}
            app.cli("browser", "close", "--surface", second)
            assert browser("eval", "document.title")["result"] == "Local document"
            stale = subprocess.run(["target/release/cmux", "--socket", str(app.socket_path), "browser", "eval", second, "document.title"],
                                   env=app.environment, capture_output=True, text=True, timeout=15)
            assert stale.returncode != 0, "closed browser reference redirected to surviving page"
            report["independent_browser_surfaces"] = "passed"
            app.cli("browser", "close")
            app.wait_for(lambda: not list(browser_dir.glob("*.pid")), "browser daemon shutdown")
            app.wait_for(lambda: not browser_processes.live_observed(), "sampled Chromium and daemon process exit")
            report["browser_process_cleanup"] = "passed"
    finally:
        # Only session PID files in this fixture-owned directory are eligible for cleanup.
        try:
            for pidfile in browser_dir.glob("*.pid"):
                subprocess.run([binary, "--session", pidfile.stem, "close"],
                               env=dict(os.environ, **environment), stdout=subprocess.DEVNULL,
                               stderr=subprocess.DEVNULL, timeout=15, check=False)
        finally:
            server.shutdown()
            server.server_close()
            serving.join(timeout=5)
            assert not serving.is_alive(), "fixture HTTP thread survived cleanup"
    report["status"] = "passed"


def main():
    """Retain real external-browser workload metadata and raw samples in a private CI artifact."""
    output = Path(os.environ.get("CMUX_BROWSER_REPORT", "target/benchmarks/real-browser-release.json"))
    report = {"workload": "real_browser_dom_and_preview", "warmup": 5, "iterations": 10,
              "includes": "CLI startup, cmux dispatch, real agent-browser/Chromium DOM actions and snapshot response; no compositor latency claim",
              "before": None, "after": None, "samples": []}
    with artifact(output, report):
        with tempfile.TemporaryDirectory(prefix="cmux-real-browser-") as directory:
            measure(Path(directory), report)
    print("real browser DOM operations, GTK preview and preserved terminal focus verified")


if __name__ == "__main__":
    main()
