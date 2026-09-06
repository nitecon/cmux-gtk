#!/usr/bin/env python3
"""Verify nested pane identity and targeted PTY routing without layout-forcing probes.

Viewport text proves terminal output ownership, not pixel visibility or geometry.
"""
import json
from pathlib import Path
import tempfile

from linux_app import running_app
from test_multi_workspace_focus import selected_surface


def main():
    """Repeat three-pane creation, verify exclusive output and reap the removed workspace's PTYs."""
    with tempfile.TemporaryDirectory(prefix="cmux-nested-routing-") as directory:
        with running_app(Path(directory)) as app:
            app.wait_for(lambda: len(app.children()) == 1, "initial shell")
            for iteration in range(8):
                workspace = json.loads(app.cli("new-workspace", "--json"))["uuid"]
                app.wait_for(lambda: len(app.children()) == 2, "new workspace shell")
                surfaces = [selected_surface(app)]
                for count in range(2):
                    app.cli("split", "--direction", "horizontal")
                    app.wait_for(lambda: len(app.children()) == count + 3, "nested split shell")
                    surfaces.append(selected_surface(app))
                assert len(set(surfaces)) == 3
                assert {row["uuid"] for row in app.surfaces()} == set(surfaces)
                focused = selected_surface(app)

                def lines(surface):
                    """Read decoded output lines without focus changes or native layout requests."""
                    return json.loads(app.cli("read-text", "--id", surface, "--json"))["text"].splitlines()

                for index, target in enumerate(surfaces):
                    marker = f"CMUX_ROUTE_{iteration}_{index}"
                    command = "printf 'CMUX_ROUTE_%s_%s\\n' " + str(iteration) + " " + str(index)
                    app.cli("send-text", command, "--id", target)
                    app.cli("send-key", "\r", "--id", target)
                    app.wait_for(lambda: marker in lines(target), "targeted executed output")
                    for other in surfaces:
                        if other != target:
                            assert marker not in lines(other), "output reached another terminal"
                    assert selected_surface(app) == focused, "targeted input changed focus"
                assert {row["uuid"] for row in app.surfaces()} == set(surfaces)
                app.cli("close-workspace", workspace)
                app.wait_for(lambda: len(app.children()) == 1, "nested workspace PTY cleanup")
    print("nested splits preserve identities, route output exclusively and reap their PTYs")


if __name__ == "__main__":
    main()
