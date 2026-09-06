"""Real browser-origin assertions for the SSH fixture's isolated remote network namespace."""
import json
from contextlib import ExitStack
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import shlex
from threading import Thread
import time


SERVER = r'''
import base64, hashlib, pathlib, json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
root = pathlib.Path(__file__).parent
identity = IDENTITY
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
            body = ("window.scriptOrigin=" + json.dumps("remote-script-" + identity) + ";").encode()
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


def verify_remote_browser(root, cli, eventually, remote_id, second_remote_id, local_id, report):
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
    surfaces = []
    cleanup = ExitStack()
    started = time.perf_counter_ns()
    try:
        url = f'http://localhost:{decoy.server_port}/redirect'
        for identity, workspace in [('first', remote_id), ('second', second_remote_id)]:
            cli('select-workspace', workspace)
            eventually(lambda: json.loads(cli('health', '--json'))['alive'] and
                       str(root / 'remote') in cli('read-text').replace('\n', '').replace('\r', ''))
            directory = root / 'remote' / identity
            directory.mkdir()
            script = directory / 'browser-server.py'
            script.write_text(SERVER.replace('PORT', str(decoy.server_port)).replace('IDENTITY', repr(identity)))
            cli('send-text', 'python3 ' + shlex.quote(str(script)) + ' &')
            cli('send-key', '\r')
            eventually(lambda: (directory / 'browser-server-ready').exists())
            cli('select-workspace', local_id)
            opened = json.loads(cli('browser', 'open', url, '--workspace', workspace, timeout=35))
            assert opened['success'] is True
            surfaces.append((opened['surface_ref'], identity))
            cleanup.callback(cli, 'browser', 'close', opened['surface_ref'])

        def loaded(surface, identity):
            """Observe all resource paths and workspace provenance through the browser API."""
            value = json.loads(cli('browser', 'eval', surface,
                                   'JSON.stringify([window.scriptOrigin,window.relativeOrigin,window.dataOrigin,window.socketOrigin])'))
            assert value['success'] is True
            return json.loads(value['data']['result']) == ['remote-script-' + identity, 'remote-relative', 'remote-data', 'remote-only']

        for surface, identity in surfaces:
            eventually(lambda: loaded(surface, identity))
        assert surfaces[0][0] != surfaces[1][0]
        # A fresh first-workspace navigation after creating the second must retain its remote origin.
        assert json.loads(cli('browser', 'goto', surfaces[0][0], url))['success'] is True
        eventually(lambda: loaded(*surfaces[0]))
        eventually(lambda: loaded(*surfaces[1]))

        def endpoint(workspace):
            """Observe the automatically forwarded endpoint for the same remote port."""
            rows = json.loads(cli('ports', '--workspace', workspace, '--json'))['ports'] or []
            return next((row['forwarded_local_port'] for row in rows
                         if row['port'] == decoy.server_port and row['provenance'] == 'remote'), None)

        eventually(lambda: endpoint(remote_id) and endpoint(second_remote_id))
        assert endpoint(remote_id) != endpoint(second_remote_id)
        assert decoy.server_port not in [endpoint(remote_id), endpoint(second_remote_id)]
        assert not requests, 'remote browser sent traffic to the local decoy'
        assert json.loads(cli('current-workspace', '--json'))['uuid'] == local_id
        assert json.loads(cli('ping', '--json'))['pong']
        report['remote_browser'] = {'resource_ready_us': (time.perf_counter_ns() - started) / 1000,
                                    'local_decoy_requests': len(requests),
                                    'checked': ['redirect', 'relative script', 'absolute localhost script', 'absolute loopback fetch', 'WebSocket', 'background workspace', 'same-port workspace isolation', 'first workspace renavigation']}
    finally:
        try:
            cleanup.close()
        finally:
            decoy.shutdown()
            decoy.server_close()
            thread.join(timeout=5)
