#!/usr/bin/env python3
"""Exercise Cursor's flat hooks.json lifecycle contract against GTK."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

from linux_app import running_app


def main():
    """Require idempotent flat-hook merge, resume binding and pane notification."""
    with tempfile.TemporaryDirectory(prefix="cmux-cursor-hooks-") as directory:
        root = Path(directory)
        home = root / "home"
        binary_dir = root / "bin"
        cursor_dir = home / ".cursor"
        cursor_dir.mkdir(parents=True)
        binary_dir.mkdir()
        provider = binary_dir / "cursor-agent"
        provider.write_text("#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$HOOK_ARGV_OUT\"\n")
        provider.chmod(0o700)
        hooks_path = cursor_dir / "hooks.json"
        hooks_path.write_text(json.dumps({"user": True, "hooks": {
            "beforeSubmitPrompt": [{"command": "true"}]
        }}))
        env = dict(os.environ, HOME=str(home), PATH=str(binary_dir) + os.pathsep + os.environ["PATH"])
        executable = str(Path("target/debug/cmux").resolve())
        subprocess.run([executable, "hooks", "setup", "cursor"], env=env, check=True, timeout=10)
        first = hooks_path.read_bytes()
        subprocess.run([executable, "hooks", "setup", "cursor"], env=env, check=True, timeout=10)
        assert hooks_path.read_bytes() == first
        decoded = json.loads(first)
        assert decoded["version"] == 1 and decoded["user"] is True
        assert decoded["hooks"]["beforeSubmitPrompt"][0]["command"] == "true"
        start = decoded["hooks"]["beforeSubmitPrompt"][-1]["command"]
        stop = decoded["hooks"]["stop"][-1]["command"]

        with running_app(root, env) as app:
            target = next(row["uuid"] for row in app.surfaces() if row["active"])
            hook_env = dict(app.environment, CMUX_SURFACE_ID=target, CMUX_SOCKET=str(app.socket_path))
            native_id = "cursor-conversation"

            def invoke(command, event, **extra):
                payload = {"hook_event_name": event, "conversation_id": native_id,
                           "working_directory": str(root), **extra}
                result = subprocess.run(["/bin/sh", "-c", command], env=hook_env,
                                        input=json.dumps(payload), text=True,
                                        capture_output=True, timeout=10)
                assert result.returncode == 0, result.stderr

            invoke(start, "beforeSubmitPrompt")
            binding = json.loads(app.cli("surface", "resume", "show", "--surface", target, "--json"))["resume_binding"]
            assert binding["kind"] == "cursor" and binding["checkpoint_id"] == native_id
            argv_output = root / "cursor-argv"
            subprocess.run(["/bin/sh", "-c", binding["command"]], env=dict(hook_env, HOOK_ARGV_OUT=str(argv_output)),
                           check=True, timeout=10)
            assert argv_output.read_text().splitlines() == ["--resume", native_id]
            invoke(stop, "stop", message="Cursor finished")
            rows = json.loads(app.cli("notifications", "list", "--json"))["notifications"]
            assert len(rows) == 1 and rows[0]["surface_id"] == target and rows[0]["body"] == "Cursor finished"
    print("Cursor flat hooks preserved configuration and routed native lifecycle state")


if __name__ == "__main__":
    main()
