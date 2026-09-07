#!/usr/bin/env python3
"""Exercise OMP and Campfire's native extension lifecycle contracts against GTK."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

from linux_app import running_app


def main():
    """Require safe extension ownership, exact resume identity and Campfire host attention."""
    with tempfile.TemporaryDirectory(prefix="cmux-omp-campfire-hooks-") as directory:
        root = Path(directory)
        home = root / "home"
        binary_dir = root / "bin"
        campfire_agent = root / "campfire-agent"
        home.mkdir()
        binary_dir.mkdir()
        for provider in ("omp", "campfire"):
            binary = binary_dir / provider
            binary.write_text("#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$HOOK_ARGV_OUT\"\n")
            binary.chmod(0o700)
        env = dict(
            os.environ,
            HOME=str(home),
            PI_CONFIG_DIR=".omp-fixture",
            CAMPFIRE_CODING_AGENT_DIR=str(campfire_agent),
            PATH=str(binary_dir) + os.pathsep + os.environ["PATH"],
        )
        executable = str(Path("target/debug/cmux").resolve())
        paths = {
            "omp": home / ".omp-fixture/agent/extensions/cmux-omp-session.ts",
            "campfire": campfire_agent / "extensions/cmux-campfire-session.ts",
        }
        modules = {}
        for provider, extension in paths.items():
            subprocess.run([executable, "hooks", "setup", provider], env=env, check=True, timeout=10)
            first = extension.read_bytes()
            subprocess.run([executable, "hooks", "setup", provider], env=env, check=True, timeout=10)
            assert extension.read_bytes() == first
            assert f"cmux-{provider}-session-extension-marker".encode() in first
            module = root / f"{provider}.mjs"
            shutil.copyfile(extension, module)
            modules[provider] = module

        harness = root / "invoke.mjs"
        harness.write_text("""
const provider = process.argv[2];
const event = process.argv[3];
const module = await import(`./${provider}.mjs`);
const handlers = new Map();
const bridge = { listeners: new Set() };
globalThis[Symbol.for('campfire.observer.v1')] = bridge;
module.default({ on(name, callback) { handlers.set(name, callback); } });
const context = { cwd: process.cwd(), sessionManager: {
  getSessionId() { return `${provider}-native-session`; }
} };
if (event === 'lifecycle') {
  for (const name of ['session_start', 'before_agent_start', 'agent_end']) {
    const callback = handlers.get(name);
    if (!callback) throw new Error(`missing ${name}`);
    callback({ message: `${provider} response ready` }, context);
    await new Promise(resolve => setTimeout(resolve, 250));
  }
} else if (event === 'observer') {
  handlers.get('session_start')?.({}, context);
  for (const listener of bridge.listeners) {
    listener({ type: 'join.requested', displayName: 'Ada', capability: 'shell' });
  }
} else {
  const callback = handlers.get(event);
  if (!callback) throw new Error(`missing ${event}`);
  callback({ message: `${provider} response ready` }, context);
}
await new Promise(resolve => setTimeout(resolve, 750));
""")

        with running_app(root, env, startup_timeout=20) as app:
            target = next(row["uuid"] for row in app.surfaces() if row["active"])
            hook_env = dict(
                app.environment,
                CMUX_SURFACE_ID=target,
                CMUX_SOCKET=str(app.socket_path),
                CAMPFIRE_SESSION_ROLE="host",
            )

            def invoke(provider, event):
                subprocess.run(
                    ["node", str(harness), provider, event], cwd=root, env=hook_env,
                    check=True, timeout=10,
                )

            def notifications():
                return json.loads(app.cli("notifications", "list", "--json"))["notifications"]

            expected_notifications = 0
            for provider in ("omp", "campfire"):
                invoke(provider, "lifecycle")
                binding = json.loads(app.cli(
                    "surface", "resume", "show", "--surface", target, "--json",
                ))["resume_binding"]
                assert binding["kind"] == provider
                assert binding["checkpoint_id"] == f"{provider}-native-session"
                argv_output = root / f"{provider}-argv"
                subprocess.run(
                    ["/bin/sh", "-c", binding["command"]], check=True, timeout=10,
                    env=dict(hook_env, HOOK_ARGV_OUT=str(argv_output)),
                )
                assert argv_output.read_text().splitlines() == [
                    "--session", f"{provider}-native-session",
                ]
                expected_notifications += 1
                app.wait_for(
                    lambda: len(notifications()) == expected_notifications,
                    f"{provider} completion notification",
                )
                rows = notifications()
                assert rows[-1]["surface_id"] == target
                assert rows[-1]["body"] == f"{provider} response ready"

            invoke("campfire", "observer")
            app.wait_for(
                lambda: len(notifications()) == expected_notifications + 1,
                "Campfire observer notification",
            )
            rows = notifications()
            assert rows[-1]["surface_id"] == target
            assert "join.requested" in rows[-1]["body"] and "Ada" in rows[-1]["body"]
    print("OMP and Campfire extensions routed resume, prompt and attention lifecycle state")


if __name__ == "__main__":
    main()
