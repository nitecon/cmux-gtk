#!/usr/bin/env python3
"""Exercise Python connection ownership using real Unix endpoints, without GTK."""
import os
from pathlib import Path
import socket
import sys
import tempfile
import time
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from cmux_socket_transport import connect_socket


class SocketTransport(unittest.TestCase):
    """Validate live connection settings and bounded startup failures."""

    def test_live_connection(self):
        """A connected socket exchanges real bytes and receives the requested I/O timeout."""
        with tempfile.TemporaryDirectory(prefix="cmux-transport-") as directory:
            path = os.path.join(directory, "socket")
            with socket.socket(socket.AF_UNIX) as server:
                server.bind(path)
                server.listen(1)
                server.settimeout(2)
                with connect_socket(path, 1, 2) as client:
                    peer, _ = server.accept()
                    with peer:
                        peer.settimeout(2)
                        client.sendall(b"hello")
                        self.assertEqual(peer.recv(5), b"hello")
                        self.assertEqual(client.gettimeout(), 2)

    def test_missing_endpoint_deadline(self):
        """A missing server fails within its startup budget and scheduling allowance."""
        with tempfile.TemporaryDirectory(prefix="cmux-transport-") as directory:
            started = time.monotonic()
            with self.assertRaises(FileNotFoundError):
                connect_socket(os.path.join(directory, "missing"), 0.03, 1)
            self.assertLess(time.monotonic() - started, 1)

    def test_invalid_timeouts(self):
        """Invalid budgets fail before creating a connection."""
        for value in (0, -1, float("nan"), float("inf")):
            with self.subTest(timeout=value), self.assertRaises(ValueError):
                connect_socket("/unused", value, 1)


if __name__ == "__main__":
    unittest.main()
