#!/usr/bin/env python3
"""Render the generated self-contained diff surface in pinned Chromium."""

import json
from pathlib import Path
import shutil
import subprocess
import tempfile

from browser_process_support import BrowserProcesses
from linux_app import running_app
from test_multi_workspace_focus import selected_surface


def main():
    """Prove agent-facing DOM, layout switching and escaped patch content end to end."""
    with tempfile.TemporaryDirectory(prefix="cmux-real-diff-") as directory:
        root = Path(directory)
        browser_dir = root / "browser"
        browser_dir.mkdir()
        browser = shutil.which("agent-browser")
        if browser is None:
            raise RuntimeError("agent-browser is required for the real diff fixture")
        repository = root / "repository"
        repository.mkdir()
        subprocess.run(["git", "init", "-q", "-b", "main", repository], check=True)
        subprocess.run(["git", "-C", repository, "config", "user.name", "cmux fixture"], check=True)
        subprocess.run(["git", "-C", repository, "config", "user.email", "fixture@example.test"], check=True)
        (repository / "one.txt").write_text("before\n")
        (repository / "two.txt").write_text("")
        subprocess.run(["git", "-C", repository, "add", "."], check=True)
        subprocess.run(["git", "-C", repository, "commit", "-qm", "base"], check=True)
        (repository / "one.txt").write_text(
            "after marker\n<script>window.cmuxInjected=true</script>\n"
        )
        (repository / "two.txt").write_text("another marker\n")
        processes = BrowserProcesses(browser_dir)
        with running_app(root, {
            "CMUX_BIN_DIR": "target/release",
            "CMUX_AGENT_BROWSER": browser,
            "AGENT_BROWSER_SOCKET_DIR": str(browser_dir),
        }) as app:
            app.wait_for(lambda: bool(app.children()), "initial terminal")
            terminal = selected_surface(app)
            comment = json.loads(app.cli(
                "comments", "add", "--repo", str(repository), "--file", "one.txt",
                "--side", "new", "--line", "1", "--line-text", "after marker",
                "--message", "Check this value", "--json",
            ))["comment"]
            opened = json.loads(app.cli(
                "diff", "--unstaged", "--cwd", str(repository), "--layout", "split", "--json",
                timeout=35,
            ))
            surface = opened["surface_ref"]

            def command(name, *arguments):
                result = json.loads(app.cli("browser", name, surface, *arguments, timeout=20))
                assert result["success"] is True, result
                return result["data"]

            command("wait", "--selector", "#viewer .split-line", "--timeout-ms", "5000")
            state = command("eval", "({files:[...document.querySelectorAll('#files button')].map(x=>x.textContent), split:document.querySelectorAll('.split-line').length, comments:[...document.querySelectorAll('.review-comment')].map(x=>({id:x.dataset.commentId,message:x.querySelector('.comment-message').textContent,label:x.querySelector('.comment-meta span').textContent})), injected:!!window.cmuxInjected, text:document.body.innerText})")["result"]
            assert state["files"] == ["one.txt", "two.txt"], state
            assert state["split"] > 0 and state["injected"] is False, state
            assert state["comments"] == [{
                "id": comment["id"], "message": "Check this value", "label": "new 1",
            }], state
            assert "after marker" in state["text"] and "another marker" not in state["text"], state
            command("click", "#viewer .split-side.add")
            command("fill", "#comment-message", "Created inside the diff viewer")
            command("click", "#comment-form button[type=submit]")

            def in_view_comment_saved():
                listed = json.loads(app.cli(
                    "comments", "list", "--repo", str(repository), "--json",
                ))["comments"]
                return any(row["message"] == "Created inside the diff viewer" for row in listed)

            app.wait_for(in_view_comment_saved, "in-view diff comment persistence")
            command(
                "wait", "--function",
                "[...document.querySelectorAll('.review-comment')].some(x => x.textContent.includes('Created inside the diff viewer'))",
                "--timeout-ms", "5000",
            )
            command("click", "#unified")
            command("wait", "--function", "document.querySelectorAll('#viewer .line').length > 0", "--timeout-ms", "1000")
            unified = command("eval", "({unified:document.querySelectorAll('#viewer .line').length, split:document.querySelectorAll('#viewer .split-line').length})")["result"]
            assert unified["unified"] > 0 and unified["split"] == 0, unified
            command("click", "#files button:nth-child(2)")
            assert "another marker" in command("eval", "document.body.innerText")["result"]
            assert selected_surface(app) == terminal, "default diff open stole terminal focus"
            assert processes.sample()["daemon_count"] == 1
    print("real Chromium rendered and exposed the bounded diff surface to browser automation")


if __name__ == "__main__":
    main()
