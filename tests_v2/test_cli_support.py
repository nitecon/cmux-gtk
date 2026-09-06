#!/usr/bin/env python3
"""Exercise shared assertions and CLI discovery for retained protocol scenarios."""
import os
from pathlib import Path
import subprocess
import sys
import time
import tempfile
import unittest
from unittest.mock import patch

from cli_support import find_cli_binary
from cmux import cmuxError
from scenario_support import require, run_command


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
