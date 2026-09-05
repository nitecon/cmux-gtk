#!/usr/bin/env python3
"""Verify Linux version flags without a display, socket or application-state writes."""

import os
from pathlib import Path
import re
import subprocess
import tempfile


def main():
    """Exercise both executables and short/long flags against isolated unavailable services."""
    binary_dir = Path(os.environ.get("CMUX_BIN_DIR", "target/debug")).resolve()
    with tempfile.TemporaryDirectory(prefix="cmux-version-") as directory:
        root = Path(directory)
        env = dict(os.environ, CMUX_SOCKET=str(root / "missing.sock"))
        for name in ("DISPLAY", "WAYLAND_DISPLAY"):
            env.pop(name, None)
        for kind in ("CONFIG_HOME", "DATA_HOME", "STATE_HOME", "RUNTIME_DIR"):
            env[f"XDG_{kind}"] = str(root / kind.lower())
        versions = []
        for binary in ("cmux", "cmux-app"):
            for flag in ("--version", "-V"):
                result = subprocess.run([str(binary_dir / binary), flag], env=env,
                                        text=True, capture_output=True, check=True, timeout=5)
                assert not result.stderr, result.stderr
                prefix = f"{binary} "
                assert result.stdout.startswith(prefix), result.stdout
                version = result.stdout.removeprefix(prefix).strip()
                assert re.fullmatch(r"\d+\.\d+\.\d+(?:[-+].+)?", version), version
                versions.append(version)
        assert len(set(versions)) == 1, versions
        assert not list(root.iterdir()), "version flags initialized application state"
    print(f"PASS: both executables report {versions[0]} without GTK or socket startup")


if __name__ == "__main__":
    main()
