#!/usr/bin/env python3
"""Exercise manual resume in an actual Ghostty-owned terminal, preserving literal launch context."""
import json
from pathlib import Path
import shlex
import subprocess
import tempfile

from linux_app import running_app


def main():
    """Register without execution, reject external invocation, then exec inside the owning PTY."""
    with tempfile.TemporaryDirectory(prefix="cmux-resume-") as directory:
        root = Path(directory)
        with running_app(root) as app:
            app.wait_for(lambda: bool(app.surfaces()), "terminal identity")
            surface = next(row["uuid"] for row in app.surfaces() if row["active"])
            app.wait_for(lambda: bool(json.loads(app.cli("read-text", "--id", surface, "--json"))["text"].strip()),
                         "terminal shell readiness")
            output = root / "resume-result"
            value = "literal $HOME and 'quotes'"
            command = "printf '%s\\n' \"$PWD\" \"$CMUX_SURFACE_ID\" \"$RESUME_VALUE\" > " + shlex.quote(str(output)) + "; exec /bin/sh"
            result = json.loads(app.cli("raw", "surface.resume.set", "--params", json.dumps({
                "surface_id": surface, "command": command, "cwd": str(root), "checkpoint_id": "test",
                "environment": {"RESUME_VALUE": value, "SERVICE_API_KEY": "must-not-persist", "CMUX_SURFACE_ID": "wrong"},
            }), "--json"))
            assert result["resume_binding"]["environment"] == {"RESUME_VALUE": value}
            assert not output.exists()
            try:
                app.cli("restore", "--surface", surface)
            except subprocess.CalledProcessError:
                pass
            else:
                raise AssertionError("restore accepted execution outside its owning terminal")
            executable = str(Path("target/debug/cmux").resolve())
            app.cli("send-text", "--id", surface, shlex.quote(executable) + " restore --checkpoint test")
            app.cli("send-key", "--id", surface, "\r")
            app.wait_for(output.exists, "resume command output")
            app.wait_for(lambda: output.read_text().splitlines() == [str(root), surface, value],
                         "literal environment and terminal identity")
            assert json.loads(app.cli("surface", "resume", "show", "--surface", surface, "--json"))["resume_binding"]["checkpoint_id"] == "test"
    print("manual resume executed in the owning PTY with validated literal context")


if __name__ == "__main__":
    main()
