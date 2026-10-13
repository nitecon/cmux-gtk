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

START = "<Gateway message>"
STOP = "</Gateway message>"


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
    """Create mixed-case Git remotes, including two same-provider agents sharing one repository."""
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
                               "git@GitHub.com:FiXtUrE/" + ("first" if name == "peer" else name).upper() + ".GiT"], timeout=5)
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


def wait_outcome(app, gateway, event_id, expected, timeout=20):
    """Wait for actual gateway-recorded delivery and show credential-free client evidence on failure."""
    try:
        app.wait_for(lambda: gateway.outcome(event_id) == expected,
                     f"event {event_id} delivery {expected}", timeout=timeout)
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
        gateway.projects["first"] = None
        gateway.registration_status = 503
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
                for method in ("gateway.bind", "gateway.accept", "gateway.report", "gateway.agent_event", "gateway.codex.start", "gateway.codex.attach", "gateway.codex.stop"):
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
                assert all(raw(app, "surface.read_text", id=surface)["input"] is None for surface in surfaces.values())
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
                # Legacy project metadata heals without agent-tools, a config edit or a new task.
                created = gateway.add(content="Incoming delegated target assignment", canonical_remote=None)
                wait_outcome(app, gateway, created, "received")
                assert not submissions(root / "first.input") and not submissions(root / "peer.input")
                app.wait_for(lambda: any("retrying automatically" in row["reason"]
                    for row in status(app)["receipts"] if row["event_id"] == str(created)), "visible metadata retry")
                independent = gateway.add(project="second", content="Metadata retries must not block another project")
                wait_outcome(app, gateway, independent, "injected")
                app.wait_for(lambda: f"task-{independent}" in submissions(root / "second.input")[-1], "other project actual input")
                expires = gateway.add(canonical_remote=None, created_at=int(time.time() * 1000) - 298_000)
                wait_outcome(app, gateway, expires, "skipped")
                assert not submissions(root / "first.input")
                with gateway.lock:
                    assert gateway.registrations and gateway.registrations[0][1] == "github.com/fixture/first"
                    gateway.registration_status = 200
                wait_outcome(app, gateway, created, "injected", timeout=45)
                assert gateway.projects["first"] == "github.com/fixture/first"
                assert gateway.events[created - 1]["canonical_remote"] is None
                attempts = [timestamp for timestamp, remote in gateway.registrations if remote == "github.com/fixture/first"]
                assert len(attempts) == 2 and attempts[1] - attempts[0] >= 29
                received = submissions(root / "first.input")
                assert len(received) == 1 and received[0].startswith(START) and received[0].endswith(STOP)
                assert received[0] == f"{START}New delegated task received: task-{created}{STOP}"
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
                    app.wait_for(lambda: f"task-{event_id}" in submissions(root / "first.input")[-1], "incremental submission")
                    assert content not in submissions(root / "first.input")[-1]
                    app.wait_for(lambda: "submission received" in raw(app, "surface.read_text", id=surfaces["first"])["text"],
                                 "incremental response")
                    assert raw(app, "surface.read_text", id=surfaces["first"])["text"] == expected_screen
                (root / "first.mode").write_text("idle")
                for role in ("user", "agent", "system"):
                    comment = dict(id="ordinary-" + role, task_id="ordinary-task", author="ordinary-writer",
                                   author_type=role, content="Context from " + role)
                    event_id = gateway.add(kind="task_commented", comment=comment)
                    wait_outcome(app, gateway, event_id, "injected")
                    assert submissions(root / "first.input")[-1] == f"{START}New comment on task: task-{event_id}{STOP}"
                completed = gateway.add(kind="task_completed", content="Completion notification")
                wait_outcome(app, gateway, completed, "injected")
                assert submissions(root / "first.input")[-1] == f"{START}Task completed: task-{completed}{STOP}"
                tracking = gateway.add(task=dict(id="tracking", title="Outgoing tracking record", kind="delegated",
                                                delegated_to_task_id="target-task"))
                wait_outcome(app, gateway, tracking, "skipped")
                absent = gateway.add(project="absent", content="Never type into the shell")
                wait_outcome(app, gateway, absent, "skipped")
                assert "Never type into the shell" not in raw(app, "surface.read_text", id=surfaces["absent"])["text"]
                # A differently named running app uses exactly the same input path.
                shutil.copyfile(root / "codex", root / "tralala")
                (root / "tralala").chmod(0o700)
                command = shlex.join([str(root / "tralala"), str(root / "absent.mode"), str(root / "absent.input"), str(root / "absent")])
                raw(app, "surface.send_text", id=surfaces["absent"], text=command)
                raw(app, "surface.send_key", id=surfaces["absent"], key="\r")
                app.wait_for(lambda: "Gateway fixture" in raw(app, "surface.read_text", id=surfaces["absent"])["text"], "generic application prompt")
                assert raw(app, "gateway.session", surface_id=surfaces["absent"])["provider"] == "tralala"
                generic = gateway.add(project="absent", content="Provider-independent delivery")
                wait_outcome(app, gateway, generic, "injected")
                app.wait_for(lambda: any(f"task-{generic}" in message for message in submissions(root / "absent.input")), "generic app actual input")
                # Native busy work and drafts defer only their original recipient; other projects proceed.
                for name in ("first", "peer"):
                    (root / (name + ".mode")).write_text("busy")
                    app.wait_for(lambda: "esc to interrupt" in raw(app, "surface.read_text", id=surfaces[name])["text"], "busy process")
                busy = gateway.add(content="Wait until both native prompts are safe")
                wait_outcome(app, gateway, busy, "queued")
                independent = gateway.add(project="second", kind="task_commented", content="Other project proceeds")
                wait_outcome(app, gateway, independent, "injected")
                for name in ("first", "peer"):
                    (root / (name + ".mode")).write_text("idle")
                wait_outcome(app, gateway, busy, "injected")
                human = "unfinished native draft"
                before = len(submissions(root / "first.input"))
                raw(app, "surface.send_text", id=surfaces["first"], text=human)
                app.wait_for(lambda: human in raw(app, "surface.read_text", id=surfaces["first"])["text"], "draft belongs to the provider")
                draft = gateway.add(content="Do not overwrite the provider draft")
                wait_outcome(app, gateway, draft, "queued")
                assert len(submissions(root / "first.input")) == before
                assert human in raw(app, "surface.read_text", id=surfaces["first"])["text"]
                raw(app, "surface.send_key", id=surfaces["first"], key="\r")
                app.wait_for(lambda: human in submissions(root / "first.input"), "human Enter reaches provider")
                wait_outcome(app, gateway, draft, "injected")
                assert submissions(root / "first.input")[-2] == human
                assert f"task-{draft}" in submissions(root / "first.input")[-1]
                for mode, visible in (("multiline", "second draft line"), ("permission", "Allow once")):
                    (root / "first.mode").write_text(mode)
                    app.wait_for(lambda: visible in raw(app, "surface.read_text", id=surfaces["first"])["text"], mode + " screen")
                    event = gateway.add(content="Wait for native " + mode)
                    wait_outcome(app, gateway, event, "queued")
                    (root / "first.mode").write_text("idle")
                    wait_outcome(app, gateway, event, "injected")
                # Truncated and large/private bodies still produce only task references; no REST detail fetch.
                truncated = gateway.add(content="Preview", truncated=True)
                with gateway.lock:
                    gateway.details[f"/v1/projects/first/tasks/task-{truncated}"] = dict(
                        id=f"task-{truncated}", title="Hydrated task", description="Full context", details="FULL_SPEC " + key)
                wait_outcome(app, gateway, truncated, "injected")
                assert gateway.full_fetches == 0
                assert submissions(root / "first.input")[-1] == f"{START}New delegated task received: task-{truncated}{STOP}"
                # Existing IDs reconcile after reconnect without a second PTY submission.
                before = len(submissions(root / "first.input"))
                app.wait_for(lambda: json.loads(journal_path.read_text())["cursor"] >= truncated, "saved reconnect cursor")
                subscription_count = len(gateway.subscriptions)
                gateway.disconnect(replay=truncated)
                offline = gateway.add(content="Do not replay an event from the connection gap")
                app.wait_for(lambda: len(gateway.subscriptions) > subscription_count and status(app)["connection"] == "Connected",
                             "saved-cursor reconnect", timeout=20)
                wait_outcome(app, gateway, offline, "skipped")
                time.sleep(2)
                assert len(submissions(root / "first.input")) == before
                assert gateway.subscriptions[-1]["consumer_id"] == instance
                assert gateway.subscriptions[-1]["after_event_id"] >= truncated
                assert gateway.heartbeats > 0 and gateway.maximum_connections == 1
                with gateway.lock:
                    gateway.server_heartbeats = False
                    heartbeat_count = gateway.heartbeats
                app.wait_for(lambda: gateway.heartbeats > heartbeat_count, "client-owned periodic heartbeat", timeout=15)
                # Expired and undated events advance receipt/cursor without any process input.
                before = len(submissions(root / "first.input"))
                for timestamp in (int(time.time() * 1000) - 300_001, 0, None):
                    expired = gateway.add(created_at=timestamp, content="Never inject historical content")
                    wait_outcome(app, gateway, expired, "skipped")
                assert len(submissions(root / "first.input")) == before
                # A retired process receives no late notification; its surviving peer still does.
                (root / "first.mode").write_text("shell")
                app.wait_for(lambda: all(s["surface_id"] != surfaces["first"] for s in raw(app, "gateway.sessions")["sessions"]), "process exec replacement")
                before = len(submissions(root / "first.input"))
                changed = gateway.add(content="Never send to the replacement shell", truncated=True)
                wait_outcome(app, gateway, changed, "injected")
                assert len(submissions(root / "first.input")) == before
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
            print("gateway lifecycle matching, native injection, native draft/busy isolation, reconnect and uncertain restart passed")
        finally:
            gateway.close()


if __name__ == "__main__":
    main()
