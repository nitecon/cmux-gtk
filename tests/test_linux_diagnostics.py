#!/usr/bin/env python3
"""Verify diagnostic snapshots and CLI-to-GTK correlation on a live application."""

import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time


def wait_for(check, timeout=15):
    """Wait for observable asynchronous state, failing within a bounded deadline."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if check():
            return
        time.sleep(0.1)
    raise AssertionError("diagnostic state did not converge")


def main():
    """Exercise the installed command contract against isolated real GTK state."""
    with tempfile.TemporaryDirectory(prefix="cmux-diagnostics-") as directory:
        root = Path(directory)
        env = dict(os.environ, CMUX_NO_UPDATE="1", GDK_BACKEND="x11",
                   LIBGL_ALWAYS_SOFTWARE="1", CMUX_LOG=str(root / "events.jsonl"))
        for kind in ("DATA_HOME", "CONFIG_HOME", "STATE_HOME", "RUNTIME_DIR"):
            path = root / kind.lower()
            path.mkdir(mode=0o700)
            env[f"XDG_{kind}"] = str(path)
        socket = root / "runtime_dir/cmux/cmux.sock"
        binary_dir = Path(os.environ.get("CMUX_BIN_DIR", "target/debug"))
        with (root / "app.log").open("w+") as log:
            app = subprocess.Popen([str(binary_dir / "cmux-app")], env=env, stdout=log, stderr=log)
            try:
                wait_for(socket.exists)
                cli = [str(binary_dir / "cmux"), "--socket", str(socket), "--json"]
                result = subprocess.run(cli + ["diagnostics"], env=env, text=True,
                                        capture_output=True, check=True, timeout=10)
                snapshot = json.loads(result.stdout)
                assert snapshot["pid"] == app.pid
                assert snapshot["resources"]["rss_kib"] > 0
                assert snapshot["resources"]["threads"] > 0
                assert snapshot["resources"]["cpu_user_us"] >= 0
                assert snapshot["resources"]["cpu_system_us"] >= 0
                assert snapshot["logging"]["active"]

                def heartbeat_ready():
                    """Observe the GTK heartbeat through the background snapshot command."""
                    result = subprocess.run(cli + ["diagnostics"], env=env, text=True,
                                            capture_output=True, check=True, timeout=10)
                    latest = json.loads(result.stdout)
                    assert latest["resources"]["cpu_user_us"] >= snapshot["resources"]["cpu_user_us"]
                    assert latest["resources"]["cpu_system_us"] >= snapshot["resources"]["cpu_system_us"]
                    heartbeat = latest["gtk_event_loop"]
                    if not heartbeat["sampled"]:
                        return False
                    if latest["terminals"]["registered"] < 1:
                        return False
                    assert heartbeat["last_delay_us"] >= 0
                    assert heartbeat["max_delay_us"] >= heartbeat["last_delay_us"]
                    assert heartbeat["sample_age_ms"] >= 0
                    return True

                wait_for(heartbeat_ready)
                for command in ("ping", "raw"):
                    arguments = [command] if command == "ping" else ["raw", "unsupported_method"]
                    result = subprocess.run(cli + ["--verbose"] + arguments, env=env,
                                            text=True, capture_output=True, timeout=10)
                    assert (result.returncode == 0) == (command == "ping")
                    trace_id = re.search(r"trace_id=([0-9a-f-]+)", result.stderr).group(1)

                    def complete():
                        """Verify all recorded stages agree on the caller correlation ID."""
                        try:
                            records = [json.loads(line) for line in (root / "events.jsonl").read_text().splitlines()]
                        except (FileNotFoundError, json.JSONDecodeError):
                            return False
                        events = {record["event"]: record["fields"] for record in records
                                  if record["fields"].get("trace_id") == trace_id}
                        if not {"rpc.gtk.start", "rpc.gtk.dispatched", "rpc.complete"} <= events.keys():
                            return False
                        assert events["rpc.complete"]["outcome"] == ("success" if command == "ping" else "error")
                        assert events["rpc.gtk.start"]["queue_wait_us"] >= 0
                        return True

                    wait_for(complete)
                result = subprocess.run(cli + ["diagnostics"], env=env, text=True,
                                        capture_output=True, check=True, timeout=10)
                counters = json.loads(result.stdout)["rpc"]
                assert counters["succeeded"] > snapshot["rpc"]["succeeded"]
                assert counters["failed"] > snapshot["rpc"]["failed"]
                assert counters["in_flight"] >= 1  # The snapshot includes its own request.
                subprocess.run(cli + ["new-workspace"], env=env, text=True,
                               capture_output=True, check=True, timeout=10)

                def saved_session():
                    """Wait for a real workspace mutation to produce persistence timing evidence."""
                    try:
                        records = [json.loads(line) for line in (root / "events.jsonl").read_text().splitlines()]
                    except (FileNotFoundError, json.JSONDecodeError):
                        return False
                    saves = [record["fields"] for record in records
                             if record["event"] == "session.save"
                             and record["fields"]["workspaces"] >= 2]
                    if not saves:
                        return False
                    latest = saves[-1]
                    assert latest["outcome"] == "success"
                    assert latest["bytes"] > 0
                    assert latest["serialization_us"] >= 0
                    assert latest["write_us"] >= 0
                    assert latest["duration_us"] >= latest["serialization_us"]
                    return True

                wait_for(saved_session)
                report_path = root / "diagnostic-report.json"
                subprocess.run([sys.executable, "scripts/collect-cmux-diagnostics.py",
                                "--binary", str(binary_dir / "cmux"), "--socket", str(socket),
                                "--samples", "2", "--interval", "0.01",
                                "--output", str(report_path)], env=env, check=True, timeout=30)
                report = json.loads(report_path.read_text())
                assert len(report["samples"]) == 2
                assert report_path.stat().st_mode & 0o777 == 0o600
                for sample in report["samples"]:
                    assert sample["snapshot"]["pid"] == app.pid
                    assert sample["trace_id"]
                    assert "error" not in sample
                failed_report_path = root / "unavailable-report.json"
                result = subprocess.run([sys.executable, "scripts/collect-cmux-diagnostics.py",
                                         "--binary", str(root / "missing-cmux"), "--samples", "1",
                                         "--output", str(failed_report_path)], env=env, timeout=15)
                assert result.returncode == 1
                failed_report = json.loads(failed_report_path.read_text())
                assert failed_report["samples"][0]["error"] == "command_unavailable"
                if output := os.environ.get("CMUX_BENCHMARK_OUT"):
                    subprocess.run([sys.executable, "scripts/benchmark-cmux.py",
                                    "--binary", str(binary_dir / "cmux"),
                                    "--socket", str(socket), "--output", output],
                                   env=env, check=True, timeout=180)
                print("diagnostic resources and successful/failed CLI-to-GTK traces verified")
            finally:
                app.terminate()
                app.wait(timeout=10)
                log.seek(0)
                if app.returncode not in (0, -15):
                    print(log.read())


if __name__ == "__main__":
    main()
