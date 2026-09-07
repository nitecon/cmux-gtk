#!/usr/bin/env python3
"""Exercise Codex's documented lifecycle hook schema against live GTK state."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

from linux_app import running_app


def main():
    """Require idempotent install, native resume identity and exact notification routing."""
    with tempfile.TemporaryDirectory(prefix="cmux-codex-hooks-") as directory:
        root = Path(directory)
        binary_dir = root / "bin"
        binary_dir.mkdir()
        provider = binary_dir / "codex"
        provider.write_text("#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$HOOK_ARGV_OUT\"\n")
        provider.chmod(0o700)
        config = root / "codex home"
        config.mkdir()
        hooks_path = config / "hooks.json"
        hooks_path.write_text(json.dumps({"description": "user hooks", "hooks": {
            "SessionStart": [{"hooks": [{"type": "command", "command": "true"}]}]
        }}))
        env = dict(os.environ, PATH=str(binary_dir) + os.pathsep + os.environ["PATH"], CODEX_HOME=str(config))
        executable = str(Path("target/debug/cmux").resolve())
        subprocess.run([executable, "hooks", "setup", "codex"], env=env, check=True, timeout=10)
        first = hooks_path.read_bytes()
        subprocess.run([executable, "hooks", "setup", "codex"], env=env, check=True, timeout=10)
        assert hooks_path.read_bytes() == first
        hooks = json.loads(first)
        assert hooks["description"] == "user hooks"
        assert hooks["hooks"]["SessionStart"][0]["hooks"][0]["command"] == "true"
        commands = {name: entries[-1]["hooks"][0]["command"] for name, entries in hooks["hooks"].items()}

        with running_app(root, {"PATH": env["PATH"], "CODEX_HOME": str(config)}) as app:
            initial = next(row for row in app.surfaces() if row["active"])
            app.cli("new-workspace", "--name", "codex-hook-target")
            target = next(row["uuid"] for row in app.surfaces() if row["active"])
            app.cli("select-workspace", initial["workspace_uuid"])
            hook_env = dict(app.environment, CMUX_SURFACE_ID=target, CMUX_SOCKET=str(app.socket_path))
            native_id = "codex-session-id"

            def event(name, **extra):
                payload = {"hook_event_name": name, "session_id": native_id, "cwd": str(root), **extra}
                result = subprocess.run(["/bin/sh", "-c", commands[name]], env=hook_env,
                                        input=json.dumps(payload), text=True, capture_output=True, timeout=10)
                assert result.returncode == 0, result.stderr

            event("SessionStart")
            event("UserPromptSubmit")
            binding = json.loads(app.cli("surface", "resume", "show", "--surface", target, "--json"))["resume_binding"]
            assert binding["kind"] == "codex" and binding["checkpoint_id"] == native_id
            assert binding["environment"]["CODEX_HOME"] == str(config)
            argv_output = root / "codex-argv"
            subprocess.run(["/bin/sh", "-c", binding["command"]], check=True, timeout=10,
                           env=dict(hook_env, HOOK_ARGV_OUT=str(argv_output)))
            assert argv_output.read_text().splitlines() == ["resume", native_id]
            event("Stop", summary="Review complete")
            rows = json.loads(app.cli("notifications", "list", "--json"))["notifications"]
            assert len(rows) == 1 and rows[0]["surface_id"] == target
            assert rows[0]["title"] == "Codex response ready" and rows[0]["body"] == "Review complete"
            event("SessionEnd")
            assert json.loads(app.cli("surface", "resume", "show", "--surface", target, "--json"))["resume_binding"] is None
    print("installed Codex hooks preserved configuration and routed native lifecycle state")


if __name__ == "__main__":
    main()
