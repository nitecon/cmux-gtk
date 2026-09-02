#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

install_system_dependencies() {
    local elevate=()
    if [[ "$(id -u)" -ne 0 ]]; then
        elevate=(sudo)
    fi
    echo "==> Installing Linux development dependencies..."
    if command -v apt-get &>/dev/null; then
        "${elevate[@]}" apt-get update
        "${elevate[@]}" apt-get install -y \
            build-essential curl xz-utils pkg-config python3 gettext libclang-dev \
            libgtk-4-dev libfontconfig1-dev libfreetype6-dev \
            libonig-dev libgl-dev libc++-dev libc++abi-dev libxml2-dev
    elif command -v dnf &>/dev/null; then
        "${elevate[@]}" dnf install -y \
            gcc gcc-c++ curl xz pkgconf-pkg-config python3 gettext clang-devel \
            gtk4-devel fontconfig-devel freetype-devel oniguruma-devel \
            mesa-libGL-devel libcxx-devel libcxxabi-devel libxml2-devel
    elif command -v pacman &>/dev/null; then
        "${elevate[@]}" pacman -S --needed --noconfirm \
            base-devel curl xz pkgconf python gettext clang gtk4 fontconfig freetype2 \
            oniguruma mesa libc++ libxml2
    else
        echo "ERROR: unsupported package manager; install GTK4, Clang, Fontconfig," >&2
        echo "Freetype, Oniguruma, OpenGL, libc++, libxml2, pkg-config, curl, and xz development packages." >&2
        exit 1
    fi
}

install_zig() {
    local zig_version zig_root platform metadata archive_name url checksum temp_dir archive
    zig_version="$(sed -n 's/.*\.minimum_zig_version = "\([^"]*\)".*/\1/p' \
        "$REPO_ROOT/ghostty/build.zig.zon" | head -1)"
    if [[ -z "$zig_version" ]]; then
        echo "ERROR: could not determine Ghostty's required Zig version." >&2
        exit 1
    fi

    case "$(uname -m)" in
        x86_64)
            platform="x86_64-linux"
            ;;
        aarch64|arm64)
            platform="aarch64-linux"
            ;;
        *)
            echo "ERROR: no Zig bootstrap is configured for $(uname -m)." >&2
            exit 1
            ;;
    esac

    zig_root="$REPO_ROOT/.tools/zig-$zig_version"
    if [[ -x "$zig_root/zig" ]] && [[ "$($zig_root/zig version)" == "$zig_version" ]]; then
        echo "==> Reusing repo-local Zig $zig_version" >&2
        printf '%s\n' "$zig_root"
        return
    fi

    metadata="$(curl -fsSL https://ziglang.org/download/index.json | python3 -c '
import json, sys
version, platform = sys.argv[1:]
entry = json.load(sys.stdin)[version][platform]
print(entry["tarball"], entry["shasum"])
' "$zig_version" "$platform")"
    read -r url checksum <<<"$metadata"
    archive_name="zig-$platform-$zig_version"

    echo "==> Installing repo-local Zig $zig_version..." >&2
    temp_dir="$(mktemp -d)"
    trap 'rm -rf "$temp_dir"' RETURN
    archive="$temp_dir/zig.tar.xz"
    curl -fL "$url" -o "$archive"
    printf '%s  %s\n' "$checksum" "$archive" | sha256sum --check - >&2
    mkdir -p "$REPO_ROOT/.tools"
    tar -xJf "$archive" -C "$temp_dir"
    rm -rf "$zig_root"
    mv "$temp_dir/$archive_name" "$zig_root"
    printf '%s\n' "$zig_root"
}

install_system_dependencies
echo "==> Initializing submodules..."
git -C "$REPO_ROOT" submodule update --init --recursive
ZIG_ROOT="$(install_zig)"

export PATH="$ZIG_ROOT:$PATH"
exec "$REPO_ROOT/scripts/setup-linux.sh"
