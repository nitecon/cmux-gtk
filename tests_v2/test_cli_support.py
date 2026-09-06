#!/usr/bin/env python3
"""Exercise shared assertions and CLI discovery for retained protocol scenarios."""
import os
from pathlib import Path
import subprocess
import sys
import time
import tempfile
import unittest
from unittest.mock import Mock, patch

from cli_support import find_cli_binary
from cmux import cmuxError
from scenario_support import require, run_command, wait_for, wait_for_browser, wait_until


class ScenarioSupportTests(unittest.TestCase):
    """Check shared failure contracts and executable discovery without GTK."""

    def test_scenario_assertions_preserve_errors(self):
        """Truthiness and failure details retain the protocol client's public error contract."""
        for value in (True, 1, "present", [0]):
            self.assertIsNone(require(value, "unused"))
        for value in (False, None, 0, "", []):
            with self.assertRaises(cmuxError) as raised:
                require(value, "expected workspace identity")
            self.assertEqual(str(raised.exception), "expected workspace identity")

    def test_command_capture_and_error_contract(self):
        """Real children preserve literal arguments, environment, both pipes and exit status."""
        command = [sys.executable, "-c",
                   "import os,sys; print(sys.argv[1]); print(os.environ['FIXTURE_VALUE'],file=sys.stderr); sys.exit(7)",
                   "literal $(false) λ"]
        result = run_command(command, env=dict(os.environ, FIXTURE_VALUE="fixture stderr"), check=False)
        self.assertEqual(result.returncode, 7)
        self.assertEqual(result.stdout, "literal $(false) λ\n")
        self.assertEqual(result.stderr, "fixture stderr\n")
        with self.assertRaisesRegex(cmuxError, "exit code 7") as raised:
            run_command(command, env=dict(os.environ, FIXTURE_VALUE="fixture stderr"))
        self.assertNotIn("fixture stderr", str(raised.exception))
        self.assertNotIn("literal", str(raised.exception))
        result = run_command([sys.executable, "-c", "import os; os.write(1,b'a'*65536); os.write(2,b'b'*65536)"])
        self.assertEqual(result.stdout, "a" * 65536)
        self.assertEqual(result.stderr, "b" * 65536)

    def test_command_bounds_validate_before_spawn(self):
        """Invalid limits reject execution; exact byte limits preserve complete output."""
        with tempfile.TemporaryDirectory(prefix="cmux-command-validation-") as directory:
            marker = Path(directory) / "started"
            command = [sys.executable, "-c", "from pathlib import Path; Path(" + repr(str(marker)) + ").touch()"]
            for limits in ({"timeout": float("nan")}, {"timeout": float("inf")},
                           {"timeout": 0}, {"timeout": -1}, {"output_limit": 0},
                           {"output_limit": 1.5}, {"output_limit": float("inf")}):
                with self.assertRaises(ValueError):
                    run_command(command, **limits)
                self.assertFalse(marker.exists())
        result = run_command([sys.executable, "-c", "import os; os.write(1,b'a'*1024); os.write(2,b'b'*1024)"],
                             output_limit=1024)
        self.assertEqual(result.stdout, "a" * 1024)
        self.assertEqual(result.stderr, "b" * 1024)

    def test_command_limits_reap_children(self):
        """Deadline and either-pipe overflow terminate and reap actual owned child processes."""
        with tempfile.TemporaryDirectory(prefix="cmux-command-limits-") as directory:
            marker = Path(directory) / "pid"
            prefix = "import os,time; from pathlib import Path; Path(" + repr(str(marker)) + ").write_text(str(os.getpid())); "
            for body, timeout, message in [
                ("time.sleep(60)", 2, "deadline"),
                ("os.close(1); os.close(2); time.sleep(60)", 2, "deadline"),
                ("os.write(1,b'x'*4096); time.sleep(60)", 5, "output limit"),
                ("os.write(2,b'x'*4096); time.sleep(60)", 5, "output limit"),
            ]:
                marker.unlink(missing_ok=True)
                started = time.monotonic()
                with self.assertRaisesRegex(cmuxError, message):
                    run_command([sys.executable, "-c", prefix + body], timeout=timeout,
                                output_limit=1024, check=False)
                self.assertLess(time.monotonic() - started, timeout + 5)
                pid = int(marker.read_text())
                with self.assertRaises(ChildProcessError):
                    os.waitpid(pid, os.WNOHANG)
                self.assertFalse(Path(f"/proc/{pid}").exists())

    def test_shared_polling_contract(self):
        """Real elapsed-time polling preserves success, deadline details and predicate failures."""
        values = iter([False, False, True])
        with patch("time.time", side_effect=AssertionError("wall clock consulted")):
            self.assertIsNone(wait_until(values.__next__, timeout_s=1, interval_s=0.001))
        started = time.monotonic()
        with self.assertRaisesRegex(cmuxError, "expected ready"):
            wait_until(lambda: False, timeout_s=0.02, interval_s=10, message="expected ready")
        self.assertLess(time.monotonic() - started, 1)
        with self.assertRaisesRegex(cmuxError, "Timed out waiting for condition"):
            wait_for(lambda: False, timeout_s=0.01)
        failure = AssertionError("predicate failed")
        with self.assertRaises(AssertionError) as raised:
            wait_until(Mock(side_effect=failure))
        self.assertIs(raised.exception, failure)
        for limit in (float("nan"), float("inf"), 0, -1):
            with self.assertRaises(ValueError):
                wait_until(lambda: True, timeout_s=limit)
            with self.assertRaises(ValueError):
                wait_until(lambda: True, interval_s=limit)

    def test_browser_polling_retries_only_observation_errors(self):
        """Transient failures retry, expiry retains context and cancellation escapes unchanged."""
        observation = Mock(side_effect=[ValueError("not ready"), False, True])
        self.assertIsNone(wait_for_browser(observation, 1, "page title"))
        self.assertEqual(observation.call_count, 3)
        transient = ValueError("page loading")
        with self.assertRaisesRegex(cmuxError, "Timed out waiting for title: page loading") as raised:
            wait_for_browser(Mock(side_effect=transient), 0.01, "title")
        self.assertIs(raised.exception.__cause__, transient)
        with self.assertRaisesRegex(cmuxError, "^Timed out waiting for title$"):
            wait_for_browser(lambda: False, 0.01, "title")
        cancelled = KeyboardInterrupt()
        with self.assertRaises(KeyboardInterrupt) as raised:
            wait_for_browser(Mock(side_effect=cancelled), 1, "title")
        self.assertIs(raised.exception, cancelled)

    def test_build_directory_and_explicit_override(self):
        """Run the chosen executable and reject invalid overrides rather than selecting another build."""
        with tempfile.TemporaryDirectory(prefix="cmux-cli-discovery-") as directory:
            root = Path(directory)
            build = root / "build with spaces"
            build.mkdir()
            binary = build / "cmux"
            binary.write_text("#!/bin/sh\nprintf '%s' build\n")
            binary.chmod(0o700)
            override = root / "selected cli"
            override.write_text("#!/bin/sh\nprintf '%s' override\n")
            override.chmod(0o700)
            with patch.dict(os.environ, {"CMUX_BIN_DIR": str(build)}, clear=True):
                self.assertEqual(subprocess.check_output([find_cli_binary()], text=True, timeout=2), "build")
                os.environ["CMUXTERM_CLI"] = str(override)
                self.assertEqual(subprocess.check_output([find_cli_binary()], text=True, timeout=2), "override")
                for invalid in (root / "missing", build):
                    os.environ["CMUXTERM_CLI"] = str(invalid)
                    with self.assertRaises(cmuxError):
                        find_cli_binary()
                override.chmod(0o600)
                os.environ["CMUXTERM_CLI"] = str(override)
                with self.assertRaises(cmuxError):
                    find_cli_binary()
                del os.environ["CMUXTERM_CLI"]
                binary.unlink()
                with self.assertRaises(cmuxError):
                    find_cli_binary()


if __name__ == "__main__":
    unittest.main()
