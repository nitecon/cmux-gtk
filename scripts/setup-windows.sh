#!/usr/bin/env bash
# Build the pinned full Ghostty embedded backend in an MSYS2 UCRT64 environment.
set -euo pipefail
CMUX_REPOSITORY="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$CMUX_REPOSITORY/ghostty"
zig build \
    -Dtarget=x86_64-windows-gnu \
    -Dapp-runtime=none \
    -Dembedded-app-thread-render=true \
    -Dfont-backend=freetype_windows \
    -Doptimize=ReleaseFast \
    -Dcpu=baseline \
    -Dsentry=false \
    -Di18n=false \
    -Demit-xcframework=false
test -f zig-out/lib/ghostty-internal-static.lib
test -f zig-out/lib/ghostty-internal.dll
test -f zig-out/lib/ghostty-internal.lib
test -f zig-out/include/ghostty.h
