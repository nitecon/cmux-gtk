#!/usr/bin/env python3
"""Exercise Amp's generated native plugin against the GTK lifecycle API."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

from linux_app import running_app


def main():
    """Require plugin ownership, native thread resume argv and exact completion routing."""
    with tempfile.TemporaryDirectory(prefix="cmux-amp-hooks-") as directory:
        root = Path(directory)
        home = root / "home"
        binary_dir = root / "bin"
        home.mkdir()
        binary_dir.mkdir()
        provider = binary_dir / "amp"
        provider.write_text("#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$HOOK_ARGV_OUT\"\n")
        provider.chmod(0o700)
        env = dict(os.environ, HOME=str(home),
                   PATH=str(binary_dir) + os.pathsep + os.environ["PATH"])
        executable = str(Path("target/debug/cmux").resolve())
        subprocess.run([executable, "hooks", "setup", "amp"], env=env, check=True, timeout=10)
        extension = home / ".config/amp/plugins/cmux-session.ts"
        first = extension.read_bytes()
        subprocess.run([executable, "hooks", "setup", "amp"], env=env, check=True, timeout=10)
        assert extension.read_bytes() == first
        assert b"cmux-amp-session-extension-marker" in first

        module = root / "cmux-session.mjs"
        shutil.copyfile(extension, module)
        harness = root / "invoke.mjs"
        harness.write_text("""
import extension from './cmux-session.mjs';
const handlers = new Map();
extension({ thread: { id: 'amp-root-thread' }, on(name, callback) { handlers.set(name, callback); } });
const event = process.argv[2];
const callback = handlers.get(event);
if (!callback) throw new Error(`missing ${event}`);
callback({ thread: { id: 'amp-native-thread' }, message: process.argv[3] || undefined }, {});
await new Promise(resolve => setTimeout(resolve, 500));
""")

        with running_app(root, env, startup_timeout=20) as app:
            target = next(row["uuid"] for row in app.surfaces() if row["active"])
            hook_env = dict(app.environment, CMUX_SURFACE_ID=target, CMUX_SOCKET=str(app.socket_path),
                            CMUX_AGENT_LAUNCH_CWD=str(root))

            def invoke(event, message=""):
                subprocess.run(["node", str(harness), event, message], cwd=root, env=hook_env,
                               check=True, timeout=10)

            invoke("session.start")
            invoke("agent.start")
            binding = json.loads(app.cli("surface", "resume", "show", "--surface", target,
                                         "--json"))["resume_binding"]
            assert binding["kind"] == "amp" and binding["checkpoint_id"] == "amp-native-thread"
            argv_output = root / "amp-argv"
            subprocess.run(["/bin/sh", "-c", binding["command"]],
                           env=dict(hook_env, HOOK_ARGV_OUT=str(argv_output)), check=True, timeout=10)
            assert argv_output.read_text().splitlines() == ["threads", "continue", "amp-native-thread"]
            invoke("agent.end", "Amp response ready")
            rows = json.loads(app.cli("notifications", "list", "--json"))["notifications"]
            assert len(rows) == 1 and rows[0]["surface_id"] == target
            assert rows[0]["body"] == "Amp response ready"
            binding = json.loads(app.cli("surface", "resume", "show", "--surface", target,
                                         "--json"))["resume_binding"]
            assert binding["checkpoint_id"] == "amp-native-thread"
    print("Amp plugin routed native thread lifecycle and exact continuation argv")


if __name__ == "__main__":
    main()
