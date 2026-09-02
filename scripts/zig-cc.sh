#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ZIG_VERSION="$(sed -n 's/.*\.minimum_zig_version = "\([^"]*\)".*/\1/p' \
    "$REPO_ROOT/ghostty/build.zig.zon" | head -1)"
ZIG_BIN="${CMUX_ZIG:-$REPO_ROOT/.tools/zig-$ZIG_VERSION/zig}"

if [[ ! -x "$ZIG_BIN" ]]; then
    echo "cmux: Zig $ZIG_VERSION is required as the Linux linker; run ./scripts/setup-linux-dev.sh" >&2
    exit 1
fi

exec "$ZIG_BIN" cc "$@"
