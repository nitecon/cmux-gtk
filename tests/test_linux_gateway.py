#!/usr/bin/env python3
"""Verify global gateway consent and legacy cleanup through the real GTK app and public CLI."""
import json
from pathlib import Path
import subprocess
import tempfile
import uuid

from linux_app import running_app


def status(app):
    """Read credential-free configuration and prepared delivery status."""
    return json.loads(app.cli("gateway", "status", "--json"))


def raw(app, method, **params):
    """Exercise public socket methods without inventing a gateway stream protocol."""
    return json.loads(app.cli("raw", method, "--params", json.dumps(params), "--json"))


def rejects(app, method, **params):
    """Require removed operations or invalid consent to fail visibly."""
    try:
        raw(app, method, **params)
    except subprocess.CalledProcessError:
        return
    raise AssertionError("operation unexpectedly accepted: " + method)


def main():
    """Upgrade manual settings without inheriting approval; save/reload global preferences without input."""
    with tempfile.TemporaryDirectory(prefix="cmux-gateway-") as directory:
        root = Path(directory)
        config = root / "config/cmux"
        config.mkdir(parents=True)
        instance = str(uuid.uuid4())
        (config / "gateway.json").write_text(json.dumps({
            "instance_id": instance,
            "config": {"enabled": True, "url": "https://gateway.example", "mappings": [
                {"workspace_id": str(uuid.uuid4()), "project_ident": "legacy-project"}]},
            "runs": [],
        }))
        key = "isolated-gateway-secret"
        with running_app(root, {"CMUX_GATEWAY_API_KEY": "", "GATEWAY_API_KEY": ""}) as app:
            app.wait_for(lambda: status(app)["connection"].startswith("Awaiting"), "prepared gateway status")
            view = status(app)
            assert view["config"]["enabled"]
            assert not view["config"]["injection_approved"]
            assert "mappings" not in view["config"] and "runs" not in view
            for method in ("gateway.bind", "gateway.accept", "gateway.report", "gateway.agent_event"):
                rejects(app, method)
            rejects(app, "gateway.configure", enabled=True, url="https://gateway.example")
            rejects(app, "gateway.configure", enabled=True, url="http://gateway.example", injection_approved=True)
            surface = next(row["uuid"] for row in app.surfaces() if row["active"])
            app.wait_for(lambda: raw(app, "surface.read_text", id=surface).get("text"), "shell readiness")
            raw(app, "surface.send_text", id=surface, text="echo CMUX_UNFINISHED_INPUT")
            raw(app, "gateway.configure", enabled=True, url="https://gateway.example", injection_approved=True, api_key=key)
            view = status(app)
            assert view["config"]["injection_approved"] and view["pending"] == 0
            assert key not in json.dumps(view)
            assert "CMUX_UNFINISHED_INPUT" in raw(app, "surface.read_text", id=surface)["text"]
            app.cli("gateway", "configure", "--url", "https://gateway.example", "--enabled")
            assert not status(app)["config"]["injection_approved"]
            app.cli("gateway", "configure", "--url", "https://gateway.example", "--enabled", "--approve-injection")
            assert status(app)["config"]["injection_approved"]
        saved = json.loads((config / "gateway.json").read_text())
        assert saved["instance_id"] == instance and "runs" not in saved
        assert "mappings" not in saved["config"] and saved["config"]["injection_approved"]
        assert key not in (config / "gateway.json").read_text()
        assert (config / "gateway-key").read_text() == key
        assert (config / "gateway-key").stat().st_mode & 0o077 == 0
        with running_app(root, {"CMUX_GATEWAY_API_KEY": "", "GATEWAY_API_KEY": ""}) as app:
            app.wait_for(lambda: status(app)["connection"].startswith("Awaiting"), "restored preferences")
            assert status(app)["config"]["injection_approved"]
            app.cli("gateway", "configure", "--url", "https://gateway.example")
            assert status(app)["connection"] == "Disabled"
    print("gateway global consent, legacy cleanup and prepared transport status passed")


if __name__ == "__main__":
    main()
