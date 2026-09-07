#!/usr/bin/env python3
"""Exercise bounded manifest discovery, right-hand placement and project-view restoration."""

import json
from pathlib import Path
import shutil
import stat
import tempfile

from linux_app import running_app


def pane_for(panes, surface):
    """Return the stable pane record containing a surface UUID."""
    return next(row for row in panes["panes"] if surface in row["surface_ids"])


def saved_has_url(root, url):
    """Observe the debounced session snapshot before the fixture sends SIGTERM."""
    try:
        session = json.loads((root / "data/cmux/session.json").read_text())
    except (FileNotFoundError, json.JSONDecodeError):
        return False

    def contains(node):
        if "surfaces" in node:
            return any(row.get("url") == url for row in node["surfaces"])
        return contains(node.get("start", {})) or contains(node.get("end", {}))

    return any(contains(workspace["layout"]) for workspace in session["workspaces"])


def main():
    """Verify Linux manifest adaptation without executing browser JavaScript."""
    with tempfile.TemporaryDirectory(prefix="cmux-project-") as directory:
        root = Path(directory)
        browser_dir = root / "browser"
        browser_dir.mkdir()
        mock = root / "agent-browser"
        shutil.copyfile(Path(__file__).parent / "fixtures/mock_agent_browser.py", mock)
        mock.chmod(0o700)
        project = root / "sample"
        (project / "src").mkdir(parents=True)
        (project / "target").mkdir()
        (project / "Cargo.toml").write_text(
            '[package]\nname = "linux-project"\nversion = "0.1.0"\n'
            '[[bin]]\nname = "project-tool"\npath = "src/main.rs"\n'
        )
        (project / "package.json").write_text(json.dumps({
            "name": "web-part", "scripts": {"check": "echo </script>safe"},
        }))
        (project / "go.mod").write_text("module example.test/project\n\ngo 1.24\n")
        (project / "src/main.rs").write_text("fn main() {}\n")
        (project / "target/ignored.txt").write_text("ignored\n")
        environment = {
            "CMUX_AGENT_BROWSER": str(mock),
            "AGENT_BROWSER_SOCKET_DIR": str(browser_dir),
            "SHELL": "/bin/bash",
        }
        with running_app(root, environment) as app:
            app.wait_for(lambda: bool(app.children()), "initial terminal")
            terminal = next(row["uuid"] for row in app.surfaces() if row["active"])
            opened = json.loads(app.cli(
                "project", str(project), "--surface", terminal, "--json", timeout=35,
            ))
            assert opened["project_root"] == str(project.resolve()), opened
            assert opened["manifest_count"] == 3 and opened["target_count"] == 5, opened
            path = Path(opened["path"])
            assert path.is_file() and stat.S_IMODE(path.stat().st_mode) == 0o600
            html = path.read_text()
            assert "linux-project" in html and "project-tool" in html
            assert "example.test/project" in html and "target/ignored.txt" not in html
            assert "</script>safe" not in html and "\\u003c/script\\u003esafe" in html
            panes = json.loads(app.cli("list-panes", "--json"))
            assert pane_for(panes, opened["uuid"])["id"] != pane_for(panes, terminal)["id"]
            assert pane_for(panes, terminal)["focused"] is True
            navigation = json.loads((browser_dir / "last-navigation.json").read_text())
            assert navigation["url"] == opened["url"], navigation
            app.wait_for(lambda: saved_has_url(root, opened["url"]), "project session snapshot")

        with running_app(root, environment) as restored:
            urls = {row["url"] for row in json.loads(restored.cli("browser", "list"))["surfaces"]}
            assert opened["url"] in urls, urls
    print("project view preserved bounded manifests, placement and restart identity")


if __name__ == "__main__":
    main()
