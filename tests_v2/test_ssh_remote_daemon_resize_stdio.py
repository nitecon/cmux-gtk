#!/usr/bin/env python3
"""Process-level integration: cmuxd-remote stdio session resize coordinator."""

from __future__ import annotations

import json
import socket
import tempfile
from contextlib import contextmanager
import shutil
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from cmux import cmuxError
from scenario_support import require as _must, run_command
from process_support import stop_process
from cmux_socket_transport import read_response


def _daemon_module_dir() -> Path:
    """Resolve the repository-owned Go module relative to this fixture, independent of cwd."""
    return Path(__file__).resolve().parents[1] / "daemon" / "remote"


def _rpc(connection: socket.socket, req_id: int, method: str, params: dict,
         *, timeout_s: float = 5.0) -> dict:
    """Bound a complete stdio request/reply, validate its identity and retire failed transport.

    The daemon's stdin/stdout share one socketpair endpoint. Framing uses the same
    four-MiB response limit as Python clients, including fragmented response reads.
    """
    deadline = time.monotonic() + timeout_s
    try:
        payload = json.dumps({"id": req_id, "method": method, "params": params},
                             separators=(",", ":")).encode() + b"\n"
        if len(payload) > 4 * 1024 * 1024:
            raise cmuxError("daemon request exceeds byte limit")
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("daemon request deadline exceeded")
        connection.settimeout(remaining)
        connection.sendall(payload)
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("daemon request deadline exceeded")
        response = json.loads(read_response(connection, bytearray(), remaining))
        _must(isinstance(response, dict) and response.get("id") == req_id,
              f"Response identity mismatch for {method}")
        return response
    except BaseException:
        connection.close()
        raise


@contextmanager
def running_daemon():
    """Build with the configured Go toolchain, launch the binary directly and reap it on every exit."""
    _must(shutil.which("go") is not None, "Go is required for remote daemon integration")
    daemon_dir = _daemon_module_dir()
    _must(daemon_dir.is_dir(), f"Missing daemon module directory: {daemon_dir}")
    with tempfile.TemporaryDirectory(prefix="cmux-daemon-resize-") as directory:
        binary = Path(directory) / "cmuxd-remote"
        run_command(["go", "-C", str(daemon_dir), "build", "-o", str(binary), "./cmd/cmuxd-remote"])
        connection, child = socket.socketpair()
        with connection, child:
            proc = subprocess.Popen([str(binary), "serve", "--stdio"],
                                    stdin=child, stdout=child, stderr=subprocess.DEVNULL)
            child.close()
            try:
                yield connection
            finally:
                connection.close()
                stop_process(proc)


def _as_int(value: object, field: str) -> int:
    """Accept JSON integer values or integral floats, rejecting booleans and other types."""
    if isinstance(value, bool):
        raise cmuxError(f"{field} should be numeric, got bool")
    if isinstance(value, int):
        return value
    if isinstance(value, float):
        if not value.is_integer():
            raise cmuxError(f"{field} should be an integer value, got float {value!r}")
        return int(value)
    raise cmuxError(f"{field} has unexpected type {type(value).__name__}: {value!r}")


def _assert_effective(resp: dict, want_cols: int, want_rows: int, label: str) -> None:
    """Require protocol success and an exact effective grid, preserving operation context on error."""
    _must(resp.get("ok") is True, f"{label} should return ok=true: {resp}")
    result = resp.get("result") or {}
    got_cols = _as_int(result.get("effective_cols"), "effective_cols")
    got_rows = _as_int(result.get("effective_rows"), "effective_rows")
    _must(
        got_cols == want_cols and got_rows == want_rows,
        f"{label} effective size mismatch: got {got_cols}x{got_rows}, want {want_cols}x{want_rows} ({resp})",
    )


def main() -> int:
    """Verify the actual daemon's attach/resize/detach/reconnect coordination through bounded stdio."""
    with running_daemon() as connection:
        hello = _rpc(connection, 1, "hello", {})
        _must(hello.get("ok") is True, f"hello should return ok=true: {hello}")
        capabilities = {str(item) for item in ((hello.get("result") or {}).get("capabilities") or [])}
        _must("session.basic" in capabilities, f"hello missing session.basic capability: {hello}")
        _must("session.resize.min" in capabilities, f"hello missing session.resize.min capability: {hello}")

        open_resp = _rpc(connection, 2, "session.open", {"session_id": "sess-e2e"})
        _assert_effective(open_resp, 0, 0, "session.open")

        attach_small = _rpc(
            connection,
            3,
            "session.attach",
            {"session_id": "sess-e2e", "attachment_id": "a-small", "cols": 90, "rows": 30},
        )
        _assert_effective(attach_small, 90, 30, "session.attach(a-small)")

        attach_large = _rpc(
            connection,
            4,
            "session.attach",
            {"session_id": "sess-e2e", "attachment_id": "a-large", "cols": 140, "rows": 50},
        )
        _assert_effective(attach_large, 90, 30, "session.attach(a-large)")

        resize_large = _rpc(
            connection,
            5,
            "session.resize",
            {"session_id": "sess-e2e", "attachment_id": "a-large", "cols": 200, "rows": 80},
        )
        _assert_effective(resize_large, 90, 30, "session.resize(a-large)")

        detach_small = _rpc(
            connection,
            6,
            "session.detach",
            {"session_id": "sess-e2e", "attachment_id": "a-small"},
        )
        _assert_effective(detach_small, 200, 80, "session.detach(a-small)")

        detach_large = _rpc(
            connection,
            7,
            "session.detach",
            {"session_id": "sess-e2e", "attachment_id": "a-large"},
        )
        _assert_effective(detach_large, 200, 80, "session.detach(a-large)")

        reattach = _rpc(
            connection,
            8,
            "session.attach",
            {"session_id": "sess-e2e", "attachment_id": "a-reconnect", "cols": 110, "rows": 40},
        )
        _assert_effective(reattach, 110, 40, "session.attach(a-reconnect)")

        status = _rpc(connection, 9, "session.status", {"session_id": "sess-e2e"})
        _assert_effective(status, 110, 40, "session.status")
        attachments = (status.get("result") or {}).get("attachments") or []
        _must(len(attachments) == 1, f"session.status should report one active attachment after reattach: {status}")

        print("PASS: cmuxd-remote stdio session.resize coordinator enforces smallest-screen-wins semantics")
        return 0


if __name__ == "__main__":
    raise SystemExit(main())
