#!/usr/bin/env python3
"""Exercise the shared nested-JSON hook registry for supported terminal agents."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

from linux_app import running_app


def main():
    """Install every registry entry and require exact native lifecycle routing."""
    with tempfile.TemporaryDirectory(prefix="cmux-json-agent-hooks-") as directory:
        root = Path(directory)
        home = root / "home"
        binary_dir = root / "bin"
        home.mkdir()
        binary_dir.mkdir()
        providers = {
            "grok": ("grok", root / "grok" / "hooks" / "cmux-session.json", "SessionStart", "Stop", None, ["-r"]),
            "gemini": ("gemini", home / ".gemini" / "settings.json", "SessionStart", "AfterAgent", "SessionEnd", ["--resume"]),
            "copilot": ("copilot", root / "copilot" / "config.json", "SessionStart", "Stop", "SessionEnd", ["--resume"]),
            "codebuddy": ("codebuddy", root / "codebuddy" / "settings.json", "SessionStart", "Stop", "SessionEnd", ["--resume"]),
            "factory": ("droid", home / ".factory" / "settings.json", "SessionStart", "Stop", "SessionEnd", ["--resume"]),
            "qoder": ("qodercli", root / "qoder" / "settings.json", "SessionStart", "Stop", "SessionEnd", ["--resume"]),
        }
        for binary, *_ in providers.values():
            path = binary_dir / binary
            path.write_text("#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$HOOK_ARGV_OUT\"\n")
            path.chmod(0o700)
        env = dict(
            os.environ,
            HOME=str(home),
            PATH=str(binary_dir) + os.pathsep + os.environ["PATH"],
            GROK_HOME=str(root / "grok"),
            COPILOT_HOME=str(root / "copilot"),
            CODEBUDDY_CONFIG_DIR=str(root / "codebuddy"),
            QODER_CONFIG_DIR=str(root / "qoder"),
        )
        executable = str(Path("target/debug/cmux").resolve())
        installed = {}
        for name, (_, config_path, *_) in providers.items():
            config_path.parent.mkdir(parents=True, exist_ok=True)
            config_path.write_text(json.dumps({"user": name, "hooks": {
                "SessionStart": [{"hooks": [{"type": "command", "command": "true"}]}]
            }}))
            subprocess.run([executable, "hooks", "setup", name], env=env, check=True, timeout=10)
            first = config_path.read_bytes()
            subprocess.run([executable, "hooks", "setup", name], env=env, check=True, timeout=10)
            assert config_path.read_bytes() == first
            decoded = json.loads(first)
            assert decoded["user"] == name
            assert decoded["hooks"]["SessionStart"][0]["hooks"][0]["command"] == "true"
            installed[name] = decoded["hooks"]

        # The six durable config syncs above can leave a loaded hosted runner briefly I/O-bound.
        with running_app(root, env, startup_timeout=20) as app:
            target = next(row["uuid"] for row in app.surfaces() if row["active"])
            hook_env = dict(app.environment, CMUX_SURFACE_ID=target, CMUX_SOCKET=str(app.socket_path))
            expected_notifications = 0
            for name, (_, _, start_name, stop_name, end_name, resume_prefix) in providers.items():
                commands = installed[name]
                native_id = name + "-session"

                def event(event_name, command_name, **extra):
                    command = commands[event_name][-1]["hooks"][0]["command"]
                    if name == "codebuddy":
                        payload = {"hookEventName": event_name, "session": {"id": native_id},
                                   "context": {"workingDirectory": str(root)}, **extra}
                    else:
                        payload = {"hook_event_name": event_name, "session_id": native_id,
                                   "cwd": str(root), **extra}
                    result = subprocess.run(["/bin/sh", "-c", command], env=hook_env,
                                            input=json.dumps(payload), text=True,
                                            capture_output=True, timeout=10)
                    assert result.returncode == 0, (name, command_name, result.stderr)

                event(start_name, "session-start")
                binding = json.loads(app.cli("surface", "resume", "show", "--surface", target, "--json"))["resume_binding"]
                assert binding["kind"] == name and binding["checkpoint_id"] == native_id
                argv_output = root / (name + "-argv")
                subprocess.run(["/bin/sh", "-c", binding["command"]], env=dict(hook_env, HOOK_ARGV_OUT=str(argv_output)),
                               check=True, timeout=10)
                assert argv_output.read_text().splitlines() == resume_prefix + [native_id]
                event(stop_name, "stop", summary=name + " complete")
                expected_notifications += 1
                rows = json.loads(app.cli("notifications", "list", "--json"))["notifications"]
                assert len(rows) == expected_notifications
                assert rows[-1]["surface_id"] == target and rows[-1]["body"] == name + " complete"
                if end_name:
                    event(end_name, "session-end")
                else:
                    app.cli("surface", "resume", "clear", "--surface", target, "--checkpoint", native_id)
                assert json.loads(app.cli("surface", "resume", "show", "--surface", target, "--json"))["resume_binding"] is None
    print("nested JSON agent hooks preserved config and routed native session lifecycles")


if __name__ == "__main__":
    main()
