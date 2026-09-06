#!/usr/bin/env bash
# Exercise the release ELF gate with real valid, forbidden and malformed artifacts.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TEMP_DIR"' EXIT
CHECK="$ROOT/scripts/check-release-libraries.sh"

# Require rejection without depending on diagnostic wording.
reject() {
    if bash "$CHECK" "$1"; then
        echo "unexpected acceptance: $1" >&2
        exit 1
    fi
}

printf '%s\n' 'int main(void) { return 0; }' > "$TEMP_DIR/clean.c"
cc "$TEMP_DIR/clean.c" -o "$TEMP_DIR/clean"
bash "$CHECK" "$TEMP_DIR/clean"
printf '%s\n' 'not an ELF file' > "$TEMP_DIR/invalid"
reject "$TEMP_DIR/invalid"
reject "$TEMP_DIR/missing"

printf '%s\n' 'int hb_fixture(void) { return 1; }' > "$TEMP_DIR/export.c"
cc -shared -fPIC "$TEMP_DIR/export.c" -o "$TEMP_DIR/export.so"
reject "$TEMP_DIR/export.so"

printf '%s\n' 'int xml_fixture(void) { return 1; }' > "$TEMP_DIR/xml.c"
cc -shared -fPIC "$TEMP_DIR/xml.c" -Wl,-soname,libxml2.so -o "$TEMP_DIR/libxml2.so"
printf '%s\n' 'extern int xml_fixture(void); int main(void) { return xml_fixture(); }' > "$TEMP_DIR/linked.c"
cc "$TEMP_DIR/linked.c" -L"$TEMP_DIR" -lxml2 -o "$TEMP_DIR/linked"
reject "$TEMP_DIR/linked"
printf '%s\n' 'PASS: release ELF inspection'
