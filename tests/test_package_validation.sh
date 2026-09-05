#!/usr/bin/env bash
# Exercise shared reporting, argument preservation and negative-match error handling.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
source "$ROOT/packaging/scripts/validation.sh"
TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TEMP_DIR"' EXIT
printf '%s\n' 'literal $(false) text' > "$TEMP_DIR/file with spaces"
check "literal command arguments" grep -Fx 'literal $(false) text' "$TEMP_DIR/file with spaces"
check "missing pattern" absent 'not present' "$TEMP_DIR/file with spaces"
check "existing pattern fails absence" absent 'literal' "$TEMP_DIR/file with spaces"
check "missing input is an error" absent 'anything' "$TEMP_DIR/missing"
[[ "$PASS" -eq 2 && "$FAIL" -eq 2 ]]
printf '%s\n' 'PASS: validation helper behavior'
