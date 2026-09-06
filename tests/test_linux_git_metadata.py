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

            def metadata():
                """Read published metadata without selecting a workspace or executing Git via RPC."""
                rows = json.loads(app.cli("list-workspaces", "--json"))["workspaces"]
                return next(row["git"] for row in rows if row["uuid"] == target["workspace_uuid"])

            def observed():
                """Compare branch and dirty state while allowing independent tracking fields."""
                value = metadata()
                return {key: value[key] for key in ("branch", "dirty")} if value else None

            app.wait_for(lambda: observed() == {"branch": "initial", "dirty": False}, "initial Git branch")
            (repo / "new.txt").write_text("new\n")
            app.wait_for(lambda: observed() == {"branch": "initial", "dirty": True}, "untracked dirty state")
            subprocess.check_call(["git", "-C", str(repo), "add", "new.txt"])
            subprocess.check_call(["git", "-C", str(repo), "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                                   "-c", "commit.gpgsign=false", "commit", "-m", "fixture"])
            app.wait_for(lambda: observed() == {"branch": "initial", "dirty": False}, "clean committed state")
            subprocess.check_call(["git", "-C", str(repo), "switch", "-c", "feature"])
            app.wait_for(lambda: observed() == {"branch": "feature", "dirty": False}, "branch change")
            assert metadata()["directory"] == str(repo)
            assert metadata()["ahead"] is None and metadata()["upstream"] is None
            subprocess.check_call(["git", "-C", str(repo), "branch", "--set-upstream-to=initial", "feature"])
            subprocess.check_call(["git", "-C", str(repo), "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                                   "-c", "commit.gpgsign=false", "commit", "--allow-empty", "-m", "ahead"])
            app.wait_for(lambda: metadata() and metadata()["ahead"] == 1, "local ahead count")
            assert metadata()["behind"] == 0 and metadata()["upstream"] == "initial"
            (repo / ".cmux").mkdir()
            (repo / ".cmux/cmux.json").write_text(json.dumps({"actions": {"fixture.repo": {"command": "touch MUST_NOT_RUN"}}}))
            (root / "cmux.json").write_text(json.dumps({"actions": {"fixture.parent": {"command": "pwd"}}}))
            app.cli("new-workspace", "--name", "observer")
            active = next(row["uuid"] for row in app.surfaces() if row["active"])
            (repo / "new.txt").write_text("edited\n")
            app.wait_for(lambda: observed() == {"branch": "feature", "dirty": True}, "background workspace change")
            assert next(row["uuid"] for row in app.surfaces() if row["active"]) == active
            actions = json.loads(app.cli("project-actions", "--workspace", target["workspace_uuid"], "--json"))
            assert actions["workspace_id"] == target["workspace_uuid"]
            assert actions["config"]["directory"] == str(repo)
            assert actions["config"]["actions"]["fixture.repo"]["intent"]["command"] == "touch MUST_NOT_RUN"
            assert "fixture.parent" not in actions["config"]["actions"]
            assert not (repo / "MUST_NOT_RUN").exists()
            assert next(row["uuid"] for row in app.surfaces() if row["active"]) == active
            # Explicit execution re-reads the definition before delivering any terminal input.
            config_file = repo / ".cmux/cmux.json"
            config_file.write_text(json.dumps({"actions": {"fixture.repo": {"command": "pwd > executed.cwd"}}}))
            try:
                app.cli("project-run", "fixture.repo", "--workspace", target["workspace_uuid"],
                        "--fingerprint", actions["config"]["actions"]["fixture.repo"]["fingerprint"])
                raise AssertionError("stale action fingerprint accepted")
            except subprocess.CalledProcessError:
                pass
            assert not (repo / "executed.cwd").exists()
            assert not (repo / "MUST_NOT_RUN").exists()
            assert next(row["uuid"] for row in app.surfaces() if row["active"]) == active
            reviewed = json.loads(app.cli("project-actions", "--workspace", target["workspace_uuid"], "--json"))
            before = {row["uuid"] for row in app.surfaces()}
            submitted = json.loads(app.cli("project-run", "fixture.repo", "--workspace", target["workspace_uuid"],
                "--fingerprint", reviewed["config"]["actions"]["fixture.repo"]["fingerprint"], "--json"))
            assert submitted["status"] == "submitted" and submitted["workspace_id"] == target["workspace_uuid"]
            assert submitted["surface_id"] not in before
            app.wait_for(lambda: (repo / "executed.cwd").exists(), "project command execution")
            assert (repo / "executed.cwd").read_text().strip() == str(repo)
            assert next(row["uuid"] for row in app.surfaces() if row["active"]) == submitted["surface_id"]
            app.cli("close-surface", submitted["surface_id"])
            config_file.write_text(json.dumps({"actions": {"fixture.repo": {
                "command": "pwd > current.cwd", "target": "currentTerminal"}}}))
            current = json.loads(app.cli("project-actions", "--workspace", target["workspace_uuid"], "--json"))
            before = {row["uuid"] for row in app.surfaces()}
            submitted = json.loads(app.cli("project-run", "fixture.repo", "--workspace", target["workspace_uuid"],
                "--fingerprint", current["config"]["actions"]["fixture.repo"]["fingerprint"], "--json"))
            assert submitted["surface_id"] == target["uuid"]
            app.wait_for(lambda: (repo / "current.cwd").exists(), "current-terminal action execution")
            assert (repo / "current.cwd").read_text().strip() == str(repo)
            assert {row["uuid"] for row in app.surfaces()} == before
            for builtin in ["cmux.newTerminal", "cmux.splitRight", "cmux.splitDown"]:
                config_file.write_text(json.dumps({"actions": {"fixture.builtin": {"builtin": builtin}}}))
                reviewed = json.loads(app.cli("project-actions", "--workspace", target["workspace_uuid"], "--json"))
                before = {row["uuid"] for row in app.surfaces()}
                created = json.loads(app.cli("project-run", "fixture.builtin", "--workspace", target["workspace_uuid"],
                    "--fingerprint", reviewed["config"]["actions"]["fixture.builtin"]["fingerprint"], "--json"))
                assert created["status"] == "submitted"
                assert created["surface_id"] not in before
                assert {row["uuid"] for row in app.surfaces()} == before | {created["surface_id"]}
                assert next(row["uuid"] for row in app.surfaces() if row["active"]) == created["surface_id"]
                app.cli("close-surface", created["surface_id"])
                app.cli("focus-surface", target["uuid"])
            config_file.write_text(json.dumps({"actions": {"fixture.workspace": {"builtin": "cmux.newWorkspace"}}}))
            reviewed = json.loads(app.cli("project-actions", "--workspace", target["workspace_uuid"], "--json"))
            workspaces_before = json.loads(app.cli("list-workspaces", "--json"))["workspaces"]
            created = json.loads(app.cli("project-run", "fixture.workspace", "--workspace", target["workspace_uuid"],
                "--fingerprint", reviewed["config"]["actions"]["fixture.workspace"]["fingerprint"], "--json"))
            assert created["workspace_id"] not in {row["uuid"] for row in workspaces_before}
            assert created["source_workspace_id"] == target["workspace_uuid"]
            current_workspace = json.loads(app.cli("current-workspace", "--json"))
            assert current_workspace["uuid"] == created["workspace_id"]
            assert current_workspace["working_directory"] == str(repo)
            assert next(row["uuid"] for row in app.surfaces() if row["active"]) == created["surface_id"]
            assert len(json.loads(app.cli("list-workspaces", "--json"))["workspaces"]) == len(workspaces_before) + 1
            app.cli("close-workspace", created["workspace_id"])
            app.cli("select-workspace", next(row["workspace_uuid"] for row in app.surfaces() if row["uuid"] == active))
            uri = "file://" + socket.gethostname() + str(root)
            app.cli("send-text", "--id", target["uuid"], "cd " + shlex.quote(str(root)) + "; printf '\\033]7;%s\\007' " + shlex.quote(uri))
            app.cli("send-key", "--id", target["uuid"], "\r")
            app.wait_for(lambda: observed() is None, "Git state cleared outside repository")
            moved = json.loads(app.cli("project-actions", "--workspace", target["workspace_uuid"], "--json"))
            assert moved["config"]["directory"] == str(root)
            assert "fixture.parent" in moved["config"]["actions"]
            assert "fixture.repo" not in moved["config"]["actions"]
            assert next(row["uuid"] for row in app.surfaces() if row["active"]) == active
    print("automatic Git branch, dirty, background update and directory invalidation passed")


if __name__ == "__main__":
    main()
