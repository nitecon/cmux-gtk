#!/usr/bin/env python3
"""Exercise real X11 window state across application restarts under Openbox."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
from functools import partial
from process_support import stop_process, wait_until


eventually = partial(wait_until, description="window state", timeout=10)

with tempfile.TemporaryDirectory(prefix="cmux-window-") as directory:
    root = Path(directory)
    env = dict(os.environ, XDG_DATA_HOME=str(root / "data"),
               XDG_CONFIG_HOME=str(root / "config"), XDG_STATE_HOME=str(root / "state"),
               XDG_RUNTIME_DIR=str(root / "runtime"), GDK_BACKEND="x11",
               LIBGL_ALWAYS_SOFTWARE="1", CMUX_NO_UPDATE="1")
    (root / "runtime").mkdir(mode=0o700)
    state_path = root / "data/cmux/window-state.json"
    app = None
    wm = subprocess.Popen(["openbox"], env=env)
    log = (root / "app.log").open("w+")

    def state():
        """Read persisted window geometry, returning an empty snapshot during missing or partial writes."""
        try:
            return json.loads(state_path.read_text())
        except (OSError, ValueError):
            return {}

    def start():
        """Launch the isolated GTK process and wait for its first visible X11 window."""
        global app
        app = subprocess.Popen(["target/debug/cmux-app"], env=env, stdout=log, stderr=log)
        windows = []

        def found():
            """Refresh the visible-window list for the owned application process."""
            result = subprocess.run(["xdotool", "search", "--onlyvisible", "--pid", str(app.pid)],
                                    capture_output=True, text=True)
            windows[:] = result.stdout.split()
            return bool(windows)

        eventually(found)
        return windows[0]

    def close(window):
        """Ask the window manager to close the window and require application exit within ten seconds."""
        subprocess.run(["wmctrl", "-ic", hex(int(window))], check=True)
        app.wait(timeout=10)

    def geometry(window):
        """Read X11 geometry as shell-style key/value fields for restart comparisons."""
        output = subprocess.check_output(["xdotool", "getwindowgeometry", "--shell", window], text=True)
        return dict(line.split("=", 1) for line in output.splitlines())

    try:
        time.sleep(0.5)
        window = start()
        subprocess.run(["xdotool", "windowsize", window, "900", "650"], check=True)
        subprocess.run(["xdotool", "windowmove", window, "100", "90"], check=True)
        eventually(lambda: state().get("width") == 900 and state().get("position") is not None)
        before = geometry(window)
        close(window)
        window = start()
        eventually(lambda: all(abs(int(geometry(window)[key]) - int(before[key])) <= 2
                               for key in ["X", "Y", "WIDTH", "HEIGHT"]))
        subprocess.run(["wmctrl", "-ir", hex(int(window)), "-b", "add,maximized_vert,maximized_horz"], check=True)
        eventually(lambda: state().get("maximized") is True)
        close(window)
        window = start()
        eventually(lambda: "_NET_WM_STATE_MAXIMIZED_VERT" in subprocess.check_output(
            ["xprop", "-id", window, "_NET_WM_STATE"], text=True))
        subprocess.run(["wmctrl", "-ir", hex(int(window)), "-b", "remove,maximized_vert,maximized_horz"], check=True)
        eventually(lambda: state().get("maximized") is False and state().get("width") == 900)
        close(window)
        print("window position, normal size and maximize state survive restarts")
    except BaseException:
        log.flush()
        log.seek(0)
        print(log.read())
        raise
    finally:
        try:
            stop_process(app)
        finally:
            try:
                stop_process(wm)
            finally:
                log.close()
