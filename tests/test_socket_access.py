#!/usr/bin/env python3
"""Exercise Linux socket filesystem protection and kernel peer authentication with real users."""
from pathlib import Path
import stat
import subprocess
import tempfile

from linux_app import running_app


FOREIGN_PROBE = r'''
import socket
import sys

path, phase = sys.argv[1:]
with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
    client.settimeout(5)
    try:
        client.connect(path)
    except PermissionError:
        if phase != "filesystem":
            raise
        print("filesystem denied")
        raise SystemExit(0)
    if phase == "filesystem":
        raise AssertionError("foreign user bypassed private socket permissions")
    try:
        client.sendall(b'{"id":1,"method":"system.ping"}\n')
        response = client.recv(1)
    except (ConnectionResetError, BrokenPipeError):
        response = b""
    if response:
        raise AssertionError("foreign peer received a protocol response")
    print("peer rejected")
'''


def foreign_probe(socket_path, phase):
    """Run a bounded unprivileged client as nobody; sudo is required in Linux CI."""
    return subprocess.check_output(
        ["sudo", "-n", "-u", "nobody", "--", "python3", "-c", FOREIGN_PROBE,
         str(socket_path), phase], text=True, timeout=10,
    ).strip()


def main():
    """Verify independent same-user access and foreign rejection in an isolated application.

    Temporarily broaden only fixture-owned path permissions to exercise peer UID
    authentication independently of filesystem denial, restoring every mode afterward.
    """
    with tempfile.TemporaryDirectory(prefix="cmux-socket-access-") as directory:
        root = Path(directory)
        with running_app(root) as app:
            # This CLI is a sibling of cmux-app, not a descendant of its terminal.
            app.cli("ping")
            assert foreign_probe(app.socket_path, "filesystem") == "filesystem denied"
            modes = [(path, stat.S_IMODE(path.stat().st_mode)) for path in
                     (root, root / "runtime", app.socket_path.parent, app.socket_path)]
            try:
                for path, _ in modes:
                    path.chmod(0o666 if path == app.socket_path else 0o755)
                for _ in range(3):
                    assert foreign_probe(app.socket_path, "peer") == "peer rejected"
                    app.cli("ping")
            finally:
                for path, mode in reversed(modes):
                    path.chmod(mode)
            assert foreign_probe(app.socket_path, "filesystem") == "filesystem denied"
            app.cli("ping")
    print("Linux control socket accepts same-user clients and rejects foreign peers")


if __name__ == "__main__":
    main()
