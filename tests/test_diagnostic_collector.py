#!/usr/bin/env python3
"""Verify interval CPU accounting without sampling a local application."""
import importlib.util
import json
import os
from pathlib import Path
import unittest
import tempfile
from copy import deepcopy

SPEC = importlib.util.spec_from_file_location("cmux_collector", Path(__file__).resolve().parents[1] / "scripts/collect-cmux-diagnostics.py")
COLLECTOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(COLLECTOR)


def record(elapsed, user, system, pid=42):
    """Construct a resource sample with explicit process identity and observation time."""
    return {"elapsed_seconds": elapsed, "snapshot": {"pid": pid, "resources": {
        "cpu_user_us": user, "cpu_system_us": system}}}


class CpuAccounting(unittest.TestCase):
    """Preserve unknown measurements and support CPU use above one core."""

    def test_interval_accounting(self):
        """Sum user/kernel deltas and normalize to one CPU without clamping multithreaded usage."""
        before = record(1, 1_000_000, 500_000)
        self.assertEqual(COLLECTOR.cpu_percent(before, record(3, 1_500_000, 1_000_000)), 50)
        self.assertEqual(COLLECTOR.cpu_percent(before, record(2, 3_000_000, 500_000)), 200)
        self.assertEqual(COLLECTOR.cpu_percent(before, record(2, 1_000_000, 500_000)), 0)

    def test_unavailable_intervals(self):
        """Failed samples, replacement processes, invalid time and regressing counters are unknown."""
        before = record(1, 100, 100)
        for after in [{"error": "command_failed"}, record(2, None, 200), record(2, 200, 200, pid=43),
                      record(1, 200, 200), record(float("nan"), 200, 200), record(2, 99, 300)]:
            with self.subTest(after=after):
                self.assertIsNone(COLLECTOR.cpu_percent(before, after))

    def test_log_tails_filter_and_bound_records(self):
        """Retain valid matching envelopes, report incomplete/corrupt lines and cap tail retention."""
        envelope = {"schema": 1, "pid": 42, "event": "rpc.complete", "fields": {"trace_id": "fixture"}}
        line = json.dumps(envelope).encode() + b"\n"
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "events.jsonl"
            other = json.dumps({**envelope, "pid": 43}).encode() + b"\n"
            path.write_bytes(line + other + b"not-json\n" + b"{unfinished")
            result = COLLECTOR.log_tail(path, {42})
            self.assertEqual(result["status"], "collected")
            self.assertEqual(result["records"], [envelope])
            self.assertEqual(result["other_process"], 1)
            self.assertEqual(result["discarded"], 2)
            path.write_bytes(b"x" * COLLECTOR.LOG_TAIL_BYTES + b"\n" + line)
            result = COLLECTOR.log_tail(path, {42})
            self.assertTrue(result["truncated"])
            self.assertEqual(result["records"], [envelope])
            path.write_bytes(line * (COLLECTOR.LOG_RECORD_COUNT + 1))
            result = COLLECTOR.log_tail(path, {42})
            self.assertEqual(len(result["records"]), COLLECTOR.LOG_RECORD_COUNT)
            self.assertTrue(result["truncated"])
            path.write_bytes(json.dumps({**envelope, "fields": {"value": float("nan")}}).encode() + b"\n")
            self.assertEqual(COLLECTOR.log_tail(path, {42})["discarded"], 1)

    def test_log_collection_handles_rotation_and_unsafe_file_types(self):
        """Missing backups are normal; missing active logs, symlinks and FIFOs produce bounded failures."""
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "events.jsonl"
            report = {"samples": [record(1, 0, 0)]}
            self.assertEqual(COLLECTOR.collect_logs(path, report)["status"], "failed")
            path.write_text("")
            result = COLLECTOR.collect_logs(path, report)
            self.assertEqual(result["status"], "collected")
            self.assertEqual(result["previous"]["status"], "missing")
            self.assertEqual(COLLECTOR.collect_logs(path, {"samples": []})["error_kind"], "no_process_identity")
            backup = Path(str(path) + ".1")
            backup.write_text('{"schema":1,"pid":42,"event":"fixture","fields":{}}\n')
            self.assertEqual(len(COLLECTOR.collect_logs(path, report)["previous"]["records"]), 1)
            path.unlink()
            path.symlink_to(backup)
            self.assertEqual(COLLECTOR.log_tail(path, {42})["status"], "failed")
            path.unlink()
            os.mkfifo(path)
            self.assertEqual(COLLECTOR.log_tail(path, {42})["status"], "failed")

    def test_idle_evidence(self):
        """Keep raw measurements while rejecting debug builds, churn, failed samples and counter resets."""
        samples = [record(1, 100, 100), record(3, 200, 200)]
        for sample in samples:
            sample["snapshot"].update(build_profile="release", terminals={"registered": 2})
        base = {"samples": samples, "requested_samples": 2}
        report = COLLECTOR.idle_evidence(deepcopy(base), 10, "revision")
        self.assertEqual(report["status"], "passed")
        self.assertEqual(report["observed_seconds"], 2)
        self.assertAlmostEqual(report["cpu_percent"], 0.01)
        self.assertEqual(report["samples"], samples)
        for field, value in [("build_profile", "debug"), ("pid", 43),
                             ("terminals", {"registered": 3}), ("terminals", {"registered": True}),
                             ("resources", {"cpu_user_us": 0, "cpu_system_us": 200})]:
            invalid = deepcopy(base)
            invalid["samples"][-1]["snapshot"][field] = value
            with self.subTest(field=field, value=value):
                result = COLLECTOR.idle_evidence(invalid, 10, "revision")
                self.assertEqual(result["status"], "failed")
                self.assertEqual(len(result["samples"]), 2)
                self.assertEqual(result["failure"]["phase"], "idle_validation")
        for series in [samples[:1], [samples[0], {"error": "command_failed"}]]:
            self.assertEqual(COLLECTOR.idle_evidence(
                {"samples": series, "requested_samples": 2}, 10, "revision")["status"], "failed")


if __name__ == "__main__":
    unittest.main()
