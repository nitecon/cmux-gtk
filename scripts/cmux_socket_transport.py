"""Shared connection ownership for the repository's Python protocol clients."""

import errno
import math
import socket
import time


def connect_socket(path: str, startup_timeout: float, io_timeout: float) -> socket.socket:
    """Retry a starting Unix server within one monotonic budget and return an owned socket.

    Set a deadline before connect, close every failed attempt, and retry only
    missing/refused endpoints. The caller owns closing the successful connection.
    """
    if not all(math.isfinite(value) and value > 0 for value in (startup_timeout, io_timeout)):
        raise ValueError("Socket timeouts must be finite and positive")
    deadline = time.monotonic() + startup_timeout
    while True:
        connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        try:
            connection.settimeout(max(0.001, deadline - time.monotonic()))
            connection.connect(path)
            connection.settimeout(io_timeout)
            return connection
        except OSError as error:
            connection.close()
            remaining = deadline - time.monotonic()
            if error.errno not in (errno.ENOENT, errno.ECONNREFUSED) or remaining <= 0:
                raise
            time.sleep(min(0.1, remaining))
        except BaseException:
            connection.close()
            raise
