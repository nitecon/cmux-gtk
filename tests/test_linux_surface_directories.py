#!/usr/bin/env python3
"""Verify per-terminal OSC 7 directories reach session snapshots without cross-routing."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import uuid


def wait_for(check):
    """Wait for native terminal output and debounced persistence within a fixed deadline."""
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        if check():
            return
        time.sleep(0.1)
    raise AssertionError("surface directories did not converge")


def main():
    """Launch two real shells, report distinct changed directories and inspect saved state."""
    with tempfile.TemporaryDirectory(prefix="cmux-directories-") as directory:
        root = Path(directory)
        env = dict(os.environ, CMUX_NO_UPDATE="1", GDK_BACKEND="x11",
                   LIBGL_ALWAYS_SOFTWARE="1", CMUX_LOG=str(root / "events.jsonl"))
        for kind in ("DATA_HOME", "CONFIG_HOME", "STATE_HOME", "RUNTIME_DIR"):
            path = root / kind.lower()
            path.mkdir(mode=0o700)
            env[f"XDG_{kind}"] = str(path)
        socket = root / "runtime_dir/cmux/cmux.sock"
        session_path = root / "data_home/cmux/session.json"
        session_path.parent.mkdir()
        script = root / "startup.sh"
        script.write_text("cd reported\nprintf '\\033]7;file://localhost%s\\007' \"$PWD\"\n"
                          "touch ready\nexec /bin/sh\n")
        workspaces = []
        for name in ("first", "second"):
            launch = root / name
            (launch / "reported").mkdir(parents=True)
            workspaces.append(dict(
                uuid=str(uuid.uuid4()), name=name, working_directory=str(launch),
                startup_script=str(script), active_pane_uuid=None,
                layout=dict(type="Leaf", pane_id=1, surface_uuid=str(uuid.uuid4()),
                            shell="/bin/sh", cwd=""),
            ))
        session_path.write_text(json.dumps(dict(version=3, active_index=0, workspaces=workspaces)))
        binary_dir = Path(os.environ.get("CMUX_BIN_DIR", "target/debug"))

        def cli(*args):
            """Call the real CLI against the isolated application with a bounded timeout."""
            return subprocess.run([str(binary_dir / "cmux"), "--socket", str(socket), *args],
                                  env=env, text=True, capture_output=True, check=True, timeout=10)

        with (root / "app.log").open("w+") as log:
            app = subprocess.Popen([str(binary_dir / "cmux-app")], env=env, stdout=log, stderr=log)
            try:
                wait_for(socket.exists)
                wait_for(lambda: (root / "first/reported/ready").exists())
                cli("select-workspace", workspaces[1]["uuid"])
                wait_for(lambda: (root / "second/reported/ready").exists())

                def saved_directories_match():
                    """Trigger the normal save path after Ghostty has processed each report."""
                    cli("rename-workspace", workspaces[1]["uuid"], "second")
                    saved = json.loads(session_path.read_text())
                    for workspace in saved["workspaces"]:
                        surfaces = workspace["layout"].get("surfaces", [])
                        expected = str(root / workspace["name"] / "reported")
                        if len(surfaces) != 1 or surfaces[0]["cwd"] != expected:
                            return False
                    return len(saved["workspaces"]) == 2

                wait_for(saved_directories_match)
                print("PASS: native directory reports stay isolated across live terminals")
            except BaseException:
                log.flush()
                log.seek(0)
                print(log.read())
                raise
            finally:
                app.terminate()
                app.wait(timeout=10)


if __name__ == "__main__":
    main()
