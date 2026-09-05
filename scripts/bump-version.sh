#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

CURRENT_VERSION="$(awk '
  /^\[package\]$/ { in_package=1; next }
  /^\[/ { in_package=0 }
  in_package && /^version = "/ { gsub(/^version = "|"$/, ""); print; exit }
' Cargo.toml)"

if [[ ! "$CURRENT_VERSION" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)$ ]]; then
  echo "Error: unsupported Cargo version: $CURRENT_VERSION" >&2
  exit 1
fi

MAJOR="${BASH_REMATCH[1]}"
MINOR="${BASH_REMATCH[2]}"
PATCH="${BASH_REMATCH[3]}"

case "${1:-minor}" in
  major) NEW_VERSION="$((MAJOR + 1)).0.0" ;;
  minor) NEW_VERSION="$MAJOR.$((MINOR + 1)).0" ;;
  patch) NEW_VERSION="$MAJOR.$MINOR.$((PATCH + 1))" ;;
  [0-9]*.[0-9]*.[0-9]*) NEW_VERSION="$1" ;;
  *)
    echo "Usage: $0 [major|minor|patch|X.Y.Z]" >&2
    exit 1
    ;;
esac

if [[ ! "$NEW_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Error: invalid version: $NEW_VERSION" >&2
  exit 1
fi

# Replace the root package version with NEW_VERSION, failing if its manifest field is absent.
update_manifest() {
  local temp_file
  temp_file="$(mktemp "${TMPDIR:-/tmp}/cmux-version.XXXXXX")"
  awk -v new_version="$NEW_VERSION" '
    BEGIN { in_package=0; updated=0 }
    /^\[package\]$/ { in_package=1 }
    /^\[/ && $0 != "[package]" { in_package=0 }
    in_package && !updated && /^version = "/ {
      print "version = \"" new_version "\""
      updated=1
      next
    }
    { print }
    END { if (!updated) exit 42 }
  ' Cargo.toml > "$temp_file"
  mv "$temp_file" Cargo.toml
}

# Replace the cmux-gtk lockfile version with NEW_VERSION, leaving dependency versions unchanged.
update_lockfile() {
  local temp_file
  temp_file="$(mktemp "${TMPDIR:-/tmp}/cmux-lock-version.XXXXXX")"
  awk -v new_version="$NEW_VERSION" '
    BEGIN { cmux_package=0; updated=0 }
    $0 == "name = \"cmux-gtk\"" { cmux_package=1 }
    cmux_package && !updated && /^version = "/ {
      print "version = \"" new_version "\""
      cmux_package=0
      updated=1
      next
    }
    { print }
    END { if (!updated) exit 42 }
  ' Cargo.lock > "$temp_file"
  mv "$temp_file" Cargo.lock
}

update_manifest
update_lockfile

echo "Updated cmux-gtk from $CURRENT_VERSION to $NEW_VERSION"
