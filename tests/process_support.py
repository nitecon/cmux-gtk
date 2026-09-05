"""Shared elapsed-time polling and owned subprocess cleanup for integration fixtures."""
from pathlib import Path
import subprocess
import time


def stop_process(process, timeout=10):
    """Terminate and reap an owned child; return True if forced killing was necessary.

    Already-exited children are reaped without signalling. This stops the direct
    child only; fixtures retain responsibility for privileged servers and process groups.
    """
    if process is None:
        return False
    if process.poll() is not None:
        process.wait()
        return False
    try:
        process.terminate()
    except ProcessLookupError:
        pass
    try:
        process.wait(timeout=timeout)
        return False
    except subprocess.TimeoutExpired:
        try:
            process.kill()
        except ProcessLookupError:
            pass
        process.wait(timeout=5)
        return True


def wait_until(predicate, description="condition", timeout=10, interval=0.1):
    """Poll until truthy or raise with context when the monotonic deadline expires.

    Predicate exceptions propagate immediately. A running predicate cannot be
    interrupted: subprocesses and I/O inside it need their own deadlines.
    """
    if timeout <= 0 or interval <= 0:
        raise ValueError("polling timeout and interval must be positive")
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            break
        time.sleep(min(interval, remaining))
    raise AssertionError(f"timed out waiting for {description}")


def linux_process_belongs_to(pid, roots):
    """Check a live Linux ancestry chain against fixture-owned root PID strings.

    Shell launchers may add a process between Ghostty and the interactive shell.
    Read at most 64 ancestors, reject malformed identities, and tolerate exits.
    This is a short-lived test observation, not a PID-reuse-safe authorization check.
    """
    pid = str(pid)
    seen = set()
    for _ in range(64):
        if not pid.isdecimal() or pid == "0" or pid in seen:
            return False
        seen.add(pid)
        try:
            status = Path(f"/proc/{pid}/status").read_text()
        except FileNotFoundError:
            return False
        if pid in roots:
            return True
        parent = next((line.split()[1] for line in status.splitlines()
                       if line.startswith("PPid:")), None)
        if parent is None:
            return False
        pid = parent
    return False


def linux_child_pids(pid):
    """Snapshot direct child PID strings across every spawning thread on Linux.

    The snapshot is not atomic. Ignore threads that exit during enumeration;
    propagate other read errors so permission failures cannot look like cleanup.
    """
    result = set()
    for path in Path(f"/proc/{pid}/task").glob("*/children"):
        try:
            result.update(path.read_text().split())
        except FileNotFoundError:
            pass
    return result
