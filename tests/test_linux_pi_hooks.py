#!/usr/bin/env python3
"""Exercise Pi's generated native extension against the GTK lifecycle API."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

from linux_app import running_app


def main():
    """Require an idempotent extension, native resume argv and exact lifecycle routing."""
    with tempfile.TemporaryDirectory(prefix="cmux-pi-hooks-") as directory:
        root = Path(directory)
        home = root / "home"
        binary_dir = root / "bin"
        agent_dir = root / "pi-agent"
        home.mkdir()
        binary_dir.mkdir()
        provider = binary_dir / "pi"
        provider.write_text("#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$HOOK_ARGV_OUT\"\n")
        provider.chmod(0o700)
        env = dict(os.environ, HOME=str(home), PI_CODING_AGENT_DIR=str(agent_dir),
                   PATH=str(binary_dir) + os.pathsep + os.environ["PATH"])
        executable = str(Path("target/debug/cmux").resolve())
        subprocess.run([executable, "hooks", "setup", "pi"], env=env, check=True, timeout=10)
        extension = agent_dir / "extensions/cmux-session.ts"
        first = extension.read_bytes()
        subprocess.run([executable, "hooks", "setup", "pi"], env=env, check=True, timeout=10)
        assert extension.read_bytes() == first
        assert b"cmux-pi-session-extension-marker" in first

        # The generated TypeScript intentionally contains plain ESM syntax so the harness can
        # execute its public callbacks without installing Pi's package on the runner.
        module = root / "cmux-session.mjs"
        shutil.copyfile(extension, module)
        harness = root / "invoke.mjs"
        harness.write_text("""
import extension from './cmux-session.mjs';
const handlers = new Map();
extension({ on(name, callback) { handlers.set(name, callback); } });
const event = process.argv[2];
const callback = handlers.get(event);
if (!callback) throw new Error(`missing ${event}`);
callback({ message: process.argv[3] || undefined, reason: process.argv[3] || undefined }, {
  cwd: process.cwd(), sessionManager: { getSessionId() { return 'pi-native-session'; } },
});
await new Promise(resolve => setTimeout(resolve, 500));
""")

        with running_app(root, env, startup_timeout=20) as app:
            target = next(row["uuid"] for row in app.surfaces() if row["active"])
            hook_env = dict(app.environment, CMUX_SURFACE_ID=target, CMUX_SOCKET=str(app.socket_path))

            def invoke(event, message=""):
                subprocess.run(["node", str(harness), event, message], cwd=root, env=hook_env,
                               check=True, timeout=10)

            invoke("session_start")
            invoke("before_agent_start")
            binding = json.loads(app.cli("surface", "resume", "show", "--surface", target,
                                         "--json"))["resume_binding"]
            assert binding["kind"] == "pi" and binding["checkpoint_id"] == "pi-native-session"
            argv_output = root / "pi-argv"
            subprocess.run(["/bin/sh", "-c", binding["command"]],
                           env=dict(hook_env, HOOK_ARGV_OUT=str(argv_output)), check=True, timeout=10)
            assert argv_output.read_text().splitlines() == ["--session", "pi-native-session"]
            invoke("agent_end", "Pi response ready")
            rows = json.loads(app.cli("notifications", "list", "--json"))["notifications"]
            assert len(rows) == 1 and rows[0]["surface_id"] == target
            assert rows[0]["body"] == "Pi response ready"
            invoke("session_shutdown", "quit")
            binding = json.loads(app.cli("surface", "resume", "show", "--surface", target,
                                         "--json"))["resume_binding"]
            assert binding is None
    print("Pi extension routed native session lifecycle and exact resume argv")


if __name__ == "__main__":
    main()
