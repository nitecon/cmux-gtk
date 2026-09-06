#!/usr/bin/env python3
"""Bounded JSON-RPC client shared by maintained tools and protocol tests.

Use _call(method, params) with the running application's documented fields.
The CLI forwards --method and --params, or prints capabilities without a method.
No convenience wrapper implies support for an upstream debug endpoint.
"""

import sys
from pathlib import Path
import json
import socket
from typing import Any, Dict, Optional

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from cmux_socket_discovery import default_socket_path as _default_socket_path
from cmux_socket_transport import connect_socket, read_response

class cmuxError(Exception):
    """Exception raised for cmux errors."""

def _decode_response(line: str, request_id: int) -> dict:
    """Validate a v2 response envelope before accessing results or structured server errors."""
    response = json.loads(line)
    if not isinstance(response, dict):
        raise ValueError("Response must be a JSON object")
    if type(response.get("id")) is not int or response["id"] != request_id:
        raise ValueError("Response ID does not match the request")
    if type(response.get("ok")) is not bool:
        raise ValueError("Response must contain a boolean ok field")
    if not response["ok"]:
        error = response.get("error")
        if not isinstance(error, dict):
            raise ValueError("Failed response must contain an error object")
        if any(key in error and not isinstance(error[key], str) for key in ("code", "message")):
            raise ValueError("Error code and message must be strings")
    return response

class cmux:
    """Client for controlling cmux via the v2 JSON Unix socket."""

    DEFAULT_SOCKET_PATH = _default_socket_path()

    def __init__(self, socket_path: str = None):
        """Resolve discovery at construction time and initialize disconnected protocol state."""
        self.socket_path = socket_path or _default_socket_path()
        self._socket: Optional[socket.socket] = None
        self._recv_buffer = bytearray()
        self._next_id: int = 1

    # ---------------------------------------------------------------------
    # Connection
    # ---------------------------------------------------------------------

    def connect(self) -> None:
        """Connect within a bounded startup budget, retaining one owned socket."""
        if self._socket is not None:
            return
        try:
            self._socket = connect_socket(self.socket_path, 10.0, 10.0)
        except OSError as error:
            raise cmuxError(f"Failed to connect: {error}") from error

    def close(self) -> None:
        """Release the connection and discard any response buffered from that server."""
        connection, self._socket = self._socket, None
        self._recv_buffer.clear()
        if connection is not None:
            connection.close()

    def __enter__(self):
        """Connect on entering a client context."""
        self.connect()
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        """Always close the client while preserving an exception from its context."""
        self.close()
        return False

    # ---------------------------------------------------------------------
    # Low-level protocol
    # ---------------------------------------------------------------------

    def _recv_line(self, timeout_s: float = 20.0) -> str:
        """Read one bounded UTF-8 JSON line, discarding a failed connection."""
        if self._socket is None:
            raise cmuxError("Not connected")
        try:
            return read_response(self._socket, self._recv_buffer, timeout_s)
        except (OSError, ValueError) as error:
            self.close()
            raise cmuxError(f"Socket response failed: {error}") from error

    def _call(self, method: str, params: Optional[Dict[str, Any]] = None, timeout_s: float = 20.0) -> Any:
        """Send one numbered JSON request, validate its response ID and raise cmuxError on server failure."""
        if self._socket is None:
            raise cmuxError("Not connected")

        req_id = self._next_id
        self._next_id += 1

        payload = {
            "id": req_id,
            "method": method,
            "params": params or {},
        }
        line = json.dumps(payload, separators=(",", ":")) + "\n"
        try:
            self._socket.sendall(line.encode("utf-8"))
        except OSError as error:
            self.close()
            raise cmuxError(f"Socket write failed: {error}") from error

        resp_line = self._recv_line(timeout_s=timeout_s)
        try:
            resp = _decode_response(resp_line, req_id)
        except (ValueError, RecursionError) as error:
            self.close()
            raise cmuxError("Invalid v2 response envelope") from error

        if resp.get("ok") is True:
            return resp.get("result")

        err = resp.get("error") or {}
        code = err.get("code") or "error"
        msg = err.get("message") or "Unknown error"
        data = err.get("data")
        if data is not None:
            raise cmuxError(f"{code}: {msg} ({data})")
        raise cmuxError(f"{code}: {msg}")

    # ---------------------------------------------------------------------
    # System
    # ---------------------------------------------------------------------

    def ping(self) -> bool:
        """Return whether the server reports a successful protocol ping."""
        res = self._call("system.ping")
        return bool((res or {}).get("pong"))

    def capabilities(self) -> dict:
        """Return the server capability map; use it to determine supported operations."""
        return dict(self._call("system.capabilities") or {})

    def identify(self, caller: Optional[dict] = None) -> dict:
        """Return server identity and focus information, optionally supplying caller metadata."""
        params: Dict[str, Any] = {}
        if caller is not None:
            params["caller"] = caller
        return dict(self._call("system.identify", params) or {})

def main() -> None:
    """Run one JSON method from CLI arguments, or print server capabilities when no method is supplied."""
    import argparse

    parser = argparse.ArgumentParser(description="cmux v2 socket client")
    parser.add_argument("-s", "--socket", default=cmux.DEFAULT_SOCKET_PATH, help="Socket path")
    parser.add_argument("--method", help="v2 method name")
    parser.add_argument("--params", default="{}", help="JSON params")

    args = parser.parse_args()

    with cmux(args.socket) as c:
        if not args.method:
            # Minimal smoke.
            print(json.dumps(c.capabilities(), indent=2, sort_keys=True))
            return
        params = json.loads(args.params)
        print(json.dumps(c._call(args.method, params), indent=2, sort_keys=True))

if __name__ == "__main__":
    main()
