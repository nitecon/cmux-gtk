#!/usr/bin/env bash
set -euo pipefail

# validate-deb.sh -- Validate a built .deb package for correct structure
# Usage: ./validate-deb.sh [path-to-deb]

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# Find the .deb file
if [[ -n "${1:-}" ]]; then
    DEB="$1"
else
    # shellcheck disable=SC2012
    DEB=$(ls -t "$REPO_ROOT"/dist/cmux-gtk_*_amd64.deb 2>/dev/null | head -1)
    if [[ -z "$DEB" ]]; then
        echo "ERROR: No .deb file found in dist/" >&2
        exit 1
    fi
fi

if [[ ! -f "$DEB" ]]; then
    echo "ERROR: .deb file not found: $DEB" >&2
    exit 1
fi

echo "Validating: $DEB"
echo ""

# shellcheck source=packaging/scripts/validation.sh
source "$SCRIPT_DIR/validation.sh"

# Cache file listing and control output
EXTRACTED_ROOT=$(mktemp -d)
trap 'rm -rf "$EXTRACTED_ROOT"' EXIT
FILE_LIST_FILE="$EXTRACTED_ROOT/file-list"
CONTROL_FILE="$EXTRACTED_ROOT/control"
dpkg-deb -c "$DEB" > "$FILE_LIST_FILE"
dpkg-deb -f "$DEB" > "$CONTROL_FILE"
dpkg-deb -x "$DEB" "$EXTRACTED_ROOT"
DESKTOP_FILE="$EXTRACTED_ROOT/usr/share/applications/io.cmux.App.desktop"
METAINFO_FILE="$EXTRACTED_ROOT/usr/share/metainfo/io.cmux.App.metainfo.xml"

# --- File listing checks ---
echo "File listing:"

check "usr/bin/cmux-app exists" \
    grep -q "\./usr/bin/cmux-app" "$FILE_LIST_FILE"

# Anchored match to avoid matching cmux-app
check "usr/bin/cmux exists" \
    grep -qE "\./usr/bin/cmux([[:space:]]|$)" "$FILE_LIST_FILE"

check "usr/lib/cmux/cmuxd-remote exists" \
    grep -q "\./usr/lib/cmux/cmuxd-remote" "$FILE_LIST_FILE"

check "usr/lib/cmux/agent-browser exists" \
    grep -q "\./usr/lib/cmux/agent-browser" "$FILE_LIST_FILE"

check "usr/bin/agent-browser exists" \
    grep -q "\./usr/bin/agent-browser" "$FILE_LIST_FILE"

check "desktop entry exists" \
    grep -q "\./usr/share/applications/io.cmux.App.desktop" "$FILE_LIST_FILE"

check "metainfo exists" \
    grep -q "\./usr/share/metainfo/io.cmux.App.metainfo.xml" "$FILE_LIST_FILE"

check "48x48 icon exists" \
    grep -q "\./usr/share/icons/hicolor/48x48/apps/io.cmux.App.png" "$FILE_LIST_FILE"

check "128x128 icon exists" \
    grep -q "\./usr/share/icons/hicolor/128x128/apps/io.cmux.App.png" "$FILE_LIST_FILE"

check "256x256 icon exists" \
    grep -q "\./usr/share/icons/hicolor/256x256/apps/io.cmux.App.png" "$FILE_LIST_FILE"

echo ""
echo "Desktop integration:"

check "desktop entry is valid" \
    desktop-file-validate "$DESKTOP_FILE"

check "desktop entry launches cmux-app" \
    grep -qx "Exec=cmux-app" "$DESKTOP_FILE"

check "desktop icon matches GTK application ID" \
    grep -qx "Icon=io.cmux.App" "$DESKTOP_FILE"

check "desktop window class matches GTK application ID" \
    grep -qx "StartupWMClass=io.cmux.App" "$DESKTOP_FILE"

check "AppStream metadata is valid" \
    appstreamcli validate --no-net "$METAINFO_FILE"

check "bash completion exists" \
    grep -q "\./usr/share/bash-completion/completions/cmux" "$FILE_LIST_FILE"

check "zsh completion exists" \
    grep -q "\./usr/share/zsh/vendor-completions/_cmux" "$FILE_LIST_FILE"

check "fish completion exists" \
    grep -q "\./usr/share/fish/vendor_completions.d/cmux.fish" "$FILE_LIST_FILE"

check "man page exists" \
    grep -q "\./usr/share/man/man1/cmux.1.gz" "$FILE_LIST_FILE"

# --- Skills & CLAUDE.md checks (Phase 12.1) ---
echo ""
echo "Skills:"

check "cmux skill SKILL.md exists" \
    grep -q "\./usr/share/cmux/skills/cmux/SKILL.md" "$FILE_LIST_FILE"

check "cmux-browser skill SKILL.md exists" \
    grep -q "\./usr/share/cmux/skills/cmux-browser/SKILL.md" "$FILE_LIST_FILE"

check "cmux-browser commands.md exists" \
    grep -q "\./usr/share/cmux/skills/cmux-browser/references/commands.md" "$FILE_LIST_FILE"

check "CLAUDE.md exists" \
    grep -q "\./usr/share/cmux/CLAUDE.md" "$FILE_LIST_FILE"

check "no cmux-debug-windows skill packaged (D-13)" \
    absent "cmux-debug-windows" "$FILE_LIST_FILE"

check "no release skill packaged (D-13)" \
    absent "skills/release" "$FILE_LIST_FILE"

# --- Metadata checks ---
echo ""
echo "Metadata:"

check "Package: cmux-gtk" \
    grep -q "^Package: cmux-gtk$" "$CONTROL_FILE"

check "Architecture: amd64" \
    grep -q "^Architecture: amd64$" "$CONTROL_FILE"

check "Version is non-empty" \
    grep -qE "^Version: .+" "$CONTROL_FILE"

check "Depends contains libnotify-bin" \
    grep -q "^Depends:.*libnotify-bin" "$CONTROL_FILE"

check "Depends contains libgtk-4-1" \
    grep -q "^Depends:.*libgtk-4-1" "$CONTROL_FILE"

check "Depends contains libfontconfig1" \
    grep -q "^Depends:.*libfontconfig1" "$CONTROL_FILE"

check "Depends contains libfreetype6" \
    grep -q "^Depends:.*libfreetype6" "$CONTROL_FILE"

check "Depends contains libonig5" \
    grep -q "^Depends:.*libonig5" "$CONTROL_FILE"

check "Depends contains libgl1" \
    grep -q "^Depends:.*libgl1" "$CONTROL_FILE"

check "Depends contains libharfbuzz0b" \
    grep -q "^Depends:.*libharfbuzz0b" "$CONTROL_FILE"

check "Depends contains libglib2.0-0" \
    grep -q "^Depends:.*libglib2.0-0" "$CONTROL_FILE"

check "Depends contains libc++1" \
    grep -q "^Depends:.*libc++1" "$CONTROL_FILE"

check "Depends contains libc++abi1" \
    grep -q "^Depends:.*libc++abi1" "$CONTROL_FILE"

check "Depends contains libxml2" \
    grep -q "^Depends:.*libxml2" "$CONTROL_FILE"

check "Depends contains libcairo2" \
    grep -q "^Depends:.*libcairo2" "$CONTROL_FILE"

echo ""
echo "Results: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
