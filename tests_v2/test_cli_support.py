#!/usr/bin/env python3
"""Exercise retained-scenario CLI discovery using real executable files."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from cli_support import find_cli_binary
from cmux import cmuxError


class CliDiscoveryTests(unittest.TestCase):
    """Check explicit binary ownership and build-directory behavior without GTK."""

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
