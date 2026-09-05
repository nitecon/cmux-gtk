#!/usr/bin/env python3
"""Real SSH/PTY, script inheritance, workspace ordering and session roundtrip."""
import getpass
import json
import os
from pathlib import Path
import shlex
import shutil
import socket
import subprocess
import tempfile
import time
import uuid


def eventually(check):
    for _ in range(150):
        if check():
            return
        time.sleep(0.1)
    raise AssertionError("workspace launch state did not converge")


with tempfile.TemporaryDirectory(prefix="cmux-workflow-") as directory:
    root = Path(directory)
    for name in ["runtime", "bin", "local", "remote", "data/cmux/bin"]:
        (root / name).mkdir(parents=True, mode=0o700)
    env = dict(os.environ, XDG_DATA_HOME=str(root / "data"),
               XDG_CONFIG_HOME=str(root / "config"), XDG_STATE_HOME=str(root / "state"),
               XDG_RUNTIME_DIR=str(root / "runtime"), GDK_BACKEND="x11",
               LIBGL_ALWAYS_SOFTWARE="1", CMUX_NO_UPDATE="1", PATH=str(root / "bin") + ":" + os.environ["PATH"])
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
        wrapper.write_text(f'#!/bin/sh\nexec /usr/bin/{command} -F {shlex.quote(str(config))} "$@"\n')
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
    local_id, remote_id = str(uuid.uuid4()), str(uuid.uuid4())

    def workspace(identity, name, **fields):
        return dict(uuid=identity, name=name, active_pane_uuid=None,
                    layout=dict(type="Leaf", pane_id=1, surface_uuid=str(uuid.uuid4()), shell="/bin/sh", cwd=""), **fields)

    session_path.write_text(json.dumps(dict(version=3, active_index=0, workspaces=[
        workspace(local_id, "Script project", working_directory=str(root / "local"), startup_script=str(script), color="#24466b"),
        workspace(remote_id, "SSH project", remote_target="cmux-ci", remote_directory=str(root / "remote")),
    ])))

    def cli(*args):
        return subprocess.check_output(["target/debug/cmux", "--socket", str(socket_path), *args], env=env, text=True, timeout=15)

    def start():
        global app
        if socket_path.exists():
            socket_path.unlink()
        app = subprocess.Popen(["target/debug/cmux-app"], env=env, stdout=app_log, stderr=app_log)
        eventually(socket_path.exists)
        time.sleep(0.5)

    def stop():
        app.terminate()
        app.wait(timeout=10)

    def session():
        return json.loads(session_path.read_text())

    def launches():
        try:
            return (root / "launches").read_text().splitlines()
        except FileNotFoundError:
            return []

    def remote_write(name):
        def written():
            if (root / "remote" / name).exists():
                return True
            cli("send-text", f"printf '%s' \"$PWD\" > {name}")
            windows = subprocess.check_output(["xdotool", "search", "--onlyvisible", "--pid", str(app.pid)], text=True).split()
            subprocess.check_call(["xdotool", "windowfocus", windows[-1], "key", "--clearmodifiers", "Return"])
            return False
        eventually(written)
        assert (root / "remote" / name).read_text() == str(root / "remote")
        assert not (root / "local" / name).exists()

    try:
        eventually(lambda: subprocess.run([str(root / "bin/ssh"), "-o", "BatchMode=yes", "cmux-ci", "true"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode == 0)
        start()
        eventually(lambda: len(launches()) >= 1)
        cli("split", "--direction", "horizontal")
        eventually(lambda: len(launches()) >= 2)
        assert all(p == str(root / "local") for p in launches())
        cli("select-workspace", remote_id)
        remote_write("first-result")
        cli("split", "--direction", "horizontal")
        remote_write("split-result")
        cli("reorder-workspace", remote_id, "0")
        eventually(lambda: session()["workspaces"][0]["uuid"] == remote_id)
        saved = session()
        assert saved["active_index"] == 0
        assert saved["workspaces"][1]["color"] == "#24466b"
        assert saved["workspaces"][1]["startup_script"] == str(script)
        assert saved["workspaces"][0]["remote_directory"] == str(root / "remote")
        stop()
        start()
        remote_write("restored-result")
        surfaces = json.loads(cli("list-surfaces", "--json"))["surfaces"]
        assert len(surfaces) == 2, surfaces
        cli("select-workspace", local_id)
        eventually(lambda: len(launches()) >= 4)
        print("script and SSH launch contexts survive splits, reorder and restart")
    except BaseException:
        for log in [app_log, ssh_log]:
            log.flush()
            log.seek(0)
            print(log.read())
        raise
    finally:
        if app and app.poll() is None:
            stop()
        pidfile = root / "sshd.pid"
        if pidfile.exists():
            subprocess.run(["sudo", "kill", pidfile.read_text().strip()], check=False)
        sshd.wait(timeout=10)
        app_log.close()
        ssh_log.close()
