#!/usr/bin/env python3
"""Minimal legacy line transport retained for discovery/transport compatibility checks.

The GTK application uses JSON RPC. Use the Rust cmux CLI or tests_v2/cmux.py for
current commands; this module no longer provides unsupported upstream wrappers.
"""

import socket
import sys
from pathlib import Path
from typing import Optional

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from cmux_socket_discovery import default_socket_path as _default_socket_path
from cmux_socket_transport import connect_socket, read_response

class cmuxError(Exception):
    """Exception raised for cmux errors"""
    pass

class cmux:
    """Client for controlling cmux via Unix socket"""

    DEFAULT_SOCKET_PATH = _default_socket_path()

    @staticmethod
    def default_socket_path() -> str:
        """Resolve the current Linux socket using shared client discovery rules."""
        return _default_socket_path()

    def __init__(self, socket_path: str = None):
        """Resolve discovery at construction time and initialize disconnected protocol state."""
        # Resolve at init time so imports don't "lock in" a stale path.
        self.socket_path = socket_path or _default_socket_path()
        self._socket: Optional[socket.socket] = None
        self._recv_buffer = bytearray()

    def connect(self) -> None:
        """Connect within a bounded startup budget, retaining one owned socket."""
        if self._socket is not None:
            return
        try:
            self._socket = connect_socket(self.socket_path, 2.0, 5.0)
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

    def _send_command(self, command: str) -> str:
        """Send a command and receive response"""
        if self._socket is None:
            raise cmuxError("Not connected")

        try:
            self._socket.sendall((command + "\n").encode())
            return read_response(self._socket, self._recv_buffer, 5.0, multiline=True)
        except (OSError, ValueError) as error:
            self.close()
            raise cmuxError(f"Socket response failed: {error}") from error

def main():
    """CLI interface for cmux"""
    import sys
    import argparse

    parser = argparse.ArgumentParser(description="cmux CLI")
    parser.add_argument("command", nargs="?", help="Command to send")
    parser.add_argument("args", nargs="*", help="Command arguments")
    parser.add_argument("-s", "--socket", default=None,
                        help="Socket path (default: auto-detect)")

    args = parser.parse_args()

    try:
        with cmux(args.socket) as client:
            if not args.command:
                # Interactive mode
                print("cmux CLI (type 'help' for commands, 'quit' to exit)")
                while True:
                    try:
                        line = input("> ").strip()
                        if line.lower() in ("quit", "exit"):
                            break
                        if line:
                            response = client._send_command(line)
                            print(response)
                    except EOFError:
                        break
                    except KeyboardInterrupt:
                        print()
                        break
            else:
                # Single command mode
                command = args.command
                if args.args:
                    command += " " + " ".join(args.args)
                response = client._send_command(command)
                print(response)
    except cmuxError as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)

if __name__ == "__main__":
    main()
