#!/usr/bin/env python3
"""Render the manifest-aware Linux project view in pinned Chromium."""

import json
from pathlib import Path
import shutil
import tempfile

from browser_process_support import BrowserProcesses
from linux_app import running_app
from test_multi_workspace_focus import selected_surface


def main():
    """Prove file, target, settings and action tabs through the public browser API."""
    with tempfile.TemporaryDirectory(prefix="cmux-real-project-") as directory:
        root = Path(directory)
        browser_dir = root / "browser"
        browser_dir.mkdir()
        browser = shutil.which("agent-browser")
        if browser is None:
            raise RuntimeError("agent-browser is required for the real project fixture")
        project = root / "manifest-project"
        (project / "src").mkdir(parents=True)
        (project / "Cargo.toml").write_text(
            '[package]\nname = "viewer-core"\nversion = "0.1.0"\n'
            '[[bin]]\nname = "viewer-cli"\npath = "src/main.rs"\n'
        )
        (project / "src/main.rs").write_text("fn main() {}\n")
        (project / "README.md").write_text("project marker\n")
        processes = BrowserProcesses(browser_dir)
        with running_app(root, {
            "CMUX_BIN_DIR": "target/release",
            "CMUX_AGENT_BROWSER": browser,
            "AGENT_BROWSER_SOCKET_DIR": str(browser_dir),
        }) as app:
            app.wait_for(lambda: bool(app.children()), "initial terminal")
            terminal = selected_surface(app)
            opened = json.loads(app.cli("project", str(project), "--json", timeout=35))
            surface = opened["surface_ref"]

            def command(name, *arguments):
                result = json.loads(app.cli("browser", name, surface, *arguments, timeout=20))
                assert result["success"] is True, result
                return result["data"]

            command("wait", "--selector", "#file-list .row", "--timeout-ms", "5000")
            files = command("eval", "[...document.querySelectorAll('#file-list .row')].map(x=>x.textContent)")["result"]
            assert "Cargo.toml" in files and "src/main.rs" in files, files
            command("click", "button[data-tab=targets]")
            targets = command("eval", "document.querySelector('#targets').innerText")["result"]
            assert "viewer-core" in targets and "viewer-cli" in targets, targets
            command("click", "button[data-tab=settings]")
            assert "Rust" in command("eval", "document.querySelector('#settings').innerText")["result"]
            command("click", "button[data-tab=actions]")
            document = command("eval", "document.querySelector('#action-json').textContent")["result"]
            assert document.startswith("{"), document
            assert selected_surface(app) == terminal, "default project open stole terminal focus"
            assert processes.sample()["daemon_count"] == 1
    print("real Chromium rendered the Linux project inspector through browser automation")


if __name__ == "__main__":
    main()
