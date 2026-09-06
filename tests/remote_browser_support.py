"""Real browser-origin assertions for the SSH fixture's isolated remote network namespace."""
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import shlex
from threading import Thread
import time


SERVER = r'''
import base64, hashlib, pathlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
root = pathlib.Path(__file__).parent
class Handler(BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'
    def log_message(self, *args): pass
    def do_GET(self):
        if self.path == '/socket':
            key = self.headers['Sec-WebSocket-Key']
            accept = base64.b64encode(hashlib.sha1((key+'258EAFA5-E914-47DA-95CA-C5AB0DC85B11').encode()).digest()).decode()
            self.send_response(101)
            self.send_header('Upgrade', 'websocket')
            self.send_header('Connection', 'Upgrade')
            self.send_header('Sec-WebSocket-Accept', accept)
            self.end_headers()
            self.wfile.write(b'\x81\x0bremote-only')
            self.wfile.flush()
            self.close_connection = True
            return
        if self.path == '/redirect':
            self.send_response(302)
            self.send_header('Location', '/index.html')
            self.send_header('Content-Length', '0')
            self.end_headers()
            return
        if self.path == '/script.js':
            body = b'window.scriptOrigin="remote-script";'
        elif self.path == '/relative.js':
            body = b'window.relativeOrigin="remote-relative";'
        elif self.path == '/data':
            body = b'remote-data'
        else:
            port = self.server.server_port
            body = (f'<script src="http://localhost:{port}/script.js"></script>'
                    '<script src="relative.js"></script><script>window.socketOrigin="pending";window.dataOrigin="pending";'
                    f'fetch("http://127.0.0.1:{port}/data").then(r=>r.text()).then(t=>window.dataOrigin=t);'
                    f'new WebSocket("ws://localhost:{port}/socket").onmessage=e=>window.socketOrigin=e.data;'
                    '</script><h1>Remote namespace</h1>').encode()
        self.send_response(200)
        self.send_header('Access-Control-Allow-Origin', '*')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)
server = ThreadingHTTPServer(('127.0.0.1', PORT), Handler)
(root/'browser-server-ready').touch()
server.serve_forever()
'''


def verify_remote_browser(root, cli, eventually, remote_id, local_id, report):
    """A same-port local decoy must receive no document, script, fetch or WebSocket traffic."""
    requests = []

    class Decoy(BaseHTTPRequestHandler):
        """Return a recognizable local response and retain only a bounded count of violations."""
        def log_message(self, *args):
            """Suppress routine request logs."""
        def do_GET(self):
            """Any local request is a routing failure; do not retain URLs or headers."""
            if len(requests) < 32:
                requests.append(True)
            self.send_response(503)
            self.end_headers()
            self.wfile.write(b'local-decoy')

    decoy = ThreadingHTTPServer(('127.0.0.1', 0), Decoy)
    thread = Thread(target=decoy.serve_forever, daemon=True)
    thread.start()
    surface = None
    started = time.perf_counter_ns()
    try:
        script = root / 'remote/browser-server.py'
        script.write_text(SERVER.replace('PORT', str(decoy.server_port)))
        cli('send-text', 'python3 ' + shlex.quote(str(script)) + ' &')
        cli('send-key', '\r')
        eventually(lambda: (root / 'remote/browser-server-ready').exists())
        cli('select-workspace', local_id)
        opened = json.loads(cli('browser', 'open', f'http://localhost:{decoy.server_port}/redirect', '--workspace', remote_id, timeout=35))
        assert opened['success'] is True
        surface = opened['surface_ref']

        def loaded():
            """Observe all resource paths through the normal agent-facing browser API."""
            value = json.loads(cli('browser', 'eval', surface,
                                   'JSON.stringify([window.scriptOrigin,window.relativeOrigin,window.dataOrigin,window.socketOrigin])'))
            assert value['success'] is True
            return json.loads(value['data']['result']) == ['remote-script', 'remote-relative', 'remote-data', 'remote-only']

        eventually(loaded)
        assert not requests, 'remote browser sent traffic to the local decoy'
        assert json.loads(cli('current-workspace', '--json'))['uuid'] == local_id
        assert json.loads(cli('ping', '--json'))['pong']
        report['remote_browser'] = {'resource_ready_us': (time.perf_counter_ns() - started) / 1000,
                                    'local_decoy_requests': len(requests),
                                    'checked': ['redirect', 'relative script', 'absolute localhost script', 'absolute loopback fetch', 'WebSocket', 'background workspace']}
    finally:
        try:
            if surface is not None:
                cli('browser', 'close', surface)
        finally:
            decoy.shutdown()
            decoy.server_close()
            thread.join(timeout=5)
