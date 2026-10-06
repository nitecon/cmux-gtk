#!/usr/bin/env python3
"""Exercise actual GTK terminal delivery against an authenticated, isolated WebSocket gateway."""
import json
import os
import shlex
from pathlib import Path
import subprocess
import tempfile

from gateway_fixture import Gateway
from linux_app import running_app


AGENT = r'''#!/usr/bin/env python3
"""A native PTY agent fixture: lifecycle hooks and bracketed-paste input, never a shell receiver."""
import json, os, pathlib, subprocess, sys, termios, tty
root = pathlib.Path(os.environ["FAKE_ROOT"])
provider = pathlib.Path(sys.argv[0]).name
native_id = provider + "-native"
binary = os.environ["CMUX_TEST_CLI"]
def event(command, name, **extra):
    payload = dict(hook_event_name=name, session_id=native_id, cwd=str(root), **extra)
    subprocess.run([binary, "hooks", provider, command], input=json.dumps(payload), text=True, check=True, timeout=10)
old = termios.tcgetattr(0)
try:
    tty.setraw(0)
    sys.stdout.write("\x1b[?2004hNATIVE_READY\r\n"); sys.stdout.flush()
    event("session-start", "SessionStart")
    (root / (provider + "-ready")).write_text(os.environ["CMUX_SURFACE_ID"])
    buffer = b""
    while True:
        buffer += os.read(0, 65536)
        end = buffer.find(b"\x1b[201~")
        if end >= 0 and buffer[end + 6:] in (b"\r", b"\n", b"\r\n"):
            start = buffer.find(b"\x1b[200~")
            assert start >= 0, repr(buffer)
            text = buffer[start + 6:end].decode()
            with (root / (provider + "-input")).open("a") as f:
                f.write(json.dumps(text) + "\n")
            buffer = b""
            event("prompt-submit", "UserPromptSubmit")
            event("stop", "Stop", last_assistant_message="One fixture turn ended; task remains open")
finally:
    termios.tcsetattr(0, termios.TCSANOW, old)
'''


def raw(app, method, **params):
    """Run the production JSON socket CLI, exposing any protocol errors to the scenario."""
    return json.loads(app.cli("raw", method, "--params", json.dumps(params), "--json"))


def status(app):
    """Read the bridge's credential-free, durable view through its public command."""
    return json.loads(app.cli("gateway", "status", "--json"))


def rejects(app, method, **params):
    """Require a clear nonzero rejection without mutating a target terminal."""
    try:
        raw(app, method, **params)
    except subprocess.CalledProcessError:
        return
    raise AssertionError("operation unexpectedly accepted: " + method)


def launch(app, root, provider):
    """Start an attached native fake agent, waiting for its real SessionStart hook."""
    surface = next(row["uuid"] for row in app.surfaces() if row["active"])
    app.wait_for(lambda: raw(app, "surface.read_text", id=surface).get("text"), "shell readiness")
    raw(app, "surface.send_text", id=surface, text=shlex.quote(str(root / "bin" / provider)))
    raw(app, "surface.send_key", id=surface, key="\r")
    app.wait_for(lambda: (root / (provider + "-ready")).exists(), provider + " native hook readiness")
    assert (root / (provider + "-ready")).read_text() == surface
    return surface


def inputs(root, provider):
    """Count actual PTY prompt submissions, excluding protocol acknowledgment counts."""
    path = root / (provider + "-input")
    return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []


def quit_app(app):
    """Use the product quit shortcut so terminal snapshots reach durable session storage."""
    windows = subprocess.check_output(["xdotool", "search", "--onlyvisible", "--pid", str(app.process.pid)], text=True, timeout=10).split()
    subprocess.check_call(["xdotool", "windowfocus", "--sync", windows[-1]], timeout=10)
    subprocess.check_call(["xdotool", "key", "--clearmodifiers", "ctrl+q"], timeout=10)
    assert app.process.wait(timeout=15) == 0
    app.socket_path.unlink(missing_ok=True)


def main():
    """Prove exact delivery, acknowledgments, busy/identity fencing, progress and restart non-replay."""
    with tempfile.TemporaryDirectory(prefix="cmux-gateway-") as directory:
        root = Path(directory)
        binary_dir = root / "bin"
        binary_dir.mkdir()
        for provider in ("claude", "codex"):
            path = binary_dir / provider
            path.write_text(AGENT)
            path.chmod(0o700)
        key = "isolated-bearer-secret"
        gateway = Gateway(key)
        env = dict(PATH=str(binary_dir) + os.pathsep + os.environ["PATH"], FAKE_ROOT=str(root),
                   CMUX_TEST_CLI=str(Path("target/debug/cmux").resolve()), CMUX_GATEWAY_API_KEY=key)
        try:
            with running_app(root, env) as app:
                assert not status(app)["config"]["enabled"]
                claude_surface = launch(app, root, "claude")
                workspace = json.loads(app.cli("current-workspace", "--json"))["uuid"]
                # Existing agent bindings register when mapping is later enabled.
                app.cli("gateway", "bind", "--workspace", workspace, "--project", "fixture-project")
                app.cli("gateway", "configure", "--url", gateway.url, "--enabled")
                app.wait_for(lambda: status(app)["connection"] == "Connected" and len(gateway.sessions) == 1, "authenticated existing-session registration")
                assert gateway.sessions[0]["surface_id"] == claude_surface
                assert gateway.sessions[0]["state"] == "idle"
                # A shell with no native binding is never registered as an executor.
                app.cli("split", "--direction", "horizontal")
                codex_surface = launch(app, root, "codex")
                app.wait_for(lambda: len(gateway.sessions) == 2, "new native session registration")
                claude = next(s for s in gateway.sessions if s["surface_id"] == claude_surface)
                assignment = gateway.assignment(claude, "Complete fixture task " + key)
                run = assignment["run_id"]
                app.wait_for(lambda: any(r["assignment"]["run_id"] == run for r in status(app)["runs"]), "visible durable assignment")
                notifications = json.loads(app.cli("notifications", "list", "--json"))["notifications"]
                assert any(n["title"] == "Delegated task ready" and n["surface_id"] == claude_surface for n in notifications)
                assert key not in json.dumps(status(app))
                rejects(app, "gateway.accept", run_id=run, confirm_ready=False)
                raw(app, "gateway.agent_event", surface_id=claude_surface, session_id="claude-native", client="claude", event="prompt")
                app.wait_for(lambda: any(s["surface_id"] == claude_surface and s["state"] == "busy" for s in gateway.sessions), "busy registration")
                rejects(app, "gateway.accept", run_id=run, confirm_ready=True)
                raw(app, "gateway.agent_event", surface_id=claude_surface, session_id="claude-native", client="claude", event="stop")
                app.wait_for(lambda: any(s["surface_id"] == claude_surface and s["state"] == "idle" for s in gateway.sessions), "idle registration")
                gateway.hold_accept = True
                app.cli("gateway", "accept", "--run", run, "--confirm-ready")
                app.wait_for(lambda: gateway.held, "gateway acceptance received")
                assert not inputs(root, "claude"), "prompt delivered before accepted response"
                gateway.hold_accept = False
                gateway.send(gateway.held.pop())
                app.wait_for(lambda: len(inputs(root, "claude")) == 1, "native target input after acknowledgment")
                app.wait_for(lambda: any(r["phase"] == "submitted" for r in status(app)["runs"]), "durable submitted state")
                assert not inputs(root, "codex"), "assignment reached the wrong terminal"
                assert run in inputs(root, "claude")[0] and key not in inputs(root, "claude")[0]
                rejects(app, "gateway.accept", run_id=run, confirm_ready=True)
                gateway.send(dict(type="accepted", run_id=run, status="running"))
                gateway.send(assignment)
                app.cli("gateway", "report", "--run", run, "--state", "waiting-input", "--message", "Which environment should I validate?")
                app.wait_for(lambda: gateway.runs[run]["status"] == "waiting_input", "visible terminal question")
                app.cli("gateway", "report", "--run", run, "--state", "running", "--message", "User answered in the same terminal")
                app.wait_for(lambda: gateway.runs[run]["status"] == "running", "question resumed")
                before = len(gateway.registrations)
                gateway.disconnect()
                app.wait_for(lambda: len(gateway.registrations) > before and status(app)["connection"] == "Connected", "reconnect reconciliation", timeout=20)
                assert len(inputs(root, "claude")) == 1
                rejects(app, "gateway.bind", workspace_id=workspace, project_ident="wrong-project")
                app.cli("gateway", "report", "--run", run, "--state", "finished", "--message", "Done", "--summary", "Implemented fixture; validated delivery; no blockers; fixture artifact")
                app.wait_for(lambda: gateway.runs[run]["finished_at"] is not None and any(r["ended"] for r in status(app)["runs"]), "durable final report")
                assert gateway.runs[run]["status"] == "needs_attention", "turn completion must not mark task done"
                assert gateway.runs[run]["summary"].startswith("Implemented fixture")
                # A readiness change while acceptance is in flight must fence native input.
                codex = next(s for s in gateway.sessions if s["surface_id"] == codex_surface)
                race = gateway.assignment(codex, "Do not submit while this agent becomes busy")
                race_id = race["run_id"]
                app.wait_for(lambda: any(r["assignment"]["run_id"] == race_id for r in status(app)["runs"]), "second assignment")
                gateway.hold_accept = True
                app.cli("gateway", "accept", "--run", race_id, "--confirm-ready")
                app.wait_for(lambda: gateway.held, "second acceptance in flight")
                raw(app, "gateway.agent_event", surface_id=codex_surface, session_id="codex-native", client="codex", event="prompt")
                app.wait_for(lambda: any(s["surface_id"] == codex_surface and s["state"] == "busy" for s in gateway.sessions), "agent became busy before acceptance")
                gateway.hold_accept = False
                gateway.send(gateway.held.pop())
                app.wait_for(lambda: any(r["assignment"]["run_id"] == race_id and r["phase"] == "uncertain" for r in status(app)["runs"]), "busy delivery fenced")
                assert not inputs(root, "codex")
                rejects(app, "gateway.accept", run_id=race_id, confirm_ready=True)
                app.wait_for(lambda: status(app)["connection"] == "Connected", "race reconnect reconciliation", timeout=20)
                app.cli("gateway", "report", "--run", race_id, "--state", "failed", "--message", "Delivery was fenced", "--summary", "No prompt sent; agent became busy before acceptance")
                app.wait_for(lambda: gateway.runs[race_id]["finished_at"] is not None, "fenced run outcome")
                rejects(app, "gateway.agent_event", surface_id=codex_surface, session_id="stale-native-session", client="codex", event="stop")
                sequences = [r["sequence"] for r in gateway.reports if r["run_id"] == run]
                assert sequences == sorted(set(sequences)) and min(sequences) > 0
                journal = root / "config/cmux/gateway.json"
                assert key not in journal.read_text()
                assert journal.stat().st_mode & 0o777 == 0o600
                quit_app(app)
            with running_app(root, env) as app:
                app.wait_for(lambda: status(app)["connection"] == "Connected", "application restart reconnect", timeout=20)
                assert any(r["assignment"]["run_id"] == run and r["ended"] for r in status(app)["runs"])
                assert len(inputs(root, "claude")) == 1
                assert status(app)["config"]["mappings"][0]["workspace_id"] == workspace
                # Restored metadata alone is conservatively busy; it cannot submit into restored shells.
                assert all(s["state"] != "idle" for s in status(app)["sessions"])
                quit_app(app)
        finally:
            gateway.close()
    print("gateway native session routing, durable delivery, progress and reconnect passed")


if __name__ == "__main__":
    main()
