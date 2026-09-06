"""Shared isolated application lifetime and protocol helpers for Linux fixtures."""
from contextlib import contextmanager
from dataclasses import dataclass
import json
import os
from pathlib import Path
import subprocess

from process_support import linux_child_pids, stop_process, wait_until


@dataclass
class Application:
    """An owned cmux process and its isolated CLI environment; no global configuration mutation."""
    process: subprocess.Popen
    environment: dict
    socket_path: Path

    def cli(self, *arguments, timeout=15):
        """Run the production CLI with captured stdout and a caller deadline (fifteen seconds by default)."""
        binary_dir = Path(self.environment.get("CMUX_BIN_DIR", "target/debug"))
        return subprocess.check_output(
            [str(binary_dir / "cmux"), "--socket", str(self.socket_path), *arguments],
            env=self.environment, text=True, timeout=timeout,
        )

    def surfaces(self):
        """Read current surface records through the production JSON CLI."""
        return json.loads(self.cli("list-surfaces", "--json"))["surfaces"]

    def children(self):
        """Collect direct child PIDs across all spawning threads, tolerating concurrent thread exits."""
        return linux_child_pids(self.process.pid)

    def wait_for(self, predicate, description, timeout=10):
        """Poll an observable condition with an elapsed-time limit; fail promptly if cmux exits."""
        def alive_and_ready():
            """Check process ownership before evaluating the caller's observable condition."""
            assert self.process.poll() is None, f"cmux exited while waiting for {description}"
            return predicate()

        wait_until(alive_and_ready, description, timeout)


@contextmanager
def running_app(root, extra_environment=None):
    """Start cmux in caller-owned temporary storage and always terminate/reap its direct child.

    Overrides allow fixtures to install a browser mock or select CMUX_BIN_DIR before startup. Failure output
    is capped at 64 KiB; forced shutdown fails an otherwise successful scenario.
    """
    environment = dict(os.environ, XDG_DATA_HOME=str(root / "data"),
                       XDG_CONFIG_HOME=str(root / "config"), XDG_STATE_HOME=str(root / "state"),
                       XDG_RUNTIME_DIR=str(root / "runtime"), GDK_BACKEND="x11",
                       LIBGL_ALWAYS_SOFTWARE="1", CMUX_NO_UPDATE="1")
    environment.update(extra_environment or {})
    (root / "runtime").mkdir(mode=0o700, exist_ok=True)
    with (root / "app.log").open("w+b") as log:
        binary_dir = Path(environment.get("CMUX_BIN_DIR", "target/debug"))
        process = subprocess.Popen([str(binary_dir / "cmux-app")], env=environment, stdout=log, stderr=log)
        app = Application(process, environment, root / "runtime/cmux/cmux.sock")
        failed = False
        try:
            app.wait_for(app.socket_path.exists, "application socket")
            yield app
        except BaseException:
            failed = True
            log.flush()
            log.seek(0)
            print(log.read(65536).decode("utf-8", errors="replace"))
            raise
        finally:
            forced = stop_process(process)
            if forced and not failed:
                raise AssertionError("cmux required forced shutdown")
