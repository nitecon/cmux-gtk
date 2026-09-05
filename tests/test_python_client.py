#!/usr/bin/env python3
"""Exercise Python client selector behavior independently of a running desktop."""
import importlib.util
from pathlib import Path
import unittest
import socket
import json
from unittest.mock import Mock

SPEC = importlib.util.spec_from_file_location("cmux_v2_client", Path(__file__).resolve().parents[1] / "tests_v2/cmux.py")
CLIENT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CLIENT)


class Selectors(unittest.TestCase):
    """Verify selector normalization and distinct absent-focus policies."""

    def test_explicit_identifiers_and_indexes(self):
        """All selector kinds preserve IDs, resolve numeric indexes and reject cross-kind refs."""
        client = CLIENT.cmux("/unused")
        identifier = "d3d5a284-29d2-4fe0-a44b-3136ed673aa9"
        for kind in ("workspace", "surface", "pane"):
            with self.subTest(kind=kind):
                resolve = getattr(client, f"_resolve_{kind}_id")
                client._call = Mock(return_value={f"{kind}s": [{"index": 3, "id": identifier}]})
                self.assertEqual(resolve(" 3 "), identifier)
                self.assertEqual(resolve(3), identifier)
                client._call.reset_mock()
                self.assertEqual(resolve(identifier), identifier)
                self.assertEqual(resolve(f"{kind}:3"), f"{kind}:3")
                self.assertIsNone(resolve(" "))
                client._call.assert_not_called()
                with self.assertRaises(CLIENT.cmuxError):
                    resolve("window:3")
                with self.assertRaises(CLIENT.cmuxError):
                    resolve(9)

    def test_current_selection_and_workspace_scope(self):
        """Missing workspace selection raises; missing pane/surface focus remains optional."""
        client = CLIENT.cmux("/unused")
        client._call = Mock(return_value={})
        with self.assertRaisesRegex(CLIENT.cmuxError, "No workspace selected"):
            client._resolve_workspace_id(None)
        self.assertIsNone(client._resolve_pane_id(None))
        self.assertIsNone(client._resolve_surface_id(None))
        client._call.return_value = {"focused": {"pane_id": "selected-pane", "surface_id": "selected-surface"}}
        self.assertEqual(client._resolve_pane_id(None), "selected-pane")
        self.assertEqual(client._resolve_surface_id(None), "selected-surface")
        client._call.return_value = {"surfaces": [{"index": 0, "id": "scoped-surface"}]}
        self.assertEqual(client._resolve_surface_id(0, "workspace:4"), "scoped-surface")
        client._call.assert_called_with("surface.list", {"workspace_id": "workspace:4"})


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
