"""Shared Linux socket discovery for repository Python clients and probes."""

import os
from pathlib import Path


def default_socket_path() -> str:
    """Honor explicit overrides, then XDG and debug candidates without opening a connection.

    Match the Rust CLI's precedence. If nothing exists yet, return the XDG path
    so clients can wait for application startup rather than guessing a macOS path.
    """
    override = os.environ.get("CMUX_SOCKET") or os.environ.get("CMUX_SOCKET_PATH")
    if override:
        return override
    runtime = Path(os.environ.get("XDG_RUNTIME_DIR") or f"/run/user/{os.getuid()}")
    if not runtime.is_absolute():
        runtime = Path(f"/run/user/{os.getuid()}")
    directory = runtime / "cmux"
    default = directory / "cmux.sock"
    if default.exists():
        return str(default)
    try:
        # A marker contains one path, not arbitrary session data; bound the read.
        with (directory / "last-socket-path").open(encoding="utf-8") as marker:
            target = marker.read(4097)
        if len(target) <= 4096 and target.strip() and Path(target.strip()).exists():
            return target.strip()
    except (OSError, UnicodeError, ValueError):
        pass
    debug = Path("/tmp/cmux-debug.sock")
    if debug.exists():
        return str(debug)
    candidates = []
    for path in Path("/tmp").glob("cmux-debug-*.sock"):
        try:
            candidates.append((path.stat().st_mtime_ns, str(path)))
        except OSError:
            # Another process may remove a socket between enumeration and stat.
            continue
    return max(candidates)[1] if candidates else str(default)
