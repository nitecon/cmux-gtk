"""Native Windows CI: real client hooks, named-pipe caller identity and two independently enrolled peers."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import uuid

from gateway_fixture import Gateway


def relay(client, native, prompt_file, record_file, directory):
    """Run actual installed hooks/tools from each native provider, with no CMUX environment hints."""
    env = {key: value for key, value in os.environ.items() if not key.startswith("CMUX_")}
    env.update(CODEX_THREAD_ID=native, CODEX_SESSION_ID=native)
    env.pop("CLAUDE_CODE_SESSION_ID", None)
    prompt = Path(prompt_file).read_text(encoding="utf-8")
    notification = "<Start Agent Gateway Message Injection>\nWindows fixture notification\n</Stop AgentGateway Message injection>"
    payload = dict(session_id=native, cwd=directory, prompt=prompt or notification, hook_event_name="UserPromptSubmit")
    hook = subprocess.run([client, "hook", "user-prompt-submit", "--agent", "codex"],
                          input=json.dumps(payload), env=env, cwd=directory, text=True,
                          capture_output=True, timeout=12, check=True)
    if not prompt:
        assert not hook.stdout.strip(), "Notification-first hook must announce quietly"
    blocked = any(line.startswith("{") and json.loads(line).get("decision") == "block"
                  for line in hook.stdout.splitlines())
    actor = json.loads(subprocess.check_output([client, "session", "--json"], env=env,
                                             cwd=directory, text=True, timeout=5))
    record_path = Path(record_file)
    record = json.loads(record_path.read_text()) if record_path.exists() else dict(enrollments=0, prompts=[])
    record["actor"] = actor
    if blocked:
        assert prompt.startswith("<cmux-session-enrollment>")
        record["enrollments"] += 1
    elif prompt:
        record["prompts"].append(prompt)
    temporary = record_path.with_suffix(".tmp")
    temporary.write_text(json.dumps(record), encoding="utf-8")
    temporary.replace(record_path)
    return 10 if blocked else 0


def verify(rpc, profile, client, fixture, python):
    """Enroll native ConPTY peers then check useful comments/completion, exact-origin skips and bound RPC."""
    root = Path(profile) / "actors"
    root.mkdir()
    directory = root / "first"
    directory.mkdir()
    subprocess.run(["git", "init", str(directory)], check=True, capture_output=True)
    subprocess.run(["git", "-C", str(directory), "remote", "add", "origin", "https://github.com/fixture/first.git"], check=True)
    script = str(Path(__file__).resolve())
    records = []
    surfaces = []

    def wait(predicate, description, seconds=40):
        """Bound every native GUI/client exchange and retain its failing condition in CI output."""
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            try:
                if predicate():
                    return
            except RuntimeError:
                pass  # New native surfaces may not have finished allocation yet.
            time.sleep(0.5)
        print("Windows actor status:", json.dumps(rpc("gateway.status"), indent=2))
        raise RuntimeError(description)

    for name in ("first", "peer"):
        workspace = rpc("workspace.create", {"name": name, "working_directory": str(directory)})
        rpc("workspace.select", {"id": workspace["uuid"]})
        surface = rpc("surface.list")["surfaces"]
        surface = next(s for s in surface if s["workspace_uuid"] == workspace["uuid"])["uuid"]
        surfaces.append(surface)
        wait(lambda: bool(rpc("surface.read_text", {"id": surface})["text"].strip()), "native shell allocation")
        record = root / (name + ".json")
        records.append(record)
        native = str(uuid.uuid4())
        command = subprocess.list2cmdline([str(fixture), "--no-daemon", str(python), script,
                                          str(client), native, str(root / (name + ".prompt")), str(record), str(directory)])
        rpc("surface.send_text", {"id": surface, "text": command})
        rpc("surface.send_key", {"id": surface, "key": "\r"})
        wait(lambda: record.exists() and "Windows actor fixture" in rpc("surface.read_text", {"id": surface})["text"], "native provider/hook startup")
    origins = [json.loads(path.read_text())["actor"]["origin"] for path in records]
    assert origins[0]["session_id"] != origins[1]["session_id"]
    assert origins[0]["instance_id"] == origins[1]["instance_id"]
    assert all(origin["os"] == "windows" for origin in origins)
    gateway = Gateway("windows-fixture-key")
    try:
        rpc("gateway.configure", dict(enabled=True, url=gateway.url, injection_approved=True, api_key="windows-fixture-key"))
        wait(lambda: rpc("gateway.status")["connection"] == "Connected", "gateway subscription")
        wait(lambda: all(rpc("gateway.session", {"surface_id": surface}).get("binding_state") == "bound" for surface in surfaces), "automatic Windows enrollment")
        for surface, record, origin in zip(surfaces, records, origins):
            context = rpc("gateway.session", {"surface_id": surface})
            assert context["session_id"] == origin["session_id"]
            snapshot = json.loads(record.read_text())
            assert snapshot["enrollments"] == 1 and not snapshot["prompts"]

        def delivered(event):
            """Require a terminal ACK for the complete two-recipient event group."""
            rows = [row for row in rpc("gateway.status")["receipts"] if str(row["event_id"]) == str(event)]
            return len(rows) == 2 and all(row["confirmed"] and row["outcome"] in ("injected", "skipped") for row in rows)

        comment = gateway.add(kind="task_commented", content="Windows peer research result", origin=origins[0])
        wait(lambda: delivered(comment), "comment peer delivery and exact-origin suppression")
        wait(lambda: len(json.loads(records[1].read_text())["prompts"]) == 1, "peer comment visible to model")
        assert not json.loads(records[0].read_text())["prompts"]
        completion = gateway.add(kind="task_completed", content="Windows peer completed", origin=origins[1])
        wait(lambda: delivered(completion), "completion peer delivery and exact-origin suppression")
        wait(lambda: len(json.loads(records[0].read_text())["prompts"]) == 1, "peer completion visible to model")
        assert len(json.loads(records[1].read_text())["prompts"]) == 1
        print("Native Windows actual SDK: two actors, named-pipe identity, notification-first hooks, consumed enrollment and peer comment/completion PASS")
    finally:
        rpc("gateway.configure", dict(enabled=False, url=gateway.url, injection_approved=False))
        gateway.close()


if __name__ == "__main__":
    assert sys.argv[1] == "--relay"
    raise SystemExit(relay(*sys.argv[2:]))
