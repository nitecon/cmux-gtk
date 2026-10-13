"""Real Codex TUI/backend acceptance, with a local deterministic Responses model in Actions only."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
import ntpath
from pathlib import Path
import re
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
        # Preselect the non-admin CI mode before Codex's first-interaction onboarding.
        windows = '[windows]\nsandbox = "unelevated"\n' if os.name == "nt" else ""
        (home / "config.toml").write_text(f'''model = "gpt-6.1-sol"
model_provider = "cmux_fixture"
cli_auth_credentials_store = "file"
[model_providers.cmux_fixture]
name = "CMUX Actions model fixture"
base_url = "http://127.0.0.1:{self.server.server_port}/v1"
wire_api = "responses"
requires_openai_auth = false
{windows}
[projects.{json.dumps(project)}]
trust_level = "trusted"
''', encoding="utf-8")

    def close(self):
        self.hold.set()
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(5)


def verify(rpc, root, codex, model, native_key):
    """Exercise real native Codex menus and defer notifications around provider-owned drafts and work."""
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
            print("Process input screen:", json.dumps(rpc("surface.read_text", {"id": surface})))
        raise AssertionError(description)

    def screen(surface):
        return rpc("surface.read_text", {"id": surface})["text"]

    def selected():
        """Observe the model menu's actual selection marker, not just successful key submission."""
        # ConPTY clipboard text can join soft-wrapped rows, including the selection marker.
        rows = re.findall(r"›\s*(\d+)\.\s", screen(surfaces[0]))
        return int(rows[-1]) if rows else None

    def received(event):
        """Codex may also request a title; observe the task notification rather than request counts."""
        notification = f"<Gateway message>New delegated task received: task-{event}</Gateway message>"
        return notification in json.dumps(model.inputs)

    try:
        rpc("gateway.configure", dict(enabled=True, url=gateway.url, injection_approved=True, api_key=gateway.key))
        wait(lambda: rpc("gateway.status")["connection"] == "Connected", "gateway connected")
        for name in ("first", "peer"):
            workspace = rpc("workspace.create", dict(name=name, working_directory=str(directory)))
            workspaces.append(workspace["uuid"])
            rpc("workspace.select", {"id": workspace["uuid"]})
            surface = next(s["uuid"] for s in rpc("surface.list")["surfaces"] if s["workspace_uuid"] == workspace["uuid"])
            surfaces.append(surface)
            wait(lambda: screen(surface).strip(), "native shell ready")
            args = [str(codex), "--no-daemon", "-a", "never", "--sandbox", "danger-full-access"]
            command = subprocess.list2cmdline(args) if os.name == "nt" else shlex.join(args)
            rpc("surface.send_text", {"id": surface, "text": command})
            native_key("Return")
            wait(lambda: any(marker in screen(surface) for marker in ("OpenAI Codex", "Trust this folder?")), "ordinary native Codex")
            if "Trust this folder?" in screen(surface):
                native_key("Return")
            wait(lambda: "? for shortcuts" in screen(surface), "ordinary editable prompt")
            assert rpc("surface.read_text", {"id": surface})["input"] is None
        sessions = [rpc("gateway.session", {"surface_id": surface}) for surface in surfaces]
        assert all(not session["composer_active"] for session in sessions)
        assert len({s["recipient_session_id"] for s in sessions}) == 2
        wait(lambda: rpc("gateway.status")["agents"] == 2, "both native recipients included in background discovery")
        assert not model.inputs, "Startup must not submit synthetic model input"
        rpc("workspace.select", {"id": workspaces[0]})
        rpc("surface.focus", {"id": surfaces[0]})
        rpc("surface.send_text", {"id": surfaces[0], "text": "/model"})
        wait(lambda: "/model" in screen(surfaces[0]), "native slash command received before Enter")
        native_key("Return")
        wait(lambda: "select model" in screen(surfaces[0]).lower(), "real /model menu opened")
        initial = selected()
        assert initial
        native_key("Down")
        wait(lambda: selected() and selected() != initial, "plain Down changes the model selection")
        native_key("Up")
        wait(lambda: selected() == initial, "plain Up restores the model selection")
        menu_event = gateway.add(content="Never submit into a model-selection menu")
        wait(lambda: gateway.outcome(menu_event) == "queued" and received(menu_event), "menu holds its recipient while peer proceeds")
        assert "select model" in screen(surfaces[0]).lower()
        native_key("Escape")
        wait(lambda: gateway.outcome(menu_event) == "injected", "closing the menu drains its original notification")
        wait(lambda: all("CMUX_INPUT_ACCEPTED" in screen(surface) and "esc to interrupt" not in screen(surface).lower()
                         for surface in surfaces), "both native model responses finish")
        human = "KEEP_NATIVE_DRAFT λ\nsecond human line"
        rpc("surface.send_text", {"id": surfaces[0], "text": human})
        wait(lambda: "KEEP_NATIVE_DRAFT" in screen(surfaces[0]), "draft stays in Codex's own input")
        model.hold.clear()
        event = gateway.add(content="Preserve the native draft")
        wait(lambda: gateway.outcome(event) == "queued" and received(event), "draft defers only its own notification")
        assert "KEEP_NATIVE_DRAFT" not in json.dumps(model.inputs)
        assert "KEEP_NATIVE_DRAFT" in screen(surfaces[0])
        busy = gateway.add(content="Wait until native work finishes")
        wait(lambda: gateway.outcome(busy) == "queued", "busy notification retained")
        native_key("Return")
        wait(lambda: "KEEP_NATIVE_DRAFT" in json.dumps(model.inputs), "native Enter submits the user's own draft")
        model.hold.set()
        wait(lambda: gateway.outcome(event) == gateway.outcome(busy) == "injected", "native prompts drain queued notifications after work")
        for event_id in (menu_event, event, busy):
            wait(lambda: received(event_id), "compact notification received by real model")
        assert "Preserve the native draft" not in json.dumps(model.inputs)
        assert any("KEEP_NATIVE_DRAFT" in json.dumps(request) and "second human line" in json.dumps(request) for request in model.inputs)
        for workspace in workspaces:
            rpc("workspace.close", {"id": workspace})
        wait(lambda: all(s["surface_id"] not in surfaces for s in rpc("gateway.sessions")["sessions"]), "pane retirement retires recipients")
        assert not gateway.errors, gateway.errors
        print("Ordinary Codex PASS: native /model arrows, native multiline draft, deferred menus/work and real model delivery")
    finally:
        model.hold.set()
        gateway.close()
