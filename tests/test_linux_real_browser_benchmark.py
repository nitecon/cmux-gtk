#!/usr/bin/env python3
"""Measure cmux browser operations against pinned agent-browser and real Chromium in CI."""

from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
from threading import Thread
import time

from benchmark_support import artifact, summarize_us, resource_delta
from linux_app import running_app
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
            opened = json.loads(app.cli("browser", "open", f"http://127.0.0.1:{server.server_port}/"))
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
            for iteration in range(15):
                if iteration == 5:
                    report["before"] = resources()
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
            report["after"] = resources()
            report["elapsed_seconds"] = time.monotonic() - started
            report["resource_delta"] = resource_delta(report["before"], report["after"], report["elapsed_seconds"])
            report["latency_us"] = {key: summarize_us([row[key] for row in report["samples"]])
                                    for key in report["samples"][0]}
            # A daemon-side evaluation error must reach the CLI as failure, not a successful outer envelope.
            failed = subprocess.run(["target/release/cmux", "--socket", str(app.socket_path), "browser", "eval",
                                     surface, "throw new Error('fixture failure')"], env=app.environment,
                                    capture_output=True, text=True, timeout=15)
            assert failed.returncode != 0
            assert selected_surface(app) == terminal
            app.cli("browser", "close")
            app.wait_for(lambda: not list(browser_dir.glob("*.pid")), "browser daemon shutdown")
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
