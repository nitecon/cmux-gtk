#!/usr/bin/env python3
"""Linux real GTK + official ordinary Codex input integration, run only in Actions."""
import json
from pathlib import Path
import tempfile
import subprocess
from linux_app import running_app
from process_input import Model, verify


def main():
    codex = Path("target/codex-fixture/executable.txt").read_text().strip()
    with tempfile.TemporaryDirectory(prefix="cmux-process-input-") as directory:
        root = Path(directory)
        model = Model()
        try:
            model.configure(root / "codex-home")
            with running_app(root, {"CODEX_HOME": str(root / "codex-home"), "SHELL": "/bin/bash"}) as app:
                def rpc(method, params=None):
                    return json.loads(app.cli("raw", method, "--params", json.dumps(params or {}), "--json", timeout=45))
                def native_key(key):
                    """Deliver physical unmodified keys through the real GTK window."""
                    windows = subprocess.check_output(["xdotool", "search", "--onlyvisible", "--pid", str(app.process.pid)], text=True).split()
                    subprocess.run(["xdotool", "windowfocus", "--sync", windows[-1], "key", "--clearmodifiers", key], check=True, timeout=10)
                verify(rpc, root, Path(codex), model, native_key)
        finally:
            model.close()


if __name__ == "__main__":
    main()
