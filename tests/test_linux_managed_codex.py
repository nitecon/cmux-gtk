#!/usr/bin/env python3
"""Linux real GTK + official native Codex queue integration, run only in Actions."""
import json
from pathlib import Path
import tempfile
from linux_app import running_app
from managed_codex import Model, verify


def main():
    codex = Path("target/codex-fixture/executable.txt").read_text().strip()
    with tempfile.TemporaryDirectory(prefix="cmux-managed-codex-") as directory:
        root = Path(directory)
        model = Model()
        try:
            model.configure(root / "codex-home")
            with running_app(root, {"CODEX_HOME": str(root / "codex-home"), "SHELL": "/bin/bash"}) as app:
                def rpc(method, params=None):
                    return json.loads(app.cli("raw", method, "--params", json.dumps(params or {}), "--json", timeout=45))
                verify(rpc, root, Path("target/debug/cmux").resolve(), Path(codex), model)
        finally:
            model.close()


if __name__ == "__main__":
    main()
