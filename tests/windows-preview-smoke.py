#!/usr/bin/env python3
"""Exercise the transferred Windows bundle: GUI startup, local RPC and a real ConPTY shell."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time


def main():
    """Run from a clean profile and check terminal execution before bounded cleanup."""
    bundle = Path(sys.argv[1]).resolve()
    # Exclude MSYS2 from PATH: the bundle must resolve its own native runtime DLLs.
    env = os.environ.copy()
    env["PATH"] = str(bundle) + os.pathsep + str(Path(env["SystemRoot"]) / "System32")
    env["XDG_DATA_DIRS"] = str(bundle / "share")
    env["CMUX_NO_UPDATE"] = "1"
    for key in ("CMUX_SOCKET", "CMUX_SOCKET_PATH", "HOME", "SHELL"):
        env.pop(key, None)
    with tempfile.TemporaryDirectory(prefix="cmux-windows-smoke-") as profile:
        for key, directory in (("XDG_CONFIG_HOME", "config"), ("XDG_DATA_HOME", "data"), ("XDG_STATE_HOME", "state"), ("XDG_CACHE_HOME", "cache"), ("XDG_RUNTIME_DIR", "runtime")):
            env[key] = str(Path(profile) / directory)
        subprocess.run([str(bundle / "cmux.exe"), "--version"], env=env, check=True, timeout=20)
        with (bundle / "smoke.log").open("w", encoding="utf-8") as log:
            app = subprocess.Popen([str(bundle / "cmux-app.exe")], env=env, stdout=log, stderr=subprocess.STDOUT)
            try:
                def rpc(method, params=None):
                    """Call the production CLI and decode its JSON response."""
                    result = subprocess.run([str(bundle / "cmux.exe"), "--json", "raw", method, "--params", json.dumps(params or {})], env=env, capture_output=True, text=True, timeout=10)
                    if result.returncode:
                        raise RuntimeError(result.stderr)
                    return json.loads(result.stdout)
                deadline = time.monotonic() + 45
                while True:
                    if app.poll() is not None:
                        raise RuntimeError(f"CMUX exited during startup: {app.returncode}")
                    try:
                        ping = rpc("system.ping")
                        break
                    except (RuntimeError, subprocess.TimeoutExpired):
                        if time.monotonic() >= deadline:
                            raise
                        time.sleep(1)
                print("Native GUI/RPC ready:", ping)
                # The command contains no contiguous result marker, preventing echoed input from passing.
                rpc("surface.send_text", {"text": 'set CMUX_SMOKE=CONPTY_OK\recho CMUX_WINDOWS_%CMUX_SMOKE%\r'})
                deadline = time.monotonic() + 20
                while True:
                    screen = rpc("surface.read_text")
                    if "CMUX_WINDOWS_CONPTY_OK" in json.dumps(screen):
                        break
                    if time.monotonic() >= deadline:
                        raise RuntimeError(f"No real ConPTY shell output: {screen}")
                    time.sleep(0.5)
                print("Real ConPTY shell input/output verified")
                (bundle / "smoke-result.json").write_text(json.dumps({"startup": True, "local_rpc": True, "conpty_shell": True}, indent=2) + "\n")
            finally:
                if app.poll() is None:
                    subprocess.run(["taskkill", "/PID", str(app.pid), "/T", "/F"], env=env, capture_output=True, timeout=15)
                app.wait(timeout=15)


if __name__ == "__main__":
    main()
