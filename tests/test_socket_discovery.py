#!/usr/bin/env python3
"""Exercise shared discovery against real temporary Unix sockets without a desktop."""
import os
from pathlib import Path
import socket
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from cmux_socket_discovery import default_socket_path


class SocketDiscovery(unittest.TestCase):
    """Verify Linux candidate precedence and bounded marker parsing."""

    def test_override_precedence(self):
        """An explicit socket wins even before it exists, with CLI-compatible precedence."""
        with patch.dict(os.environ, {"CMUX_SOCKET": "/missing/first", "CMUX_SOCKET_PATH": "/missing/second"}, clear=True):
            self.assertEqual(default_socket_path(), "/missing/first")
            del os.environ["CMUX_SOCKET"]
            self.assertEqual(default_socket_path(), "/missing/second")

    def test_runtime_and_marker(self):
        """The standard bound socket wins over a valid marker; the marker wins after closure."""
        with tempfile.TemporaryDirectory(prefix="cmux-discovery-") as temporary:
            root = Path(temporary)
            directory = root / "cmux"
            directory.mkdir()
            primary = directory / "cmux.sock"
            alternate = directory / "alternate.sock"
            with socket.socket(socket.AF_UNIX) as first, socket.socket(socket.AF_UNIX) as second:
                first.bind(str(primary))
                second.bind(str(alternate))
                (directory / "last-socket-path").write_text(str(alternate) + "\n")
                with patch.dict(os.environ, {"XDG_RUNTIME_DIR": temporary}, clear=True):
                    self.assertEqual(default_socket_path(), str(primary))
                    primary.unlink()
                    self.assertEqual(default_socket_path(), str(alternate))
                    for content in [(str(alternate) + " " * 4097).encode(), b"\xff", b"/missing/cmux.sock"]:
                        with self.subTest(marker=content[:40]):
                            (directory / "last-socket-path").write_bytes(content)
                            self.assertNotEqual(default_socket_path(), str(alternate))


if __name__ == "__main__":
    unittest.main()
