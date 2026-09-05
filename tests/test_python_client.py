#!/usr/bin/env python3
"""Exercise Python client selector behavior independently of a running desktop."""
import importlib.util
from pathlib import Path
import unittest
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


if __name__ == "__main__":
    unittest.main()
