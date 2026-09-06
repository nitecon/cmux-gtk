#!/usr/bin/env python3
"""Attribute a real terminal child listener and clear it after exit without selecting its workspace."""
import json
from pathlib import Path
import shlex
import socket
import tempfile

from linux_app import running_app


def main():
    """Distinguish terminal-owned listeners from an unrelated fixture listener on the same machine."""
    with tempfile.TemporaryDirectory(prefix="cmux-ports-") as directory, socket.socket() as unrelated:
        root = Path(directory)
        unrelated.bind(("127.0.0.1", 0))
        unrelated.listen()
        foreign_port = unrelated.getsockname()[1]
        script = root / "listener.py"
        script.write_text("import socket,time,pathlib,os\n"
                          "root=pathlib.Path(__file__).parent\n"
                          "s=socket.socket(); s.bind(('127.0.0.1',0)); s.listen()\n"
                          "(root/'port').write_text(str(s.getsockname()[1]))\n"
                          "(root/'pid').write_text(str(os.getpid()))\n"
                          "while not (root/'stop').exists(): time.sleep(0.05)\n"
                          "s.close()\n")
        with running_app(root) as app:
            target = next(row for row in app.surfaces() if row["active"])
            app.wait_for(lambda: bool(app.children()), "terminal ready")
            app.cli("send-text", "--id", target["uuid"], "python3 " + shlex.quote(str(script)))
            app.cli("send-key", "--id", target["uuid"], "\r")
            app.wait_for(lambda: (root / "pid").exists(), "owned server ready")
            port = int((root / "port").read_text())
            pid = int((root / "pid").read_text())
            app.cli("new-workspace", "--name", "observer")
            selected = next(row["uuid"] for row in app.surfaces() if row["active"])

            def ports():
                """Inspect current workspace metadata through the production CLI."""
                workspaces = json.loads(app.cli("list-workspaces", "--json"))["workspaces"]
                return next(row["ports"] for row in workspaces if row["uuid"] == target["workspace_uuid"])

            app.wait_for(lambda: ports() and any(row["port"] == port for row in ports()), "attributed listener", timeout=20)
            rows = ports()
            assert {"surface_uuid": target["uuid"], "address": "127.0.0.1", "port": port,
                    "pid": pid, "provenance": "local"} in rows, rows
            assert not any(row["port"] == foreign_port for row in rows)
            assert next(row["uuid"] for row in app.surfaces() if row["active"]) == selected
            (root / "stop").touch()
            app.wait_for(lambda: ports() is not None and not any(row["port"] == port for row in ports()),
                         "listener removed after exit", timeout=20)
            assert next(row["uuid"] for row in app.surfaces() if row["active"]) == selected
    print("local listener attribution, unrelated-process exclusion and exit cleanup passed")


if __name__ == "__main__":
    main()
