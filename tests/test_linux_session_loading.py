#!/usr/bin/env python3
"""Exercise session-load outcomes and usable GTK startup with isolated session files."""
import json
from pathlib import Path
import tempfile
import uuid

from linux_app import running_app


def main():
    """Observe real startup records for absent, malformed, incompatible and valid sessions."""
    def leaf():
        """Assign each restored terminal its own persistent identity."""
        return {"type": "Leaf", "pane_id": 1, "surface_uuid": str(uuid.uuid4()), "shell": "/bin/sh", "cwd": "/tmp"}

    deep_layout = leaf()
    for _ in range(16):
        deep_layout = {"type": "Split", "orientation": "horizontal", "ratio": 0.5, "start": leaf(), "end": deep_layout}
    deep_session = json.dumps({"version": 3, "active_index": 0, "workspaces": [{
        "uuid": str(uuid.uuid4()), "name": "Deep project layout", "active_pane_uuid": None, "layout": deep_layout}]}).encode()
    cases = [
        (deep_session, "success", 3, 1),
        (None, "missing", None, None),
        ("symlink-loop", "read_error", None, None),
        (b"{broken", "decode_error", None, None),
        (b'{"version":3,"active_index":0,"workspaces":[],"unknown":"\xff"}', "decode_error", None, None),
        (b'{"version":99,"active_index":0,"workspaces":[]}', "unsupported_version", 99, None),
        (b'{"version":3,"active_index":0,"workspaces":[]}', "success", 3, 0),
    ]
    for content, outcome, version, workspaces in cases:
        with tempfile.TemporaryDirectory(prefix="cmux-session-load-") as directory:
            root = Path(directory)
            session_dir = root / "data/cmux"
            session_dir.mkdir(parents=True)
            if content == "symlink-loop":
                (session_dir / "session.json").symlink_to("session.json")
            elif content is not None:
                (session_dir / "session.json").write_bytes(content)
            events_path = root / "events.jsonl"
            with running_app(root, {"CMUX_LOG": str(events_path)}) as app:
                app.cli("ping")
                if content == deep_session:
                    records = json.loads(app.cli("list-workspaces", "--json"))["workspaces"]
                    assert len(records) == 1 and records[0]["name"] == "Deep project layout"
                    assert records[0]["pane_count"] == 17 and records[0]["surface_count"] == 17, records

                def observed():
                    """Wait for the buffered writer to publish exactly one classified startup load."""
                    try:
                        with events_path.open() as log:
                            records = [json.loads(line) for line in log.read(1024 * 1024).splitlines()]
                    except (FileNotFoundError, json.JSONDecodeError):
                        return False
                    loads = [record["fields"] for record in records if record["event"] == "session.load"]
                    if not loads:
                        return False
                    assert len(loads) == 1, loads
                    load = loads[0]
                    assert load["outcome"] == outcome, load
                    assert load["version"] == version, load
                    assert load["workspaces"] == workspaces, load
                    assert load["duration_us"] >= 0, load
                    if outcome in {"read_error", "decode_error"}:
                        assert load["error_category"] is not None, load
                    return True

                app.wait_for(observed, "classified session load")
    print("session loading reports outcomes and leaves GTK responsive")


if __name__ == "__main__":
    main()
