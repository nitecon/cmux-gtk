#!/usr/bin/env python3
"""Exercise the real GTK app against a native lifecycle WebSocket and native foreground-agent fixtures in CI."""
import json
from pathlib import Path
import shlex
import shutil
import subprocess
import tempfile
import time
import uuid

from gateway_fixture import Gateway
from linux_app import running_app

START = "<Start Agent Gateway Message Injection>"
STOP = "</Stop AgentGateway Message injection>"


def status(app):
    """Read global configuration and bounded credential-free delivery outcomes."""
    return json.loads(app.cli("gateway", "status", "--json"))


def raw(app, method, **params):
    """Use the public socket for settings and human terminal input, never synthetic stream ingress."""
    return json.loads(app.cli("raw", method, "--params", json.dumps(params), "--json"))


def rejects(app, method, **params):
    """Require removed operations or missing explicit consent to fail visibly."""
    try:
        raw(app, method, **params)
    except subprocess.CalledProcessError:
        return
    raise AssertionError("operation unexpectedly accepted: " + method)


def submissions(path):
    """Read actual agent stdin submissions rather than relying on terminal echo or receipt claims."""
    return path.read_text().split("\n===SUBMITTED===\n")[:-1] if path.exists() else []


def setup(root):
    """Create isolated projects, including two same-provider agents sharing one repository."""
    executable = root / "codex"
    subprocess.check_call(["cc", "-Wall", "-Wextra", "-O2", str(Path(__file__).parent / "fixtures/gateway_agent.c"),
                           "-o", str(executable)], timeout=20)
    native_claude = root / ".local/share/claude/versions/2.1.288"
    native_claude.parent.mkdir(parents=True)
    shutil.copyfile(executable, native_claude)
    native_claude.chmod(0o700)
    (root / "claude").symlink_to(native_claude)
    workspaces = []
    for index, name in enumerate(("first", "second", "absent", "peer")):
        project = root / name
        project.mkdir()
        subprocess.check_call(["git", "init", "-q", str(project)], timeout=5)
        subprocess.check_call(["git", "-C", str(project), "remote", "add", "origin",
                               "git@github.com:fixture/" + ("first" if name == "peer" else name) + ".git"], timeout=5)
        mode = root / (name + ".mode")
        mode.write_text("idle")
        workspaces.append(dict(uuid=str(uuid.uuid4()), name=name, working_directory=str(project),
                               startup_script=None, active_pane_uuid=None,
                               layout=dict(type="Leaf", pane_id=index + 1, surface_uuid=str(uuid.uuid4()),
                                           shell="/bin/bash", cwd="")))
    session = root / "data/cmux/session.json"
    session.parent.mkdir(parents=True)
    session.write_text(json.dumps(dict(version=3, active_index=0, workspaces=workspaces)))
    return workspaces


def wait_outcome(app, gateway, event_id, expected):
    """Wait for actual gateway-recorded delivery and show credential-free client evidence on failure."""
    try:
        app.wait_for(lambda: gateway.outcome(event_id) == expected,
                     f"event {event_id} delivery {expected}", timeout=20)
    except BaseException:
        print(json.dumps(status(app), indent=2))
        raise


def main():
    """Verify matching, safety, lifecycle semantics, receipt ordering, reconnect fences and restart uncertainty."""
    with tempfile.TemporaryDirectory(prefix="cmux-gateway-") as directory:
        root = Path(directory)
        workspaces = setup(root)
        key = "isolated-gateway-secret"
        gateway = Gateway(key)
        try:
            gateway.add(content="Historical task must not replay on first enable")
            config = root / "config/cmux"
            config.mkdir(parents=True)
            instance = str(uuid.uuid4())
            journal_path = config / "gateway.json"
            journal_path.write_text(json.dumps(dict(instance_id=instance,
                config=dict(enabled=False, url=gateway.url, mappings=[dict(workspace_id="old", project_ident="old")]), runs=[])))
            environment = {"CMUX_GATEWAY_API_KEY": "", "GATEWAY_API_KEY": "", "SHELL": "/bin/bash"}
            with running_app(root, environment) as app:
                app.wait_for(lambda: status(app)["connection"] == "Disabled", "global preferences loaded")
                view = status(app)
                assert not view["config"]["injection_approved"] and "mappings" not in view["config"]
                for method in ("gateway.bind", "gateway.accept", "gateway.report", "gateway.agent_event"):
                    rejects(app, method)
                rejects(app, "gateway.configure", enabled=True, url=gateway.url)
                rejects(app, "gateway.configure", enabled=True, url="http://gateway.example", injection_approved=True)
                for workspace in workspaces:
                    app.cli("select-workspace", workspace["uuid"])
                    surface = next(row["uuid"] for row in app.surfaces() if row["workspace_uuid"] == workspace["uuid"])
                    app.wait_for(lambda: bool(raw(app, "surface.read_text", id=surface)["text"].strip()), "workspace PTY allocation")
                    name = workspace["name"]
                    if name != "absent":
                        arguments = [str(root / ("codex" if name in ("first", "peer") else "claude")),
                                     str(root / (name + ".mode")), str(root / (name + ".input")), str(root / name)]
                        raw(app, "surface.send_text", id=surface, text=shlex.join(arguments))
                        raw(app, "surface.send_key", id=surface, key="\r")
                        app.wait_for(lambda: "Gateway fixture" in raw(app, "surface.read_text", id=surface)["text"], "interactive foreground agent")
                app.wait_for(lambda: len(app.surfaces()) == 4, "all native surfaces")
                surfaces = {workspace["name"]: next(row["uuid"] for row in app.surfaces()
                            if row["workspace_uuid"] == workspace["uuid"]) for workspace in workspaces}
                # Identity is available with transport disabled and is exact to each running agent.
                contexts = {name: raw(app, "gateway.session", surface_id=surface)
                            for name, surface in surfaces.items() if name != "absent"}
                assert len({context["session_id"] for context in contexts.values()}) == 3
                assert contexts["first"]["provider"] == contexts["peer"]["provider"] == "codex"
                assert contexts["second"]["provider"] == "claude"
                assert all(context["instance_id"] == instance and context["os"] == "linux" for context in contexts.values())
                assert raw(app, "gateway.session", surface_id=surfaces["first"]) == contexts["first"]
                assert len(raw(app, "gateway.sessions")["sessions"]) == 3
                rejects(app, "gateway.session", surface_id=surfaces["absent"])
                raw(app, "gateway.configure", enabled=True, url=gateway.url, injection_approved=False, api_key=key)
                app.wait_for(lambda: status(app)["connection"] == "Connected", "native Bearer subscription", timeout=20)
                assert gateway.subscriptions[0]["after_event_id"] is None
                assert gateway.consumers[instance] == 1
                no_consent = gateway.add(content="No consent")
                wait_outcome(app, gateway, no_consent, "skipped")
                app.wait_for(lambda: json.loads(journal_path.read_text())["cursor"] == no_consent, "recorded receipt persisted")
                raw(app, "gateway.configure", enabled=True, url=gateway.url, injection_approved=True)
                try:
                    app.wait_for(lambda: status(app)["connection"] == "Connected" and status(app)["agents"] == 3,
                                 "foreground agent discovery", timeout=20)
                except BaseException:
                    print(json.dumps(status(app), indent=2))
                    raise
                selected = next(row["uuid"] for row in app.surfaces() if row["active"])
                created = gateway.add(content="Incoming delegated target assignment")
                wait_outcome(app, gateway, created, "injected")
                received = submissions(root / "first.input")
                assert len(received) == 1 and received[0].startswith(START) and received[0].endswith(STOP)
                assert "Task specification" in received[0]
                assert len(submissions(root / "peer.input")) == 1
                assert next(row["uuid"] for row in app.surfaces() if row["active"]) == selected
                # Shared author/machine IDs do not suppress another Codex terminal's coordination context.
                for kind in ("task_commented", "task_completed"):
                    first_before = len(submissions(root / "first.input"))
                    peer_before = len(submissions(root / "peer.input"))
                    event_id = gateway.add(kind=kind, origin={key: contexts["first"][key]
                        for key in ("session_id", "instance_id", "provider", "os")})
                    wait_outcome(app, gateway, event_id, "injected")
                    assert len(submissions(root / "first.input")) == first_before
                    assert len(submissions(root / "peer.input")) == peer_before + 1
                    recorded = json.loads(gateway.receipts[event_id]["summary"])["recipients"]
                    assert {entry["session_id"]: entry["status"] for entry in recorded} == {
                        contexts["first"]["session_id"]: "skipped", contexts["peer"]["session_id"]: "injected"}
                peer_before = len(submissions(root / "peer.input"))
                first_before = len(submissions(root / "first.input"))
                reply = gateway.add(kind="task_commented", origin={key: contexts["peer"][key]
                    for key in ("session_id", "instance_id", "provider", "os")})
                wait_outcome(app, gateway, reply, "injected")
                assert len(submissions(root / "peer.input")) == peer_before
                assert len(submissions(root / "first.input")) == first_before + 1
                # Foreign Windows provenance is context, not a local provider/OS assignment filter.
                foreign = gateway.add(kind="task_completed", origin=dict(session_id=str(uuid.uuid4()),
                    instance_id=str(uuid.uuid4()), provider="codex", os="windows"))
                wait_outcome(app, gateway, foreign, "injected")
                assert len(json.loads(gateway.receipts[foreign]["summary"])["recipients"]) == 2
                # An incremental TUI owns the screen: long injections must not alter untouched rows or scroll it.
                (root / "first.mode").write_text("incremental")
                app.wait_for(lambda: "permission checks preserved" in raw(app, "surface.read_text", id=surfaces["first"])["text"],
                             "incremental screen")
                baseline = raw(app, "surface.read_text", id=surfaces["first"])["text"]
                expected_screen = baseline.replace("Gateway fixture: permission checks preserved",
                                                   "Gateway fixture: submission received")
                for index in range(2):
                    content = f"Incremental display check {index}\n" + ("Long wrapped message " * 100 + "\n") * 3
                    event_id = gateway.add(content=content)
                    wait_outcome(app, gateway, event_id, "injected")
                    app.wait_for(lambda: content in submissions(root / "first.input")[-1], "incremental submission")
                    app.wait_for(lambda: "submission received" in raw(app, "surface.read_text", id=surfaces["first"])["text"],
                                 "incremental response")
                    assert raw(app, "surface.read_text", id=surfaces["first"])["text"] == expected_screen
                (root / "first.mode").write_text("idle")
                for role in ("user", "agent", "system"):
                    comment = dict(id="ordinary-" + role, task_id="ordinary-task", author="ordinary-writer",
                                   author_type=role, content="Context from " + role)
                    event_id = gateway.add(kind="task_commented", comment=comment)
                    wait_outcome(app, gateway, event_id, "injected")
                completed = gateway.add(kind="task_completed", content="Completion notification")
                wait_outcome(app, gateway, completed, "injected")
                assert "already done" in submissions(root / "first.input")[-1]
                tracking = gateway.add(task=dict(id="tracking", title="Outgoing tracking record", kind="delegated",
                                                delegated_to_task_id="target-task"))
                wait_outcome(app, gateway, tracking, "skipped")
                absent = gateway.add(project="absent", content="Never type into the shell")
                wait_outcome(app, gateway, absent, "skipped")
                assert "Never type into the shell" not in raw(app, "surface.read_text", id=surfaces["absent"])["text"]
                # A busy terminal acknowledges promptly so another project's comment can be delivered.
                (root / "first.mode").write_text("busy")
                (root / "peer.mode").write_text("busy")
                app.wait_for(lambda: "esc to interrupt" in raw(app, "surface.read_text", id=surfaces["first"])["text"], "busy screen")
                app.wait_for(lambda: "esc to interrupt" in raw(app, "surface.read_text", id=surfaces["peer"])["text"], "busy peer screen")
                before = len(submissions(root / "first.input"))
                gateway.record_delay = 2
                busy = gateway.add(content="Wait while busy")
                wait_outcome(app, gateway, busy, "queued")
                (root / "peer.mode").write_text("idle")
                gateway.record_delay = 0
                # An old queued confirmation cannot hide a newer peer result with the same aggregate status.
                app.wait_for(lambda: any(entry["session_id"] == contexts["peer"]["session_id"]
                    and entry["status"] == "injected" for entry in json.loads(gateway.receipts[busy]["summary"])["recipients"]),
                    "partial peer delivery acknowledged while first remains busy", timeout=10)
                assert gateway.outcome(busy) == "queued"
                independent = gateway.add(project="second", kind="task_commented", content="Other project proceeds")
                wait_outcome(app, gateway, independent, "injected")
                assert len(submissions(root / "first.input")) == before
                (root / "first.mode").write_text("idle")
                wait_outcome(app, gateway, busy, "injected")
                # Real human input must stay intact until explicitly cleared.
                raw(app, "surface.send_text", id=surfaces["first"], text="unfinished human draft")
                app.wait_for(lambda: "unfinished human draft" in raw(app, "surface.read_text", id=surfaces["first"])["text"], "draft screen")
                draft = gateway.add(content="Wait for draft")
                wait_outcome(app, gateway, draft, "queued")
                time.sleep(1)
                assert all("Wait for draft" not in entry for entry in submissions(root / "first.input"))
                raw(app, "surface.send_key", id=surfaces["first"], key="\x15")
                wait_outcome(app, gateway, draft, "injected")
                # The shaded Codex composer must distinguish status rows from multiline drafts and dialogs.
                for mode, visible in (("multiline", "second draft line"), ("permission", "Allow once")):
                    (root / "first.mode").write_text(mode)
                    app.wait_for(lambda: visible in raw(app, "surface.read_text", id=surfaces["first"])["text"], mode + " screen")
                    blocked = gateway.add(content="Wait for " + mode)
                    wait_outcome(app, gateway, blocked, "queued")
                    time.sleep(1)
                    assert all("Wait for " + mode not in entry for entry in submissions(root / "first.input"))
                    (root / "first.mode").write_text("idle")
                    wait_outcome(app, gateway, blocked, "injected")
                # Full details are fetched before a truncated task becomes agent input; bearer text is redacted.
                truncated = gateway.add(content="Preview", truncated=True)
                with gateway.lock:
                    gateway.details[f"/v1/projects/first/tasks/task-{truncated}"] = dict(
                        id=f"task-{truncated}", title="Hydrated task", description="Full context", details="FULL_SPEC " + key)
                wait_outcome(app, gateway, truncated, "injected")
                assert gateway.full_fetches == 1
                assert "FULL_SPEC [redacted]" in submissions(root / "first.input")[-1]
                # Existing IDs reconcile after reconnect without a second PTY submission.
                before = len(submissions(root / "first.input"))
                app.wait_for(lambda: json.loads(journal_path.read_text())["cursor"] >= truncated, "saved reconnect cursor")
                subscription_count = len(gateway.subscriptions)
                gateway.disconnect(replay=truncated)
                app.wait_for(lambda: len(gateway.subscriptions) > subscription_count and status(app)["connection"] == "Connected",
                             "saved-cursor reconnect", timeout=20)
                time.sleep(2)
                assert len(submissions(root / "first.input")) == before
                assert gateway.subscriptions[-1]["consumer_id"] == instance
                assert gateway.subscriptions[-1]["after_event_id"] >= truncated
                assert gateway.heartbeats > 0 and gateway.maximum_connections == 1
                with gateway.lock:
                    gateway.server_heartbeats = False
                    heartbeat_count = gateway.heartbeats
                app.wait_for(lambda: gateway.heartbeats > heartbeat_count, "client-owned periodic heartbeat", timeout=15)
                # Replacement shell, even with the same PID after exec, retires the original queued target.
                (root / "first.mode").write_text("busy")
                app.wait_for(lambda: "esc to interrupt" in raw(app, "surface.read_text", id=surfaces["first"])["text"], "busy before process replacement")
                changed = gateway.add(content="Never send to the replacement shell")
                wait_outcome(app, gateway, changed, "queued")
                (root / "first.mode").write_text("shell")
                wait_outcome(app, gateway, changed, "injected")
                recorded = json.loads(gateway.receipts[changed]["summary"])["recipients"]
                assert next(entry["status"] for entry in recorded if entry["session_id"] == contexts["first"]["session_id"]) == "skipped"
                assert all("replacement shell" not in entry for entry in submissions(root / "first.input"))
                view = status(app)
                assert key not in json.dumps(view) and "Historical task" not in json.dumps(view)
                assert all(key not in entry for entry in submissions(root / "first.input"))
                assert not (root / ".codex").exists() and not (root / ".claude").exists()
            saved = json.loads(journal_path.read_text())
            assert saved["instance_id"] == instance and "runs" not in saved and "mappings" not in saved["config"]
            assert key not in journal_path.read_text() and key not in (root / "app.log").read_text()
            assert (config / "gateway-key").read_text() == key
            assert (config / "gateway-key").stat().st_mode & 0o077 == 0
            # Simulate a crash between PTY submission and confirmed result; replay is deliberately fenced.
            receipt = next(row for row in saved["receipts"] if row["event_id"] == str(truncated))
            receipt["outcome"] = "submitting"
            receipt["confirmed"] = False
            journal_path.write_text(json.dumps(saved))
            with gateway.lock:
                gateway.receipts[truncated]["status"] = "queued"
            before = len(submissions(root / "first.input"))
            with running_app(root, environment) as app:
                wait_outcome(app, gateway, truncated, "uncertain")
                assert len(submissions(root / "first.input")) == before
                app.wait_for(lambda: any(row["outcome"] == "uncertain" for row in status(app)["receipts"]), "visible uncertain receipt")
                app.cli("gateway", "configure", "--url", gateway.url)
                assert status(app)["connection"] == "Disabled"
            print("gateway lifecycle matching, native injection, busy/draft safety, reconnect and uncertain restart passed")
        finally:
            gateway.close()


if __name__ == "__main__":
    main()
