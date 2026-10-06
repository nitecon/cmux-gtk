"""Isolated version-one HTTP/WebSocket gateway fixture using only the standard library."""
import base64
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from queue import Queue, Empty
import socket
import struct
from threading import Event, RLock, Thread
import uuid


def read_exact(stream, length):
    """Read one bounded framing field, distinguishing EOF from a partial frame."""
    data = stream.read(length)
    if len(data) != length:
        raise EOFError("WebSocket closed")
    return data


def read_message(stream):
    """Decode one masked native-client text frame with the production message budget."""
    first, second = read_exact(stream, 2)
    length = second & 127
    if length == 126:
        length = struct.unpack("!H", read_exact(stream, 2))[0]
    elif length == 127:
        length = struct.unpack("!Q", read_exact(stream, 8))[0]
    assert length <= 65536 and first & 128
    mask = read_exact(stream, 4) if second & 128 else b""
    data = read_exact(stream, length)
    if mask:
        data = bytes(value ^ mask[index % 4] for index, value in enumerate(data))
    if first & 15 == 8:
        raise EOFError("WebSocket close")
    assert first & 15 == 1
    return json.loads(data)


def frame(value):
    """Encode a server text frame without masking or fragmentation."""
    data = json.dumps(value).encode()
    assert len(data) <= 65536
    header = bytes([129, len(data)]) if len(data) < 126 else bytes([129, 126]) + struct.pack("!H", len(data))
    return header + data


class Gateway:
    """Own a loopback fake gateway, stable session/run history and exact exchange evidence."""
    def __init__(self, key):
        self.key = key
        self.lock = RLock()
        self.sessions = []
        self.runs = {}
        self.connection = None
        self.registrations = []
        self.reports = []
        self.accepts = []
        self.hold_accept = False
        self.held = []
        self.errors = []
        gateway = self

        class Handler(BaseHTTPRequestHandler):
            """Authenticate both native upgrade and run reconciliation without external services."""
            protocol_version = "HTTP/1.1"

            def log_message(self, *_):
                """Keep fixture logs free of headers and payloads."""

            def do_GET(self):
                """Serve bounded metadata or transfer ownership to the framed connection loop."""
                if self.headers.get("Authorization") != "Bearer " + gateway.key:
                    self.send_error(401)
                    return
                if self.path == "/v1/execution/connect":
                    accept = base64.b64encode(hashlib.sha1((self.headers["Sec-WebSocket-Key"] + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()).decode()
                    self.send_response(101)
                    self.send_header("Upgrade", "websocket")
                    self.send_header("Connection", "Upgrade")
                    self.send_header("Sec-WebSocket-Accept", accept)
                    self.end_headers()
                    self.close_connection = True
                    gateway.serve(self.connection, self.rfile)
                else:
                    assert self.path.startswith("/v1/projects/") and self.path.endswith("/execution/runs"), self.path
                    with gateway.lock:
                        data = json.dumps(list(gateway.runs.values())).encode()
                    self.send_response(200)
                    self.send_header("Content-Type", "application/json")
                    self.send_header("Content-Length", str(len(data)))
                    self.end_headers()
                    self.wfile.write(data)

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.server.daemon_threads = True
        self.url = f"http://127.0.0.1:{self.server.server_port}"
        self.thread = Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def serve(self, connection, stream):
        """Handle input in one reader and serialize all output through one owned writer."""
        outgoing = Queue()
        stopped = Event()
        with self.lock:
            self.connection = (connection, outgoing)

        def writer():
            """Send queued fake responses; shutting down this socket unblocks the reader."""
            while not stopped.is_set():
                try:
                    value = outgoing.get(timeout=0.2)
                except Empty:
                    continue
                try:
                    connection.sendall(frame(value))
                except OSError:
                    break

        sending = Thread(target=writer, daemon=True)
        sending.start()
        try:
            while True:
                value = read_message(stream)
                with self.lock:
                    if value["type"] == "register":
                        assert value["protocol_version"] == 1
                        self.registrations.append(value)
                        self.sessions = []
                        for native in value["sessions"]:
                            key = str(uuid.uuid5(uuid.NAMESPACE_URL, value["instance_id"] + native["surface_id"] + native["session_id"]))
                            run = next((r["id"] for r in self.runs.values() if r["session_key"] == key and r["finished_at"] is None), None)
                            self.sessions.append(dict(native, session_key=key, run_id=run))
                        outgoing.put(dict(type="registered", protocol_version=1, sessions=self.sessions))
                    elif value["type"] == "heartbeat":
                        outgoing.put(dict(type="heartbeat_ack"))
                    elif value["type"] == "accepted":
                        self.accepts.append(value)
                        run = self.runs[value["run_id"]]
                        assert run["session_key"] == value["session_key"]
                        run["status"] = "running"
                        response = dict(type="accepted", run_id=run["id"], status="running")
                        if self.hold_accept:
                            self.held.append(response)
                        else:
                            outgoing.put(response)
                    elif value["type"] == "report":
                        run = self.runs[value["run_id"]]
                        assert run["session_key"] == value["session_key"]
                        if value["sequence"] > run["last_sequence"] and run["finished_at"] is None:
                            self.reports.append(value)
                            run["last_sequence"] = value["sequence"]
                            run["status"] = {"running": "running", "waiting_input": "waiting_input", "finished": "needs_attention", "failed": "failed"}[value["state"]]
                            if value["state"] in ("finished", "failed"):
                                run["finished_at"] = 1
                            if value.get("summary"):
                                run["summary"] = value["summary"]
                        outgoing.put(dict(type="recorded", run_id=run["id"], status=run["status"], sequence=run["last_sequence"]))
                    else:
                        raise AssertionError(value)
        except (OSError, EOFError):
            pass
        except BaseException as error:
            self.errors.append(repr(error))
        finally:
            stopped.set()
            sending.join(timeout=2)
            with self.lock:
                if self.connection and self.connection[0] is connection:
                    self.connection = None

    def send(self, value):
        """Queue a server message on the currently owned connection."""
        with self.lock:
            assert self.connection is not None
            self.connection[1].put(value)

    def assignment(self, session, prompt="Implement the isolated fixture task"):
        """Persist and offer an assignment with gateway-generated UUIDs and exact routing."""
        value = dict(type="assignment", run_id=str(uuid.uuid4()), task_id=str(uuid.uuid4()),
                     project_ident=session["project_ident"], session_key=session["session_key"],
                     workspace_id=session["workspace_id"], surface_id=session["surface_id"],
                     session_id=session["session_id"], action="execute", prompt=prompt)
        with self.lock:
            self.runs[value["run_id"]] = dict(id=value["run_id"], session_key=session["session_key"],
                                             status="assigned", last_sequence=0, finished_at=None, summary="")
        self.send(value)
        return value

    def disconnect(self):
        """Simulate lost transport without scheduling replacement execution."""
        with self.lock:
            if self.connection:
                self.connection[0].shutdown(socket.SHUT_RDWR)
                self.connection[0].close()

    def close(self):
        """Stop only fixture-owned socket/server threads and expose worker errors."""
        self.disconnect()
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=3)
        assert not self.errors, self.errors
