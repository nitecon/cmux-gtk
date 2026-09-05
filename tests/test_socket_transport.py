#!/usr/bin/env python3
"""Exercise Python connection ownership using real Unix endpoints, without GTK."""
import os
from pathlib import Path
import socket
import sys
import tempfile
import time
import threading
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from cmux_socket_transport import connect_socket, read_response


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


class ResponseFraming(unittest.TestCase):
    """Verify byte bounds, deadlines and protocol-specific framing on Unix socket pairs."""

    def test_fragmented_unicode_and_coalesced_lines(self):
        """UTF-8 split across writes survives decoding and the next line remains buffered."""
        reader, writer = socket.socketpair()
        with reader, writer:
            def send_fragments():
                """Send a multibyte character across writes followed by two line delimiters."""
                writer.sendall(b"\xe2")
                time.sleep(0.02)
                writer.sendall(b"\x82\xac\nnext\n")
            sender = threading.Thread(target=send_fragments)
            sender.start()
            try:
                buffer = bytearray()
                self.assertEqual(read_response(reader, buffer, 1), "€")
                self.assertEqual(read_response(reader, buffer, 1), "next")
            finally:
                sender.join(timeout=2)
            self.assertFalse(sender.is_alive())

    def test_oversize_and_silent_peer(self):
        """Oversized replies fail within the byte cap and silent peers reach a total deadline."""
        reader, writer = socket.socketpair()
        with reader, writer:
            writer.sendall(b"x" * 17)
            buffer = bytearray()
            with self.assertRaisesRegex(ValueError, "byte limit"):
                read_response(reader, buffer, 1, limit=16)
            self.assertLessEqual(len(buffer), 17)
            with self.assertRaises(TimeoutError):
                read_response(reader, bytearray(), 0.03)

    def test_multiline_and_truncated_json_line(self):
        """V1 preserves multiple lines; V2 rejects EOF without its line delimiter."""
        reader, writer = socket.socketpair()
        with reader, writer:
            writer.sendall(b"one\ntwo\n")
            self.assertEqual(read_response(reader, bytearray(), 1, multiline=True), "one\ntwo")
            writer.sendall(b"truncated")
            writer.shutdown(socket.SHUT_WR)
            with self.assertRaises(ConnectionError):
                read_response(reader, bytearray(), 1)


if __name__ == "__main__":
    unittest.main()
