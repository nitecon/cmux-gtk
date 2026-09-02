#!/usr/bin/env bash
set -euo pipefail

REPOSITORY="${CMUX_GITHUB_REPOSITORY:-nitecon/cmux-gtk}"
INSTALL_DIR="${CMUX_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${CMUX_VERSION:-latest}"

case "$(uname -m)" in
    x86_64|amd64) ARCH=x86_64 ;;
    aarch64|arm64) ARCH=aarch64 ;;
    *) echo "ERROR: unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac

ASSET="cmux-gtk-linux-${ARCH}.tar.gz"
if [[ "$VERSION" == latest ]]; then
    BASE_URL="https://github.com/${REPOSITORY}/releases/latest/download"
else
    VERSION="${VERSION#v}"
    BASE_URL="https://github.com/${REPOSITORY}/releases/download/v${VERSION}"
fi

TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TEMP_DIR"' EXIT

echo "==> Downloading ${ASSET}..."
curl -fL "${BASE_URL}/${ASSET}" -o "$TEMP_DIR/$ASSET"
curl -fL "${BASE_URL}/${ASSET}.sha256" -o "$TEMP_DIR/$ASSET.sha256"
(
    cd "$TEMP_DIR"
    sha256sum --check "$ASSET.sha256"
    mkdir payload
    tar -xzf "$ASSET" -C payload
)

for binary in cmux cmux-app; do
    candidate="$TEMP_DIR/payload/$binary"
    if [[ ! -x "$candidate" ]]; then
        echo "ERROR: release archive does not contain executable $binary" >&2
        exit 1
    fi
    "$candidate" --version >/dev/null
done

mkdir -p "$INSTALL_DIR"
for binary in cmux-app cmux; do
    install -m 0755 "$TEMP_DIR/payload/$binary" "$INSTALL_DIR/$binary"
done

echo "==> Installed cmux in $INSTALL_DIR"
echo "    Browser panes are optional. To enable them:"
echo "    npm install -g agent-browser && agent-browser install"
