#!/usr/bin/env python3
"""Exercise bounded JSON-RPC client validation against real socket-pair replies."""
import importlib.util
from pathlib import Path
import unittest
import socket
import json

SPEC = importlib.util.spec_from_file_location("cmux_v2_client", Path(__file__).resolve().parents[1] / "tests_v2/cmux.py")
CLIENT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CLIENT)


class ResponseValidation(unittest.TestCase):
    """Exercise production request handling against real socket-pair replies."""

    def test_malformed_response_closes_connection(self):
        """Malformed envelopes and boolean IDs cannot poison the next request."""
        replies = [b"not json", b"[]", b'{"id":true,"ok":true}',
                   b'{"id":2,"ok":true}', b'{"id":1,"ok":"true"}',
                   b'{"id":1,"ok":false,"error":"bad"}',
                   b'{"id":1,"ok":false,"error":{"code":[]}}']
        for reply in replies:
            with self.subTest(reply=reply):
                reader, writer = socket.socketpair()
                with reader, writer:
                    reader.settimeout(1)
                    client = CLIENT.cmux("/unused")
                    client._socket = reader
                    writer.sendall(reply + b"\n")
                    with self.assertRaisesRegex(CLIENT.cmuxError, "Invalid v2 response"):
                        client._call("system.ping", timeout_s=1)
                    self.assertIsNone(client._socket)
                    self.assertEqual(reader.fileno(), -1)

    def test_server_error_preserves_valid_connection(self):
        """A correctly framed server error allows the following numbered request to succeed."""
        reader, writer = socket.socketpair()
        with reader, writer:
            reader.settimeout(1)
            client = CLIENT.cmux("/unused")
            client._socket = reader
            replies = [{"id": 1, "ok": False, "error": {"code": "not_found", "message": "missing"}},
                       {"id": 2, "ok": True, "result": {"pong": True}}]
            writer.sendall("".join(json.dumps(reply) + "\n" for reply in replies).encode())
            with self.assertRaisesRegex(CLIENT.cmuxError, "not_found: missing"):
                client._call("missing.method", timeout_s=1)
            self.assertIs(client._socket, reader)
            self.assertEqual(client._call("system.ping", timeout_s=1), {"pong": True})
            client.close()


if __name__ == "__main__":
    unittest.main()
