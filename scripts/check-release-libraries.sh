#!/usr/bin/env bash
set -euo pipefail

if [[ "$#" -eq 0 ]]; then
    echo "usage: $0 <binary> [binary ...]" >&2
    exit 2
fi

for binary in "$@"; do
    if [[ ! -f "$binary" ]]; then
        echo "RUNTIME-LIB-CHECK-FAIL: missing binary: $binary" >&2
        exit 1
    fi

    if readelf --dynamic "$binary" | grep -Eq '\(NEEDED\).*lib(xml2|icu[[:alnum:]_-]*|lzma)\.so'; then
        echo "RUNTIME-LIB-CHECK-FAIL: $binary dynamically links a bundled XML dependency" >&2
        exit 1
    fi

    if readelf --dyn-syms --wide "$binary" | \
        awk '$5 != "LOCAL" && $8 ~ /^(hb_|FT_)/ { found=1 } END { exit !found }'; then
        echo "RUNTIME-LIB-CHECK-FAIL: $binary exports bundled HarfBuzz/FreeType symbols" >&2
        exit 1
    fi

    echo "RUNTIME-LIB-CHECK-OK: $binary keeps bundled libraries private"
done
