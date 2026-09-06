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

    def test_cpu_model_compatibility(self):
        """Unknown, changed or malformed CPU identities must not masquerade as matched hardware."""
        candidate = copy.deepcopy(self.baseline)
        for snapshot in (candidate["before"], candidate["after"]):
            snapshot["cpu_model"] = "Example CPU"
        with self.assertRaises(ValueError):
            COMPARISON.compare_reports(self.baseline, candidate)
        self.assertEqual(COMPARISON.compare_reports(candidate, candidate)["matched_settings"]["cpu_model"], "Example CPU")
        changed = copy.deepcopy(candidate)
        for snapshot in (changed["before"], changed["after"]):
            snapshot["cpu_model"] = "Different CPU"
        with self.assertRaises(ValueError):
            COMPARISON.compare_reports(candidate, changed)
        candidate["after"]["cpu_model"] = "Different CPU"
        with self.assertRaises(ValueError):
            COMPARISON.compare_reports(candidate, candidate)
        for model in [True, 7, "", " ", "x" * 257, "bad\nlabel"]:
            for snapshot in (candidate["before"], candidate["after"]):
                snapshot["cpu_model"] = model
            with self.assertRaises(ValueError):
                COMPARISON.compare_reports(candidate, candidate)

    def test_opengl_context_compatibility(self):
        """Compare stable first-context identities and reject unknown, changed or malformed labels."""
        candidate = copy.deepcopy(self.baseline)
        context = {"vendor": "Mesa", "renderer": "llvmpipe", "version": "4.5"}
        for snapshot in (candidate["before"], candidate["after"]):
            snapshot["first_opengl_context"] = context.copy()
        self.assertEqual(
            COMPARISON.compare_reports(candidate, candidate)["matched_settings"]["first_opengl_context"],
            context,
        )
        with self.assertRaises(ValueError):
            COMPARISON.compare_reports(self.baseline, candidate)
        changed = copy.deepcopy(candidate)
        changed["after"]["first_opengl_context"]["renderer"] = "Other GPU"
        with self.assertRaises(ValueError):
            COMPARISON.compare_reports(changed, changed)
        changed["before"]["first_opengl_context"]["renderer"] = "Other GPU"
        with self.assertRaises(ValueError):
            COMPARISON.compare_reports(candidate, changed)
        for invalid in [True, {}, {**context, "extra": "unexpected"},
                        *({**context, "renderer": label} for label in
                          [False, "", " ", "x" * 257, "λ" * 129, "bad\nlabel"])]:
            for snapshot in (candidate["before"], candidate["after"]):
                snapshot["first_opengl_context"] = invalid
            with self.subTest(context=invalid), self.assertRaises(ValueError):
                COMPARISON.compare_reports(candidate, candidate)
        for snapshot in (candidate["before"], candidate["after"]):
            snapshot["first_opengl_context"] = dict.fromkeys(context)
        self.assertEqual(
            COMPARISON.compare_reports(candidate, candidate)["matched_settings"]["first_opengl_context"],
            dict.fromkeys(context),
        )

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

    def memory_fixture(self):
        """Derive synthetic release-shaped input from archived debug samples for comparator tests only."""
        report = COMPARISON.load_report(ROOT / "Docs/Benchmarks/e08b976f/memory-churn.json")
        report["build_profile"] = "release"
        for sample in report["samples"]:
            sample["snapshot"]["build_profile"] = "release"
        return report

    def test_memory_resource_deltas(self):
        """Recompute RSS metrics from raw samples, preserving absolute changes with no invented gates."""
        baseline = self.memory_fixture()
        candidate = copy.deepcopy(baseline)
        for sample in candidate["samples"]:
            if sample["phase"] == "redraw":
                sample["snapshot"]["resources"]["rss_kib"] += 1024
        result = COMPARISON.compare_reports(baseline, candidate)
        metrics = result["resources"]
        self.assertEqual(metrics["redraw_final_rss_median_kib"]["delta"], 1024)
        self.assertEqual(metrics["redraw_rss_growth_kib"]["delta"], 0)
        self.assertEqual(metrics["split_close_rss_growth_kib"]["delta"], 0)
        self.assertGreater(result["baseline_window"]["redraw_observed_seconds"], 0)
        self.assertGreaterEqual(metrics["redraw_cpu_percent"]["baseline"], 0)
        split_live = copy.deepcopy(baseline["samples"][1])
        split_live.update(phase="split_live", iteration=1,
                          elapsed_seconds=(baseline["samples"][1]["elapsed_seconds"]
                                           + baseline["samples"][2]["elapsed_seconds"]) / 2)
        split_live["snapshot"]["terminals"]["registered"] = 2
        baseline["samples"].insert(2, split_live)
        self.assertTrue(COMPARISON.compare_reports(baseline, baseline)["matched_settings"]["live_split_sample"])
        with self.assertRaises(ValueError):
            COMPARISON.compare_reports(baseline, candidate)

    def test_memory_rejects_partial_or_inconsistent_evidence(self):
        """Reject incomplete phases, process changes, counter regressions and incompatible debug reports."""
        mutations = [
            (("status",), "failed"), (("shutdown_forced",), True),
            (("build_profile",), "debug"), (("host", "machine"), ""),
            (("workload", "split_close_cycles"), 45.0),
            (("samples", 2, "phase"), "redraw"),
            (("samples", 2, "iteration"), True),
            (("samples", 2, "elapsed_seconds"), float("nan")),
            (("samples", 2, "elapsed_seconds"), 0),
            (("samples", 2, "snapshot", "pid"), 1),
            (("samples", 2, "snapshot", "build_profile"), "debug"),
            (("samples", 2, "snapshot", "terminals", "registered"), 1),
            (("samples", 2, "snapshot", "resources", "rss_kib"), True),
            (("samples", 2, "snapshot", "resources", "cpu_user_us"), 0),
        ]
        for path, value in mutations:
            report = self.memory_fixture()
            target = report
            for key in path[:-1]:
                target = target[key]
            target[path[-1]] = value
            with self.subTest(path=path), self.assertRaises(ValueError):
                COMPARISON.compare_reports(report, report)
        report = self.memory_fixture()
        report["samples"] = report["samples"][:20]
        with self.assertRaises(ValueError):
            COMPARISON.compare_reports(report, report)
        debug = COMPARISON.load_report(ROOT / "Docs/Benchmarks/e08b976f/memory-churn.json")
        with self.assertRaises(ValueError):
            COMPARISON.compare_reports(debug, debug)
        with self.assertRaises(ValueError):
            COMPARISON.compare_reports(self.baseline, self.memory_fixture())

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
