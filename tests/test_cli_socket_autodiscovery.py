#!/usr/bin/env python3
"""Verify Linux CLI socket precedence against isolated real Unix endpoints."""
from contextlib import ExitStack
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile

from process_support import stop_process


def assert_endpoint(cli, server, environment, arguments, label):
    """Require a real CLI ping at the selected endpoint and reap the owned process.

    Socket reads, accepts and subprocess completion have separate five-second
    budgets. Only the expected endpoint replies, so incorrect discovery fails.
    """
    process = subprocess.Popen(
        [str(cli), *arguments, "ping", "--json"], env=environment,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
    )
    try:
        peer, _ = server.accept()
        with peer:
            peer.settimeout(5)
            with peer.makefile("rb") as reader:
                line = reader.readline(4097)
            assert line.endswith(b"\n") and len(line) <= 4096, "invalid CLI request frame"
            request = json.loads(line)
            assert request["method"] == "system.ping", request
            result = {"endpoint": label}
            peer.sendall(json.dumps({"id": request["id"], "ok": True, "result": result}).encode() + b"\n")
        stdout, stderr = process.communicate(timeout=5)
        assert process.returncode == 0, (process.returncode, stderr)
        assert json.loads(stdout) == result, stdout
    finally:
        stop_process(process)
        process.stdout.close()
        process.stderr.close()


def main():
    """Verify flag, modern/legacy overrides, XDG socket and marker precedence in order."""
    cli = Path(os.environ.get("CMUX_BIN_DIR", "target/debug")).resolve() / "cmux"
    with tempfile.TemporaryDirectory(prefix="cmux-discovery-") as directory, ExitStack() as stack:
        root = Path(directory)
        runtime = root / "runtime" / "cmux"
        runtime.mkdir(parents=True)
        paths = {name: root / f"{name}.sock" for name in ("flag", "modern", "legacy", "marker")}
        paths["xdg"] = runtime / "cmux.sock"
        servers = {}
        for name, path in paths.items():
            server = stack.enter_context(socket.socket(socket.AF_UNIX, socket.SOCK_STREAM))
            server.bind(str(path))
            server.listen(1)
            server.settimeout(5)
            servers[name] = server
        (runtime / "last-socket-path").write_text(f" {paths['marker']}\n")
        environment = dict(os.environ, XDG_RUNTIME_DIR=str(runtime.parent),
                           CMUX_SOCKET=str(paths["modern"]), CMUX_SOCKET_PATH=str(paths["legacy"]))
        environment.pop("CMUX_TAG", None)
        assert_endpoint(cli, servers["flag"], environment, ["--socket", str(paths["flag"])], "flag")
        assert_endpoint(cli, servers["modern"], environment, [], "modern")
        environment.pop("CMUX_SOCKET")
        assert_endpoint(cli, servers["legacy"], environment, [], "legacy")
        environment.pop("CMUX_SOCKET_PATH")
        assert_endpoint(cli, servers["xdg"], environment, [], "xdg")
        servers["xdg"].close()
        paths["xdg"].unlink()
        assert_endpoint(cli, servers["marker"], environment, [], "marker")
    print("PASS: Linux CLI honors explicit, environment, XDG and marker socket precedence")


if __name__ == "__main__":
    main()
