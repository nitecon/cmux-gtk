#!/usr/bin/env python3
"""Exercise bounded fixture cleanup with real child processes in CI."""
import select
import signal
import subprocess
import sys
import unittest

from process_support import stop_process


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


if __name__ == "__main__":
    unittest.main()
