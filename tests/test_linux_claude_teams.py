#!/usr/bin/env python3
"""Exercise the Claude Teams launcher and native tmux split translation in GTK."""
import json
from pathlib import Path
import shlex
import subprocess
import tempfile

from linux_app import running_app


def main():
    """Require managed launch environment, argument forwarding and an executable teammate pane."""
    with tempfile.TemporaryDirectory(prefix="cmux-claude-teams-") as directory:
        root = Path(directory)
        fake_bin = root / "bin"
        fake_bin.mkdir()
        teammate = root / "teammate-ran"
        marker = root / "teams-complete"
        launch = root / "launch.json"
        claude = fake_bin / "claude"
        claude.write_text(
            "#!/usr/bin/env python3\n"
            "import json,os,subprocess,sys\n"
            f"json.dump({{'argv':sys.argv[1:],'teams':os.environ.get('CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS'),'tmux':os.environ.get('TMUX'),'pane':os.environ.get('TMUX_PANE'),'program':os.environ.get('TERM_PROGRAM')}},open({str(launch)!r},'w'))\n"
            "created=[]\n"
            "for index in range(3):\n"
            f" command={('printf teammate > ' + shlex.quote(str(teammate)))!r} if index == 2 else ':'\n"
            " created.append(subprocess.check_output(['tmux','split-window','-h','-P','-F','#{pane_id}',command],text=True).strip())\n"
            " subprocess.run(['tmux','select-layout','-t','cmux:1','main-vertical'],check=True)\n"
            "assert len(set(created)) == 3\n"
            "panes=subprocess.check_output(['tmux','list-panes','-t',os.environ['TMUX_PANE'],'-F','#{pane_id}'],text=True).splitlines()\n"
            "assert len(panes) == 4\n"
            f"open({str(marker)!r},'w').write('ok')\n"
        )
        claude.chmod(0o700)
        environment = {"PATH": str(fake_bin) + ":" + __import__("os").environ["PATH"], "SHELL": "/bin/bash"}
        with running_app(root, environment) as app:
            source = next(row["uuid"] for row in app.surfaces() if row["active"])
            binary = Path(app.environment.get("CMUX_BIN_DIR", "target/debug")).resolve() / "cmux"
            subprocess.run(
                [str(binary), "claude-teams", "--model", "sonnet"],
                env=dict(
                    app.environment,
                    CMUX_SURFACE_ID=source,
                    CMUX_SOCKET_PATH=str(app.socket_path),
                ),
                check=True,
                timeout=30,
            )
            assert marker.exists()
            app.wait_for(teammate.exists, "native teammate command")
            app.wait_for(lambda: len(app.surfaces()) == 4, "native teammate splits")
            payload = json.loads(launch.read_text())
            assert payload["argv"][:3] == ["--teammate-mode", "auto", "--append-system-prompt"]
            assert "NAMED teammates" in payload["argv"][3]
            assert payload["argv"][-2:] == ["--model", "sonnet"]
            assert payload["teams"] == "1" and payload["program"] == "cmux"
            assert payload["tmux"].startswith("cmux,") and payload["pane"] == "%" + source


if __name__ == "__main__":
    main()
