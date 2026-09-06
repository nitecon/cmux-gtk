#!/usr/bin/env python3
"""Verify caller UUID/TTY precedence, hard workspace scope and workspace-only notification identity."""
import json
from pathlib import Path
import shlex
import subprocess
import tempfile
import uuid

from linux_app import running_app
from process_support import stop_process
from test_linux_resume_approval import key, quit_app, window


def main():
    """Attribute actual PTYs without focus guesses; persist and navigate workspace-only messages."""
    with tempfile.TemporaryDirectory(prefix="cmux-caller-") as directory:
        root = Path(directory)
        wm = subprocess.Popen(["openbox"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            with running_app(root) as app:
                app.wait_for(lambda: bool(app.surfaces()), "initial terminal")
                first = next(row for row in app.surfaces() if row["active"])
                tty_file = root / "caller-tty"
                app.cli("send-text", "--id", first["uuid"], "tty > " + shlex.quote(str(tty_file)))
                app.cli("send-key", "--id", first["uuid"], "\r")
                app.wait_for(lambda: tty_file.exists() and tty_file.stat().st_size, "native caller TTY")
                tty = tty_file.read_text().strip()
                assert tty.startswith("/dev/pts/")
                key(window(app), "ctrl+t")
                app.wait_for(lambda: len(app.surfaces()) == 2, "source inactive sibling")
                app.cli("new-workspace", "--name", "caller background")
                second = next(row for row in app.surfaces() if row["active"])
                key(window(app), "ctrl+t")
                app.wait_for(lambda: len(app.surfaces()) == 4, "background sibling")
                selected = next(row for row in app.surfaces() if row["active"])

                def call(method="notification.create_for_caller", **params):
                    """Exercise production method parsing and GTK attribution."""
                    return json.loads(app.cli("raw", method, "--params", json.dumps(params), "--json"))

                def rows():
                    """Read retained history without selecting a terminal."""
                    return json.loads(app.cli("notifications", "list", "--json"))["notifications"]

                native = call(caller_tty=tty, title="native TTY")
                assert native["surface_id"] == first["uuid"]
                stronger = call(caller_tty=tty, preferred_surface_id=second["uuid"], prefer_tty=True)
                assert stronger["surface_id"] == second["uuid"]
                moved = call(preferred_workspace_id=first["workspace_uuid"], preferred_surface_id=second["uuid"])
                assert moved["workspace_id"] == second["workspace_uuid"]
                scoped = call(preferred_workspace_id=first["workspace_uuid"], preferred_surface_id=second["uuid"],
                              preferred_workspace_is_explicit=True)
                assert scoped["workspace_id"] == first["workspace_uuid"] and scoped["surface_id"] is None
                for params in ({"caller_tty": "/dev/pts/999999"}, {"preferred_surface_id": str(uuid.uuid4())},
                               {"preferred_workspace_id": str(uuid.uuid4()), "preferred_workspace_is_explicit": True,
                                "preferred_surface_id": first["uuid"]}, {"prefer_tty": "yes"},
                               {"caller_tty": "x" * 257}, {"preferred_surface_id": "bad"}):
                    before = rows()
                    try:
                        call(**params)
                    except subprocess.CalledProcessError:
                        pass
                    else:
                        raise AssertionError("invalid caller evidence was accepted")
                    assert rows() == before
                assert next(row["uuid"] for row in app.surfaces() if row["active"]) == selected["uuid"]
                call("notification.clear", caller=True, caller_tty=tty)
                assert all(row["surface_id"] != first["uuid"] for row in rows())
                assert any(row["id"] == scoped["id"] for row in rows()), "surface clear erased workspace-only message"
                ambient = call(title="workspace only")
                assert ambient["surface_id"] is None and ambient["workspace_id"] == second["workspace_uuid"]
                app.cli("select-workspace", first["workspace_uuid"])
                app.cli("notifications", "open", ambient["id"])
                assert next(row["uuid"] for row in app.surfaces() if row["active"]) == selected["uuid"]
                saved = rows()
                quit_app(app)
            with running_app(root) as app:
                assert rows() == saved, "nullable terminal identity did not survive session restore"
        finally:
            stop_process(wm)
    print("caller native TTY/UUID scope, workspace-only history and restoration passed")


if __name__ == "__main__":
    main()
