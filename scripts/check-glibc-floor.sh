#!/usr/bin/env bash
set -euo pipefail

if [[ "$#" -lt 2 ]]; then
    echo "usage: $0 <maximum-glibc-version> <binary> [binary ...]" >&2
    exit 2
fi

MAXIMUM="$1"
shift

for binary in "$@"; do
    if [[ ! -f "$binary" ]]; then
        echo "GLIBC-CHECK-FAIL: missing binary: $binary" >&2
        exit 1
    fi

    versions="$(readelf --version-info "$binary" \
        | sed -n 's/.*Name: GLIBC_\([0-9][0-9.]*\).*/\1/p' \
        | sort -Vu)"
    if [[ -z "$versions" ]]; then
        echo "GLIBC-CHECK-FAIL: no GLIBC requirements found in $binary" >&2
        exit 1
    fi

    required="$(printf '%s\n' "$versions" | tail -n 1)"
    highest="$(printf '%s\n%s\n' "$MAXIMUM" "$required" | sort -V | tail -n 1)"
    if [[ "$highest" != "$MAXIMUM" ]]; then
        echo "GLIBC-CHECK-FAIL: $binary requires GLIBC_$required (maximum GLIBC_$MAXIMUM)" >&2
        exit 1
    fi

    echo "GLIBC-CHECK-OK: $binary requires at most GLIBC_$required (maximum GLIBC_$MAXIMUM)"
done
