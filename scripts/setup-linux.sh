#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

echo "==> Initializing submodules..."
git submodule update --init --recursive

ZIG_VERSION="$(sed -n 's/.*\.minimum_zig_version = "\([^"]*\)".*/\1/p' ghostty/build.zig.zon | head -1)"
if [[ -z "$ZIG_VERSION" ]]; then
    echo "ERROR: could not determine Ghostty's required Zig version." >&2
    exit 1
fi
LOCAL_ZIG="$REPO_ROOT/.tools/zig-$ZIG_VERSION/zig"
if [[ -x "$LOCAL_ZIG" ]]; then
    export PATH="$(dirname "$LOCAL_ZIG"):$PATH"
fi

echo "==> Checking system dependencies..."
if ! pkg-config --exists gtk4 2>/dev/null; then
    echo "ERROR: GTK4 development files are missing." >&2
    echo "Run: ./scripts/setup-linux-dev.sh" >&2
    exit 1
fi

if ! command -v zig &>/dev/null; then
    echo "ERROR: Zig $ZIG_VERSION is required to build libghostty."
    echo "Run: ./scripts/setup-linux-dev.sh"
    exit 1
fi

if [ "$(zig version)" != "$ZIG_VERSION" ]; then
    echo "ERROR: Zig $ZIG_VERSION is required; found $(zig version)."
    exit 1
fi

echo "==> Resolving agent-browser..."
if AGENT_BROWSER_PATH="$($REPO_ROOT/scripts/resolve-agent-browser.sh)"; then
    echo "==> Using agent-browser at: $AGENT_BROWSER_PATH"
else
    echo "==> agent-browser not found; browser panes will be unavailable."
    echo "    Install it with: npm install -g agent-browser && agent-browser install"
fi

echo "==> Building libghostty.a from ghostty submodule..."
cd ghostty

# Verify submodule is initialized
if [ ! -f "build.zig" ]; then
    echo "ERROR: ghostty submodule not initialized. Run: git submodule update --init --recursive"
    exit 1
fi

zig build \
    -Dapp-runtime=none \
    -Doptimize=ReleaseFast \
    -Dcpu=baseline \
    -Dgtk-x11=true \
    -Dgtk-wayland=true

if [[ -f zig-out/lib/ghostty-internal.a ]]; then
    ln -sfn ghostty-internal.a zig-out/lib/libghostty.a
fi

if [[ ! -f zig-out/lib/libghostty.a ]]; then
    echo "ERROR: Ghostty build produced no compatible static library." >&2
    exit 1
fi

echo "==> libghostty.a available at: $(pwd)/zig-out/lib/libghostty.a"
ls -lhL zig-out/lib/libghostty.a
