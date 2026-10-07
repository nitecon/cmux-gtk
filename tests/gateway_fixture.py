"""Isolated native RFC6455 gateway fixture with durable consumer cursors and delivery receipts."""
import base64
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import select
import socket
import struct
import threading
import time


def read_exact(connection, length):
    """Read a complete masked-client frame field or raise EOF for deterministic cleanup."""
    value = bytearray()
    while len(value) < length:
        chunk = connection.recv(length - len(value))
        if not chunk:
            raise EOFError
        value.extend(chunk)
    return bytes(value)


def read_frame(connection):
    """Decode one bounded RFC6455 text/control frame, requiring native client masking."""
    first, second = read_exact(connection, 2)
    assert first & 0x80 and second & 0x80
    length = second & 127
    if length == 126:
        length = struct.unpack("!H", read_exact(connection, 2))[0]
    elif length == 127:
        length = struct.unpack("!Q", read_exact(connection, 8))[0]
    assert length <= 65536
    mask = read_exact(connection, 4)
    data = read_exact(connection, length)
    payload = bytes(value ^ mask[index % 4] for index, value in enumerate(data))
    if first & 15 == 8:
        raise EOFError
    assert first & 15 == 1
    return json.loads(payload)


def send_frame(connection, value):
    """Write one unmasked bounded server text frame using the protocol's JSON envelope."""
    data = json.dumps(value).encode()
    assert len(data) <= 65536
    prefix = bytes([0x81, len(data)]) if len(data) < 126 else b"\x81\x7e" + struct.pack("!H", len(data))
    connection.sendall(prefix + data)


class Gateway:
    """Own a loopback server and peer lifetimes; test observations contain no bearer credential."""
    def __init__(self, key):
        """Allocate shared journal-like server state before launching its owned HTTP listener."""
        self.key = key
        self.events = []
        self.details = {}
        self.consumers = {}
        self.receipts = {}
        self.subscriptions = []
        self.heartbeats = 0
        self.record_delay = 0
        self.server_heartbeats = True
        self.full_fetches = 0
        self.replay = None
        self.sockets = set()
        self.maximum_connections = 0
        self.errors = []
        self.lock = threading.RLock()
        fixture = self

        class Handler(BaseHTTPRequestHandler):
            """Serve only the published project/task/stream endpoints with header authentication."""
            protocol_version = "HTTP/1.1"

            def log_message(self, *_arguments):
                """Keep fixture credentials and HTTP traffic out of test output."""

            def do_GET(self):
                """Authorize native bearer requests; upgraded sockets remain owned until disconnect."""
                if self.headers.get("Authorization") != "Bearer " + fixture.key:
                    self.send_error(401)
                    return
                if self.path == "/v1/projects":
                    self.respond([{"ident": name, "canonical_remote": "github.com/fixture/" + name}
                                  for name in ("first", "second", "absent")])
                elif self.path.startswith("/v1/projects/"):
                    with fixture.lock:
                        fixture.full_fetches += 1
                        detail = fixture.details[self.path]
                    self.respond(detail)
                elif self.path == "/v1/tasks/stream":
                    accept = base64.b64encode(hashlib.sha1((self.headers["Sec-WebSocket-Key"] +
                        "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()).decode()
                    self.send_response(101)
                    self.send_header("Upgrade", "websocket")
                    self.send_header("Connection", "Upgrade")
                    self.send_header("Sec-WebSocket-Accept", accept)
                    self.end_headers()
                    self.close_connection = True
                    try:
                        fixture.peer(self.connection)
                    except (EOFError, OSError):
                        pass
                    except BaseException as error:
                        with fixture.lock:
                            fixture.errors.append(repr(error))
                else:
                    self.send_error(404)

            def respond(self, value):
                """Return a known-length REST JSON body without redirect behavior."""
                data = json.dumps(value).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.url = "http://127.0.0.1:" + str(self.server.server_port)

    def peer(self, connection):
        """Offer one event at a time; receipt advances the cursor while later updates preserve terminal states."""
        connection.settimeout(5)
        with self.lock:
            self.sockets.add(connection)
            self.maximum_connections = max(self.maximum_connections, len(self.sockets))
        try:
            subscribe = read_frame(connection)
            assert subscribe["type"] == "subscribe" and subscribe["protocol_version"] == 1
            consumer = subscribe["consumer_id"]
            with self.lock:
                tail = self.events[-1]["id"] if self.events else 0
                saved = self.consumers.get(consumer, tail)
                cursor = subscribe.get("after_event_id")
                cursor = saved if cursor is None else cursor
                assert 0 <= cursor <= saved
                self.consumers.setdefault(consumer, cursor)
                self.subscriptions.append(subscribe)
                replay, self.replay = self.replay, None
            send_frame(connection, dict(type="subscribed", protocol_version=1,
                                        consumer_id=consumer, cursor=cursor, latest_event_id=tail))
            inflight = None
            heartbeat = 0
            while True:
                with self.lock:
                    if inflight is None:
                        event = next((event for event in self.events if event["id"] == replay), None) if replay else next(
                            (event for event in self.events if event["id"] > cursor), None)
                        replay = None
                        if event:
                            inflight = event["id"]
                            send_frame(connection, dict(type="event", event=event))
                if self.server_heartbeats and time.monotonic() - heartbeat > 0.8:
                    send_frame(connection, dict(type="heartbeat"))
                    heartbeat = time.monotonic()
                readable, _, _ = select.select([connection], [], [], 0.05)
                if not readable:
                    continue
                value = read_frame(connection)
                if value["type"] == "heartbeat":
                    with self.lock:
                        self.heartbeats += 1
                    send_frame(connection, dict(type="heartbeat_ack"))
                else:
                    assert value["type"] == "ack"
                    event_id = value["event_id"]
                    with self.lock:
                        assert event_id <= self.consumers[consumer] or event_id == inflight
                        previous = self.receipts.get(event_id, {})
                        if previous.get("status") in ("injected", "skipped", "failed", "uncertain"):
                            status = previous["status"]
                        elif previous.get("status") == "queued" and value["status"] == "received":
                            status = "queued"
                        else:
                            status = value["status"]
                            self.receipts[event_id] = value
                        cursor = max(cursor, event_id)
                        self.consumers[consumer] = max(self.consumers[consumer], event_id)
                    if event_id == inflight:
                        inflight = None
                    time.sleep(self.record_delay)
                    send_frame(connection, dict(type="recorded", event_id=event_id, status=status))
        finally:
            with self.lock:
                self.sockets.discard(connection)

    def add(self, project="first", kind="task_created", content="Inspect this task", **changes):
        """Append a lifecycle snapshot and return its monotonically increasing event ID."""
        with self.lock:
            event_id = len(self.events) + 1
            task = dict(id="task-" + str(event_id), title=content, description="Ordinary task context",
                        details="Task specification", status="todo", kind="normal", delegated_to_task_id=None)
            comment = dict(id="comment-" + str(event_id), author="ordinary-writer", author_type="user",
                           content=content, task_id=task["id"])
            event = dict(id=event_id, kind=kind, project_ident=project, canonical_remote="github.com/fixture/" + project,
                         task=task, comment=comment if kind != "task_created" else None,
                         delegation=None, created_at=0, truncated=False, deliveries=[])
            event.update(changes)
            self.events.append(event)
            return event_id

    def outcome(self, event_id):
        """Expose saved receipt status for polling from the integration scenario."""
        with self.lock:
            return self.receipts.get(event_id, {}).get("status")

    def disconnect(self, replay=None):
        """Force reconnect, optionally replaying one already-offered event to test client fences."""
        with self.lock:
            self.replay = replay
            for connection in list(self.sockets):
                try:
                    connection.shutdown(socket.SHUT_RDWR)
                except OSError:
                    pass

    def close(self):
        """Stop the listener and all upgraded peers before the test deletes its isolated directories."""
        self.disconnect()
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=5)
        assert not self.errors, self.errors
