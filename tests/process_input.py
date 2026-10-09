"""Real Codex TUI/backend acceptance, with a local deterministic Responses model in Actions only."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
import ntpath
from pathlib import Path
import shlex
import subprocess
import threading
import time
import uuid

from gateway_fixture import Gateway


class Model:
    """The provider remains real; only external model inference is replaced with bounded local SSE."""
    def __init__(self):
        self.inputs = []
        self.hold = threading.Event()
        self.hold.set()
        model = self

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def log_message(self, *_args):
                pass

            def do_GET(self):
                data = json.dumps({"data": [{"id": "cmux-test", "object": "model", "owned_by": "fixture"}]}).encode()
                self.send_response(200)
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

            def do_POST(self):
                size = int(self.headers["Content-Length"])
                assert size <= 4 * 1024 * 1024
                request = json.loads(self.rfile.read(size))
                model.inputs.append(request)
                assert model.hold.wait(60), "Model busy fixture exceeded bound"
                response_id = "resp_" + uuid.uuid4().hex
                item = {"type": "message", "id": "msg_" + uuid.uuid4().hex, "role": "assistant", "status": "completed",
                        "content": [{"type": "output_text", "text": "CMUX_INPUT_ACCEPTED", "annotations": []}]}
                response = {"id": response_id, "object": "response", "status": "completed", "output": [item],
                            "usage": {"input_tokens": 10, "output_tokens": 5, "total_tokens": 15}}
                events = [
                    {"type": "response.created", "response": {**response, "status": "in_progress", "output": []}},
                    {"type": "response.output_item.added", "output_index": 0, "item": {**item, "status": "in_progress", "content": []}},
                    {"type": "response.content_part.added", "item_id": item["id"], "output_index": 0, "content_index": 0, "part": {"type": "output_text", "text": "", "annotations": []}},
                    {"type": "response.output_text.delta", "item_id": item["id"], "output_index": 0, "content_index": 0, "delta": "CMUX_INPUT_ACCEPTED"},
                    {"type": "response.output_item.done", "output_index": 0, "item": item},
                    {"type": "response.completed", "response": response},
                ]
                data = b"".join(("data: " + json.dumps(event) + "\n\n").encode() for event in events)
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def configure(self, home):
        """Only caller-owned CI configuration is written; production launch uses existing user configuration."""
        home.mkdir(parents=True, exist_ok=True)
        project = str(home.parent / "process-input" / "first")
        # Native Codex uses Windows path separators even when the CI Python exposes slash paths.
        project = ntpath.normpath(project) if ntpath.splitdrive(project)[0] else project
        (home / "config.toml").write_text(f'''model = "cmux-test"
model_provider = "cmux_fixture"
cli_auth_credentials_store = "file"
[model_providers.cmux_fixture]
name = "CMUX Actions model fixture"
base_url = "http://127.0.0.1:{self.server.server_port}/v1"
wire_api = "responses"
requires_openai_auth = false
[projects.{json.dumps(project)}]
trust_level = "trusted"
''', encoding="utf-8")

    def close(self):
        self.hold.set()
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(5)


def verify(rpc, root, codex, model):
    """Use ordinary official Codex TUIs, with local drafts and provider-independent process input."""
    root = Path(root) / "process-input"
    root.mkdir()
    directory = root / "first"
    directory.mkdir()
    subprocess.run(["git", "init", "-q", str(directory)], check=True, capture_output=True)
    subprocess.run(["git", "-C", str(directory), "remote", "add", "origin", "git@github.com:fixture/first.git"], check=True)
    gateway = Gateway("process-input-gateway")
    surfaces, workspaces = [], []

    def wait(predicate, description, seconds=60):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            if predicate():
                return
            time.sleep(0.5)
        print("Process input status:", json.dumps(rpc("gateway.status"), indent=2))
        print("Observed model requests:", len(model.inputs))
        for surface in surfaces:
            print("Process input screen:", rpc("surface.read_text", {"id": surface}))
            print("Process input scrollback:", rpc("surface.read_scrollback", {"id": surface})["text"])
        raise AssertionError(description)

    def editor(surface):
        return rpc("surface.read_text", {"id": surface})["input"]

    try:
        rpc("gateway.configure", dict(enabled=True, url=gateway.url, injection_approved=True, api_key=gateway.key))
        wait(lambda: rpc("gateway.status")["connection"] == "Connected", "gateway connected")
        for name in ("first", "peer"):
            workspace = rpc("workspace.create", dict(name=name, working_directory=str(directory)))
            workspaces.append(workspace["uuid"])
            rpc("workspace.select", {"id": workspace["uuid"]})
            surface = next(s["uuid"] for s in rpc("surface.list")["surfaces"] if s["workspace_uuid"] == workspace["uuid"])
            surfaces.append(surface)
            wait(lambda: rpc("surface.read_text", {"id": surface})["text"].strip(), "native shell ready")
            # This isolated model emits text only; avoid the Windows first-run sandbox setup dialog.
            args = [str(codex), "--no-daemon", "-a", "never", "--sandbox", "danger-full-access"]
            command = subprocess.list2cmdline(args) if os.name == "nt" else shlex.join(args)
            rpc("surface.send_text", {"id": surface, "text": command})
            rpc("surface.send_key", {"id": surface, "key": "\r"})
            def startup_ready():
                """Observe test UI only; production execution never inspects provider screens."""
                screen = rpc("surface.read_text", {"id": surface})
                return screen["input"]["active"] and any(marker in screen["text"]
                    for marker in ("OpenAI Codex", "Trust this folder?"))
            wait(startup_ready, "ordinary native Codex and local editor")
            if "Trust this folder?" in rpc("surface.read_text", {"id": surface})["text"]:
                # The fixture owns this empty temporary Git project; normal Enter confirms its access.
                rpc("surface.send_key", {"id": surface, "key": "\r"})
                wait(lambda: "OpenAI Codex" in rpc("surface.read_text", {"id": surface})["text"], "normal empty Enter confirms fixture folder access")
            # Startup may initialize native sandbox state asynchronously; observe only this isolated test UI.
            wait(lambda: "? for shortcuts" in rpc("surface.read_text", {"id": surface})["text"], "fixture reaches its ordinary TUI")
            time.sleep(1)
        sessions = [rpc("gateway.session", {"surface_id": surface}) for surface in surfaces]
        assert all(s["delivery_transport"] == "cmux_input_queue" for s in sessions)
        assert len({s["recipient_session_id"] for s in sessions}) == 2
        assert not model.inputs, "Startup must not submit synthetic model input"
        human = "KEEP_THIS_LOCAL_DRAFT λ\nsecond human line"
        rpc("surface.send_text", {"id": surfaces[0], "text": human})
        model.hold.clear()
        event = gateway.add(content="First ordinary process event")
        wait(lambda: gateway.outcome(event) == "injected", "complete event submitted while human draft stays local")
        wait(lambda: len(model.inputs) >= 2, "both real providers received first message")
        assert editor(surfaces[0])["draft"] == human, (ascii(human), ascii(editor(surfaces[0])["draft"]))
        assert "KEEP_THIS_LOCAL_DRAFT" not in json.dumps(model.inputs)
        busy = gateway.add(content="Ordinary process event while busy")
        wait(lambda: gateway.outcome(busy) == "injected", "input submitted to both busy processes")
        assert editor(surfaces[0])["draft"] == human, (ascii(human), ascii(editor(surfaces[0])["draft"]))
        rows = [r for r in rpc("gateway.status")["receipts"] if r["event_id"] in (str(event), str(busy))]
        assert len(rows) == 4 and all("model receipt is not yet confirmed" in r["reason"] for r in rows)
        model.hold.set()
        wait(lambda: len(model.inputs) >= 4, "provider drains complete busy messages")
        wait(lambda: all("CMUX_INPUT_ACCEPTED" in rpc("surface.read_text", {"id": s})["text"] for s in surfaces), "real TUI renders model output")
        rpc("surface.send_key", {"id": surfaces[0], "key": "\r"})
        wait(lambda: "KEEP_THIS_LOCAL_DRAFT" in json.dumps(model.inputs), "plain Enter submits whole human message")
        assert editor(surfaces[0])["draft"] == ""
        assert any("KEEP_THIS_LOCAL_DRAFT" in json.dumps(request) and "second human line" in json.dumps(request) for request in model.inputs)
        for workspace in workspaces:
            rpc("workspace.close", {"id": workspace})
        wait(lambda: all(s["surface_id"] not in surfaces for s in rpc("gateway.sessions")["sessions"]), "pane retirement retires input recipients")
        assert not gateway.errors, gateway.errors
        print("Ordinary Codex PASS: two native TUIs, local multiline draft, busy message delivery, human Enter and visible response")
    finally:
        model.hold.set()
        gateway.close()
