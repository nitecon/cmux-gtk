#!/usr/bin/env python3
"""cmux v2 Python Client

A client library for programmatically controlling cmux via the Unix socket.

This client speaks the v2 JSON line protocol (one JSON request/response per line).
It intentionally mirrors the existing v1 Python client's convenience API so the
existing test suite can be ported with minimal churn.

Protocol:
  Request:  {"id": 1, "method": "surface.list", "params": {..}}
  Response: {"id": 1, "ok": true, "result": {...}}

Notes:
- Convenience wrappers include upstream debug APIs that the Linux server may not implement.
  Query capabilities and treat method-not-found as unsupported; wrappers are not feature guarantees.
- v2 uses stable UUID handles for workspaces/panes/surfaces.
- For test convenience, this client accepts integer indexes for many methods and
  resolves them to IDs using list calls.
"""

import base64
import sys
from pathlib import Path
import json
import os
import socket
import time
import uuid
from typing import Any, Dict, List, Optional, Tuple, Union


sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from cmux_socket_discovery import default_socket_path as _default_socket_path
from cmux_socket_transport import connect_socket, read_response

class cmuxError(Exception):
    """Exception raised for cmux errors."""


def _decode_response(line: str, request_id: int) -> dict:
    """Validate a v2 response envelope before accessing results or structured server errors."""
    response = json.loads(line)
    if not isinstance(response, dict):
        raise ValueError("Response must be a JSON object")
    if type(response.get("id")) is not int or response["id"] != request_id:
        raise ValueError("Response ID does not match the request")
    if type(response.get("ok")) is not bool:
        raise ValueError("Response must contain a boolean ok field")
    if not response["ok"]:
        error = response.get("error")
        if not isinstance(error, dict):
            raise ValueError("Failed response must contain an error object")
        if any(key in error and not isinstance(error[key], str) for key in ("code", "message")):
            raise ValueError("Error code and message must be strings")
    return response


def _looks_like_uuid(s: str) -> bool:
    """Return whether the supplied string parses as a UUID."""
    try:
        uuid.UUID(s)
        return True
    except Exception:
        return False


def _looks_like_ref(s: str, kind: Optional[str] = None) -> bool:
    """Validate a typed window/workspace/pane/surface reference and optional expected kind."""
    parts = s.split(":", 1)
    if len(parts) != 2:
        return False
    ref_kind, ordinal = parts[0].strip().lower(), parts[1].strip()
    if kind is not None and ref_kind != kind:
        return False
    if ref_kind not in {"window", "workspace", "pane", "surface"}:
        return False
    return ordinal.isdigit()


def _unescape_backslash_controls(s: str) -> str:
    """Interpret \n/\r/\t/\\ sequences in a string.

    v2 can carry raw newlines via JSON, but a lot of existing callsites use
    backslash escapes (because v1 was line-oriented). This keeps the API
    ergonomic for tests and scripts.
    """

    out: List[str] = []
    i = 0
    while i < len(s):
        ch = s[i]
        if ch != "\\" or i + 1 >= len(s):
            out.append(ch)
            i += 1
            continue

        nxt = s[i + 1]
        if nxt == "n":
            out.append("\n")
            i += 2
        elif nxt == "r":
            out.append("\r")
            i += 2
        elif nxt == "t":
            out.append("\t")
            i += 2
        elif nxt == "\\":
            out.append("\\")
            i += 2
        else:
            # Preserve unknown escapes literally.
            out.append(ch)
            i += 1
    return "".join(out)


class cmux:
    """Client for controlling cmux via the v2 JSON Unix socket."""

    DEFAULT_SOCKET_PATH = _default_socket_path()

    def __init__(self, socket_path: str = None):
        """Resolve discovery at construction time and initialize disconnected protocol state."""
        self.socket_path = socket_path or _default_socket_path()
        self._socket: Optional[socket.socket] = None
        self._recv_buffer = bytearray()
        self._next_id: int = 1

    # ---------------------------------------------------------------------
    # Connection
    # ---------------------------------------------------------------------

    def connect(self) -> None:
        """Connect within a bounded startup budget, retaining one owned socket."""
        if self._socket is not None:
            return
        try:
            self._socket = connect_socket(self.socket_path, 10.0, 10.0)
        except OSError as error:
            raise cmuxError(f"Failed to connect: {error}") from error

    def close(self) -> None:
        """Release the connection and discard any response buffered from that server."""
        connection, self._socket = self._socket, None
        self._recv_buffer.clear()
        if connection is not None:
            connection.close()

    def __enter__(self):
        """Connect on entering a client context."""
        self.connect()
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        """Always close the client while preserving an exception from its context."""
        self.close()
        return False

    # ---------------------------------------------------------------------
    # Low-level protocol
    # ---------------------------------------------------------------------

    def _recv_line(self, timeout_s: float = 20.0) -> str:
        """Read one bounded UTF-8 JSON line, discarding a failed connection."""
        if self._socket is None:
            raise cmuxError("Not connected")
        try:
            return read_response(self._socket, self._recv_buffer, timeout_s)
        except (OSError, ValueError) as error:
            self.close()
            raise cmuxError(f"Socket response failed: {error}") from error

    def _call(self, method: str, params: Optional[Dict[str, Any]] = None, timeout_s: float = 20.0) -> Any:
        """Send one numbered JSON request, validate its response ID and raise cmuxError on server failure."""
        if self._socket is None:
            raise cmuxError("Not connected")

        req_id = self._next_id
        self._next_id += 1

        payload = {
            "id": req_id,
            "method": method,
            "params": params or {},
        }
        line = json.dumps(payload, separators=(",", ":")) + "\n"
        try:
            self._socket.sendall(line.encode("utf-8"))
        except OSError as error:
            self.close()
            raise cmuxError(f"Socket write failed: {error}") from error

        resp_line = self._recv_line(timeout_s=timeout_s)
        try:
            resp = _decode_response(resp_line, req_id)
        except (ValueError, RecursionError) as error:
            self.close()
            raise cmuxError("Invalid v2 response envelope") from error

        if resp.get("ok") is True:
            return resp.get("result")

        err = resp.get("error") or {}
        code = err.get("code") or "error"
        msg = err.get("message") or "Unknown error"
        data = err.get("data")
        if data is not None:
            raise cmuxError(f"{code}: {msg} ({data})")
        raise cmuxError(f"{code}: {msg}")

    # ---------------------------------------------------------------------
    # ID resolution helpers (index -> id)
    # ---------------------------------------------------------------------

    def _resolve_reference(self, kind: str, value: Union[str, int, None], workspace_id: Optional[str] = None) -> Optional[str]:
        """Resolve a workspace/pane/surface selector, retaining each kind's current-selection policy."""
        if value is None:
            if kind == "workspace":
                selected = (self._call("workspace.current") or {}).get("workspace_id")
                if not selected:
                    raise cmuxError("No workspace selected")
                return str(selected)
            focused = (self._call("system.identify") or {}).get("focused") or {}
            selected = focused.get(f"{kind}_id") if isinstance(focused, dict) else None
            return None if selected in (None, "", {}) else str(selected)

        if not isinstance(value, int):
            value = str(value).strip()
            if not value:
                return None
            if value.isdigit():
                value = int(value)
            elif _looks_like_ref(value, kind) or _looks_like_uuid(value):
                return value
            else:
                raise cmuxError(f"Invalid {kind} id: {value}")

        params: Dict[str, Any] = {}
        if workspace_id:
            params["workspace_id"] = workspace_id
        items = (self._call(f"{kind}.list", params) or {}).get(f"{kind}s") or []
        for row in items:
            if int(row.get("index", -1)) == value:
                return str(row.get("id"))
        raise cmuxError(f"{kind.title()} index not found: {value}")

    def _resolve_workspace_id(self, workspace: Union[str, int, None]) -> Optional[str]:
        """Resolve the current workspace, an index, typed reference or UUID; reject invalid identifiers."""
        return self._resolve_reference("workspace", workspace)

    def _resolve_surface_id(self, surface: Union[str, int, None], workspace_id: Optional[str] = None) -> Optional[str]:
        """Resolve focus or a surface index in the requested workspace, preserving explicit refs and UUIDs."""
        return self._resolve_reference("surface", surface, workspace_id)

    def _resolve_pane_id(self, pane: Union[str, int, None], workspace_id: Optional[str] = None) -> Optional[str]:
        """Resolve focus or a pane index in the requested workspace, preserving explicit refs and UUIDs."""
        return self._resolve_reference("pane", pane, workspace_id)

    # ---------------------------------------------------------------------
    # System
    # ---------------------------------------------------------------------

    def ping(self) -> bool:
        """Return whether the server reports a successful protocol ping."""
        res = self._call("system.ping")
        return bool((res or {}).get("pong"))

    def capabilities(self) -> dict:
        """Return the server capability map; use it to determine supported operations."""
        return dict(self._call("system.capabilities") or {})

    def identify(self, caller: Optional[dict] = None) -> dict:
        """Return server identity and focus information, optionally supplying caller metadata."""
        params: Dict[str, Any] = {}
        if caller is not None:
            params["caller"] = caller
        return dict(self._call("system.identify", params) or {})

    # ---------------------------------------------------------------------
    # Windows
    # ---------------------------------------------------------------------

    def list_windows(self) -> List[dict]:
        """Return the window records advertised by the server."""
        res = self._call("window.list") or {}
        return list(res.get("windows") or [])

    def current_window(self) -> str:
        """Return the current window ID, raising if the response omits it."""
        res = self._call("window.current") or {}
        wid = res.get("window_id")
        if not wid:
            raise cmuxError(f"window.current returned no window_id: {res}")
        return str(wid)

    def new_window(self) -> str:
        """Request a new window and require its ID in the response."""
        res = self._call("window.create") or {}
        wid = res.get("window_id")
        if not wid:
            raise cmuxError(f"window.create returned no window_id: {res}")
        return str(wid)

    def focus_window(self, window_id: str) -> None:
        """Request focus for an explicit window ID."""
        self._call("window.focus", {"window_id": str(window_id)})

    def close_window(self, window_id: str) -> None:
        """Request closure of an explicit window ID."""
        self._call("window.close", {"window_id": str(window_id)})

    # ---------------------------------------------------------------------
    # Workspaces
    # ---------------------------------------------------------------------

    def list_workspaces(self, window_id: Optional[str] = None) -> List[Tuple[int, str, str, bool]]:
        """Return index, ID, title and selection tuples, optionally restricted to a window."""
        params: Dict[str, Any] = {}
        if window_id is not None:
            params["window_id"] = str(window_id)
        res = self._call("workspace.list", params) or {}
        out: List[Tuple[int, str, str, bool]] = []
        for row in res.get("workspaces") or []:
            out.append((
                int(row.get("index", 0)),
                str(row.get("id")),
                str(row.get("title", "")),
                bool(row.get("selected", False)),
            ))
        return out

    def new_workspace(self, window_id: Optional[str] = None) -> str:
        """Create a workspace in the optional window and require its returned ID."""
        params: Dict[str, Any] = {}
        if window_id is not None:
            params["window_id"] = str(window_id)
        res = self._call("workspace.create", params) or {}
        wsid = res.get("workspace_id")
        if not wsid:
            raise cmuxError(f"workspace.create returned no workspace_id: {res}")
        return str(wsid)

    def select_workspace(self, workspace: Union[str, int]) -> None:
        """Resolve a workspace index or identifier and request its selection."""
        wsid = self._resolve_workspace_id(workspace)
        self._call("workspace.select", {"workspace_id": wsid})

    def rename_workspace(self, title: str, workspace: Union[str, int, None] = None) -> None:
        """Trim and validate a nonempty title before renaming the resolved workspace."""
        renamed = str(title).strip()
        if not renamed:
            raise cmuxError("rename_workspace requires a non-empty title")
        wsid = self._resolve_workspace_id(workspace)
        params: Dict[str, Any] = {"title": renamed}
        if wsid:
            params["workspace_id"] = wsid
        self._call("workspace.rename", params)

    def current_workspace(self) -> str:
        """Return the selected workspace ID or raise when no workspace is selected."""
        wsid = self._resolve_workspace_id(None)
        if not wsid:
            raise cmuxError("No current workspace")
        return wsid

    def next_workspace(self) -> str:
        """Select the next workspace and require its returned ID."""
        res = self._call("workspace.next") or {}
        wsid = res.get("workspace_id")
        if not wsid:
            raise cmuxError(f"workspace.next returned no workspace_id: {res}")
        return str(wsid)

    def previous_workspace(self) -> str:
        """Select the previous workspace and require its returned ID."""
        res = self._call("workspace.previous") or {}
        wsid = res.get("workspace_id")
        if not wsid:
            raise cmuxError(f"workspace.previous returned no workspace_id: {res}")
        return str(wsid)

    def last_workspace(self) -> str:
        """Request the last workspace and require its returned ID."""
        res = self._call("workspace.last") or {}
        wsid = res.get("workspace_id")
        if not wsid:
            raise cmuxError(f"workspace.last returned no workspace_id: {res}")
        return str(wsid)

    def move_workspace_to_window(self, workspace: Union[str, int], window_id: str, focus: bool = True) -> None:
        """Move a resolved workspace to a window, forwarding the requested focus policy."""
        wsid = self._resolve_workspace_id(workspace)
        self._call(
            "workspace.move_to_window",
            {"workspace_id": wsid, "window_id": str(window_id), "focus": bool(focus)},
        )

    def reorder_workspace(
        self,
        workspace: Union[str, int],
        *,
        index: Optional[int] = None,
        before_workspace: Union[str, int, None] = None,
        after_workspace: Union[str, int, None] = None,
        window_id: Optional[str] = None,
    ) -> None:
        """Reorder a workspace using exactly one index, before-workspace or after-workspace target."""
        wsid = self._resolve_workspace_id(workspace)
        params: Dict[str, Any] = {"workspace_id": wsid}

        targets = 0
        if index is not None:
            params["index"] = int(index)
            targets += 1
        if before_workspace is not None:
            params["before_workspace_id"] = self._resolve_workspace_id(before_workspace)
            targets += 1
        if after_workspace is not None:
            params["after_workspace_id"] = self._resolve_workspace_id(after_workspace)
            targets += 1
        if targets != 1:
            raise cmuxError("reorder_workspace requires exactly one target: index|before_workspace|after_workspace")

        if window_id is not None:
            params["window_id"] = str(window_id)

        self._call("workspace.reorder", params)

    def close_workspace(self, workspace_id: str) -> None:
        """Resolve the workspace identifier and request its closure."""
        wsid = self._resolve_workspace_id(workspace_id)
        self._call("workspace.close", {"workspace_id": wsid})

    # Backwards-compatible aliases
    def list_tabs(self) -> List[Tuple[int, str, str, bool]]:
        """Alias list_workspaces for callers using the older tab vocabulary."""
        return self.list_workspaces()

    def new_tab(self) -> str:
        """Alias new_workspace and return the created workspace ID."""
        return self.new_workspace()

    def close_tab(self, workspace_id: str) -> None:
        """Alias close_workspace for older tab-oriented callers."""
        return self.close_workspace(workspace_id)

    def select_tab(self, workspace: Union[str, int]) -> None:
        """Alias select_workspace for older tab-oriented callers."""
        return self.select_workspace(workspace)

    def current_tab(self) -> str:
        """Alias current_workspace and return its ID."""
        return self.current_workspace()

    # ---------------------------------------------------------------------
    # Surfaces / panes
    # ---------------------------------------------------------------------

    def list_surfaces(self, workspace: Union[str, int, None] = None) -> List[Tuple[int, str, bool]]:
        """Return surface index, ID and focus tuples, optionally restricted to a workspace."""
        params: Dict[str, Any] = {}
        if workspace is not None:
            wsid = self._resolve_workspace_id(workspace)
            params["workspace_id"] = wsid
        res = self._call("surface.list", params) or {}
        out: List[Tuple[int, str, bool]] = []
        for row in res.get("surfaces") or []:
            out.append((
                int(row.get("index", 0)),
                str(row.get("id")),
                bool(row.get("focused", False)),
            ))
        return out

    def focus_surface(self, surface: Union[str, int]) -> None:
        """Resolve and validate a surface before requesting focus."""
        sid = self._resolve_surface_id(surface)
        if not sid:
            raise cmuxError(f"Invalid surface: {surface!r}")
        self._call("surface.focus", {"surface_id": sid})

    def focus_surface_by_panel(self, surface_id: str) -> None:
        # In v2, surface_id is the panel UUID.
        """Focus a panel using its v2 surface identifier."""
        self.focus_surface(surface_id)

    def new_split(self, direction: str) -> str:
        """Split in the requested direction and require the new surface ID."""
        res = self._call("surface.split", {"direction": direction}) or {}
        sid = res.get("surface_id")
        if not sid:
            raise cmuxError(f"surface.split returned no surface_id: {res}")
        return str(sid)

    def drag_surface_to_split(self, surface: Union[str, int], direction: str) -> None:
        """Resolve a surface and request moving it into a directional split."""
        sid = self._resolve_surface_id(surface)
        if not sid:
            raise cmuxError(f"Invalid surface: {surface!r}")
        self._call("surface.drag_to_split", {"surface_id": sid, "direction": direction})

    def new_pane(self, direction: str = "right", panel_type: str = "terminal", url: str = None) -> str:
        """Create a directional pane of the requested type and return its surface ID."""
        params: Dict[str, Any] = {"direction": direction, "type": panel_type}
        if url:
            params["url"] = url
        res = self._call("pane.create", params) or {}
        sid = res.get("surface_id")
        if not sid:
            raise cmuxError(f"pane.create returned no surface_id: {res}")
        return str(sid)

    def new_surface(self, pane: Union[str, int, None] = None, panel_type: str = "terminal", url: str = None) -> str:
        """Create a terminal or browser surface in an optional resolved pane and return its ID."""
        params: Dict[str, Any] = {"type": panel_type}
        if pane is not None:
            pid = self._resolve_pane_id(pane)
            if not pid:
                raise cmuxError(f"Invalid pane: {pane!r}")
            params["pane_id"] = pid
        if url:
            params["url"] = url
        res = self._call("surface.create", params) or {}
        sid = res.get("surface_id")
        if not sid:
            raise cmuxError(f"surface.create returned no surface_id: {res}")
        return str(sid)

    def close_surface(self, surface: Union[str, int, None] = None) -> None:
        """Request closure of an explicit surface or let the server select the current one."""
        params: Dict[str, Any] = {}
        if surface is not None:
            sid = self._resolve_surface_id(surface)
            if not sid:
                raise cmuxError(f"Invalid surface: {surface!r}")
            params["surface_id"] = sid
        self._call("surface.close", params)

    def move_surface(
        self,
        surface: Union[str, int],
        *,
        pane: Union[str, int, None] = None,
        workspace: Union[str, int, None] = None,
        window_id: Optional[str] = None,
        before_surface: Union[str, int, None] = None,
        after_surface: Union[str, int, None] = None,
        index: Optional[int] = None,
        focus: bool = True,
    ) -> None:
        """Resolve supplied destinations and placement refs, then request a surface move with focus policy."""
        sid = self._resolve_surface_id(surface)
        if not sid:
            raise cmuxError(f"Invalid surface: {surface!r}")

        params: Dict[str, Any] = {"surface_id": sid, "focus": bool(focus)}
        if pane is not None:
            pid = self._resolve_pane_id(pane)
            if not pid:
                raise cmuxError(f"Invalid pane: {pane!r}")
            params["pane_id"] = pid
        if workspace is not None:
            wsid = self._resolve_workspace_id(workspace)
            if not wsid:
                raise cmuxError(f"Invalid workspace: {workspace!r}")
            params["workspace_id"] = wsid
        if window_id is not None:
            params["window_id"] = str(window_id)
        if before_surface is not None:
            before_id = self._resolve_surface_id(before_surface)
            if not before_id:
                raise cmuxError(f"Invalid before_surface: {before_surface!r}")
            params["before_surface_id"] = before_id
        if after_surface is not None:
            after_id = self._resolve_surface_id(after_surface)
            if not after_id:
                raise cmuxError(f"Invalid after_surface: {after_surface!r}")
            params["after_surface_id"] = after_id
        if index is not None:
            params["index"] = int(index)

        self._call("surface.move", params)

    def reorder_surface(
        self,
        surface: Union[str, int],
        *,
        index: Optional[int] = None,
        before_surface: Union[str, int, None] = None,
        after_surface: Union[str, int, None] = None,
    ) -> None:
        """Reorder a surface using exactly one index, before-surface or after-surface target."""
        sid = self._resolve_surface_id(surface)
        if not sid:
            raise cmuxError(f"Invalid surface: {surface!r}")

        params: Dict[str, Any] = {"surface_id": sid}
        targets = 0
        if index is not None:
            params["index"] = int(index)
            targets += 1
        if before_surface is not None:
            before_id = self._resolve_surface_id(before_surface)
            if not before_id:
                raise cmuxError(f"Invalid before_surface: {before_surface!r}")
            params["before_surface_id"] = before_id
            targets += 1
        if after_surface is not None:
            after_id = self._resolve_surface_id(after_surface)
            if not after_id:
                raise cmuxError(f"Invalid after_surface: {after_surface!r}")
            params["after_surface_id"] = after_id
            targets += 1
        if targets != 1:
            raise cmuxError("reorder_surface requires exactly one target: index|before_surface|after_surface")

        self._call("surface.reorder", params)

    def trigger_flash(self, surface: Union[str, int, None] = None) -> None:
        """Request attention highlighting for an explicit or server-selected surface."""
        params: Dict[str, Any] = {}
        if surface is not None:
            sid = self._resolve_surface_id(surface)
            if not sid:
                raise cmuxError(f"Invalid surface: {surface!r}")
            params["surface_id"] = sid
        self._call("surface.trigger_flash", params)

    def refresh_surfaces(self, workspace: Union[str, int, None] = None) -> None:
        """Request surface refresh for an optional resolved workspace."""
        params: Dict[str, Any] = {}
        if workspace is not None:
            wsid = self._resolve_workspace_id(workspace)
            params["workspace_id"] = wsid
        self._call("surface.refresh", params)

    def surface_health(self, workspace: Union[str, int, None] = None) -> List[dict]:
        """Return server surface-health records for an optional workspace."""
        params: Dict[str, Any] = {}
        if workspace is not None:
            wsid = self._resolve_workspace_id(workspace)
            params["workspace_id"] = wsid
        res = self._call("surface.health", params) or {}
        return list(res.get("surfaces") or [])

    def clear_history(self, surface: Union[str, int, None] = None, workspace: Union[str, int, None] = None) -> None:
        """Clear history for an optional surface resolved within the supplied workspace."""
        params: Dict[str, Any] = {}
        if workspace is not None:
            wsid = self._resolve_workspace_id(workspace)
            params["workspace_id"] = wsid
        if surface is not None:
            sid = self._resolve_surface_id(surface, workspace_id=params.get("workspace_id"))
            if not sid:
                raise cmuxError(f"Invalid surface: {surface!r}")
            params["surface_id"] = sid
        self._call("surface.clear_history", params)

    # ---------------------------------------------------------------------
    # Pane commands
    # ---------------------------------------------------------------------

    def list_panes(self) -> List[Tuple[int, str, int, bool]]:
        """Return pane index, ID, surface-count and focus tuples."""
        res = self._call("pane.list") or {}
        out: List[Tuple[int, str, int, bool]] = []
        for row in res.get("panes") or []:
            out.append((
                int(row.get("index", 0)),
                str(row.get("id")),
                int(row.get("surface_count", 0)),
                bool(row.get("focused", False)),
            ))
        return out

    def focus_pane(self, pane: Union[str, int]) -> None:
        """Resolve and validate a pane before requesting focus."""
        pid = self._resolve_pane_id(pane)
        if not pid:
            raise cmuxError(f"Invalid pane: {pane!r}")
        self._call("pane.focus", {"pane_id": pid})

    def list_pane_surfaces(self, pane: Union[str, int, None] = None) -> List[Tuple[int, str, str, bool]]:
        """Return index, ID, title and selection tuples for an optional pane."""
        params: Dict[str, Any] = {}
        if pane is not None:
            pid = self._resolve_pane_id(pane)
            params["pane_id"] = pid
        res = self._call("pane.surfaces", params) or {}
        out: List[Tuple[int, str, str, bool]] = []
        for row in res.get("surfaces") or []:
            out.append((
                int(row.get("index", 0)),
                str(row.get("id")),
                str(row.get("title", "")),
                bool(row.get("selected", False)),
            ))
        return out

    def swap_pane(self, pane: Union[str, int], target_pane: Union[str, int], focus: bool = True) -> None:
        """Resolve two panes and request swapping them with the chosen focus policy."""
        source = self._resolve_pane_id(pane)
        target = self._resolve_pane_id(target_pane)
        if not source or not target:
            raise cmuxError(f"Invalid panes: pane={pane!r}, target_pane={target_pane!r}")
        self._call("pane.swap", {"pane_id": source, "target_pane_id": target, "focus": bool(focus)})

    def break_pane(self, pane: Union[str, int, None] = None, surface: Union[str, int, None] = None, focus: bool = True) -> str:
        """Request detaching the selected pane or surface and require the destination workspace ID."""
        params: Dict[str, Any] = {"focus": bool(focus)}
        if pane is not None:
            pid = self._resolve_pane_id(pane)
            if not pid:
                raise cmuxError(f"Invalid pane: {pane!r}")
            params["pane_id"] = pid
        if surface is not None:
            sid = self._resolve_surface_id(surface)
            if not sid:
                raise cmuxError(f"Invalid surface: {surface!r}")
            params["surface_id"] = sid
        res = self._call("pane.break", params) or {}
        wsid = res.get("workspace_id")
        if not wsid:
            raise cmuxError(f"pane.break returned no workspace_id: {res}")
        return str(wsid)

    def join_pane(
        self,
        target_pane: Union[str, int],
        pane: Union[str, int, None] = None,
        surface: Union[str, int, None] = None,
        focus: bool = True,
    ) -> None:
        """Resolve a target pane and optional source pane/surface before requesting a join."""
        target = self._resolve_pane_id(target_pane)
        if not target:
            raise cmuxError(f"Invalid target_pane: {target_pane!r}")
        params: Dict[str, Any] = {"target_pane_id": target, "focus": bool(focus)}
        if pane is not None:
            source = self._resolve_pane_id(pane)
            if not source:
                raise cmuxError(f"Invalid pane: {pane!r}")
            params["pane_id"] = source
        if surface is not None:
            sid = self._resolve_surface_id(surface)
            if not sid:
                raise cmuxError(f"Invalid surface: {surface!r}")
            params["surface_id"] = sid
        self._call("pane.join", params)

    def last_pane(self) -> str:
        """Request the last pane and require its returned ID."""
        res = self._call("pane.last") or {}
        pid = res.get("pane_id")
        if not pid:
            raise cmuxError(f"pane.last returned no pane_id: {res}")
        return str(pid)

    # ---------------------------------------------------------------------
    # Input
    # ---------------------------------------------------------------------

    def send(self, text: str) -> None:
        """Expand supported backslash control escapes and send text to the server-selected surface."""
        text2 = _unescape_backslash_controls(text)
        self._call("surface.send_text", {"text": text2})

    def send_surface(self, surface: Union[str, int], text: str) -> None:
        """Resolve a surface and send text after expanding supported backslash control escapes."""
        sid = self._resolve_surface_id(surface)
        if not sid:
            raise cmuxError(f"Invalid surface: {surface!r}")
        text2 = _unescape_backslash_controls(text)
        self._call("surface.send_text", {"surface_id": sid, "text": text2})

    def send_key(self, key: str) -> None:
        """Send a named key to the server-selected surface."""
        self._call("surface.send_key", {"key": key})

    def send_key_surface(self, surface: Union[str, int], key: str) -> None:
        """Resolve a surface and send the named key to it."""
        sid = self._resolve_surface_id(surface)
        if not sid:
            raise cmuxError(f"Invalid surface: {surface!r}")
        self._call("surface.send_key", {"surface_id": sid, "key": key})

    def send_ctrl_c(self) -> None:
        """Send the Ctrl+C key through the shared key command."""
        self.send_key("ctrl-c")

    def send_ctrl_d(self) -> None:
        """Send the Ctrl+D key through the shared key command."""
        self.send_key("ctrl-d")

    # ---------------------------------------------------------------------
    # Notifications
    # ---------------------------------------------------------------------

    def notify(self, title: str, subtitle: str = "", body: str = "") -> None:
        """Request a notification with title, subtitle and body."""
        self._call("notification.create", {"title": title, "subtitle": subtitle, "body": body})

    def notify_surface(self, surface: Union[str, int], title: str, subtitle: str = "", body: str = "") -> None:
        """Associate a notification with a resolved surface."""
        sid = self._resolve_surface_id(surface)
        if not sid:
            raise cmuxError(f"Invalid surface: {surface!r}")
        self._call(
            "notification.create_for_surface",
            {"surface_id": sid, "title": title, "subtitle": subtitle, "body": body},
        )

    def list_notifications(self) -> list[dict]:
        """Return notification records advertised by the server."""
        res = self._call("notification.list") or {}
        return list(res.get("notifications") or [])

    def clear_notifications(self) -> None:
        """Request clearing the server notification list."""
        self._call("notification.clear")

    def set_app_focus(self, active: Union[bool, None]) -> None:
        """Set the debug focus override to active/inactive, or clear it with None."""
        if active is None:
            state = "clear"
        else:
            state = "active" if active else "inactive"
        self._call("app.focus_override.set", {"state": state})

    def simulate_app_active(self) -> None:
        """Request the upstream application-activation simulation hook."""
        self._call("app.simulate_active")

    # Debug-only: focus via notification flow
    def focus_notification(self, workspace: Union[str, int], surface: Union[str, int, None] = None) -> None:
        """Invoke the debug notification-focus path for a workspace and optional surface."""
        wsid = self._resolve_workspace_id(workspace)
        params: Dict[str, Any] = {"workspace_id": wsid}
        if surface is not None:
            sid = self._resolve_surface_id(surface, workspace_id=wsid)
            params["surface_id"] = sid
        self._call("debug.notification.focus", params)

    # ---------------------------------------------------------------------
    # Browser
    # ---------------------------------------------------------------------

    def open_browser(self, url: str = None) -> str:
        """Request a browser split with an optional URL and require its surface ID."""
        params: Dict[str, Any] = {}
        if url:
            params["url"] = url
        res = self._call("browser.open_split", params) or {}
        sid = res.get("surface_id")
        if not sid:
            raise cmuxError(f"browser.open_split returned no surface_id: {res}")
        return str(sid)

    def navigate(self, panel_id: str, url: str) -> None:
        """Resolve a browser surface and request navigation to the supplied URL."""
        sid = self._resolve_surface_id(panel_id)
        if not sid:
            raise cmuxError(f"Invalid surface: {panel_id!r}")
        self._call("browser.navigate", {"surface_id": sid, "url": url})

    def browser_back(self, panel_id: str) -> None:
        """Request backward navigation in the resolved browser surface."""
        sid = self._resolve_surface_id(panel_id)
        self._call("browser.back", {"surface_id": sid})

    def browser_forward(self, panel_id: str) -> None:
        """Request forward navigation in the resolved browser surface."""
        sid = self._resolve_surface_id(panel_id)
        self._call("browser.forward", {"surface_id": sid})

    def browser_reload(self, panel_id: str) -> None:
        """Request reloading the resolved browser surface."""
        sid = self._resolve_surface_id(panel_id)
        self._call("browser.reload", {"surface_id": sid})

    def get_url(self, panel_id: str) -> str:
        """Return the resolved browser surface URL, or an empty string when omitted."""
        sid = self._resolve_surface_id(panel_id)
        res = self._call("browser.url.get", {"surface_id": sid}) or {}
        return str(res.get("url") or "")

    def focus_webview(self, panel_id: str) -> None:
        """Request browser-content focus for the resolved surface."""
        sid = self._resolve_surface_id(panel_id)
        self._call("browser.focus_webview", {"surface_id": sid})

    def is_webview_focused(self, panel_id: str) -> bool:
        """Return the server-reported browser-content focus state."""
        sid = self._resolve_surface_id(panel_id)
        res = self._call("browser.is_webview_focused", {"surface_id": sid}) or {}
        return bool(res.get("focused"))

    def wait_for_webview_focus(self, panel_id: str, timeout_s: float = 2.0) -> None:
        """Poll browser-content focus every 50 ms and raise when the polling timeout expires."""
        start = time.time()
        while time.time() - start < timeout_s:
            if self.is_webview_focused(panel_id):
                return
            time.sleep(0.05)
        raise cmuxError(f"Timed out waiting for webview focus: {panel_id}")

    # ---------------------------------------------------------------------
    # Debug / test-only
    # ---------------------------------------------------------------------

    def set_shortcut(self, name: str, combo: str) -> None:
        """Configure a named shortcut through the upstream debug hook."""
        self._call("debug.shortcut.set", {"name": name, "combo": combo})

    def simulate_shortcut(self, combo: str) -> None:
        """Request a synthetic shortcut through the upstream debug hook."""
        self._call("debug.shortcut.simulate", {"combo": combo})

    def simulate_type(self, text: str) -> None:
        """Expand backslash control escapes and invoke the upstream debug typing hook."""
        text2 = _unescape_backslash_controls(text)
        self._call("debug.type", {"text": text2})

    def activate_app(self) -> None:
        """Request application activation through the upstream debug hook."""
        self._call("debug.app.activate")

    def open_command_palette_rename_tab_input(self, window_id: Optional[str] = None) -> None:
        """Invoke the upstream palette rename-input hook for an optional window."""
        params: Dict[str, Any] = {}
        if window_id is not None:
            params["window_id"] = str(window_id)
        self._call("debug.command_palette.rename_tab.open", params)

    def command_palette_results(self, window_id: str, limit: int = 20) -> dict:
        """Return upstream palette results for a window and requested result limit."""
        res = self._call(
            "debug.command_palette.results",
            {"window_id": str(window_id), "limit": int(limit)},
        ) or {}
        return dict(res)

    def command_palette_rename_select_all(self) -> bool:
        """Query the upstream palette rename-input select-all setting."""
        res = self._call("debug.command_palette.rename_input.select_all") or {}
        return bool(res.get("enabled"))

    def set_command_palette_rename_select_all(self, enabled: bool) -> bool:
        """Set and return the upstream palette rename-input select-all setting."""
        res = self._call("debug.command_palette.rename_input.select_all", {"enabled": bool(enabled)}) or {}
        return bool(res.get("enabled"))

    def is_terminal_focused(self, panel: Union[str, int]) -> bool:
        """Query the upstream terminal-focus debug hook for a resolved surface."""
        sid = self._resolve_surface_id(panel)
        res = self._call("debug.terminal.is_focused", {"surface_id": sid}) or {}
        return bool(res.get("focused"))

    def read_terminal_text(self, panel: Union[str, int, None] = None) -> str:
        """Read plain or base64 terminal text, falling back to the older debug method if unavailable."""
        params: Dict[str, Any] = {}
        if panel is not None:
            sid = self._resolve_surface_id(panel)
            params["surface_id"] = sid
        try:
            res = self._call("surface.read_text", params) or {}
            if "text" in res:
                return str(res.get("text") or "")
            b64 = str(res.get("base64") or "")
            raw = base64.b64decode(b64) if b64 else b""
            return raw.decode("utf-8", errors="replace")
        except cmuxError as exc:
            # Back-compat for older builds that only expose the debug method.
            if "method_not_found" not in str(exc):
                raise

        res = self._call("debug.terminal.read_text", params) or {}
        b64 = str(res.get("base64") or "")
        raw = base64.b64decode(b64) if b64 else b""
        return raw.decode("utf-8", errors="replace")

    def render_stats(self, panel: Union[str, int, None] = None) -> dict:
        """Return the stats object from the upstream terminal render-statistics hook."""
        params: Dict[str, Any] = {}
        if panel is not None:
            sid = self._resolve_surface_id(panel)
            params["surface_id"] = sid
        res = self._call("debug.terminal.render_stats", params) or {}
        # Server wraps the underlying stats object under "stats".
        return dict(res.get("stats") or {})

    def layout_debug(self) -> dict:
        """Return the layout object from the upstream layout debug hook."""
        res = self._call("debug.layout") or {}
        # Server wraps LayoutDebugResponse under "layout".
        return dict(res.get("layout") or {})

    def panel_snapshot_reset(self, panel: Union[str, int]) -> None:
        """Reset upstream snapshot tracking for a resolved surface."""
        sid = self._resolve_surface_id(panel)
        self._call("debug.panel_snapshot.reset", {"surface_id": sid})

    def panel_snapshot(self, panel: Union[str, int], label: str = "") -> dict:
        """Capture upstream panel diagnostics and normalize surface_id to the v1 panel_id key."""
        sid = self._resolve_surface_id(panel)
        params: Dict[str, Any] = {"surface_id": sid}
        if label:
            params["label"] = label
        res = dict(self._call("debug.panel_snapshot", params) or {})
        # Normalize key to match the v1 client (panel_id).
        if "panel_id" not in res and "surface_id" in res:
            res["panel_id"] = res.get("surface_id")
        return res

    def bonsplit_underflow_count(self) -> int:
        """Read the legacy Bonsplit underflow counter from the upstream debug hook."""
        res = self._call("debug.bonsplit_underflow.count") or {}
        return int(res.get("count") or 0)

    def reset_bonsplit_underflow_count(self) -> None:
        """Reset the legacy Bonsplit underflow counter through the upstream debug hook."""
        self._call("debug.bonsplit_underflow.reset")

    def empty_panel_count(self) -> int:
        """Read the upstream debug counter for empty panels."""
        res = self._call("debug.empty_panel.count") or {}
        return int(res.get("count") or 0)

    def reset_empty_panel_count(self) -> None:
        """Reset the upstream debug counter for empty panels."""
        self._call("debug.empty_panel.reset")

    def flash_count(self, surface: Union[str, int]) -> int:
        """Read the upstream flash counter for a resolved surface."""
        sid = self._resolve_surface_id(surface)
        res = self._call("debug.flash.count", {"surface_id": sid}) or {}
        return int(res.get("count") or 0)

    def reset_flash_counts(self) -> None:
        """Reset all upstream debug flash counters."""
        self._call("debug.flash.reset")

    def screenshot(self, label: str = "") -> dict:
        """Request an upstream window screenshot with an optional label."""
        params: Dict[str, Any] = {}
        if label:
            params["label"] = label
        return dict(self._call("debug.window.screenshot", params) or {})


def main() -> None:
    """Run one JSON method from CLI arguments, or print server capabilities when no method is supplied."""
    import argparse

    parser = argparse.ArgumentParser(description="cmux v2 socket client")
    parser.add_argument("-s", "--socket", default=cmux.DEFAULT_SOCKET_PATH, help="Socket path")
    parser.add_argument("--method", help="v2 method name")
    parser.add_argument("--params", default="{}", help="JSON params")

    args = parser.parse_args()

    with cmux(args.socket) as c:
        if not args.method:
            # Minimal smoke.
            print(json.dumps(c.capabilities(), indent=2, sort_keys=True))
            return
        params = json.loads(args.params)
        print(json.dumps(c._call(args.method, params), indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
