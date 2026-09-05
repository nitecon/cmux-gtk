#!/usr/bin/env bash
# Shared release-version lookup for the repository's literal Cargo package version.

# Print the literal version in the manifest's [package] table, or fail if absent.
# Accepts one manifest path. Supports the quoted single-line field used by this
# repository; workspace-inherited versions require an explicit parser extension.
package_version() {
    local manifest="$1" version
    version="$(awk '
        /^[[:space:]]*\[package\][[:space:]]*(#.*)?$/ { in_package=1; next }
        /^[[:space:]]*\[/ { in_package=0 }
        in_package && /^[[:space:]]*version[[:space:]]*=[[:space:]]*"/ {
            sub(/^[^"]*"/, "")
            sub(/".*$/, "")
            print
            exit
        }
    ' "$manifest")" || return
    if [[ ! "$version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
        printf 'ERROR: expected a literal X.Y.Z package version in %s\n' "$manifest" >&2
        return 1
    fi
    printf '%s\n' "$version"
}
