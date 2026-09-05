"""Shared legacy sidebar text parsing; these helpers do not establish GTK API parity."""

from process_support import wait_until


def parse_sidebar_state(text: str) -> dict[str, str]:
    """Read top-level key/value rows, ignoring two-space-indented child rows.

    Split only at the first equals sign, trim field edges and retain the last
    value for duplicate keys. Malformed rows are ignored; empty values survive.
    """
    data: dict[str, str] = {}
    for line in (text or "").splitlines():
        if not line or line.startswith("  ") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        data[key.strip()] = value.strip()
    return data


def wait_for_state_field(client, key: str, expected: str,
                         timeout: float = 8.0, interval: float = 0.1) -> dict[str, str]:
    """Return the first matching sidebar snapshot within a monotonic polling budget.

    Client errors propagate. Each socket call needs its own timeout because the
    polling deadline cannot interrupt a blocked client operation.
    """
    state: dict[str, str] = {}

    def matches():
        """Replace the observed snapshot and compare the requested field exactly."""
        nonlocal state
        state = parse_sidebar_state(client.sidebar_state())
        return state.get(key) == expected

    wait_until(matches, f"{key}={expected!r}", timeout=timeout, interval=interval)
    return state
