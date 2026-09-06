#!/usr/bin/env python3
"""Execute the installed OpenCode plugin against a live GTK session."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

from linux_app import running_app


def main():
    """Require preserved registration, native resume and exact-surface idle routing."""
    with tempfile.TemporaryDirectory(prefix="cmux-opencode-hooks-") as directory:
        root = Path(directory)
        config = root / "opencode config"
        binary_dir = root / "bin"
        config.mkdir()
        binary_dir.mkdir()
        (config / "package.json").write_text('{"type":"module"}')
        config_path = config / "opencode.json"
        config_path.write_text(json.dumps({"theme": "user", "plugin": ["user-plugin"]}))
        provider = binary_dir / "opencode"
        provider.write_text("#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$HOOK_ARGV_OUT\"\n")
        provider.chmod(0o700)
        env = dict(os.environ, PATH=str(binary_dir) + os.pathsep + os.environ["PATH"],
                   OPENCODE_CONFIG_DIR=str(config))
        executable = str(Path("target/debug/cmux").resolve())
        subprocess.run([executable, "hooks", "setup", "opencode"], env=env, check=True, timeout=10)
        plugin = config / "plugins" / "cmux-session.js"
        first_config, first_plugin = config_path.read_bytes(), plugin.read_bytes()
        subprocess.run([executable, "hooks", "setup", "opencode"], env=env, check=True, timeout=10)
        assert config_path.read_bytes() == first_config and plugin.read_bytes() == first_plugin
        decoded = json.loads(first_config)
        assert decoded["theme"] == "user"
        assert decoded["plugin"] == ["user-plugin", "./plugins/cmux-session.js"]
        subprocess.run(["node", "--check", str(plugin)], check=True, timeout=10)

        with running_app(root, env) as app:
            target = next(row["uuid"] for row in app.surfaces() if row["active"])
            hook_env = dict(app.environment, CMUX_SURFACE_ID=target, CMUX_SOCKET=str(app.socket_path))
            native_id = "opencode-session"

            def event(event_type, properties):
                event_json = json.dumps({"type": event_type, "properties": properties})
                script = (
                    f"import plugin from {json.dumps(plugin.as_uri())};"
                    f"const hooks=await plugin({{directory:{json.dumps(str(root))}}});"
                    f"await hooks.event({{event:{event_json}}});"
                )
                subprocess.run(["node", "--input-type=module", "--eval", script],
                               env=hook_env, check=True, timeout=10)

            info = {"info": {"id": native_id, "directory": str(root)}}
            event("session.created", info)
            binding = json.loads(app.cli("surface", "resume", "show", "--surface", target, "--json"))["resume_binding"]
            assert binding["kind"] == "opencode" and binding["checkpoint_id"] == native_id
            argv_output = root / "opencode-argv"
            subprocess.run(["/bin/sh", "-c", binding["command"]], env=dict(hook_env, HOOK_ARGV_OUT=str(argv_output)),
                           check=True, timeout=10)
            assert argv_output.read_text().splitlines() == ["--session", native_id]
            event("session.status", {**info, "status": {"type": "idle"}})
            rows = json.loads(app.cli("notifications", "list", "--json"))["notifications"]
            assert len(rows) == 1 and rows[0]["surface_id"] == target
            event("session.deleted", info)
            assert json.loads(app.cli("surface", "resume", "show", "--surface", target, "--json"))["resume_binding"] is None
    print("OpenCode plugin registered idempotently and routed its native lifecycle")


if __name__ == "__main__":
    main()
