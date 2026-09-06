#!/usr/bin/env python3
"""Execute generated Claude hook commands against GTK using the documented native payload contract."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

from linux_app import running_app


def main():
    """Install twice in isolated configuration, execute handlers, and reject stale/malformed events."""
    with tempfile.TemporaryDirectory(prefix="cmux-claude-hooks-") as directory:
        root = Path(directory)
        binary_dir = root / "bin"
        binary_dir.mkdir()
        # Discovery-only provider shim: no external agent session or service is contacted.
        provider = binary_dir / "claude"
        provider.write_text("#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$HOOK_ARGV_OUT\"\n")
        provider.chmod(0o700)
        config = root / "claude settings"
        config.mkdir()
        settings_path = config / "settings.json"
        settings_path.write_text(json.dumps({"model": "user-choice", "hooks": {"SessionStart": [
            {"hooks": [{"type": "command", "command": "true"}]}
        ]}}))
        env = dict(os.environ, PATH=str(binary_dir) + os.pathsep + os.environ["PATH"], CLAUDE_CONFIG_DIR=str(config))
        executable = str(Path("target/debug/cmux").resolve())
        subprocess.run([executable, "hooks", "setup", "claude"], env=env, check=True, timeout=10)
        first = settings_path.read_bytes()
        subprocess.run([executable, "hooks", "setup", "claude"], env=env, check=True, timeout=10)
        assert settings_path.read_bytes() == first
        settings = json.loads(first)
        assert settings["model"] == "user-choice"
        assert settings["hooks"]["SessionStart"][0]["hooks"][0]["command"] == "true"
        commands = {event: entries[-1]["hooks"][0]["command"] for event, entries in settings["hooks"].items()}
        settings_path.write_text("{broken")
        invalid = subprocess.run([executable, "hooks", "setup", "claude"], env=env, capture_output=True, timeout=10)
        assert invalid.returncode != 0 and settings_path.read_text() == "{broken"
        settings_path.write_bytes(first)
        with running_app(root, {"PATH": env["PATH"], "CLAUDE_CONFIG_DIR": str(config)}) as app:
            app.cli("new-workspace", "--name", "hook-target")
            target = next(row["uuid"] for row in app.surfaces() if row["active"])
            hook_env = dict(app.environment, CMUX_SURFACE_ID=target, CMUX_SOCKET=str(app.socket_path))
            native_id = "session-with-'quotes'"

            def event(name, session_id, valid=True):
                """Run the actual installed shell command with native JSON on stdin and bounded waits."""
                result = subprocess.run(["/bin/sh", "-c", commands[name]], env=hook_env,
                                        input=json.dumps({"hook_event_name": name, "session_id": session_id, "cwd": str(root)}),
                                        text=True, capture_output=True, timeout=10)
                assert (result.returncode == 0) == valid, result.stderr

            event("SessionStart", native_id)
            binding = json.loads(app.cli("surface", "resume", "show", "--surface", target, "--json"))["resume_binding"]
            assert binding["checkpoint_id"] == native_id and binding["kind"] == "claude"
            assert binding["cwd"] == str(root)
            assert binding["environment"]["CLAUDE_CONFIG_DIR"] == str(config)
            argv_output = root / "provider-argv"
            subprocess.run(["/bin/sh", "-c", binding["command"]], check=True, timeout=10,
                           env=dict(hook_env, HOOK_ARGV_OUT=str(argv_output)))
            assert argv_output.read_text().splitlines() == ["--resume", native_id]
            event("SessionEnd", "older-session", valid=False)
            assert json.loads(app.cli("surface", "resume", "show", "--surface", target, "--json"))["resume_binding"] == binding
            event("SessionEnd", native_id)
            assert json.loads(app.cli("surface", "resume", "show", "--surface", target, "--json"))["resume_binding"] is None
            event("SessionStart", "", valid=False)
            missing = subprocess.run([executable, "--socket", str(root / "missing.sock"), "hooks", "claude", "session-start"],
                                     env=hook_env, input="{}", text=True, capture_output=True, timeout=10)
            assert missing.returncode != 0
    print("installed Claude hooks preserved configuration and routed native checkpoint lifecycle")


if __name__ == "__main__":
    main()
