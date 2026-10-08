#!/usr/bin/env python3
"""CI: ordinary registration works and legacy hook announcements cannot generate enrollment input."""
import json
import os
from pathlib import Path
import shlex
import socket
import struct
import subprocess
import sys
import tempfile
import time
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
                    if kind == b"S":
                        # Regression trigger only: an old client hook must not enable synthetic identity input.
                        hook = subprocess.run([client, "hook", "user-prompt-submit", "--agent", "codex"],
                            input=json.dumps(payload), env=env, cwd=root / "first", text=True,
                            capture_output=True, timeout=10, check=True)
                        assert not hook.stdout.strip()
                    actor = json.loads(subprocess.check_output([client, "session", "--json"], env=env,
                                       cwd=root / "first", text=True, timeout=5))
                    record = records.setdefault(native, dict(model_prompts=[]))
                    if kind == b"U":
                        record["model_prompts"].append(prompt)
                    record["actor"] = actor
                    temporary = root / "actor.tmp"
                    temporary.write_text(json.dumps(records))
                    temporary.replace(root / "actors.json")
                    connection.sendall(b"\x00")
                except BaseException as error:
                    (root / "broker-error").write_text(str(error)[:2048])
                    connection.sendall(b"\xff")
                    raise
    finally:
        server.close()
        endpoint.unlink(missing_ok=True)


def records(root):
    """Persist only ordinary registration metadata and actual model-visible inputs."""
    assert not (root / "broker-error").exists(), (root / "broker-error").read_text() if (root / "broker-error").exists() else ""
    return json.loads((root / "actors.json").read_text()) if (root / "actors.json").exists() else {}


def main():
    """Verify removal of identity input while ordinary registration and native delivery remain usable."""
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
                raw(app,"gateway.configure",enabled=True,url=gateway.url,injection_approved=True,api_key="fixture-key")
                app.wait_for(lambda: status(app)["connection"]=="Connected","gateway stream",20)
                app.wait_for(lambda: status(app)["agents"]==2,"two native peers",20)
                # More than two observer/drain ticks: legacy hooks must produce no identity prompts.
                time.sleep(2)
                contexts=raw(app,"gateway.sessions")["sessions"]
                assert len(contexts)==2 and all(c["binding_state"]=="unbound" for c in contexts)
                assert all(not r["model_prompts"] for r in records(root).values())
                assert all(r["actor"]["binding_state"]=="unbound" for r in records(root).values())

                event=gateway.add(kind="task_commented",content="Native peer research result")
                wait_outcome(app,gateway,event,"injected")
                app.wait_for(lambda: all(len(records(root)[n]["model_prompts"])==1 for n in native.values()),
                             "useful lifecycle comment reaches both native peers",20)
                (root/"peer.mode").write_text("busy")
                app.wait_for(lambda:"esc to interrupt" in raw(app,"surface.read_text",id=surfaces["peer"])["text"],"busy peer")
                completion=gateway.add(kind="task_completed",content="Native peer completed")
                app.wait_for(lambda: any(r["event_id"]==str(completion) and r["outcome"]=="injected" for r in status(app)["receipts"])
                    and any(r["event_id"]==str(completion) and r["outcome"]=="queued" for r in status(app)["receipts"]),
                    "busy peer waits while ready peer progresses",20)
                assert len(records(root)[native["peer"]]["model_prompts"])==1
                (root/"peer.mode").write_text("idle")
                wait_outcome(app,gateway,completion,"injected")
                app.wait_for(lambda: all(len(records(root)[n]["model_prompts"])==2 for n in native.values()),
                             "completion reaches waiting native peer",20)
                before={k:len(v["model_prompts"]) for k,v in records(root).items()}
                gateway.disconnect(replay=completion)
                app.wait_for(lambda:len(gateway.subscriptions)>=2,"transport reconnect",20)
                app.wait_for(lambda:status(app)["connection"]=="Connected","connected again",20)
                assert before=={k:len(v["model_prompts"]) for k,v in records(root).items()}
                stop_process(executor)
                endpoint.unlink(missing_ok=True)
                executor = subprocess.Popen([sys.executable, str(Path(__file__).resolve()),
                    "--broker", str(endpoint), client, str(root)], env=executor_env)
                wait_until(endpoint.exists,"restarted shared host",10)
                restarted=gateway.add(kind="task_commented",content="Ordinary registration after restart")
                wait_outcome(app,gateway,restarted,"injected")
                app.wait_for(lambda: all(len(records(root)[n]["model_prompts"])==3 for n in native.values()),
                             "ordinary tools continue after backend restart",20)
                assert {r["actor"]["origin"]["session_id"] for r in records(root).values()}=={o["session_id"] for o in origins.values()}
                assert all("cmux-session-enrollment" not in prompt for r in records(root).values() for prompt in r["model_prompts"])
                print("ordinary registered tools: two peers, no enrollment input/blocking, busy gating and restart/reconnect PASS")
        finally:
            if executor is not None:
                stop_process(executor)
            gateway.close()


if __name__=="__main__":
    if len(sys.argv)>1 and sys.argv[1]=="--broker":
        broker(*sys.argv[2:])
    else:
        main()
