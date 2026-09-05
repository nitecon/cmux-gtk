#!/usr/bin/env python3
"""Verify comparison against archived CI evidence and incompatible or corrupted reports."""
import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("comparison", ROOT / "scripts/compare-cmux-benchmarks.py")
COMPARISON = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(COMPARISON)


class Comparison(unittest.TestCase):
    """Exercise raw measurements and compatibility guards without running cmux."""

    def setUp(self):
        """Load the preserved successful CI baseline as realistic input evidence."""
        self.baseline = COMPARISON.load_report(ROOT / "Docs/Benchmarks/e08b976f/cli-round-trip.json")

    def test_recalculates_raw_samples(self):
        """A slower candidate is measured from samples even if its cached summaries are stale."""
        candidate = copy.deepcopy(self.baseline)
        candidate["latency_us"]["samples"] = [value * 1.2 for value in candidate["latency_us"]["samples"]]
        result = COMPARISON.compare_reports(self.baseline, candidate)
        for metric in result["latency"].values():
            self.assertAlmostEqual(metric["change_percent"], 20)
        self.assertAlmostEqual(result["latency"]["median"]["baseline_us"], 1657.496)

    def test_rejects_incompatible_evidence(self):
        """Partial, changed-runtime and corrupt samples must not yield comparison numbers."""
        candidates = []
        for key, value in [("status", "failed"), ("warmup", 0), ("iterations", 99)]:
            candidate = copy.deepcopy(self.baseline)
            candidate[key] = value
            candidates.append(candidate)
        candidate = copy.deepcopy(self.baseline)
        candidate["before"]["build_profile"] = "debug"
        candidates.append(candidate)
        for value in [float("nan"), float("inf"), -1, True]:
            candidate = copy.deepcopy(self.baseline)
            candidate["latency_us"]["samples"][0] = value
            candidates.append(candidate)
        for candidate in candidates:
            with self.assertRaises(ValueError):
                COMPARISON.compare_reports(self.baseline, candidate)

    def test_rendering_override_compatibility(self):
        """Recorded rendering requests must agree; old unknown metadata is not assumed equivalent."""
        candidate = copy.deepcopy(self.baseline)
        for snapshot in (candidate["before"], candidate["after"]):
            snapshot["libgl_software_override"] = True
        with self.assertRaises(ValueError):
            COMPARISON.compare_reports(self.baseline, candidate)
        result = COMPARISON.compare_reports(candidate, candidate)
        self.assertIs(result["matched_settings"]["libgl_software_override"], True)
        candidate["after"]["libgl_software_override"] = 1
        with self.assertRaises(ValueError):
            COMPARISON.compare_reports(candidate, candidate)

    def test_matching_invalid_metadata_is_rejected(self):
        """Identically corrupted reports must not pass just because their settings match."""
        mutations = [
            (("schema",), True),
            (("warmup",), True),
            (("warmup",), -1),
            (("includes",), ""),
            (("host",), {}),
            (("host", "machine"), ""),
            (("before", "pid"), 0),
            (("after", "pid"), True),
            (("before", "gtk_version"), ""),
            (("after", "requested_backend"), "wayland"),
            (("after", "build_profile"), "debug"),
            (("before", "terminals", "registered"), -1),
            (("after", "terminals", "registered"), 3),
            (("after", "terminals", "registered"), True),
        ]
        for path, value in mutations:
            with self.subTest(path=path, value=value):
                report = copy.deepcopy(self.baseline)
                target = report
                for key in path[:-1]:
                    target = target[key]
                target[path[-1]] = value
                with self.assertRaises(ValueError):
                    COMPARISON.compare_reports(report, report)
        report = copy.deepcopy(self.baseline)
        report.update(iterations=1, completed_iterations=True)
        report["latency_us"]["samples"] = [1.0]
        with self.assertRaises(ValueError):
            COMPARISON.compare_reports(report, report)

    def test_cli_rejects_unrepresentable_and_oversized_reports(self):
        """Invalid input exits cleanly with no partial JSON or Python traceback."""
        with tempfile.TemporaryDirectory(prefix="cmux-comparison-") as directory:
            baseline_path = Path(directory) / "baseline.json"
            candidate_path = Path(directory) / "candidate.json"
            baseline = copy.deepcopy(self.baseline)
            baseline["latency_us"]["samples"] = [1e-300] * baseline["iterations"]
            baseline_path.write_text(json.dumps(baseline))
            candidate = copy.deepcopy(self.baseline)
            candidate["latency_us"]["samples"] = [1e300] * candidate["iterations"]
            payloads = [json.dumps(candidate).encode(), b" " * (COMPARISON.MAX_REPORT_BYTES + 1)]
            for payload in payloads:
                candidate_path.write_bytes(payload)
                result = subprocess.run(
                    [sys.executable, str(ROOT / "scripts/compare-cmux-benchmarks.py"),
                     str(baseline_path), str(candidate_path)],
                    capture_output=True, text=True, timeout=10,
                )
                self.assertEqual(result.returncode, 2)
                self.assertEqual(result.stdout, "")
                self.assertIn("cannot compare reports", result.stderr)
                self.assertNotIn("Traceback", result.stderr)


if __name__ == "__main__":
    unittest.main()
