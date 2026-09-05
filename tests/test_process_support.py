#!/usr/bin/env python3
"""Exercise bounded fixture cleanup with real child processes in CI."""
import select
import signal
import subprocess
import sys
import unittest
from unittest.mock import patch

from process_support import stop_process, wait_until


class ProcessCleanup(unittest.TestCase):
    """Verify child ownership for normal and signal-resistant shutdown."""

    def test_forced_cleanup(self):
        """A child ignoring TERM is killed and reaped after the grace period."""
        child = subprocess.Popen([sys.executable, "-c",
            "import signal; signal.signal(signal.SIGTERM, signal.SIG_IGN); print('ready', flush=True); signal.pause()"],
            stdout=subprocess.PIPE, text=True)
        try:
            self.assertTrue(select.select([child.stdout], [], [], 5)[0], "child did not become ready")
            self.assertEqual(child.stdout.readline().strip(), "ready")
            self.assertTrue(stop_process(child, timeout=0.03))
            self.assertEqual(child.returncode, -signal.SIGKILL)
        finally:
            if child.poll() is None:
                child.kill()
            child.wait(timeout=5)
            child.stdout.close()

    def test_exited_child(self):
        """An already-exited child needs no forced cleanup and retains its exit code."""
        child = subprocess.Popen([sys.executable, "-c", "raise SystemExit(7)"])
        child.wait(timeout=5)
        self.assertFalse(stop_process(child))
        self.assertEqual(child.returncode, 7)
        self.assertFalse(stop_process(None))


class Polling(unittest.TestCase):
    """Verify polling budgets include predicate work and failures remain diagnostic."""

    def test_slow_predicate_consumes_deadline(self):
        """An unsuccessful check exhausting the budget gets no extra retry or sleep."""
        with patch("process_support.time.monotonic", side_effect=[0, 0, 2]), \
                patch("process_support.time.sleep") as sleep:
            with self.assertRaisesRegex(AssertionError, "waiting for terminal exit"):
                wait_until(lambda: False, "terminal exit", timeout=1)
            sleep.assert_not_called()

    def test_success_after_retry(self):
        """A condition may converge on a later attempt before the deadline."""
        results = iter([False, True])
        with patch("process_support.time.monotonic", side_effect=[0, 0, 0.95, 0.99]), \
                patch("process_support.time.sleep") as sleep:
            wait_until(lambda: next(results), timeout=1)
            self.assertAlmostEqual(sleep.call_args.args[0], 0.05)

    def test_predicate_failure_propagates(self):
        """Unexpected fixture failures are not hidden by retries."""
        def fail():
            """Represent a failed CLI invocation inside a polling condition."""
            raise RuntimeError("CLI failed")

        with self.assertRaisesRegex(RuntimeError, "CLI failed"):
            wait_until(fail)


if __name__ == "__main__":
    unittest.main()
