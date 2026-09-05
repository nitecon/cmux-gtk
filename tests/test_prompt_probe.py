#!/usr/bin/env python3
"""Exercise prompt-probe cleanup without touching a running desktop session."""

import importlib.util
from pathlib import Path
import unittest
from unittest.mock import MagicMock, patch

SPEC = importlib.util.spec_from_file_location(
    "prompt_probe", Path(__file__).resolve().parents[1] / "scripts/probe-pure-prompt-duplication.py"
)
PROBE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PROBE)


class PromptProbeCleanup(unittest.TestCase):
    """Verify temporary workspace ownership around a failed selection."""

    def test_selection_failure_closes_workspace_and_restores_original(self):
        """A failure immediately after creation still closes the temporary workspace."""
        client = MagicMock()

        def respond(method, params):
            """Model creation followed by a selection error, allowing cleanup calls."""
            if method == "workspace.current":
                return {"workspace_id": "original"}
            if method == "workspace.create":
                return {"workspace_id": "temporary"}
            if method == "workspace.select" and params["workspace_id"] == "temporary":
                raise PROBE.cmuxError("selection failed")
            return {}

        client._call.side_effect = respond
        factory = MagicMock()
        factory.return_value.__enter__.return_value = client
        with patch.object(PROBE, "cmux", factory), patch("sys.argv", ["probe"]):
            with self.assertRaisesRegex(PROBE.cmuxError, "selection failed"):
                PROBE.main()
        calls = client._call.call_args_list
        self.assertEqual(calls[-2].args, ("workspace.close", {"workspace_id": "temporary"}))
        self.assertEqual(calls[-1].args, ("workspace.select", {"workspace_id": "original"}))
        factory.assert_called_once_with(None)


if __name__ == "__main__":
    unittest.main()
