#!/usr/bin/env python3
"""Exercise automatic Git branch/dirty discovery against real isolated repositories."""
import json
from pathlib import Path
import shlex
import socket
import subprocess
import tempfile

from linux_app import running_app


def main():
    """Observe changes without API-triggered refresh, and clear metadata after leaving the repository."""
    with tempfile.TemporaryDirectory(prefix="cmux-git-") as directory:
        root = Path(directory)
        repo = root / "repo"
        repo.mkdir()
        subprocess.check_call(["git", "init", "-b", "initial", str(repo)])
        with running_app(root) as app:
            app.cli("new-workspace", "--name", "repo", "--cwd", str(repo))
            target = next(row for row in app.surfaces() if row["active"])

            def observed():
                """Read published metadata without selecting a workspace or executing Git via RPC."""
                rows = json.loads(app.cli("list-workspaces", "--json"))["workspaces"]
                return next(row["git"] for row in rows if row["uuid"] == target["workspace_uuid"])

            app.wait_for(lambda: observed() == {"branch": "initial", "dirty": False}, "initial Git branch")
            (repo / "new.txt").write_text("new\n")
            app.wait_for(lambda: observed() == {"branch": "initial", "dirty": True}, "untracked dirty state")
            subprocess.check_call(["git", "-C", str(repo), "add", "new.txt"])
            subprocess.check_call(["git", "-C", str(repo), "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                                   "-c", "commit.gpgsign=false", "commit", "-m", "fixture"])
            app.wait_for(lambda: observed() == {"branch": "initial", "dirty": False}, "clean committed state")
            subprocess.check_call(["git", "-C", str(repo), "switch", "-c", "feature"])
            app.wait_for(lambda: observed() == {"branch": "feature", "dirty": False}, "branch change")
            app.cli("new-workspace", "--name", "observer")
            active = next(row["uuid"] for row in app.surfaces() if row["active"])
            (repo / "new.txt").write_text("edited\n")
            app.wait_for(lambda: observed() == {"branch": "feature", "dirty": True}, "background workspace change")
            assert next(row["uuid"] for row in app.surfaces() if row["active"]) == active
            uri = "file://" + socket.gethostname() + str(root)
            app.cli("send-text", "--id", target["uuid"], "cd " + shlex.quote(str(root)) + "; printf '\\033]7;%s\\007' " + shlex.quote(uri))
            app.cli("send-key", "--id", target["uuid"], "\r")
            app.wait_for(lambda: observed() is None, "Git state cleared outside repository")
            assert next(row["uuid"] for row in app.surfaces() if row["active"]) == active
    print("automatic Git branch, dirty, background update and directory invalidation passed")


if __name__ == "__main__":
    main()
