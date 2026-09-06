#!/usr/bin/env python3
"""Verify Linux CLI output failures using closed pipes and an isolated application."""
import os
from pathlib import Path
import subprocess
import tempfile

from linux_app import running_app
from process_support import stop_process


def run_with_closed_stdout(command, environment):
    """Run an owned CLI with no pipe reader, closing descriptors even when launch fails."""
    reader, writer = os.pipe()
    os.close(reader)
    process = None
    try:
        try:
            process = subprocess.Popen(command, stdout=writer, stderr=subprocess.PIPE,
                                       text=True, env=environment, close_fds=True)
        finally:
            os.close(writer)
        _, stderr = process.communicate(timeout=10)
        assert process.returncode == 0, (process.returncode, stderr)
        assert not stderr, stderr
    finally:
        stop_process(process)
        if process is not None:
            process.stderr.close()


def main():
    """Check help/version and real RPC output; non-pipe write failures must still fail."""
    cli = Path(os.environ.get("CMUX_BIN_DIR", "target/debug")).resolve() / "cmux"
    for flag in ("--version", "--help"):
        run_with_closed_stdout([str(cli), flag], os.environ)
    with tempfile.TemporaryDirectory(prefix="cmux-cli-output-") as directory:
        with running_app(Path(directory)) as app:
            command = [str(cli), "--socket", str(app.socket_path), "ping", "--json"]
            run_with_closed_stdout(command, app.environment)
            with open("/dev/full", "wb") as full:
                result = subprocess.run(command, env=app.environment, stdout=full,
                                        stderr=subprocess.PIPE, text=True, timeout=10)
            assert result.returncode == 1, (result.returncode, result.stderr)
            assert "cannot write stdout" in result.stderr, result.stderr
            assert "panicked" not in result.stderr, result.stderr
    print("PASS: closed pipes exit cleanly and other stdout failures report a command error")


if __name__ == "__main__":
    main()
