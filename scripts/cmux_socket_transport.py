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


def read_response(connection: socket.socket, buffer: bytearray, timeout: float,
                  *, multiline: bool = False, limit: int = 4 * 1024 * 1024) -> str:
    """Read bounded bytes before decoding UTF-8, preserving coalesced v2 response lines.

    V1 multiline replies end after 100 ms idle following a newline or at EOF.
    V2 requires a newline. Both modes enforce one total monotonic read budget.
    The caller must discard the connection after a framing error or timeout.
    """
    import select

    if not math.isfinite(timeout) or timeout <= 0 or limit <= 0:
        raise ValueError("Response timeout and byte limit must be positive")
    deadline = time.monotonic() + timeout
    original_timeout = connection.gettimeout()
    try:
        while True:
            newline = buffer.find(b"\n")
            if not multiline and newline >= 0:
                if newline > limit:
                    raise ValueError("Response exceeds byte limit")
                line = bytes(buffer[:newline])
                del buffer[:newline + 1]
                return line.decode("utf-8")
            if len(buffer) > limit:
                raise ValueError("Response exceeds byte limit")
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("Timed out waiting for response")
            idle = multiline and newline >= 0
            ready, _, _ = select.select([connection], [], [], min(0.1, remaining) if idle else remaining)
            if not ready:
                if idle and time.monotonic() < deadline:
                    result = bytes(buffer).removesuffix(b"\n").decode("utf-8")
                    buffer.clear()
                    return result
                raise TimeoutError("Timed out waiting for response")
            connection.settimeout(max(0.001, deadline - time.monotonic()))
            chunk = connection.recv(min(8192, limit + 1 - len(buffer)))
            if not chunk:
                if not multiline:
                    raise ConnectionError("Socket closed before response newline")
                result = bytes(buffer).removesuffix(b"\n").decode("utf-8")
                buffer.clear()
                return result
            buffer.extend(chunk)
    finally:
        connection.settimeout(original_timeout)
