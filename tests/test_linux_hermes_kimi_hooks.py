#!/usr/bin/env python3
"""Exercise Hermes Agent YAML and Kimi TOML lifecycle hooks against GTK."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib

from linux_app import running_app


def hermes_commands(config):
    """Read event commands from the small direct Hermes YAML shape."""
    commands = {}
    event = None
    for line in config.splitlines():
        if line.startswith("  ") and not line.startswith("    ") and line.strip().endswith(":"):
            event = line.strip()[:-1]
        elif event and line.strip().startswith("- command:") and "hooks hermes-agent" in line:
            commands[event] = json.loads(line.split("command:", 1)[1].strip())
    return commands


def main():
    """Require owned config blocks, Hermes approval entries and exact resume/attention."""
    with tempfile.TemporaryDirectory(prefix="cmux-hermes-kimi-hooks-") as directory:
        root = Path(directory)
        home = root / "home"
        binary_dir = root / "bin"
        hermes_home = root / "hermes"
        kimi_home = root / "kimi-code"
        home.mkdir()
        binary_dir.mkdir()
        hermes_home.mkdir()
        kimi_home.mkdir()
        for binary in ("hermes", "kimi"):
            path = binary_dir / binary
            path.write_text("#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$HOOK_ARGV_OUT\"\n")
            path.chmod(0o700)
        hermes_config = hermes_home / "config.yaml"
        hermes_config.write_text(
            "model: user/model\nhooks:\n  post_llm_call:\n"
            "    - command: \"echo user\"\n      timeout: 9\n"
        )
        allowlist = hermes_home / "shell-hooks-allowlist.json"
        allowlist.write_text(json.dumps({"user": True, "approvals": [
            {"event": "user", "command": "echo user", "scope": "keep"}
        ]}))
        kimi_config = kimi_home / "config.toml"
        kimi_config.write_text('model = "user-model"\n')
        env = dict(
            os.environ,
            HOME=str(home),
            HERMES_HOME=str(hermes_home),
            KIMI_CODE_HOME=str(kimi_home),
            PATH=str(binary_dir) + os.pathsep + os.environ["PATH"],
        )
        executable = str(Path("target/debug/cmux").resolve())
        for provider, path in (("hermes-agent", hermes_config), ("kimi", kimi_config)):
            subprocess.run([executable, "hooks", "setup", provider], env=env, check=True, timeout=10)
            first = path.read_bytes()
            subprocess.run([executable, "hooks", "setup", provider], env=env, check=True, timeout=10)
            assert path.read_bytes() == first

        hermes_text = hermes_config.read_text()
        assert "model: user/model" in hermes_text and 'command: "echo user"' in hermes_text
        h_commands = hermes_commands(hermes_text)
        assert set(h_commands) == {
            "on_session_start", "pre_llm_call", "post_llm_call",
            "pre_approval_request", "on_session_finalize",
        }
        approvals = json.loads(allowlist.read_text())
        assert approvals["user"] is True
        assert any(row.get("scope") == "keep" for row in approvals["approvals"])
        assert sum("hooks hermes-agent" in row.get("command", "") for row in approvals["approvals"]) == 5

        kimi = tomllib.loads(kimi_config.read_text())
        assert kimi["model"] == "user-model"
        k_commands = {entry["event"]: entry["command"] for entry in kimi["hooks"]}
        assert set(k_commands) == {"SessionStart", "UserPromptSubmit", "Notification", "Stop", "SessionEnd"}
        assert all(entry["timeout"] == 10 for entry in kimi["hooks"])

        with running_app(root, env, startup_timeout=20) as app:
            target = next(row["uuid"] for row in app.surfaces() if row["active"])
            hook_env = dict(app.environment, CMUX_SURFACE_ID=target, CMUX_SOCKET=str(app.socket_path))
            providers = {
                "hermes-agent": (
                    h_commands, "on_session_start", "pre_llm_call", "post_llm_call",
                    "on_session_finalize", "extra", ["--resume"],
                ),
                "kimi": (
                    k_commands, "SessionStart", "UserPromptSubmit", "Stop",
                    "SessionEnd", "top", ["--resume"],
                ),
            }
            expected_notifications = 0
            for provider, (commands, start, prompt, stop, end, id_location, prefix) in providers.items():
                native_id = f"{provider}-native-session"

                def invoke(event, message=None):
                    payload = {"hook_event_name": event, "cwd": str(root)}
                    if id_location == "extra":
                        payload["extra"] = {"session_key": native_id}
                    else:
                        payload["session_id"] = native_id
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
                assert argv_output.read_text().splitlines() == prefix + [native_id]
                invoke(stop, f"{provider} response ready")
                expected_notifications += 1
                rows = json.loads(app.cli("notifications", "list", "--json"))["notifications"]
                assert len(rows) == expected_notifications and rows[-1]["surface_id"] == target
                assert rows[-1]["body"] == f"{provider} response ready"
                invoke(end)
                binding = json.loads(app.cli(
                    "surface", "resume", "show", "--surface", target, "--json",
                ))["resume_binding"]
                assert binding is None
    print("Hermes and Kimi hooks preserved native config and routed lifecycle state")


if __name__ == "__main__":
    main()
