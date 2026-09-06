#!/usr/bin/env python3
"""Real SSH/PTY, script inheritance, workspace ordering and session roundtrip."""
import getpass
import json
import os
from pathlib import Path
import shlex
import platform
import sys
import shutil
import socket
import subprocess
import tempfile
import time
import uuid
from functools import partial
from process_support import stop_process, wait_until


eventually = partial(wait_until, description="workspace launch state", timeout=15)

with tempfile.TemporaryDirectory(prefix="cmux-workflow-") as directory:
    root = Path(directory)
    binary_dir = Path(os.environ.get("CMUX_BIN_DIR", "target/debug"))
    expected_profile = os.environ.get("CMUX_EXPECT_PROFILE", "debug")
    report_output = os.environ.get("CMUX_SSH_REPORT")
    report = {"schema": 1, "workload": "ssh_workspace_launch_split_restart", "status": "failed",
              "host": {"platform": platform.platform(), "machine": platform.machine()},
              "revision": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True, timeout=10).strip(),
              "warmup": 0, "expected_marker_operations": 6, "upload_failure_policy": "fail scp while the shared first-attempt guard is absent",
              "transport": "loopback OpenSSH with real owned Go daemon",
              "includes": "Cold startup, deployment retry, readiness polling and remote marker execution; heterogeneous phases are not a steady-state latency distribution",
              "startups": [], "operations": []}
    completed = False
    forced_shutdown = None
    for name in ["runtime", "bin", "local", "remote", "data/cmux/bin"]:
        (root / name).mkdir(parents=True, mode=0o700)
    env = dict(os.environ, XDG_DATA_HOME=str(root / "data"),
               XDG_CONFIG_HOME=str(root / "config"), XDG_STATE_HOME=str(root / "state"),
               XDG_RUNTIME_DIR=str(root / "runtime"), GDK_BACKEND="x11",
               LIBGL_ALWAYS_SOFTWARE="1", CMUX_NO_UPDATE="1", CMUX_LOG=str(root / "events.jsonl"),
               PATH=str(root / "bin") + ":" + os.environ["PATH"])
    shutil.copy2("target/cmuxd-remote", root / "data/cmux/bin/cmuxd-remote-linux-amd64")
    for key in ["host-key", "client-key"]:
        subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(root / key)], check=True)
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    server_config = root / "sshd_config"
    server_config.write_text(f"""Port {port}
ListenAddress 127.0.0.1
HostKey {root}/host-key
PidFile {root}/sshd.pid
AuthorizedKeysFile {root}/client-key.pub
StrictModes no
PasswordAuthentication no
KbdInteractiveAuthentication no
UsePAM yes
Subsystem sftp internal-sftp
""")
    config = root / "ssh_config"
    config.write_text(f"""Host cmux-ci
  HostName 127.0.0.1
  Port {port}
  User {getpass.getuser()}
  IdentityFile {root}/client-key
  IdentitiesOnly yes
  StrictHostKeyChecking no
  UserKnownHostsFile /dev/null
  LogLevel ERROR
""")
    for command in ["ssh", "scp"]:
        wrapper = root / "bin" / command
        invocation = f'/usr/bin/{command} -F {shlex.quote(str(config))} "$@"'
        if command == "ssh":
            wrapper.write_text(f'#!/bin/sh\ncase "$*" in\n*"serve --stdio") tee {root}/rpc-in | {invocation} | tee {root}/rpc-out ;;\n*) exec {invocation} ;;\nesac\n')
        else:
            # Verify the client retries deployment itself after a failed upload.
            wrapper.write_text(f'#!/bin/sh\nif [ ! -e {root}/scp-attempted ]; then touch {root}/scp-attempted; exit 1; fi\nexec {invocation}\n')
        wrapper.chmod(0o755)
    ssh_log = (root / "sshd.log").open("w+")
    subprocess.run(["sudo", "mkdir", "-p", "/run/sshd"], check=True)
    sshd = subprocess.Popen(["sudo", "/usr/sbin/sshd", "-D", "-e", "-f", str(server_config)], stdout=ssh_log, stderr=ssh_log)
    app_log = (root / "app.log").open("w+")
    app = None
    socket_path = root / "runtime/cmux/cmux.sock"
    session_path = root / "data/cmux/session.json"
    script = root / "startup 'quoted'.sh"
    script.write_text(f'printf "%s\\n" "$PWD" >> {shlex.quote(str(root / "launches"))}\nexec /bin/sh\n')
    local_id, remote_id, second_remote_id = (str(uuid.uuid4()) for _ in range(3))

    def workspace(identity, name, **fields):
        """Construct one persisted workspace fixture with a fresh terminal surface and supplied launch fields."""
        return dict(uuid=identity, name=name, active_pane_uuid=None,
                    layout=dict(type="Leaf", pane_id=1, surface_uuid=str(uuid.uuid4()), shell="/bin/sh", cwd=""), **fields)

    session_path.write_text(json.dumps(dict(version=3, active_index=0, workspaces=[
        workspace(local_id, "Script project", working_directory=str(root / "local"), startup_script=str(script), color="#24466b"),
        workspace(remote_id, "SSH project", remote_target="cmux-ci", remote_directory=str(root / "remote")),
        workspace(second_remote_id, "Second SSH project", remote_target="cmux-ci", remote_directory=str(root / "remote")),
    ])))

    def cli(*args):
        """Run the selected CLI against this isolated application with a fifteen-second process timeout."""
        return subprocess.check_output([str(binary_dir / "cmux"), "--socket", str(socket_path), *args], env=env, text=True, timeout=15)

    def start():
        """Remove a stale fixture socket, launch GTK and wait for its control socket to appear."""
        global app
        if socket_path.exists():
            socket_path.unlink()
        tick = time.perf_counter_ns()
        app = subprocess.Popen([str(binary_dir / "cmux-app")], env=env, stdout=app_log, stderr=app_log)
        eventually(socket_path.exists)
        report["startups"].append({"pid": app.pid, "socket_ready_us": (time.perf_counter_ns() - tick) / 1000})
        time.sleep(0.5)

    def stop():
        """Quit through the real GTK action so owned worker cancellation and log draining can finish."""
        windows = subprocess.check_output(
            ["xdotool", "search", "--onlyvisible", "--pid", str(app.pid)], text=True, timeout=10,
        ).split()
        assert windows, "application has no visible window for normal quit"
        subprocess.check_call(
            ["xdotool", "windowfocus", windows[-1], "key", "--clearmodifiers", "ctrl+q"], timeout=10,
        )
        assert app.wait(timeout=15) == 0, "normal GTK quit failed"

    def session():
        """Read the current persisted session for workspace and launch-state assertions."""
        return json.loads(session_path.read_text())

    def launches():
        """Return directories recorded by the startup script, or no records before its first launch."""
        try:
            return (root / "launches").read_text().splitlines()
        except FileNotFoundError:
            return []

    def remote_write(name):
        """Observe native/prompt readiness, submit once, then verify complete remote marker contents."""
        def ready():
            """Wait read-only for realization and this fixture's remote-directory prompt before sending input."""
            if not json.loads(cli("health", "--json"))["alive"]:
                return False
            text = cli("read-text").replace("\n", "").replace("\r", "")
            return str(root / "remote") in text
        started = time.perf_counter_ns()
        eventually(ready)
        readiness_us = (time.perf_counter_ns() - started) / 1000
        before = json.loads(cli("diagnostics", "--json"))
        assert before["pid"] == app.pid and before["build_profile"] == expected_profile
        assert before["first_opengl_context"] is not None
        tick = time.perf_counter_ns()
        cli("send-text", f"printf '%s' \"$PWD\" > {name}")
        windows = subprocess.check_output(
            ["xdotool", "search", "--onlyvisible", "--pid", str(app.pid)], text=True, timeout=10,
        ).split()
        subprocess.check_call(
            ["xdotool", "windowfocus", windows[-1], "key", "--clearmodifiers", "Return"], timeout=10,
        )
        def written():
            """Require the expected payload, tolerating the brief create-before-write observation window."""
            try:
                return (root / "remote" / name).read_text() == str(root / "remote")
            except FileNotFoundError:
                return False
        eventually(written)
        assert not (root / "local" / name).exists()
        duration_us = (time.perf_counter_ns() - tick) / 1000
        after = json.loads(cli("diagnostics", "--json"))
        assert after["pid"] == app.pid and after["build_profile"] == expected_profile
        report["operations"].append({"phase": name, "readiness_us": readiness_us,
                                     "input_to_marker_us": duration_us, "before": before, "after": after})

    def remote_setup_traced():
        """Require matching local request lifetimes and remote handler timings for both PTY setups."""
        try:
            with (root / "events.jsonl").open() as source:
                lines = source.read(8 * 1024 * 1024).splitlines()
        except FileNotFoundError:
            return False
        starts, completed, connections, handshakes, launches_by_trace = {}, [], {}, {}, {}
        for line in lines:
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            if event["pid"] != app.pid:
                continue
            if event["event"] == "ssh.rpc.begin":
                starts[event["fields"]["trace_id"]] = event["fields"]
            elif event["event"] == "ssh.rpc.complete":
                completed.append(event["fields"])
            elif event["event"] == "ssh.connection.begin":
                connections[event["fields"]["trace_id"]] = event["fields"]
            elif event["event"] == "workspace.ssh.launch":
                launches_by_trace[event["fields"]["trace_id"]] = event["fields"]
            elif event["event"] == "ssh.handshake.complete":
                handshakes[event["fields"]["trace_id"]] = event["fields"]
        successful = [fields for fields in completed if fields["outcome"] == "success"]
        if any(sum(fields["method"] == method for fields in successful) < 2
               for method in ("session.spawn", "proxy.stream.subscribe")):
            return False
        for fields in successful:
            identity = fields["trace_id"]
            assert str(uuid.UUID(identity)) == identity
            assert identity in starts
            assert fields["request_id"] == starts[identity]["request_id"]
            assert fields["workspace_id"] == starts[identity]["workspace_id"]
            parent = fields["parent_trace_id"]
            assert parent == starts[identity]["parent_trace_id"] and parent in connections
            launch = launches_by_trace[connections[parent]["parent_trace_id"]]
            assert launch["workspace_id"] == fields["workspace_id"]
            assert launch["outcome"] == "scheduled" and launch["origin"] == "restore"
            assert handshakes[parent]["outcome"] == "success"
            assert type(handshakes[parent]["remote_handler_duration_us"]) is int
            assert type(fields["remote_handler_duration_us"]) is int
            assert fields["remote_handler_duration_us"] >= 0
            assert fields["duration_us"] >= 0
        return True

    try:
        eventually(lambda: subprocess.run([str(root / "bin/ssh"), "-o", "BatchMode=yes", "cmux-ci", "true"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode == 0)
        start()
        eventually(lambda: len(launches()) >= 1)
        cli("split", "--direction", "horizontal")
        eventually(lambda: len(launches()) >= 2)
        assert all(p == str(root / "local") for p in launches())
        cli("select-workspace", remote_id)
        remote_write("first-result")
        # Discover a service started inside the actual SSH PTY, without selecting it while polling.
        listener_script = root / "remote" / "listener.py"
        listener_script.write_text("import socket,pathlib,time\n"
                                   "root=pathlib.Path(__file__).parent\n"
                                   "s=socket.socket();s.bind(('127.0.0.1',0));s.listen()\n"
                                   "(root/'listener-port').write_text(str(s.getsockname()[1]))\n"
                                   "s.settimeout(0.1);connections=0\n"
                                   "while not (root/'listener-stop').exists():\n"
                                   " try:\n"
                                   "  c,_=s.accept();c.settimeout(5)\n"
                                   "  connections+=1\n"
                                   "  if connections==1:\n"
                                   "   while c.recv(32768): pass\n"
                                   "   c.sendall(b'cmux-forwarded-'*8192)\n"
                                   "  elif connections==2:\n"
                                   "   c.sendall(b'early-response');c.shutdown(socket.SHUT_WR)\n"
                                   "   request=bytearray()\n"
                                   "   while chunk:=c.recv(32768):\n"
                                   "    request.extend(chunk);assert len(request)<=131072\n"
                                   "   (root/'late-request').write_bytes(request)\n"
                                   "  else:\n"
                                   "   c.sendall(b'listener-retired');s.close();c.settimeout(20)\n"
                                   "   assert c.recv(1)==b''\n"
                                   "   (root/'retired-client-closed').touch()\n"
                                   "   c.close();break\n"
                                   "  c.close()\n"
                                   " except socket.timeout: pass\n"
                                   "s.close()\n")
        cli("send-text", "python3 " + shlex.quote(str(listener_script)) + " &")
        cli("send-key", "\r")
        eventually(lambda: (root / "remote/listener-port").exists())
        listener_port = int((root / "remote/listener-port").read_text())
        cli("select-workspace", second_remote_id)
        def remote_ports():
            """Read the originating workspace through the normal CLI with remote provenance intact."""
            return json.loads(cli("ports", "--workspace", remote_id, "--json"))["ports"]
        eventually(lambda: remote_ports() and any(row["port"] == listener_port and row["provenance"] == "remote"
                                                  for row in remote_ports()))
        assert json.loads(cli("current-workspace", "--json"))["uuid"] == second_remote_id
        eventually(lambda: any(row["port"] == listener_port and row.get("forwarded_local_port") for row in (remote_ports() or [])))
        forwarded_port = next(row["forwarded_local_port"] for row in remote_ports() if row["port"] == listener_port)
        # Loopback SSH shares this host: the real remote listener already occupies its preferred port.
        assert forwarded_port != listener_port, "forwarding did not publish its collision fallback port"
        forwarding_before = json.loads(cli("diagnostics", "--json"))["remote_forwarding"]
        with socket.create_connection(("127.0.0.1", forwarded_port), timeout=10) as forwarded:
            forwarded.settimeout(10)
            forwarded.sendall(b"request finished")
            forwarded.shutdown(socket.SHUT_WR)
            payload = bytearray()
            while chunk := forwarded.recv(32768):
                payload.extend(chunk)
                assert len(payload) <= 15 * 8192
        assert bytes(payload) == b"cmux-forwarded-" * 8192
        eventually(lambda: json.loads(cli("diagnostics", "--json"))["remote_forwarding"]["active_client_tasks"] == 0)
        forwarding_after = json.loads(cli("diagnostics", "--json"))["remote_forwarding"]
        assert forwarding_after["remote_write_acknowledged_bytes"] - forwarding_before["remote_write_acknowledged_bytes"] == len(b"request finished")
        assert forwarding_after["local_write_completed_bytes"] - forwarding_before["local_write_completed_bytes"] == len(payload)
        assert forwarding_after["rejected_data_chunks"] == forwarding_before["rejected_data_chunks"]
        assert forwarding_after["close_requests"] > forwarding_before["close_requests"]
        assert forwarding_after["confirmed_closes"] > forwarding_before["confirmed_closes"]
        assert forwarding_after["failed_closes"] == forwarding_before["failed_closes"]
        report["forwarding"] = {"before": forwarding_before, "after": forwarding_after}
        # Remote FIN must reach this client without closing its still-writable request direction.
        late_request = b"request-after-response-fin" * 4096
        with socket.create_connection(("127.0.0.1", forwarded_port), timeout=10) as forwarded:
            forwarded.settimeout(10)
            early_response = bytearray()
            while chunk := forwarded.recv(32768):
                early_response.extend(chunk)
                assert len(early_response) <= len(b"early-response")
            assert early_response == b"early-response"
            forwarded.sendall(late_request)
            forwarded.shutdown(socket.SHUT_WR)
        late_request_path = root / "remote/late-request"
        eventually(lambda: late_request_path.exists() and late_request_path.read_bytes() == late_request)
        eventually(lambda: json.loads(cli("diagnostics", "--json"))["remote_forwarding"]["active_client_tasks"] == 0)
        reverse_after = json.loads(cli("diagnostics", "--json"))["remote_forwarding"]
        assert reverse_after["remote_write_acknowledged_bytes"] - forwarding_after["remote_write_acknowledged_bytes"] == len(late_request)
        assert reverse_after["local_write_completed_bytes"] - forwarding_after["local_write_completed_bytes"] == len(early_response)
        assert reverse_after["rejected_data_chunks"] == forwarding_after["rejected_data_chunks"]
        assert reverse_after["confirmed_closes"] > forwarding_after["confirmed_closes"]
        assert reverse_after["failed_closes"] == forwarding_after["failed_closes"]
        report["forwarding"]["reverse_half_close_after"] = reverse_after
        assert json.loads(cli("current-workspace", "--json"))["uuid"] == second_remote_id
        assert json.loads(cli("ping", "--json"))["pong"]
        # The server closes only its listening socket, keeping the accepted socket alive.
        # Discovery must retire forwarding and close that idle client without a transport restart.
        retirement_started = time.perf_counter_ns()
        with socket.create_connection(("127.0.0.1", forwarded_port), timeout=10) as forwarded:
            forwarded.settimeout(20)
            retired_response = bytearray()
            while chunk := forwarded.recv(32768):
                retired_response.extend(chunk)
                assert len(retired_response) <= len(b"listener-retired")
            assert retired_response == b"listener-retired"
        eventually(lambda: (root / "remote/retired-client-closed").exists())
        eventually(lambda: remote_ports() is not None and not any(row["port"] == listener_port for row in remote_ports()))
        eventually(lambda: json.loads(cli("diagnostics", "--json"))["remote_forwarding"]["active_listener_tasks"] == 0)
        retired_after = json.loads(cli("diagnostics", "--json"))["remote_forwarding"]
        assert retired_after["active_client_tasks"] == 0
        assert retired_after["confirmed_closes"] > reverse_after["confirmed_closes"]
        assert retired_after["failed_closes"] == reverse_after["failed_closes"]
        report["forwarding"]["service_retirement"] = {
            "duration_us": (time.perf_counter_ns() - retirement_started) / 1000,
            "after": retired_after,
        }
        assert json.loads(cli("current-workspace", "--json"))["uuid"] == second_remote_id
        assert json.loads(cli("ping", "--json"))["pong"]
        cli("select-workspace", remote_id)

        cli("split", "--direction", "horizontal")
        remote_write("split-result")
        eventually(remote_setup_traced)
        cli("select-workspace", second_remote_id)
        remote_write("second-workspace-result")
        cli("select-workspace", remote_id)
        remote_write("first-workspace-still-live")
        cli("reorder-workspace", remote_id, "0")
        eventually(lambda: session()["workspaces"][0]["uuid"] == remote_id)
        saved = session()
        assert saved["active_index"] == 0
        assert saved["workspaces"][1]["color"] == "#24466b"
        assert saved["workspaces"][1]["startup_script"] == str(script)
        assert saved["workspaces"][0]["remote_directory"] == str(root / "remote")
        stop()
        with (root / "events.jsonl").open() as source:
            stopped_events = [json.loads(line) for line in source.read(8 * 1024 * 1024).splitlines()]
        assert any(event["pid"] == app.pid and event["event"] == "ssh.connection.complete"
                   and event["fields"]["outcome"] == "cancelled" for event in stopped_events)
        start()
        remote_write("restored-result")
        surfaces = json.loads(cli("list-surfaces", "--json"))["surfaces"]
        assert len([s for s in surfaces if s["workspace_uuid"] == remote_id]) == 2, surfaces
        assert len([s for s in surfaces if s["workspace_uuid"] == local_id]) == 2, surfaces
        cli("select-workspace", second_remote_id)
        remote_write("second-workspace-restored")
        cli("select-workspace", local_id)
        eventually(lambda: len(launches()) >= 4)
        completed = True
        print("script and SSH launch contexts survive splits, reorder and restart")
    except BaseException:
        for log in [app_log, ssh_log]:
            log.flush()
            log.seek(0)
            print(log.read())
        for name in ["rpc-in", "rpc-out"]:
            path = root / name
            if path.exists():
                print(name, path.read_text()[-16000:])
        raise
    finally:
        try:
            forced_shutdown = stop_process(app)
            assert not forced_shutdown, "workspace fixture required forced shutdown"
        finally:
            try:
                pidfile = root / "sshd.pid"
                if pidfile.exists():
                    subprocess.run(["sudo", "kill", pidfile.read_text().strip()], check=False, timeout=10)
                sshd.wait(timeout=10)
            finally:
                app_log.close()
                ssh_log.close()
                if report_output:
                    report["status"] = "passed" if completed and not forced_shutdown and sys.exc_info()[0] is None else "failed"
                    report["shutdown_forced"] = forced_shutdown
                    output = Path(report_output)
                    output.parent.mkdir(parents=True, exist_ok=True)
                    descriptor = os.open(output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
                    with os.fdopen(descriptor, "w", encoding="utf-8") as artifact:
                        artifact.write(json.dumps(report, indent=2) + "\n")
