#!/usr/bin/env python3
"""
Legacy debug-API scenario for focus and input routing during split churn.

Requires is_terminal_focused, simulate_type and simulate_shortcut from the
upstream debug contract; this is not evidence of current GTK keyboard routing.
The marker verifies the receiving shell's CMUX_SURFACE_ID, not rendered pixels.
Both protocol suites share this file but import their own adjacent cmux client.
The scenario leaves its workspace and fixed /tmp marker behind and must not run
concurrently. Migrate it to the isolated Linux harness before enabling it in CI.
"""

import os
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from cmux import cmux, cmuxError


SOCKET_PATH = os.environ.get("CMUX_SOCKET", "/tmp/cmux-debug.sock")
FOCUS_FILE = Path("/tmp/cmux_focus_routing.txt")


def _focused_surface_id(c: cmux) -> str:
    """Return the first selected surface UUID or fail when none is reported."""
    surfaces = c.list_surfaces()
    for _, sid, focused in surfaces:
        if focused:
            return sid
    raise cmuxError(f"No focused surface in list_surfaces: {surfaces}")


def _wait_for_file_content(path: Path, timeout_s: float = 3.0) -> str:
    """Poll for nonempty stripped marker text, retrying transient read failures.

    The wall-clock budget does not preempt an individual filesystem operation.
    """
    start = time.time()
    while time.time() - start < timeout_s:
        if path.exists():
            try:
                data = path.read_text().strip()
            except Exception:
                data = ""
            if data:
                return data
        time.sleep(0.05)
    raise cmuxError(f"Timed out waiting for file content: {path}")


def _wait_for_terminal_focus(c: cmux, panel_id: str, timeout_s: float = 6.0) -> None:
    """Poll legacy native-focus state; propagate RPC failures or time out.

    The wall-clock budget does not bound an individual client request.
    """
    start = time.time()
    while time.time() - start < timeout_s:
        if c.is_terminal_focused(panel_id):
            return
        time.sleep(0.05)
    raise cmuxError(f"Timed out waiting for terminal focus: {panel_id}")


def _focus_and_wait(c: cmux, panel_id: str, *, total_timeout_s: float = 8.0) -> None:
    """
    Activate and focus a surface up to four times, tolerating transient errors.

    Activation errors are ignored; the final focus error is included on failure.
    Nested polling and client calls may exceed the outer wall-clock budget.
    """
    deadline = time.time() + total_timeout_s
    last_err = None
    attempt = 0
    while time.time() < deadline and attempt < 4:
        attempt += 1
        try:
            c.activate_app()
        except Exception:
            pass
        try:
            c.focus_surface_by_panel(panel_id)
        except Exception as e:
            last_err = e
            time.sleep(0.15)
            continue
        time.sleep(0.2)
        try:
            _wait_for_terminal_focus(c, panel_id, timeout_s=2.5)
            return
        except Exception as e:
            last_err = e
            time.sleep(0.15)

    raise cmuxError(f"Failed to focus terminal surface (panel_id={panel_id}): {last_err}")


def _assert_routed_to_surface(c: cmux, expected_surface_id: str, panel_id: str) -> None:
    """Retry focused debug input until a shell marker matches the expected UUID.

    Each attempt requests focus and removes the shared marker best-effort; a
    focus failure propagates immediately. This does not test physical GTK keys.
    """
    last_actual = "<empty>"
    for attempt in range(4):
        _focus_and_wait(c, panel_id, total_timeout_s=4.0)
        if FOCUS_FILE.exists():
            try:
                FOCUS_FILE.unlink()
            except Exception:
                pass

        # Write the currently focused surface id into a well-known file.
        c.simulate_type(f"echo $CMUX_SURFACE_ID > {FOCUS_FILE}")
        c.simulate_shortcut("enter")
        try:
            actual = _wait_for_file_content(FOCUS_FILE, timeout_s=3.0 + (attempt * 0.5))
        except cmuxError:
            actual = ""
        if actual == expected_surface_id:
            return
        last_actual = actual or "<empty>"
        time.sleep(0.15)

    raise cmuxError(
        f"Input routed to wrong surface after retries: expected={expected_surface_id} actual={last_actual}"
    )


def main() -> int:
    """Create tabs and churn splits against a running legacy debug server.

    Fail on misrouted shell input; the client connection closes on exit, but
    workspace and marker cleanup remain the caller's responsibility.
    """
    with cmux(SOCKET_PATH) as c:
        # Create a separate workspace in the existing application.
        c.new_workspace()
        time.sleep(0.2)
        # Focus-sensitive assertions require the main window to be key.
        # Remote invocation may leave the application inactive.
        c.activate_app()
        time.sleep(0.2)

        # Create a bunch of terminals to stress layout/focus code paths.
        for _ in range(12):
            c.new_surface(panel_type="terminal")
            time.sleep(0.02)

        surfaces = c.list_surfaces()
        if not surfaces:
            raise cmuxError("Expected at least one surface after new_workspace")
        left_id = surfaces[0][1]

        # Create a split to exercise view reparenting and layout updates.
        right_id = c.new_split("right")
        if not right_id:
            # Should not happen with current server, but keep a fallback for older behavior.
            right_id = _focused_surface_id(c)
        time.sleep(0.25)

        # Focus left then right, verifying both first responder and input routing.
        _focus_and_wait(c, left_id, total_timeout_s=8.0)
        _assert_routed_to_surface(c, left_id, left_id)

        _focus_and_wait(c, right_id, total_timeout_s=8.0)
        _assert_routed_to_surface(c, right_id, right_id)

        # Stress: repeated split/close should never leave focus on a detached/hidden terminal.
        for _ in range(10):
            new_id = c.new_split("right")
            time.sleep(0.1)
            _focus_and_wait(c, new_id, total_timeout_s=8.0)
            _assert_routed_to_surface(c, new_id, new_id)

            c.close_surface(new_id)
            time.sleep(0.25)
            focused = _focused_surface_id(c)
            _focus_and_wait(c, focused, total_timeout_s=8.0)
            _assert_routed_to_surface(c, focused, focused)

    print("PASS: terminal focus routing")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
