#!/usr/bin/env bash
# Download the latest Schisaciaccole macOS release from GitHub, install it into
# /Applications and strip the quarantine attribute so Gatekeeper lets the
# unsigned app launch.
#
# Usage:
#   ./install-macos.sh            # latest release
#   ./install-macos.sh v1.2.3     # specific tag
set -euo pipefail

REPO="JustCris654/schisaciaccole"
APP_NAME="Schisaciaccole.app"
DEST="/Applications/${APP_NAME}"
TAG="${1:-}"

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "This script only runs on macOS." >&2
    exit 1
fi

if [[ "$(uname -m)" != "arm64" ]]; then
    echo "Only an arm64 (Apple Silicon) build is published; this Mac is $(uname -m)." >&2
    exit 1
fi

if [[ -n "$TAG" ]]; then
    api="https://api.github.com/repos/${REPO}/releases/tags/${TAG}"
else
    api="https://api.github.com/repos/${REPO}/releases/latest"
fi

echo "Looking up release: ${TAG:-latest}"
url="$(curl -fsSL "$api" \
    | grep -o '"browser_download_url": *"[^"]*-macos-arm64\.zip"' \
    | head -n 1 \
    | sed -E 's/.*"(https[^"]+)"/\1/')"

if [[ -z "$url" ]]; then
    echo "No macos-arm64.zip asset found in release ${TAG:-latest}." >&2
    exit 1
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "Downloading ${url}"
curl -fL --progress-bar -o "$tmp/app.zip" "$url"

ditto -x -k "$tmp/app.zip" "$tmp/extracted"

app="$(find "$tmp/extracted" -maxdepth 2 -name '*.app' -type d | head -n 1)"
if [[ -z "$app" ]]; then
    echo "No .app found inside the downloaded archive." >&2
    exit 1
fi

# Writing to /Applications may need admin rights.
SUDO=""
if [[ ! -w "/Applications" ]] || { [[ -e "$DEST" ]] && [[ ! -w "$DEST" ]]; }; then
    SUDO="sudo"
fi

if [[ -e "$DEST" ]]; then
    echo "Replacing existing ${DEST}"
    $SUDO rm -rf "$DEST"
fi

$SUDO ditto "$app" "$DEST"
$SUDO xattr -dr com.apple.quarantine "$DEST"

echo "Installed ${DEST}"
echo "Launch with: open -a Schisaciaccole or search in the application launcher 'Schisaciaccole'"
