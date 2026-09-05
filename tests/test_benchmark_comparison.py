#!/usr/bin/env python3
"""Verify comparison against archived CI evidence and incompatible or corrupted reports."""
import copy
import importlib.util
from pathlib import Path
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


if __name__ == "__main__":
    unittest.main()
