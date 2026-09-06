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

    def test_previous_prompts_are_not_duplicate_preprompt(self):
        """Repeated ordinary shell prompts are valid history, while duplicate context rows remain detectable."""
        preprompt, prompt = PROBE._prompt_block("$ \n$ \n$ ")
        self.assertEqual(preprompt, [])
        self.assertEqual(prompt, "$ ")
        preprompt, _ = PROBE._prompt_block("❯ old command\nproject/main\nproject/main\n❯ ")
        self.assertEqual(PROBE._duplicate_run_length(preprompt), 2)

    def test_selection_failure_closes_workspace_and_restores_original(self):
        """A failure immediately after creation still closes the temporary workspace."""
        client = MagicMock()

        def respond(method, params):
            """Model creation followed by a selection error, allowing cleanup calls."""
            if method == "workspace.current":
                return {"uuid": "original"}
            if method == "workspace.create":
                return {"uuid": "temporary"}
            if method == "workspace.select" and params["id"] == "temporary":
                raise PROBE.cmuxError("selection failed")
            return {}

        client._call.side_effect = respond
        factory = MagicMock()
        factory.return_value.__enter__.return_value = client
        with patch.object(PROBE, "cmux", factory), patch("sys.argv", ["probe"]):
            with self.assertRaisesRegex(PROBE.cmuxError, "selection failed"):
                PROBE.main()
        calls = client._call.call_args_list
        self.assertEqual(calls[-2].args, ("workspace.close", {"id": "temporary"}))
        self.assertEqual(calls[-1].args, ("workspace.select", {"id": "original"}))
        factory.assert_called_once_with(None)


if __name__ == "__main__":
    unittest.main()
