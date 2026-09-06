#!/usr/bin/env python3
"""
End-to-end test for sidebar listening ports auto-detection.

This covers regressions where a listening server (e.g. `python3 -m http.server`)
doesn't show up in the sidebar ports row.

Run with a tagged instance to avoid unix socket conflicts:
  CMUX_TAG=<tag> python3 tests/test_sidebar_ports.py
"""

from __future__ import annotations

import os
import shutil
import socket
import subprocess
import sys
import time
from pathlib import Path

# Add the directory containing cmux.py to the path
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from cmux import cmux, cmuxError  # noqa: E402
from process_support import stop_process
from sidebar_support import parse_sidebar_state as _parse_sidebar_state
from sidebar_support import wait_for_observation as _wait_for


# Historically, ports detection only checked a small allowlist. This test
# intentionally uses a port outside that set to avoid regressions where ports
# "work" only for the allowlist.
_HISTORICAL_ALLOWLIST = {8000, 8080, 8888, 5173, 3000, 3001, 5000, 5432}
_PREFERRED_BIND_HOST = "127.0.0.1"


def _find_free_allowed_port() -> int:
    # Prefer a random ephemeral port to avoid flakiness from well-known ports
    # being grabbed by background services.
    """Choose an ephemeral loopback port outside the historical allowlist; closing the probe leaves a bind race for the caller to handle."""
    for _ in range(50):
        s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        try:
            s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            s.bind((_PREFERRED_BIND_HOST, 0))
            port = int(s.getsockname()[1])
            if port not in _HISTORICAL_ALLOWLIST:
                return port
        finally:
            try:
                s.close()
            except Exception:
                pass

    raise RuntimeError("Failed to find a free test port (outside historical allowlist).")


def _start_external_server(base: Path, port: int) -> subprocess.Popen:
    """
    Start an http.server outside cmux and ensure it is actually listening.
    Failed readiness terminates and reaps the child before propagating the error.
    A successful return transfers cleanup ownership to the caller.
    """
    proc = subprocess.Popen(
        [sys.executable, "-m", "http.server", str(port), "--bind", _PREFERRED_BIND_HOST],
        cwd=str(base),
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    try:
        _wait_for_lsof_listen_pid(port, expected_pid=proc.pid, timeout=6.0)
    except BaseException:
        stop_process(proc, timeout=2)
        raise
    return proc


def _reported_ports(state: dict[str, str]) -> set[int]:
    """Parse a required legacy ports field; malformed data cannot prove listener absence.

    Empty text and none mean no ports. Numeric rows use comma-separated ASCII
    decimal ports in 1..65535; whitespace is trimmed and duplicates collapse.
    """
    raw = state.get("ports")
    if not isinstance(raw, str):
        raise ValueError("sidebar snapshot has no ports field")
    raw = raw.strip()
    if raw in ("", "none"):
        return set()
    ports = set()
    for item in raw.split(","):
        item = item.strip()
        if not item.isascii() or not item.isdecimal() or not 1 <= int(item) <= 65535:
            raise ValueError("sidebar snapshot contains an invalid port")
        ports.add(int(item))
    return ports


def _wait_for_port(client: cmux, port: int, timeout: float = 18.0) -> dict[str, str]:
    """Retry legacy sidebar snapshots until the requested numeric port appears."""
    def pred():
        """Return the matching port or listener observation for the enclosing retry loop."""
        state = _parse_sidebar_state(client.sidebar_state())
        return state if port in _reported_ports(state) else None

    return _wait_for(pred, timeout=timeout, interval=0.15, label=f"ports include {port}")


def _wait_for_port_absent(client: cmux, port: int, timeout: float = 18.0) -> dict[str, str]:
    """Retry legacy sidebar snapshots until the port is absent, including empty port rows."""
    def pred():
        """Return the matching port or listener observation for the enclosing retry loop."""
        state = _parse_sidebar_state(client.sidebar_state())
        return state if port not in _reported_ports(state) else None

    return _wait_for(pred, timeout=timeout, interval=0.15, label=f"ports do not include {port}")


def _assert_port_absent_for_duration(client: cmux, port: int, duration: float = 6.0, interval: float = 0.15) -> None:
    """
    Assert the port does not appear in sidebar_state during the full duration.
    This is important to catch "machine-wide ports" leaking into a fresh tab.
    """
    start = time.monotonic()
    while time.monotonic() - start < duration:
        state = _parse_sidebar_state(client.sidebar_state())
        if port in _reported_ports(state):
            raise AssertionError(f"Port {port} unexpectedly appeared in sidebar ports")
        time.sleep(interval)


def _listener_pids(port: int) -> list[int]:
    """Observe lsof listener PIDs with a two-second subprocess budget.

    Only clean no-match output is accepted as empty. Tool errors, warnings and
    malformed identities fail observation; this is not a system-wide permission guarantee.
    """
    result = subprocess.run(
        ["lsof", "-nP", f"-iTCP:{port}", "-sTCP:LISTEN", "-t", "+w"],
        capture_output=True, text=True, timeout=2,
    )
    if result.stderr.strip() or result.returncode not in (0, 1):
        raise RuntimeError(f"lsof observation failed (exit={result.returncode}): {result.stderr.strip()}")
    if result.returncode == 1:
        if result.stdout.strip():
            raise RuntimeError("lsof returned partial output with a failed status")
        return []
    pids = []
    for line in result.stdout.splitlines():
        line = line.strip()
        if not line:
            continue
        if not line.isdecimal() or int(line) <= 0:
            raise ValueError("lsof returned an invalid listener PID")
        pids.append(int(line))
    return pids


def _wait_for_lsof_listen_pid(port: int, expected_pid: int | None, timeout: float = 8.0) -> int:
    """
    Wait until `lsof -iTCP:<port> -sTCP:LISTEN` returns a pid.
    If expected_pid is provided, require that pid to be present.
    """

    def pred():
        """Return the expected listener, or the first observed listener when no identity is required."""
        pids = _listener_pids(port)
        if not pids:
            return None
        if expected_pid is not None and expected_pid not in pids:
            return None
        return expected_pid if expected_pid is not None else pids[0]

    value = _wait_for(pred, timeout=timeout, interval=0.15, label=f"lsof LISTEN pid for {port}")
    return int(value)


def _wait_for_lsof_listen_gone(port: int, timeout: float = 8.0) -> None:
    """Wait for a clean empty observation; tool failures are retried and cannot satisfy absence."""
    def pred():
        """Require successful observation with no listener identities."""
        return not _listener_pids(port)

    _wait_for(pred, timeout=timeout, interval=0.15, label=f"lsof no LISTEN for {port}")


def main() -> int:
    """Compare external versus shell-owned listener attribution through legacy sidebar APIs; owns the external child, while the shell server requires explicit command cleanup."""
    tag = os.environ.get("CMUX_TAG") or ""
    if not tag:
        print("Tip: set CMUX_TAG=<tag> when running this test to avoid socket conflicts.")

    base = Path("/tmp") / f"cmux_ports_test_{os.getpid()}"
    pid_file = base / "server.pid"
    log_file = base / "server.log"
    external_proc: subprocess.Popen | None = None

    try:
        if base.exists():
            shutil.rmtree(base)
        base.mkdir(parents=True, exist_ok=True)

        # Start a listening server outside cmux. A fresh tab should NOT show this port,
        # since ports should be attributed to the shell session in the tab.
        port = None
        last_start_err: Exception | None = None
        for _ in range(8):
            try:
                port = _find_free_allowed_port()
                external_proc = _start_external_server(base, port)
                break
            except Exception as e:
                last_start_err = e
                continue
        if port is None or external_proc is None:
            raise RuntimeError(f"Failed to start external http.server. Last error: {last_start_err}")

        with cmux() as client:
            new_tab_id = client.new_tab()
            client.select_tab(new_tab_id)
            time.sleep(0.8)

            # Trigger a prompt cycle (and thus a ports scan burst) before checking absence.
            client.send("echo cmux_ports_test\n")
            _assert_port_absent_for_duration(client, port, duration=6.0)

            # Stop the external server, then reuse the port inside the tab.
            stop_process(external_proc, timeout=3)
            external_proc = None
            _wait_for_lsof_listen_gone(port, timeout=8.0)

            # Start a server in the background and capture its PID so we can clean up.
            client.send(f"rm -f {pid_file} {log_file}\n")
            client.send(
                f"python3 -m http.server {port} --bind {_PREFERRED_BIND_HOST} > {log_file} 2>&1 & echo $! > {pid_file}\n"
            )

            _wait_for(lambda: pid_file.exists(), timeout=4.0, interval=0.1, label="pid file")
            pid = int(pid_file.read_text(encoding="utf-8").strip())

            # Ensure the server is actually listening (sanity check + reduces flakiness).
            _wait_for_lsof_listen_pid(port, expected_pid=pid, timeout=8.0)

            # Wait for the sidebar to report the port.
            _wait_for_port(client, port, timeout=18.0)

            # Cleanup server.
            client.send(f"kill {pid} >/dev/null 2>&1 || true\n")

            _wait_for_lsof_listen_gone(port, timeout=8.0)
            _wait_for_port_absent(client, port, timeout=18.0)

            try:
                client.close_tab(new_tab_id)
            except Exception:
                pass

        print("Sidebar ports test passed.")
        return 0

    except (cmuxError, AssertionError, RuntimeError, ValueError) as e:
        print(f"Sidebar ports test failed: {e}")
        return 1
    finally:
        stop_process(external_proc, timeout=2)
        try:
            shutil.rmtree(base)
        except Exception:
            pass


if __name__ == "__main__":
    raise SystemExit(main())
