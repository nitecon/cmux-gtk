#!/usr/bin/env python3
"""CI: actual client hooks/tools below one shared executor, independently bound to native GTK terminals."""
import json
import os
from pathlib import Path
import shlex
import socket
import struct
import subprocess
import sys
import tempfile
import uuid

from gateway_fixture import Gateway, read_exact
from linux_app import running_app
from process_support import stop_process, wait_until
from test_linux_gateway import raw, setup, status, wait_outcome


def broker(endpoint, client, root):
    """Run the production client from a shared native executor, using each actual invocation's native ID."""
    root = Path(root)
    endpoint = Path(endpoint)
    endpoint.unlink(missing_ok=True)
    server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    server.bind(str(endpoint))
    server.listen(4)
    records = json.loads((root / "actors.json").read_text()) if (root / "actors.json").exists() else {}
    try:
        while True:
            connection, _ = server.accept()
            with connection:
                try:
                    kind = read_exact(connection, 1)
                    native, prompt = [read_exact(connection, struct.unpack("!I", read_exact(connection, 4))[0]).decode()
                                      for _ in range(2)]
                    assert len(native) <= 256 and len(prompt) <= 65536
                    # Registration uses native conversation metadata, independent of process ancestry or CMUX hints.
                    env = {k: v for k, v in os.environ.items() if not k.startswith("CMUX_")}
                    env.update(CODEX_THREAD_ID=native, CODEX_SESSION_ID=native)
                    initial_notification = "<Start Agent Gateway Message Injection>\nGateway fixture task notification\n</Stop AgentGateway Message injection>"
                    payload = dict(session_id=native, cwd=str(root / "first"), prompt=initial_notification if kind==b"S" else prompt,
                                   hook_event_name="UserPromptSubmit")
                    hook = subprocess.run([client, "hook", "user-prompt-submit",
                                           "--agent", "codex"], input=json.dumps(payload), env=env,
                                          cwd=root / "first", text=True, capture_output=True, timeout=10, check=True)
                    if kind==b"S":
                        assert not hook.stdout.strip(), "notification hook must announce privately without model context"
                    blocked = any(line.startswith("{") and json.loads(line).get("decision") == "block"
                                  for line in hook.stdout.splitlines())
                    actor = json.loads(subprocess.check_output([client, "session", "--json"], env=env,
                                       cwd=root / "first", text=True, timeout=5))
                    record = records.setdefault(native, dict(model_prompts=[], enrollments=0))
                    if kind == b"U":
                        if blocked:
                            assert prompt.startswith("<cmux-session-enrollment>")
                            record["enrollments"] += 1
                        else:
                            record["model_prompts"].append(prompt)
                    record["actor"] = actor
                    temporary = root / "actor.tmp"
                    temporary.write_text(json.dumps(records))
                    temporary.replace(root / "actors.json")
                    connection.sendall(bytes([int(blocked)]))
                except BaseException as error:
                    (root / "broker-error").write_text(str(error)[:2048])
                    connection.sendall(b"\xff")
                    raise
    finally:
        server.close()
        endpoint.unlink(missing_ok=True)


def records(root):
    """Only actor metadata and model-visible inputs are persisted; bootstrap tokens are never recorded here."""
    assert not (root / "broker-error").exists(), (root / "broker-error").read_text() if (root / "broker-error").exists() else ""
    return json.loads((root / "actors.json").read_text()) if (root / "actors.json").exists() else {}


def main():
    """Verify automatic enrollment, exact logical echoes, busy peers, conversation reset and reconnect fences."""
    client = str(Path(os.environ["CMUX_ACTOR_CLIENT"]).resolve())
    with tempfile.TemporaryDirectory(prefix="cmux-actor-") as directory:
        root = Path(directory)
        workspaces = setup(root)
        endpoint = root / "broker.sock"
        gateway = Gateway("fixture-key")
        executor = None
        try:
            gateway.add(content="History before enable")
            with running_app(root, {"HOME":str(root), "SHELL":"/bin/bash", "CMUX_GATEWAY_API_KEY":"", "GATEWAY_API_KEY":""}) as app:
                executor_env = dict(app.environment)
                for key in list(executor_env):
                    if key.startswith("CMUX_") or key.startswith("CODEX_") or key == "CLAUDE_CODE_SESSION_ID":
                        executor_env.pop(key)
                executor = subprocess.Popen([sys.executable,
                    str(Path(__file__).resolve()), "--broker", str(endpoint), client, str(root)], env=executor_env)
                wait_until(endpoint.exists, "shared executor socket", 10)
                native = {name:str(uuid.uuid4()) for name in ("first","peer")}
                surfaces = {}
                for workspace in workspaces:
                    name = workspace["name"]
                    if name not in native:
                        continue
                    app.cli("select-workspace",workspace["uuid"])
                    surface = next(s["uuid"] for s in app.surfaces() if s["workspace_uuid"]==workspace["uuid"])
                    surfaces[name]=surface
                    native_file=root/(name+".native")
                    native_file.write_text(native[name])
                    command = ["env", "CMUX_FIXTURE_ACTOR_BROKER="+str(endpoint),"CMUX_FIXTURE_NATIVE_FILE="+str(native_file),
                               str(root/"codex"), str(root/(name+".mode")), str(root/(name+".input")),str(root/name)]
                    raw(app,"surface.send_text",id=surface,text=shlex.join(command))
                    raw(app,"surface.send_key",id=surface,key="\r")
                    app.wait_for(lambda: "Gateway fixture" in raw(app,"surface.read_text",id=surface)["text"],"native frontend startup hook",30)
                app.wait_for(lambda: len(records(root))==2,"two independent client actors",20)
                initial=records(root)
                origins={name:initial[value]["actor"]["origin"] for name,value in native.items()}
                assert origins["first"]["instance_id"]==origins["peer"]["instance_id"]
                assert origins["first"]["session_id"]!=origins["peer"]["session_id"]
                assert initial[native["first"]]["actor"]["base_id"]==initial[native["peer"]]["actor"]["base_id"]
                assert initial[native["first"]]["actor"]["session_slot"]!=initial[native["peer"]]["actor"]["session_slot"]
                assert all("executor_generation" not in row["actor"] for row in initial.values())
                assert all(r["enrollments"]==0 for r in initial.values()) # Consent is still off.
                raw(app,"gateway.configure",enabled=True,url=gateway.url,injection_approved=True,api_key="fixture-key")
                app.wait_for(lambda: status(app)["connection"]=="Connected","gateway stream",20)

                def bound():
                    contexts=raw(app,"gateway.sessions")["sessions"]
                    return len(contexts)==2 and all(c.get("binding_state")=="bound" for c in contexts)
                app.wait_for(bound,"hook-consumed automatic terminal enrollment",30)
                for name, surface in surfaces.items():
                    context=raw(app,"gateway.session",surface_id=surface)
                    assert context["session_id"]==origins[name]["session_id"]
                    assert context["instance_id"]==origins[name]["instance_id"]
                    assert records(root)[native[name]]["enrollments"]==1
                    assert not records(root)[native[name]]["model_prompts"]
                # Exact-origin comments remain useful to the other same-project/same-provider terminal.
                event=gateway.add(kind="task_commented",content="Peer research result",origin=origins["first"])
                wait_outcome(app,gateway,event,"injected")
                app.wait_for(lambda: len(records(root)[native["peer"]]["model_prompts"])==1,"peer model sees useful comment",20)
                assert not records(root)[native["first"]]["model_prompts"]
                assert "Peer research result" in records(root)[native["peer"]]["model_prompts"][0]
                # Busy recipient is pinned to its known actor; a conversation reset must not inherit queued work.
                (root/"peer.mode").write_text("busy")
                app.wait_for(lambda:"esc to interrupt" in raw(app,"surface.read_text",id=surfaces["peer"])["text"],"busy peer")
                queued=gateway.add(kind="task_commented",content="Work pinned to old conversation",origin=origins["first"])
                wait_outcome(app,gateway,queued,"queued")
                replacement=str(uuid.uuid4())
                (root/"peer.native").write_text(replacement)
                raw(app,"surface.send_text",id=surfaces["peer"],text="Human starts a new conversation")
                raw(app,"surface.send_key",id=surfaces["peer"],key="\r")
                app.wait_for(lambda: replacement in records(root),"new provider native conversation",20)
                (root/"peer.mode").write_text("idle")
                app.wait_for(lambda: raw(app,"gateway.session",surface_id=surfaces["peer"]).get("provider_session_id")==replacement,
                             "renewed generation-fenced membership",30)
                wait_outcome(app,gateway,queued,"skipped")
                assert all("Work pinned" not in p for r in records(root).values() for p in r["model_prompts"])
                current=records(root)[replacement]["actor"]["origin"]
                assert current["session_id"]!=origins["peer"]["session_id"]
                completion=gateway.add(kind="task_completed",content="Peer finished research",origin=current)
                wait_outcome(app,gateway,completion,"injected")
                app.wait_for(lambda: len(records(root)[native["first"]]["model_prompts"])==1,"completion reaches implementer",20)
                before={k:len(v["model_prompts"]) for k,v in records(root).items()}
                gateway.disconnect(replay=completion)
                app.wait_for(lambda:len(gateway.subscriptions)>=2,"transport reconnect",20)
                app.wait_for(lambda:status(app)["connection"]=="Connected","connected again",20)
                assert before=={k:len(v["model_prompts"]) for k,v in records(root).items()}
                journal=json.loads((root/"config/cmux/gateway.json").read_text())
                assert "enrollment_token" not in json.dumps(journal)
                enrolled_before_restart=records(root)[native["first"]]["enrollments"]
                # An executor restart cannot change logical registration or retire live frontend membership.
                for name in native:
                    (root/(name+".mode")).write_text("busy")
                app.wait_for(lambda:all("esc to interrupt" in raw(app,"surface.read_text",id=s)["text"] for s in surfaces.values()),
                             "frontends remain busy while backend exits")
                stop_process(executor)
                endpoint.unlink(missing_ok=True)
                executor = subprocess.Popen([sys.executable, str(Path(__file__).resolve()),
                    "--broker", str(endpoint), client, str(root)], env=executor_env)
                wait_until(endpoint.exists, "restarted shared executor socket", 10)
                contexts=raw(app,"gateway.sessions")["sessions"]
                assert len(contexts)==2 and all(c["binding_state"]=="bound" for c in contexts)
                assert {c["session_id"] for c in contexts}=={origins["first"]["session_id"],current["session_id"]}
                restarted=gateway.add(kind="task_commented",content="Research after backend restart",origin=current)
                wait_outcome(app,gateway,restarted,"queued")
                for name in native:
                    (root/(name+".mode")).write_text("idle")
                wait_outcome(app,gateway,restarted,"injected")
                app.wait_for(lambda:any("Research after backend restart" in p for p in records(root)[native["first"]]["model_prompts"]),
                             "ordinary registration survives executor restart",20)
                assert records(root)[native["first"]]["actor"]["origin"]==origins["first"]
                assert records(root)[native["first"]]["enrollments"]==enrolled_before_restart
                print("actual registered client: plain shared host, two numbered sessions, consumed enrollment, peer comments/completion and restart/reconnect fences PASS")
        finally:
            if executor is not None:
                stop_process(executor)
            gateway.close()


if __name__=="__main__":
    if len(sys.argv)>1 and sys.argv[1]=="--broker":
        broker(*sys.argv[2:])
    else:
        main()
