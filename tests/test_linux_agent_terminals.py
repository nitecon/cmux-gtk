#!/usr/bin/env python3
"""Verify agent argv, background startup, launch context and session persistence in real PTYs."""

import json
import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import uuid

from linux_app import running_app
from process_support import stop_process
from test_linux_resume_approval import quit_app


def terminal(directory, agent=None):
    """Build a backward-compatible terminal snapshot with an optional fixed agent choice."""
    result = {"type": "Terminal", "surface_uuid": str(uuid.uuid4()),
              "shell": "/bin/sh", "cwd": str(directory)}
    if agent is not None:
        result["agent"] = agent
    return result


def workspace(name, directory, surfaces, script=None):
    """Keep a selected terminal identity while allowing other sibling tabs to remain hidden."""
    return {"uuid": str(uuid.uuid4()), "name": name,
            "working_directory": str(directory), "startup_script": str(script) if script else None,
            "active_pane_uuid": surfaces[0]["surface_uuid"],
            "layout": {"type": "Pane", "active_surface_uuid": surfaces[0]["surface_uuid"],
                       "surfaces": surfaces}}


def main():
    """Launch all hidden agents twice, then change each restore choice and observe new arguments."""
    with tempfile.TemporaryDirectory(prefix="cmux-agent-terminals-") as directory:
        root = Path(directory)
        project = root / "project with 'quotes'"
        project.mkdir()
        binary_dir = root / "bin"
        binary_dir.mkdir()
        log_dir = root / "launches"
        log_dir.mkdir()
        fixture = """#!/usr/bin/python3
import json, os, pathlib, sys
record = {"kind": pathlib.Path(sys.argv[0]).name, "argv": sys.argv[1:],
          "cwd": os.getcwd(), "surface": os.environ["CMUX_SURFACE_ID"],
          "value": os.environ.get("LAUNCH_FIXTURE"), "tty": os.isatty(0)}
with (pathlib.Path(os.environ["AGENT_LAUNCH_LOG"]) / (record["surface"] + ".jsonl")).open("a") as log:
    log.write(json.dumps(record) + "\\n")
print("AGENT-LAUNCHED", flush=True)
"""
        for kind in ("codex", "claude"):
            executable = binary_dir / kind
            executable.write_text(fixture)
            executable.chmod(0o755)
        script = root / "startup.sh"
        script.write_text("sleep 0.25\nexport PATH=" + shlex.quote(str(binary_dir)) + ':"$PATH"\n')
        config = root / "config/ghostty"
        config.mkdir(parents=True)
        (config / "config").write_text("command = /bin/sh\nshell-integration = none\n")
        plain = terminal(project)
        agents = [terminal(project, {"kind": kind, "resume_last": resume})
                  for kind in ("codex", "claude") for resume in (False, True)]
        selected = workspace("ordinary terminal", project, [plain])
        background = workspace("background agents", project, agents, script)
        background["launch_environment"] = {"LAUNCH_FIXTURE": "literal $HOME and 'quotes'"}
        session = root / "data/cmux/session.json"
        session.parent.mkdir(parents=True)
        session.write_text(json.dumps({"version": 3, "active_index": 0,
                                       "workspaces": [selected, background]}))
        env = {"SHELL": "/bin/sh", "AGENT_LAUNCH_LOG": str(log_dir)}
        wm = subprocess.Popen(["openbox"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            for cycle in range(3):
                with running_app(root, env) as app:
                    def launches():
                        """Count complete fixture output records, tolerating a concurrently written final line."""
                        records = {}
                        for saved in agents:
                            path = log_dir / (saved["surface_uuid"] + ".jsonl")
                            try:
                                records[saved["surface_uuid"]] = [json.loads(line) for line in path.read_text().splitlines()]
                            except (FileNotFoundError, json.JSONDecodeError):
                                return None
                        return records if all(len(rows) == cycle + 1 for rows in records.values()) else None

                    # Never select the agents: every hidden tab must start on its own.
                    app.wait_for(launches, "all background agent launches", timeout=20)
                    records = launches()
                    for saved in agents:
                        choice = saved["agent"]
                        argv = (["resume", "--last"] if choice["kind"] == "codex" else ["--continue"]) if choice["resume_last"] else []
                        observed = records[saved["surface_uuid"]][-1]
                        assert observed == {"kind": choice["kind"], "argv": argv, "cwd": str(project),
                                            "surface": saved["surface_uuid"],
                                            "value": background["launch_environment"]["LAUNCH_FIXTURE"], "tty": True}, observed
                    assert json.loads(app.cli("current-workspace", "--json"))["uuid"] == selected["uuid"]
                    assert next(row["uuid"] for row in app.surfaces() if row["active"]) == plain["surface_uuid"]
                    assert not (log_dir / (plain["surface_uuid"] + ".jsonl")).exists()
                    result = root / f"ordinary-{cycle}"
                    app.wait_for(lambda: bool(json.loads(app.cli("read-text", "--id", plain["surface_uuid"], "--json"))["text"].strip()), "ordinary shell readiness")
                    app.cli("send-text", "--id", plain["surface_uuid"], "printf regular > " + shlex.quote(str(result)))
                    app.cli("send-key", "--id", plain["surface_uuid"], "\r")
                    app.wait_for(lambda: result.exists() and result.read_text() == "regular", "ordinary terminal remains usable")
                    quit_app(app)
                saved_session = json.loads(session.read_text())
                stored = saved_session["workspaces"][1]["layout"]["surfaces"]
                assert [(row["surface_uuid"], row["agent"]) for row in stored] == [(row["surface_uuid"], row["agent"]) for row in agents]
                assert "agent" not in saved_session["workspaces"][0]["layout"]["surfaces"][0]
                if cycle == 1:
                    for row, expected in zip(stored, agents):
                        row["agent"]["resume_last"] = not row["agent"]["resume_last"]
                        expected["agent"]["resume_last"] = row["agent"]["resume_last"]
                    session.write_text(json.dumps(saved_session))
        finally:
            stop_process(wm)
    print("Codex/Claude launched once per restart in hidden PTYs with saved argv and context; ordinary terminals stayed usable")


if __name__ == "__main__":
    main()
