"""Bounded cleanup for subprocesses directly owned by Linux integration fixtures."""
import subprocess


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
