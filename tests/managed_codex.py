"""Real Codex TUI/backend acceptance, with a local deterministic Responses model in Actions only."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
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
                        "content": [{"type": "output_text", "text": "CMUX_NATIVE_QUEUE_ACCEPTED", "annotations": []}]}
                response = {"id": response_id, "object": "response", "status": "completed", "output": [item],
                            "usage": {"input_tokens": 10, "output_tokens": 5, "total_tokens": 15}}
                events = [
                    {"type": "response.created", "response": {**response, "status": "in_progress", "output": []}},
                    {"type": "response.output_item.added", "output_index": 0, "item": {**item, "status": "in_progress", "content": []}},
                    {"type": "response.content_part.added", "item_id": item["id"], "output_index": 0, "content_index": 0, "part": {"type": "output_text", "text": "", "annotations": []}},
                    {"type": "response.output_text.delta", "item_id": item["id"], "output_index": 0, "content_index": 0, "delta": "CMUX_NATIVE_QUEUE_ACCEPTED"},
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
        (home / "config.toml").write_text(f'''model = "cmux-test"
model_provider = "cmux_fixture"
cli_auth_credentials_store = "file"
[model_providers.cmux_fixture]
name = "CMUX Actions model fixture"
base_url = "http://127.0.0.1:{self.server.server_port}/v1"
wire_api = "responses"
requires_openai_auth = false
[projects.{json.dumps(str(home.parent / "managed" / "first"))}]
trust_level = "trusted"
''', encoding="utf-8")

    def close(self):
        self.hold.set()
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(5)


def verify(rpc, root, cmux, codex, model):
    """Launch two real native provider peers, retain drafts, queue while busy and fence replacements."""
    root = Path(root) / "managed"
    root.mkdir()
    directory = root / "first"
    directory.mkdir()
    subprocess.run(["git", "init", "-q", str(directory)], check=True, capture_output=True)
    subprocess.run(["git", "-C", str(directory), "remote", "add", "origin", "git@github.com:fixture/first.git"], check=True)
    gateway = Gateway("managed-codex-gateway")

    def wait(predicate, description, seconds=60):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            if predicate():
                return
            time.sleep(0.5)
        print("Managed provider status:", json.dumps(rpc("gateway.status"), indent=2))
        for surface in surfaces:
            print("Managed provider screen:", rpc("surface.read_text", {"id": surface})["text"])
        raise AssertionError(description)

    surfaces = []
    try:
        rpc("gateway.configure", {"enabled": True, "url": gateway.url, "injection_approved": True, "api_key": gateway.key})
        wait(lambda: rpc("gateway.status")["connection"] == "Connected", "gateway connected")
        for name in ("managed-first", "managed-peer"):
            workspace = rpc("workspace.create", {"name": name, "working_directory": str(directory)})
            rpc("workspace.select", {"id": workspace["uuid"]})
            surface = next(s["uuid"] for s in rpc("surface.list")["surfaces"] if s["workspace_uuid"] == workspace["uuid"])
            surfaces.append(surface)
            wait(lambda: rpc("surface.read_text", {"id": surface})["text"].strip(), "native shell ready")
            args = [str(cmux), "codex", "--", "-a", "never", "--sandbox", "read-only"]
            if os.name == "nt":
                command = f'set "PATH={codex.parent};%PATH%" && ' + subprocess.list2cmdline(args)
            else:
                command = "PATH=" + shlex.quote(str(codex.parent)) + ':"$PATH" ' + shlex.join(args)
            rpc("surface.send_text", {"id": surface, "text": command})
            rpc("surface.send_key", {"id": surface, "key": "\r"})
            wait(lambda: "OpenAI Codex" in rpc("surface.read_text", {"id": surface})["text"], "real native Codex TUI startup")
        wait(lambda: len(rpc("gateway.status")["managed_codex"]) == 2 and all(m["process_pid"] for m in rpc("gateway.status")["managed_codex"]), "two tracked native TUI generations")
        # A same-user caller outside the pane cannot claim another terminal's managed launch.
        try:
            rpc("gateway.codex.start", {"surface_id": surfaces[0], "executable": str(codex)})
        except (RuntimeError, subprocess.CalledProcessError):
            pass
        else:
            raise AssertionError("Foreign launcher must not acquire a pane")
        sessions = [rpc("gateway.session", {"surface_id": surface}) for surface in surfaces]
        assert all(s["delivery_transport"] == "codex_queue" for s in sessions)
        assert len({s["codex_thread_id"] for s in sessions}) == 2
        assert len({s["recipient_session_id"] for s in sessions}) == 2
        # This genuine draft deliberately makes terminal injection unsafe, but must not block native queuing.
        rpc("surface.send_text", {"id": surfaces[0], "text": "KEEP_THIS_DRAFT"})
        model.hold.clear()
        event = gateway.add(content="Managed native queue first event", project="first")
        wait(lambda: gateway.outcome(event) == "injected", "native queue accepted for both peers despite draft")
        wait(lambda: len(model.inputs) >= 2, "queued input reached the real provider/model request")
        assert "KEEP_THIS_DRAFT" in rpc("surface.read_text", {"id": surfaces[0]})["text"]
        busy_event = gateway.add(content="Managed native queue busy followup", project="first")
        wait(lambda: gateway.outcome(busy_event) == "injected", "native queue accepted while both providers busy")
        assert "KEEP_THIS_DRAFT" in rpc("surface.read_text", {"id": surfaces[0]})["text"]
        receipts = rpc("gateway.status")["receipts"]
        native_receipts = [r for r in receipts if r["event_id"] in (str(event), str(busy_event))]
        assert len(native_receipts) == 4
        assert all("model receipt is not yet confirmed" in r["reason"] for r in native_receipts)
        # Public snapshots must not contain the per-launch endpoint/capability.
        public = json.dumps(rpc("gateway.status"))
        assert "token" not in public and "endpoint" not in public
        model.hold.set()
        wait(lambda: len(model.inputs) >= 4, "provider autonomously drains busy followups")
        wait(lambda: all("CMUX_NATIVE_QUEUE_ACCEPTED" in rpc("surface.read_text", {"id": surface})["text"] for surface in surfaces), "native TUI renders actual model response")
        for surface in surfaces:
            rpc("surface.close", {"id": surface})
        wait(lambda: not rpc("gateway.status")["managed_codex"], "pane retirement reaps managed backend lifetimes")
        assert not gateway.errors, gateway.errors
        print("Real Codex managed queue PASS: two native TUIs, exact threads, draft preservation, busy queue, visible response and retirement")
    finally:
        model.hold.set()
        gateway.close()
