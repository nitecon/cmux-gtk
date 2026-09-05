#!/usr/bin/env python3
"""Exercise bounded fixture cleanup with real child processes in CI."""
from concurrent.futures import ThreadPoolExecutor
import os
import select
import signal
import subprocess
import sys
import unittest
from unittest.mock import patch

from process_support import linux_child_pids, linux_process_belongs_to, stop_process, wait_until
from pty_support import capture_prompt_session


class PromptCapture(unittest.TestCase):
    """Exercise real PTY output and child cleanup without requiring zsh or GTK."""

    def test_output_and_nonzero_exit(self):
        """Capture shell bytes and expose a failed child exit rather than accepting partial output."""
        output = capture_prompt_session([sys.executable, "-c", "print('prompt-marker')"], os.environ)
        self.assertIn(b"prompt-marker", output)
        with self.assertRaises(subprocess.CalledProcessError) as failure:
            capture_prompt_session([sys.executable, "-c", "raise SystemExit(7)"], os.environ)
        self.assertEqual(failure.exception.returncode, 7)

    def test_oversized_output_reaps_child(self):
        """A noisy live process exceeds the byte cap and is reaped on the error path."""
        children = []
        launch = subprocess.Popen

        def record_child(*args, **kwargs):
            """Launch a real child and retain its handle for cleanup assertions."""
            child = launch(*args, **kwargs)
            children.append(child)
            return child

        with patch("pty_support.subprocess.Popen", side_effect=record_child):
            with self.assertRaisesRegex(ValueError, "one MiB"):
                capture_prompt_session([sys.executable, "-c",
                    "import os,time; os.write(1,b'x'*2097152); time.sleep(60)"], os.environ)
        self.assertEqual(len(children), 1)
        self.assertIsNotNone(children[0].poll())

    def test_failed_launch_closes_descriptors(self):
        """A missing executable releases both PTY descriptors before propagating launch failure."""
        import pty
        master, slave = pty.openpty()
        with patch("pty_support.pty.openpty", return_value=(master, slave)):
            with self.assertRaises(FileNotFoundError):
                capture_prompt_session(["/missing/cmux-prompt-fixture"], os.environ)
        for descriptor in (master, slave):
            with self.assertRaises(OSError):
                os.fstat(descriptor)


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


class LinuxProcessOwnership(unittest.TestCase):
    """Verify root matching and ancestry against real live processes in Linux CI."""

    def test_worker_spawned_child(self):
        """Process snapshots include children spawned by a live worker, then lose reaped children."""
        with ThreadPoolExecutor(max_workers=1) as worker:
            child = worker.submit(subprocess.Popen,
                [sys.executable, "-c", "import time; time.sleep(60)"]).result(timeout=5)
            try:
                self.assertIn(str(child.pid), linux_child_pids(os.getpid()))
            finally:
                stop_process(child)
            self.assertNotIn(str(child.pid), linux_child_pids(os.getpid()))

    def test_child_ancestry(self):
        """A launched child belongs to its parent tree, but not an unrelated root."""
        child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"])
        try:
            self.assertTrue(linux_process_belongs_to(child.pid, {str(child.pid)}))
            self.assertTrue(linux_process_belongs_to(child.pid, {str(os.getpid())}))
            self.assertTrue(linux_process_belongs_to(child.pid, {str(os.getppid())}))
            self.assertFalse(linux_process_belongs_to(child.pid, {"0"}))
            self.assertFalse(linux_process_belongs_to("not-a-pid", {str(child.pid)}))
        finally:
            stop_process(child)
        self.assertFalse(linux_process_belongs_to(child.pid, {str(os.getpid())}))


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
