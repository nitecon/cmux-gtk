"""Shared legacy terminal probes; upstream health/input contracts require Linux adaptation.

Setup is best-effort and may leave workspaces behind. Polling uses wall-clock
budgets and cannot preempt client calls. Callers own marker paths and cleanup.
"""
import time
from pathlib import Path

from cmux import cmux


def ensure_focused_terminal(client: cmux) -> None:
    """
    Make sure the currently selected workspace has a focused terminal surface.

    Developer sessions (and some prior tests) may leave the browser focused,
    causing send/send_key to fail with "No focused terminal".
    """
    # Start from a clean workspace so indices are predictable.
    try:
        ws_id = client.new_workspace()
        client.select_workspace(ws_id)
        time.sleep(0.5)
    except Exception:
        pass

    try:
        health = client.surface_health()
        term = next((h for h in health if h.get("type") == "terminal"), None)
        if term is None:
            # Fallback: create a terminal surface.
            client.new_surface(panel_type="terminal")
            time.sleep(0.3)
            health = client.surface_health()
            term = next((h for h in health if h.get("type") == "terminal"), None)
        if term is not None:
            client.focus_surface(term["index"])
            time.sleep(0.2)
            wait_for_terminal_in_window(client, term["index"], timeout=5.0)
    except Exception:
        pass


def wait_for_terminal_in_window(client: cmux, surface_idx: int, timeout: float = 5.0) -> bool:
    """Wait until a terminal surface index reports in_window=true via surface_health()."""
    start = time.time()
    while time.time() - start < timeout:
        try:
            health = client.surface_health()
        except Exception:
            health = []
        for h in health:
            if h.get("index") == surface_idx and h.get("type") == "terminal" and h.get("in_window"):
                return True
        time.sleep(0.2)
    return False


def wait_for_marker(marker: Path, timeout: float = 5.0) -> bool:
    """Wait for a marker file to appear."""
    start = time.time()
    while time.time() - start < timeout:
        if marker.exists():
            return True
        time.sleep(0.1)
    return False


def clear_marker(marker: Path):
    """Remove marker file if it exists."""
    marker.unlink(missing_ok=True)


def verify_terminal_responsive(client: cmux, marker: Path, surface_idx: int = None, retries: int = 3) -> bool:
    """
    Verify a terminal is responsive by running a command.
    Returns True if the terminal executed the command successfully.
    """
    for attempt in range(retries):
        clear_marker(marker)

        # Send Ctrl+C first to clear any pending state
        try:
            if surface_idx is not None:
                client.send_key_surface(surface_idx, "ctrl-c")
            else:
                client.send_key("ctrl-c")
        except Exception:
            # Surface may be transiently unavailable during layout/tree updates.
            time.sleep(0.5)
            continue
        time.sleep(0.3)

        # Send command to create marker
        cmd = f"touch {marker}\n"
        try:
            if surface_idx is not None:
                client.send_surface(surface_idx, cmd)
            else:
                client.send(cmd)
        except Exception:
            time.sleep(0.5)
            continue

        if wait_for_marker(marker, timeout=3.0):
            return True

        # Wait a bit before retry
        time.sleep(0.5)

    return False
