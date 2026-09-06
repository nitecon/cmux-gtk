#!/usr/bin/env python3
"""Session-aware browser CLI/socket fixture for GTK tab lifecycle scenarios."""
import argparse
import json
import os
import socket
import subprocess
import sys
import time
from pathlib import Path


def socket_dir():
    """Resolve the isolated socket root supplied by the owning shell fixture."""
    return Path(os.environ["AGENT_BROWSER_SOCKET_DIR"])


def run_daemon(session):
    """Serve bounded mock exchanges for one session and publish its stream port and PID."""
    root = socket_dir()
    root.mkdir(parents=True, exist_ok=True)
    socket_path = root / f"{session}.sock"
    socket_path.unlink(missing_ok=True)
    server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    server.bind(str(socket_path))
    server.listen()
    (root / f"{session}.stream").write_text("9\n", encoding="utf-8")
    (root / "mock.pid").write_text(f"{os.getpid()}\n", encoding="utf-8")
    while True:
        connection, _ = server.accept()
        with connection:
            with connection.makefile("rb") as reader:
                request = reader.readline(4 * 1024 * 1024 + 1)
            if request:
                payload = json.loads(request)
                action = payload.get("action")
                if action == "navigate":
                    (root / "last-navigation.json").write_text(json.dumps(payload), encoding="utf-8")
                if action == "navigate" and (root / "pause-navigate").exists():
                    (root / "navigate-waiting").write_text("ready", encoding="utf-8")
                    deadline = time.monotonic() + 10
                    while (root / "pause-navigate").exists():
                        if time.monotonic() >= deadline:
                            raise TimeoutError("fixture navigation was never released")
                        time.sleep(0.01)
                connection.sendall(b'{"success":true,"data":{}}\n')
                if action == "close":
                    server.close()
                    socket_path.unlink(missing_ok=True)
                    (root / f"{session}.stream").unlink(missing_ok=True)
                    (root / "mock.pid").unlink(missing_ok=True)
                    return


def ensure_daemon(session):
    """Reuse the requested session or launch its mock daemon and wait for its socket."""
    path = socket_dir() / f"{session}.sock"
    try:
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as probe:
            probe.settimeout(1)
            probe.connect(str(path))
        return
    except OSError:
        pass
    subprocess.Popen(
        [sys.executable, __file__, "--mock-daemon", "--session", session],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
    )
    for _ in range(100):
        if path.exists():
            return
        time.sleep(0.01)
    raise RuntimeError("mock agent-browser daemon did not start")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--session", default=os.environ.get("AGENT_BROWSER_SESSION", "cmux"))
    parser.add_argument("--mock-daemon", action="store_true")
    args, _ = parser.parse_known_args()
    if args.mock_daemon:
        run_daemon(args.session)
    else:
        ensure_daemon(args.session)
        print(json.dumps({"success": True, "data": {}}))
