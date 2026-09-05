#!/usr/bin/env python3
"""
End-to-end test for generic sidebar metadata commands.

Validates:
1) report_meta stores icon/url/priority/format metadata
2) metadata list ordering follows priority
3) set_status remains compatible as an alias-style metadata writer
4) clear_meta removes metadata entries
"""

from __future__ import annotations

import os
import sys
import time

# Add the directory containing cmux.py to the path
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from cmux import cmux, cmuxError  # noqa: E402
from sidebar_support import wait_for_state_field as _wait_for_state_field


def main() -> int:
    """Verify legacy metadata ordering, attributes, aliases and removal on the connected app."""
    tag = os.environ.get("CMUX_TAG") or ""
    if not tag:
        print("Tip: set CMUX_TAG=<tag> when running this test to avoid socket conflicts.")

    pr_url = "https://github.com/manaflow-ai/cmux/pull/337"

    try:
        with cmux() as client:
            new_tab_id = client.new_tab()
            client.select_tab(new_tab_id)
            time.sleep(0.6)

            tab_id = client.current_workspace()

            client.report_meta(
                "task",
                "**Review** PR 337",
                icon="sf:doc.text.magnifyingglass",
                url=pr_url,
                priority=50,
                format="markdown",
                tab=tab_id,
            )
            client.report_meta(
                "context",
                "issue-336-sidebar-pr-metadata",
                icon="text:CTX",
                priority=10,
                tab=tab_id,
            )
            _wait_for_state_field(client, "status_count", "2")

            listed = client.list_meta(tab=tab_id).splitlines()
            if len(listed) != 2:
                raise AssertionError(f"Expected 2 metadata entries, got {len(listed)}: {listed}")

            if not listed[0].startswith("task="):
                raise AssertionError(f"Expected first entry to be task metadata. Got: {listed[0]}")
            if "priority=50" not in listed[0]:
                raise AssertionError(f"Expected task entry to include priority. Got: {listed[0]}")
            if "format=markdown" not in listed[0]:
                raise AssertionError(f"Expected markdown format in task entry. Got: {listed[0]}")
            if f"url={pr_url}" not in listed[0]:
                raise AssertionError(f"Expected URL in task entry. Got: {listed[0]}")

            client.set_status("agent", "in progress", icon="text:AI", priority=80, tab=tab_id)
            _wait_for_state_field(client, "status_count", "3")

            listed = client.list_meta(tab=tab_id).splitlines()
            if not listed[0].startswith("agent="):
                raise AssertionError(f"Expected highest-priority agent entry first. Got: {listed[0]}")

            client.clear_meta("task", tab=tab_id)
            _wait_for_state_field(client, "status_count", "2")

            listed = client.list_meta(tab=tab_id).splitlines()
            if any(line.startswith("task=") for line in listed):
                raise AssertionError(f"Task metadata should be cleared. Got: {listed}")

            try:
                client.close_tab(new_tab_id)
            except Exception:
                pass

        print("Sidebar metadata test passed.")
        return 0
    except (cmuxError, AssertionError) as e:
        print(f"Sidebar metadata test failed: {e}")
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
