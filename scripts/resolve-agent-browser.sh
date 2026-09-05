#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ $# -gt 0 ]]; then
    echo "Usage: $0" >&2
    exit 2
fi

# Print the first executable browser adapter, honoring the explicit override; return nonzero if unavailable.
resolve_agent_browser() {
    local candidate

    if [[ -n "${CMUX_AGENT_BROWSER:-}" ]]; then
        if [[ -x "$CMUX_AGENT_BROWSER" ]]; then
            printf '%s\n' "$CMUX_AGENT_BROWSER"
            return 0
        fi
        echo "ERROR: CMUX_AGENT_BROWSER is not executable: $CMUX_AGENT_BROWSER" >&2
        return 1
    fi

    if candidate="$(command -v agent-browser 2>/dev/null)" && [[ -x "$candidate" ]]; then
        printf '%s\n' "$candidate"
        return 0
    fi

    for candidate in \
        /usr/lib/cmux/agent-browser \
        /usr/lib64/cmux/agent-browser \
        "$REPO_ROOT/node_modules/.bin/agent-browser" \
        "$REPO_ROOT/target/release/agent-browser" \
        "$REPO_ROOT/agent-browser/cli/target/release/agent-browser"; do
        if [[ -x "$candidate" ]]; then
            printf '%s\n' "$candidate"
            return 0
        fi
    done

    return 1
}

if ! resolve_agent_browser; then
    echo "ERROR: agent-browser was not found; browser panes will be unavailable." >&2
    echo "Install it with: npm install -g agent-browser && agent-browser install" >&2
    echo "Or set CMUX_AGENT_BROWSER to a local executable." >&2
    exit 1
fi
