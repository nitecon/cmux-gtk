#!/usr/bin/env python3
import json
import os
import socket
import subprocess
import sys
import time
from pathlib import Path


def socket_dir():
    return Path(os.environ["AGENT_BROWSER_SOCKET_DIR"])


def run_daemon():
    root = socket_dir()
    root.mkdir(parents=True, exist_ok=True)
    socket_path = root / "cmux.sock"
    socket_path.unlink(missing_ok=True)
    server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    server.bind(socket_path)
    server.listen()
    (root / "cmux.stream").write_text("9\n", encoding="utf-8")
    (root / "mock.pid").write_text(f"{os.getpid()}\n", encoding="utf-8")
    while True:
        connection, _ = server.accept()
        with connection:
            request = connection.makefile("rb").readline()
            if request:
                connection.sendall(b'{"success":true,"data":{}}\n')


def ensure_daemon():
    path = socket_dir() / "cmux.sock"
    try:
        probe = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        probe.connect(str(path))
        probe.close()
        return
    except OSError:
        pass
    subprocess.Popen(
        [sys.executable, __file__, "--mock-daemon"],
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
    if "--mock-daemon" in sys.argv:
        run_daemon()
    else:
        ensure_daemon()
        print(json.dumps({"success": True, "data": {}}))
