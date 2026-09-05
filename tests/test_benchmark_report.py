#!/usr/bin/env python3
"""Verify benchmark evidence retention without running a local benchmark workload."""
import importlib.util
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("cmux_benchmark", Path(__file__).resolve().parents[1] / "scripts/benchmark-cmux.py")
BENCHMARK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BENCHMARK)


class BenchmarkReport(unittest.TestCase):
    """Exercise result accounting using controlled command successes and failures."""

    def test_partial_failure(self):
        """A failing second ping retains the first latency and initial process snapshot."""
        before = {"build_profile": "release", "pid": 42}
        with patch.object(BENCHMARK, "call", side_effect=[before, {"pong": True}, subprocess.TimeoutExpired("private command", 10)]):
            report = BENCHMARK.measure("unused", "unused", 3, 0)
        self.assertEqual(report["status"], "failed")
        self.assertEqual(report["completed_iterations"], 1)
        self.assertEqual(len(report["latency_us"]["samples"]), 1)
        self.assertEqual(report["before"], before)
        self.assertEqual(report["failure"], {"phase": "measurement", "error_kind": "TimeoutExpired"})
        self.assertNotIn("private command", str(report))

    def test_warmup_failure(self):
        """Invalid warmup responses fail without inventing latency samples."""
        with patch.object(BENCHMARK, "call", return_value={"pong": False}):
            report = BENCHMARK.measure("unused", "unused", 1, 1)
        self.assertEqual(report["failure"]["phase"], "warmup")
        self.assertIsNone(report["latency_us"]["median"])
        self.assertEqual(report["completed_iterations"], 0)

    def test_success_and_process_replacement(self):
        """Successful measurements require the same process at both resource snapshots."""
        for final_pid, expected in [(42, "passed"), (43, "failed")]:
            with self.subTest(final_pid=final_pid), patch.object(BENCHMARK, "call", side_effect=[
                    {"build_profile": "release", "pid": 42}, {"pong": True}, {"pid": final_pid}]):
                report = BENCHMARK.measure("unused", "unused", 1, 0)
                self.assertEqual(report["status"], expected)
                self.assertEqual(report["completed_iterations"], 1)


if __name__ == "__main__":
    unittest.main()
