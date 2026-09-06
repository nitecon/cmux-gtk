"""Shared assertions and bounded Linux subprocesses for retained protocol scenarios."""
import locale
import math
import os
import selectors
import signal
import subprocess
import time
from cmux import cmuxError
from process_support import wait_until as poll_until


def require(condition: object, message: str) -> None:
    """Raise the protocol client's existing error with the supplied detail when a condition is false."""
    if not condition:
        raise cmuxError(message)


def run_command(cmd: list[str], *, env: dict[str, str] | None = None,
                check: bool = True, timeout: float = 300,
                output_limit: int = 1024 * 1024) -> subprocess.CompletedProcess[str]:
    """Capture a Linux command with a total deadline and a byte cap per output pipe.

    The child owns a new process group, killed on timeout, overflow or interrupted
    capture. Both pipes are drained concurrently; cleanup reaps the direct child.
    Nonzero exits return normally only with check=False. Failure messages omit
    arguments and child output, which can contain credentials. Bounds apply even
    when check=False. Docker/SSH mutations already completed are not rolled back.
    """
    if not math.isfinite(timeout) or timeout <= 0 or not isinstance(output_limit, int) or output_limit <= 0:
        raise ValueError("command timeout must be finite and positive; output limit must be a positive integer")
    deadline = time.monotonic() + timeout
    with subprocess.Popen(cmd, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                          start_new_session=True) as process:
        buffers = [bytearray(), bytearray()]
        try:
            with selectors.DefaultSelector() as selector:
                for index, pipe in enumerate((process.stdout, process.stderr)):
                    os.set_blocking(pipe.fileno(), False)
                    selector.register(pipe, selectors.EVENT_READ, index)
                while selector.get_map():
                    remaining = deadline - time.monotonic()
                    if remaining <= 0:
                        raise cmuxError("Command exceeded its deadline")
                    for key, _ in selector.select(min(remaining, 0.1)):
                        try:
                            chunk = os.read(key.fd, 16384)
                        except BlockingIOError:
                            continue
                        if not chunk:
                            selector.unregister(key.fileobj)
                            continue
                        buffer = buffers[key.data]
                        if len(buffer) + len(chunk) > output_limit:
                            raise cmuxError("Command exceeded its output limit")
                        buffer.extend(chunk)
            try:
                process.wait(timeout=max(0, deadline - time.monotonic()))
            except subprocess.TimeoutExpired as error:
                raise cmuxError("Command exceeded its deadline") from error
        except BaseException:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait()
            raise
        finally:
            process.stdout.close()
            process.stderr.close()
        if check and process.returncode:
            raise cmuxError(f"Command failed with exit code {process.returncode}")
        encoding = locale.getpreferredencoding(False)
        output = [bytes(buffer).decode(encoding).replace("\r\n", "\n").replace("\r", "\n")
                  for buffer in buffers]
        return subprocess.CompletedProcess(cmd, process.returncode, *output)


def wait_until(predicate, timeout_s: float = 4.0, interval_s: float = 0.05,
               message: str = "timeout") -> None:
    """Poll on the shared monotonic clock, preserving the scenario's deadline error.

    Predicate errors escape unchanged; callbacks must bound their own blocking work.
    """
    poll_until(predicate, timeout=timeout_s, interval=interval_s,
               deadline_error=cmuxError(message))


def wait_for(pred, timeout_s: float = 8.0, step_s: float = 0.1) -> None:
    """Preserve the remote scenario polling defaults and error through the shared loop."""
    wait_until(pred, timeout_s, step_s, "Timed out waiting for condition")
