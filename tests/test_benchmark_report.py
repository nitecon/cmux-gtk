#!/usr/bin/env python3
"""Verify benchmark evidence retention without running a local benchmark workload."""
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch
from benchmark_support import artifact, summarize_us, resource_delta

SPEC = importlib.util.spec_from_file_location("cmux_benchmark", Path(__file__).resolve().parents[1] / "scripts/benchmark-cmux.py")
BENCHMARK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BENCHMARK)


class BenchmarkReport(unittest.TestCase):
    """Exercise result accounting using controlled command successes and failures."""

    def test_native_artifact_failure_and_exclusive_creation(self):
        """Partial native measurements survive errors privately without leaking messages or replacing evidence."""
        with tempfile.TemporaryDirectory() as directory, patch("benchmark_support.subprocess.check_output", return_value="abc\n"):
            output = Path(directory) / "report.json"
            report = {"samples": [12.5]}
            with self.assertRaises(ValueError), artifact(output, report):
                raise ValueError("private workload detail")
            contents = output.read_text()
            saved = json.loads(contents)
            self.assertEqual(saved["samples"], [12.5])
            self.assertEqual(saved["status"], "failed")
            self.assertEqual(saved["error_kind"], "ValueError")
            self.assertNotIn("private workload detail", contents)
            self.assertEqual(output.stat().st_mode & 0o777, 0o600)
            with self.assertRaises(FileExistsError), artifact(output, {}):
                self.fail("existing artifact admitted a new workload")
            self.assertEqual(output.read_text(), contents)

    def test_native_latency_summary(self):
        """Nearest-rank summaries retain zero latency and reject unusable sample sets."""
        self.assertEqual(summarize_us([0, 10]), {"median": 5, "p95": 10, "p99": 10})
        for samples in ([], [-1], [float("nan")], [float("inf")]):
            with self.assertRaises(ValueError):
                summarize_us(samples)

    def test_native_resource_intervals(self):
        """CPU accounting distinguishes idle zero, multicore usage and unavailable/replaced processes."""
        before = {"pid": 42, "resources": dict.fromkeys(
            ("cpu_user_us", "cpu_system_us", "rss_kib", "threads", "file_descriptors"), 10)}
        after = {"pid": 42, "resources": dict(before["resources"], cpu_user_us=2000010)}
        self.assertEqual(resource_delta(before, after, 1)["cpu_percent"], 200)
        self.assertEqual(resource_delta(before, before, 1)["cpu_percent"], 0)
        for invalid in ({"pid": 43}, {"pid": 42, "resources": {}},
                        {"pid": 42, "resources": dict(before["resources"], cpu_user_us=0)}):
            with self.assertRaises(ValueError):
                resource_delta(before, invalid, 1)

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
