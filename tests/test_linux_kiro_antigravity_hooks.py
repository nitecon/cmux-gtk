#!/usr/bin/env python3
"""Exercise Kiro and Antigravity's distinct JSON hook formats against GTK."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

from linux_app import running_app


def main():
    """Require idempotent format-aware setup, native resume argv and exact attention."""
    with tempfile.TemporaryDirectory(prefix="cmux-kiro-antigravity-hooks-") as directory:
        root = Path(directory)
        home = root / "home"
        binary_dir = root / "bin"
        kiro_home = root / "kiro"
        home.mkdir()
        binary_dir.mkdir()
        for binary in ("kiro-cli", "agy"):
            path = binary_dir / binary
            path.write_text("#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$HOOK_ARGV_OUT\"\n")
            path.chmod(0o700)
        env = dict(
            os.environ,
            HOME=str(home),
            KIRO_HOME=str(kiro_home),
            PATH=str(binary_dir) + os.pathsep + os.environ["PATH"],
        )
        executable = str(Path("target/debug/cmux").resolve())
        kiro_path = kiro_home / "agents/cmux.json"
        antigravity_path = home / ".gemini/config/hooks.json"
        kiro_path.parent.mkdir(parents=True)
        antigravity_path.parent.mkdir(parents=True)
        kiro_path.write_text(json.dumps({"user": "kiro", "tools": ["read"]}))
        antigravity_path.write_text(json.dumps({"user": "antigravity", "companion": {"x": 1}}))
        for provider, path in (("kiro", kiro_path), ("antigravity", antigravity_path)):
            subprocess.run([executable, "hooks", "setup", provider], env=env, check=True, timeout=10)
            first = path.read_bytes()
            subprocess.run([executable, "hooks", "setup", provider], env=env, check=True, timeout=10)
            assert path.read_bytes() == first

        kiro = json.loads(kiro_path.read_text())
        assert kiro["user"] == "kiro" and kiro["tools"] == ["read"]
        assert kiro["name"] == "cmux"
        kiro_commands = {
            event: entries[-1]["command"] for event, entries in kiro["hooks"].items()
        }
        assert all(entry[-1]["timeout_ms"] == 5000 for entry in kiro["hooks"].values())
        antigravity = json.loads(antigravity_path.read_text())
        assert antigravity["user"] == "antigravity" and antigravity["companion"] == {"x": 1}
        antigravity_commands = {
            event: entries[-1]["command"] for event, entries in antigravity["cmux"].items()
        }
        assert all(entry[-1]["timeout"] == 10 for entry in antigravity["cmux"].values())

        with running_app(root, env, startup_timeout=20) as app:
            target = next(row["uuid"] for row in app.surfaces() if row["active"])
            hook_env = dict(app.environment, CMUX_SURFACE_ID=target, CMUX_SOCKET=str(app.socket_path))
            providers = {
                "kiro": (kiro_commands, "agentSpawn", "userPromptSubmit", "stop",
                         "session_id", ["chat", "--resume-id"]),
                "antigravity": (antigravity_commands, "SessionStart", "PreInvocation", "Stop",
                                "conversation_id", ["--conversation"]),
            }
            expected_notifications = 0
            for provider, (commands, start, prompt, stop, id_key, resume_prefix) in providers.items():
                native_id = f"{provider}-native-session"

                def invoke(event, message=None):
                    payload = {"hook_event_name": event, id_key: native_id, "cwd": str(root)}
                    if message:
                        payload["message"] = message
                    result = subprocess.run(
                        ["/bin/sh", "-c", commands[event]], env=hook_env,
                        input=json.dumps(payload), text=True, capture_output=True, timeout=10,
                    )
                    assert result.returncode == 0, result.stderr

                invoke(start)
                invoke(prompt)
                binding = json.loads(app.cli(
                    "surface", "resume", "show", "--surface", target, "--json",
                ))["resume_binding"]
                assert binding["kind"] == provider and binding["checkpoint_id"] == native_id
                argv_output = root / f"{provider}-argv"
                subprocess.run(
                    ["/bin/sh", "-c", binding["command"]], check=True, timeout=10,
                    env=dict(hook_env, HOOK_ARGV_OUT=str(argv_output)),
                )
                assert argv_output.read_text().splitlines() == resume_prefix + [native_id]
                invoke(stop, f"{provider} response ready")
                expected_notifications += 1
                rows = json.loads(app.cli("notifications", "list", "--json"))["notifications"]
                assert len(rows) == expected_notifications
                assert rows[-1]["surface_id"] == target
                assert rows[-1]["body"] == f"{provider} response ready"
    print("Kiro and Antigravity hooks preserved native schemas and routed lifecycle state")


if __name__ == "__main__":
    main()
