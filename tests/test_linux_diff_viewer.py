#!/usr/bin/env python3
"""Exercise bounded diff preparation, right-hand placement and durable browser ownership."""

import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import tempfile

from linux_app import running_app


def pane_for(panes, surface):
    """Return the stable pane reference containing a surface UUID."""
    return next(row["id"] for row in panes["panes"] if surface in row["surface_ids"])


def main():
    """Verify file and Git inputs without running browser code in this mock-backed stage."""
    with tempfile.TemporaryDirectory(prefix="cmux-diff-") as directory:
        root = Path(directory)
        browser_dir = root / "browser"
        browser_dir.mkdir()
        mock = root / "agent-browser"
        shutil.copyfile(Path(__file__).parent / "fixtures/mock_agent_browser.py", mock)
        mock.chmod(0o700)
        environment = {
            "CMUX_AGENT_BROWSER": str(mock),
            "AGENT_BROWSER_SOCKET_DIR": str(browser_dir),
            "SHELL": "/bin/bash",
        }
        patch = root / "review.patch"
        patch.write_text(
            "diff --git a/alpha.txt b/alpha.txt\n"
            "--- a/alpha.txt\n+++ b/alpha.txt\n@@ -1 +1,2 @@\n-old\n+new\n"
            "+</script><script>window.cmuxInjected=true</script>\n"
            "diff --git a/beta.txt b/beta.txt\n"
            "--- a/beta.txt\n+++ b/beta.txt\n@@ -0,0 +1 @@\n+second file\n"
        )
        with running_app(root, environment) as app:
            app.wait_for(lambda: bool(app.children()), "initial terminal")
            terminal = next(row["uuid"] for row in app.surfaces() if row["active"])
            before = json.loads(app.cli("list-panes", "--json"))
            opened = json.loads(
                app.cli(
                    "diff", str(patch), "--layout", "split", "--title", "Review", "--json",
                    timeout=35,
                )
            )
            assert opened["source"] == str(patch) and opened["layout"] == "split", opened
            assert Path(opened["path"]).is_file() and opened["url"].startswith("file://"), opened
            assert stat.S_IMODE(Path(opened["path"]).stat().st_mode) == 0o600
            html = Path(opened["path"]).read_text()
            assert "alpha.txt" in html and "beta.txt" in html and "second file" in html
            assert "</script><script>window.cmuxInjected" not in html
            assert "\\u003c/script\\u003e\\u003cscript\\u003ewindow.cmuxInjected" in html
            panes = json.loads(app.cli("list-panes", "--json"))
            assert pane_for(panes, terminal) != pane_for(panes, opened["uuid"]), panes
            assert next(row for row in panes["panes"] if row["focused"])["active_surface_uuid"] == terminal
            assert len(panes["panes"]) == len(before["panes"]) + 1
            navigation = json.loads((browser_dir / "last-navigation.json").read_text())
            assert navigation["url"] == opened["url"], navigation

            repository = root / "repository"
            repository.mkdir()
            subprocess.run(["git", "init", "-q", "-b", "main", repository], check=True)
            subprocess.run(["git", "-C", repository, "config", "user.name", "cmux fixture"], check=True)
            subprocess.run(["git", "-C", repository, "config", "user.email", "fixture@example.test"], check=True)
            tracked = repository / "tracked.txt"
            tracked.write_text("before\n")
            subprocess.run(["git", "-C", repository, "add", "tracked.txt"], check=True)
            subprocess.run(["git", "-C", repository, "commit", "-qm", "base"], check=True)
            tracked.write_text("after\n")
            comment = json.loads(app.cli(
                "comments", "add", "--repo", str(repository), "--file", "tracked.txt",
                "--side", "new", "--line", "1", "--line-text", "after",
                "--message", "Please verify <the changed value>.", "--json",
            ))["comment"]
            listed = json.loads(app.cli("comments", "list", "--repo", str(repository), "--json"))
            assert listed["count"] == 1 and listed["comments"][0]["id"] == comment["id"], listed
            git_view = json.loads(
                app.cli("diff", "--unstaged", "--cwd", str(repository), "--focus", "--json", timeout=35)
            )
            assert git_view["source"] == "git unstaged"
            git_html = Path(git_view["path"]).read_text()
            assert "after" in git_html and comment["id"] in git_html
            assert "Please verify \\u003cthe changed value\\u003e." in git_html
            panes = json.loads(app.cli("list-panes", "--json"))
            assert next(row for row in panes["panes"] if row["focused"])["active_surface_uuid"] == git_view["uuid"]

            consumed = json.loads(app.cli(
                "comments", "consume", comment["id"], "--repo", str(repository), "--json",
            ))
            assert consumed["consumed"] == 1, consumed
            assert json.loads(app.cli(
                "comments", "list", "--repo", str(repository), "--json",
            ))["count"] == 0
            historical = json.loads(app.cli(
                "comments", "list", "--repo", str(repository), "--all", "--json",
            ))
            assert historical["count"] == 1 and historical["comments"][0]["consumedAt"] is not None
            assert json.loads(app.cli(
                "comments", "delete", comment["id"], "--repo", str(repository), "--json",
            ))["ok"] is True
            writers = [
                subprocess.Popen(
                    [
                        "target/debug/cmux", "comments", "add", "--repo", str(repository),
                        "--file", "tracked.txt", "--side", "new", "--line", "1",
                        "--message", f"parallel comment {index}", "--json",
                    ],
                    env=app.environment,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    text=True,
                )
                for index in range(8)
            ]
            for writer in writers:
                _, stderr = writer.communicate(timeout=10)
                assert writer.returncode == 0, stderr
            concurrent = json.loads(app.cli(
                "comments", "list", "--repo", str(repository), "--json",
            ))
            assert concurrent["count"] == 8, concurrent
            assert json.loads(app.cli(
                "comments", "consume", "--all", "--repo", str(repository), "--json",
            ))["consumed"] == 8

            hook_payload = json.dumps({
                "hook_event_name": "UserPromptSubmit",
                "session_id": "diff-session",
                "cwd": str(repository),
            })
            hook = subprocess.run(
                ["target/debug/cmux", "--socket", str(app.socket_path),
                 "hooks", "claude", "prompt-submit"],
                env=dict(app.environment, CMUX_SURFACE_ID=terminal), input=hook_payload,
                capture_output=True, text=True, timeout=10,
            )
            assert hook.returncode == 0, hook.stderr
            turn_one = repository / "turn-one.txt"
            turn_one.write_text("created in first turn\n")
            first_turn = json.loads(app.cli(
                "diff", "--last-turn", "--cwd", str(repository),
                "--surface", terminal, "--session", "diff-session", "--json", timeout=35,
            ))
            first_html = Path(first_turn["path"]).read_text()
            assert "created in first turn" in first_html, first_html

            hook = subprocess.run(
                ["target/debug/cmux", "--socket", str(app.socket_path),
                 "hooks", "claude", "prompt-submit"],
                env=dict(app.environment, CMUX_SURFACE_ID=terminal), input=hook_payload,
                capture_output=True, text=True, timeout=10,
            )
            assert hook.returncode == 0, hook.stderr
            turn_two = repository / "turn-two.txt"
            turn_two.write_text("created in second turn\n")
            second_turn = json.loads(app.cli(
                "diff", "--last-turn", "--cwd", str(repository),
                "--surface", terminal, "--session", "diff-session", "--json", timeout=35,
            ))
            second_html = Path(second_turn["path"]).read_text()
            assert "created in second turn" in second_html, second_html
            assert "created in first turn" not in second_html, second_html

            missing = json.loads(app.cli(
                "diff", "--last-turn", "--cwd", str(repository),
                "--surface", terminal, "--session", "missing-session", "--json", timeout=35,
            ))
            assert "created in second turn" not in Path(missing["path"]).read_text()

            fifo = root / "blocked.patch"
            os.mkfifo(fifo)
            rejected = subprocess.run(
                ["target/debug/cmux", "--socket", str(app.socket_path), "diff", str(fifo)],
                env=app.environment, capture_output=True, text=True, timeout=5,
            )
            assert rejected.returncode == 1 and "expected a regular file" in rejected.stderr, rejected.stderr

        with running_app(root, environment) as restored:
            restored_urls = {
                row["url"] for row in json.loads(restored.cli("browser", "list"))["surfaces"]
            }
            expected_urls = {
                opened["url"], git_view["url"], first_turn["url"],
                second_turn["url"], missing["url"],
            }
            assert expected_urls <= restored_urls, restored_urls
    print("diff surfaces preserve bounded input, placement, focus and restart identity")


if __name__ == "__main__":
    main()
