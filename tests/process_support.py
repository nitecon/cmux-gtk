"""Shared elapsed-time polling and owned subprocess cleanup for integration fixtures."""
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
