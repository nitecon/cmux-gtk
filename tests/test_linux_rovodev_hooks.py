#!/usr/bin/env python3
"""Exercise Rovo Dev's YAML hooks and bounded session metadata inference."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

from linux_app import running_app


def installed_commands(config):
    """Read the generated command immediately following each owned event."""
    commands = {}
    event = None
    owned = False
    for line in config.splitlines():
        if "# cmux hooks rovodev begin" in line:
            owned = True
        elif "# cmux hooks rovodev end" in line:
            owned = False
        elif owned and "- name:" in line:
            event = line.split(":", 1)[1].strip()
        elif owned and event and "- command:" in line:
            commands[event] = json.loads(line.split("command:", 1)[1].strip())
    return commands


def write_session(root, session_id, workspace, timestamp):
    """Create the public Rovo metadata shape with deterministic freshness."""
    session = root / session_id
    session.mkdir(parents=True)
    metadata = session / "metadata.json"
    context = session / "session_context.json"
    metadata.write_text(json.dumps({"title": "Rovo session", "workspace_path": str(workspace)}))
    context.write_text('{"message_history":[]}')
    os.utime(metadata, (timestamp, timestamp))
    os.utime(context, (timestamp, timestamp))


def main():
    """Require YAML preservation, newest matching identity and exact restore routing."""
    with tempfile.TemporaryDirectory(prefix="cmux-rovodev-hooks-") as directory:
        root = Path(directory)
        home = root / "home"
        binary_dir = root / "bin"
        sessions = root / "sessions"
        workspace = root / "repo"
        home.mkdir()
        binary_dir.mkdir()
        sessions.mkdir()
        workspace.mkdir()
        provider = binary_dir / "acli"
        provider.write_text("#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$HOOK_ARGV_OUT\"\n")
        provider.chmod(0o700)
        write_session(sessions, "rovo-old", workspace, 100)
        write_session(sessions, "rovo-current", workspace, 200)
        write_session(sessions, "rovo-unrelated", root, 300)
        config_dir = home / ".rovodev"
        config_dir.mkdir()
        config_path = config_dir / "config.yml"
        config_path.write_text("sessions:\n  persistenceDir: /keep/user/value\neventHooks:\n  enabled: true\n")
        env = dict(os.environ, HOME=str(home), CMUX_ROVODEV_SESSIONS_DIR=str(sessions),
                   PATH=str(binary_dir) + os.pathsep + os.environ["PATH"])
        executable = str(Path("target/debug/cmux").resolve())
        subprocess.run([executable, "hooks", "setup", "rovodev"], env=env, check=True, timeout=10)
        first = config_path.read_bytes()
        subprocess.run([executable, "hooks", "setup", "rovo"], env=env, check=True, timeout=10)
        assert config_path.read_bytes() == first
        config = first.decode()
        assert "persistenceDir: /keep/user/value" in config and "enabled: true" in config
        commands = installed_commands(config)
        assert set(commands) == {"on_complete", "on_error", "on_tool_permission"}

        with running_app(root, env, startup_timeout=20) as app:
            target = next(row["uuid"] for row in app.surfaces() if row["active"])
            hook_env = dict(app.environment, CMUX_SURFACE_ID=target, CMUX_SOCKET=str(app.socket_path))

            def invoke(event, **extra):
                payload = {"hook_event_name": event, "cwd": str(workspace), **extra}
                result = subprocess.run(["/bin/sh", "-c", commands[event]], env=hook_env,
                                        input=json.dumps(payload), text=True,
                                        capture_output=True, timeout=10)
                assert result.returncode == 0, result.stderr

            invoke("on_tool_permission")
            binding = json.loads(app.cli("surface", "resume", "show", "--surface", target,
                                         "--json"))["resume_binding"]
            assert binding["kind"] == "rovodev" and binding["checkpoint_id"] == "rovo-current"
            argv_output = root / "rovo-argv"
            subprocess.run(["/bin/sh", "-c", binding["command"]],
                           env=dict(hook_env, HOOK_ARGV_OUT=str(argv_output)), check=True, timeout=10)
            assert argv_output.read_text().splitlines() == [
                "rovodev", "run", "--restore", "rovo-current"
            ]
            invoke("on_complete", message="Rovo response ready")
            rows = json.loads(app.cli("notifications", "list", "--json"))["notifications"]
            assert len(rows) == 1 and rows[0]["surface_id"] == target
            assert rows[0]["body"] == "Rovo response ready"
            binding = json.loads(app.cli("surface", "resume", "show", "--surface", target,
                                         "--json"))["resume_binding"]
            assert binding["checkpoint_id"] == "rovo-current"
    print("Rovo Dev YAML hooks inferred and routed the newest matching durable session")


if __name__ == "__main__":
    main()
