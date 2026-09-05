#!/usr/bin/env bash
# Exercise package-table selection and explicit failure of unsupported version forms.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
source "$ROOT/scripts/release-version.sh"
TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TEMP_DIR"' EXIT
MANIFEST="$TEMP_DIR/manifest with spaces.toml"
cat > "$MANIFEST" <<'TOML'
[workspace.package]
version = "9.9.9"
[package] # root package
name = "cmux-gtk"
  version = "1.2.3" # release
[dependencies.example]
version = "8.8.8"
TOML
[[ "$(package_version "$MANIFEST")" == 1.2.3 ]]
for value in 'version.workspace = true' 'version = "01.2.3"' 'version = "1.2.3-beta"'; do
    printf '[package]\n%s\n' "$value" > "$MANIFEST"
    if package_version "$MANIFEST"; then
        echo "FAIL: accepted unsupported version form" >&2
        exit 1
    fi
done
printf '[dependencies.example]\nversion = "1.2.3"\n' > "$MANIFEST"
if package_version "$MANIFEST" || package_version "$TEMP_DIR/missing"; then
    echo "FAIL: accepted absent package or manifest" >&2
    exit 1
fi
printf '%s\n' 'PASS: release version lookup'
