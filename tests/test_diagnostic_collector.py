#!/usr/bin/env python3
"""Verify interval CPU accounting without sampling a local application."""
import importlib.util
from pathlib import Path
import unittest

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


if __name__ == "__main__":
    unittest.main()
