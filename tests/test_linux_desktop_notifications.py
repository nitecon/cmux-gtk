#!/usr/bin/env python3
"""Exercise the desktop-helper action contract against real GTK routing, without a desktop daemon."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

from linux_app import running_app
from process_support import stop_process
from test_linux_resume_approval import key, window

HELPER = r'''#!/usr/bin/env python3
import json
import os
from pathlib import Path
import sys
import time
root = Path(os.environ["CMUX_DESKTOP_FIXTURE"])
(root / "arguments").write_text(json.dumps(sys.argv[1:]))
for _ in range(1000):
    if (root / "click").exists():
        print("default", flush=True)
        break
    time.sleep(0.01)
'''


def main():
    """Keep delivery unfocused, then open its exact sibling or reject a dismissed message's action."""
    with tempfile.TemporaryDirectory(prefix="cmux-desktop-") as directory:
        root = Path(directory)
        executable = root / "notify-send"
        executable.write_text(HELPER)
        executable.chmod(0o700)
        environment = {"PATH": str(root) + os.pathsep + os.environ["PATH"],
                       "CMUX_DESKTOP_FIXTURE": str(root), "CMUX_LOG": str(root / "events.jsonl")}
        wm = subprocess.Popen(["openbox"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            with running_app(root, environment) as app:
                app.wait_for(lambda: bool(app.surfaces()), "initial terminal")
                initial = next(row for row in app.surfaces() if row["active"])
                app.cli("new-workspace", "--name", "desktop target")
                key(window(app), "ctrl+t")
                app.wait_for(lambda: len(app.surfaces()) == 3, "sibling terminal")
                target = next(row for row in app.surfaces() if row["active"])
                app.cli("select-workspace", initial["workspace_uuid"])

                def active():
                    """Observe exact focus without changing it."""
                    return next(row["uuid"] for row in app.surfaces() if row["active"])

                def messages():
                    """Read stable inbox identity and read state."""
                    return json.loads(app.cli("notifications", "list", "--json"))["notifications"]

                for dismissed in (False, True):
                    result = json.loads(app.cli("notify", "--surface", target["uuid"],
                                                "--title=--literal title", "--subtitle", "Review & wait",
                                                "--body", "<text> $HOME", "--json"))
                    app.wait_for(lambda: (root / "arguments").exists() and (root / "arguments").stat().st_size,
                                 "desktop command")
                    arguments = json.loads((root / "arguments").read_text())
                    assert arguments[-3:] == ["--", "--literal title", "Review &amp; wait\n&lt;text&gt; $HOME"]
                    assert "--action=default=Open terminal" in arguments
                    assert active() == initial["uuid"]
                    if dismissed:
                        app.cli("notifications", "dismiss", "--id", result["id"])
                    (root / "click").touch()
                    if dismissed:
                        app.wait_for(lambda: "notification.desktop_action outcome=stale_target" in
                                     (root / "events.jsonl").read_text(), "stale click rejected")
                        assert active() == initial["uuid"]
                    else:
                        app.wait_for(lambda: active() == target["uuid"], "desktop click exact sibling")
                        assert next(row for row in messages() if row["id"] == result["id"])["is_read"]
                    (root / "click").unlink()
                    (root / "arguments").unlink()
                    app.cli("select-workspace", initial["workspace_uuid"])
        finally:
            stop_process(wm)
    print("desktop payload escaping, exact sibling actions and stale action rejection passed")


if __name__ == "__main__":
    main()
