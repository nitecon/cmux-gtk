#!/usr/bin/env python3
"""Exercise the maintained prompt diagnostic against GTK and observe actual prompt submissions."""
from collections import Counter
import json
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile

from linux_app import running_app


def main():
    """Supply a controlled Bash prompt and verify three Enter events, workspace restoration and PTY cleanup."""
    with tempfile.TemporaryDirectory(prefix="cmux-prompt-probe-") as directory:
        root = Path(directory)
        prompt_log = root / "prompts"
        rc = root / "bash.rc"
        hook = "printf '%s\\n' \"$$\" >> " + shlex.quote(str(prompt_log))
        rc.write_text("PS1='$ '\nPROMPT_COMMAND=" + shlex.quote(hook) + "\n")
        shell = root / "shell"
        shell.write_text("#!/bin/sh\nexec /bin/bash --noprofile --rcfile " + shlex.quote(str(rc)) + " -i\n")
        shell.chmod(0o700)
        ghostty_config = root / "config" / "ghostty"
        ghostty_config.mkdir(parents=True)
        # Use Ghostty's explicit launch setting: a custom SHELL path can fall back
        # to the account shell and would not install this fixture's prompt hook.
        (ghostty_config / "config").write_text("command = " + str(shell) + "\n")
        with running_app(root) as app:
            app.wait_for(lambda: len(app.children()) == 1, "initial shell")
            original = json.loads(app.cli("current-workspace", "--json"))["uuid"]
            subprocess.run(
                [sys.executable, "scripts/probe-pure-prompt-duplication.py", "--socket", str(app.socket_path),
                 "--enters", "3", "--delay", "0.2"],
                env=app.environment, check=True, timeout=45,
            )
            assert json.loads(app.cli("current-workspace", "--json"))["uuid"] == original
            assert len(json.loads(app.cli("list-workspaces", "--json"))["workspaces"]) == 1
            app.wait_for(lambda: len(app.children()) == 1, "probe workspace PTY cleanup")
            counts = Counter(prompt_log.read_text().splitlines())
            assert len(counts) == 2 and max(counts.values()) >= 4, "Enter did not produce three new prompts"
    print("prompt probe submitted Enter and restored the original workspace")


if __name__ == "__main__":
    main()
