#!/usr/bin/env python3
"""Verify shared sidebar parsing and polling contracts without a desktop session."""
import unittest
import subprocess
import tempfile
from pathlib import Path
from unittest.mock import Mock, patch

from sidebar_support import parse_sidebar_state, wait_for_state_field, wait_for_observation


class SidebarSupport(unittest.TestCase):
    """Exercise text edge cases and observation behavior used by legacy scenarios."""

    def test_listener_observation_distinguishes_errors(self):
        """Failed or partial lsof output cannot be interpreted as successful listener removal."""
        from test_sidebar_ports import _listener_pids
        for code, stdout, stderr, expected in (
            (0, "12\n34\n", "", [12, 34]),
            (1, "", "", []),
            (2, "", "", None),
            (1, "12\n", "", None),
            (0, "", "permission warning", None),
            (0, "invalid\n", "", None),
            (0, "0\n", "", None),
        ):
            result = subprocess.CompletedProcess([], code, stdout, stderr)
            with self.subTest(code=code, stdout=stdout, stderr=stderr), \
                    patch("test_sidebar_ports.subprocess.run", return_value=result):
                if expected is None:
                    with self.assertRaises((RuntimeError, ValueError)):
                        _listener_pids(12345)
                else:
                    self.assertEqual(_listener_pids(12345), expected)

    def test_failed_server_readiness_reaps_child(self):
        """The launcher retains cleanup ownership until it can return a ready server handle."""
        from test_sidebar_ports import _start_external_server
        children = []
        launch = subprocess.Popen

        def record_child(*args, **kwargs):
            """Retain the real child handle so the failed-readiness path can be verified."""
            child = launch(*args, **kwargs)
            children.append(child)
            return child

        with tempfile.TemporaryDirectory(prefix="cmux-port-readiness-") as directory:
            with patch("test_sidebar_ports.subprocess.Popen", side_effect=record_child), \
                    patch("test_sidebar_ports._wait_for_lsof_listen_pid", side_effect=RuntimeError("not ready")):
                with self.assertRaisesRegex(RuntimeError, "not ready"):
                    _start_external_server(Path(directory), 0)
        self.assertEqual(len(children), 1)
        self.assertIsNotNone(children[0].poll())

    def test_transient_observation_recovers(self):
        """The retrying contract preserves a successful snapshot after transient failure and empty state."""
        expected = {"cwd": "/observed"}
        predicate = Mock(side_effect=[ConnectionError("reconnecting"), None, expected])
        self.assertIs(wait_for_observation(predicate, 1, 0.001, "directory report"), expected)
        self.assertEqual(predicate.call_count, 3)

    def test_failed_observation_retains_cause(self):
        """Expiry carries the last transient exception rather than losing the actual failure."""
        cause = ConnectionError("closed")
        with self.assertRaisesRegex(AssertionError, "Last error: closed") as failure:
            wait_for_observation(Mock(side_effect=cause), 0.01, 0.001, "directory report")
        self.assertIs(failure.exception.__cause__, cause)

    def test_top_level_fields(self):
        """Nested rows cannot overwrite top-level state; equals signs and Unicode survive."""
        text = "cwd=/first\n  cwd=/nested\nmalformed\ncwd= /tmp/日本語 \r\nurl=https://host/?a=b\nempty=\n"
        self.assertEqual(parse_sidebar_state(text), {
            "cwd": "/tmp/日本語", "url": "https://host/?a=b", "empty": "",
        })
        self.assertEqual(parse_sidebar_state(""), {})

    def test_wait_returns_matching_snapshot(self):
        """An empty expected field must exist, and the returned snapshot is the matching one."""
        client = Mock()
        client.sidebar_state.side_effect = ["other=before", "other=after\nvalue="]
        self.assertEqual(wait_for_state_field(client, "value", "", interval=0.001), {
            "other": "after", "value": "",
        })
        self.assertEqual(client.sidebar_state.call_count, 2)

    def test_client_error_propagates(self):
        """A broken socket fails immediately rather than being converted to a timeout."""
        client = Mock()
        client.sidebar_state.side_effect = ConnectionError("closed")
        with self.assertRaisesRegex(ConnectionError, "closed"):
            wait_for_state_field(client, "cwd", "/tmp")
        client.sidebar_state.assert_called_once()


if __name__ == "__main__":
    unittest.main()
