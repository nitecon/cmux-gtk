"""Bounded PTY capture for shell prompt regression probes."""
import errno
import os
import pty
import select
import subprocess
import time

from process_support import stop_process


def capture_prompt_session(command, environment, cwd=None, *,
                           prompt_delay=1.2, exit_delay=2.8, duration=4.5):
    """Capture at most one MiB while sending an empty command followed by exit.

    Delays use a monotonic clock. The caller supplies an interactive shell
    command and isolated environment. Normal EOF must be followed by a zero
    exit within five seconds; every failure closes both PTY descriptors and
    terminates/reaps the direct child. Descendants remain the caller's concern.
    """
    master, slave = pty.openpty()
    process = None
    try:
        try:
            process = subprocess.Popen(command, cwd=cwd, stdin=slave, stdout=slave,
                                       stderr=slave, env=environment, close_fds=True)
        finally:
            os.close(slave)
        output = bytearray()
        started = time.monotonic()
        phase = 0
        while time.monotonic() - started < duration:
            remaining = duration - (time.monotonic() - started)
            readable, _, _ = select.select([master], [], [], max(0, min(0.2, remaining)))
            if readable:
                try:
                    chunk = os.read(master, 4096)
                except OSError as error:
                    if error.errno == errno.EIO:
                        break  # Linux PTY EOF after its slave closes.
                    raise
                if not chunk:
                    break
                if len(output) + len(chunk) > 1024 * 1024:
                    raise ValueError("prompt capture exceeds one MiB")
                output.extend(chunk)
            elapsed = time.monotonic() - started
            if phase == 0 and elapsed > prompt_delay:
                os.write(master, b"\n")
                phase = 1
            elif phase == 1 and elapsed > exit_delay:
                os.write(master, b"exit\n")
                phase = 2
        code = process.wait(timeout=5)
        if code != 0:
            raise subprocess.CalledProcessError(code, command)
        return bytes(output)
    finally:
        try:
            stop_process(process, timeout=1)
        finally:
            os.close(master)
