#!/usr/bin/env bash
# Install the official CUE CLI from the upstream release tarball.
# Usage: scripts/install-cue.sh [version] [destination]
set -euo pipefail

VERSION="${1:-v0.17.1}"
DEST="${2:-$HOME/.local/bin}"

case "$(uname -s)" in
  Darwin) OS=darwin ;;
  Linux) OS=linux ;;
  *) echo "unsupported OS: $(uname -s)" >&2; exit 1 ;;
esac

case "$(uname -m)" in
  arm64|aarch64) ARCH=arm64 ;;
  x86_64|amd64) ARCH=amd64 ;;
  *) echo "unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac

# Pinned hashes for the release used in CI. Other versions still install, but
# are not checksum-verified.
EXPECTED=""
if [ "$VERSION" = "v0.17.1" ]; then
  case "${OS}-${ARCH}" in
    darwin-amd64) EXPECTED=80aa026c3f47400c7bfc228b3422fd56b7c6c5ea4d70686f8d8f01ced716d3de ;;
    darwin-arm64) EXPECTED=64921403f012a97f89494c03605db2fbf7d9daa77dc2631819ac4406cb2e8074 ;;
    linux-amd64)  EXPECTED=a39b0c97695069d95d276d99be0f5dbabb081d801bfdc9ba49b76efaf94e2369 ;;
    linux-arm64)  EXPECTED=0d729be30d52c952ca38fc9dcb692caa09d8463fa0b64df5781312779183fbcd ;;
  esac
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

URL="https://github.com/cue-lang/cue/releases/download/${VERSION}/cue_${VERSION}_${OS}_${ARCH}.tar.gz"
echo "downloading $URL"
curl -fsSL "$URL" -o "$TMP/cue.tgz"

if [ -n "$EXPECTED" ]; then
  if command -v sha256sum >/dev/null 2>&1; then
    echo "$EXPECTED  $TMP/cue.tgz" | sha256sum -c -
  else
    echo "$EXPECTED  $TMP/cue.tgz" | shasum -a 256 -c -
  fi
fi

tar -xzf "$TMP/cue.tgz" -C "$TMP"
mkdir -p "$DEST"
install -m 0755 "$TMP/cue" "$DEST/cue"

INSTALLED="$("$DEST/cue" version | head -1)"
case "$INSTALLED" in
  *"${VERSION#v}"*) ;;
  *) echo "installed CUE does not match requested $VERSION: $INSTALLED" >&2; exit 1 ;;
esac
echo "$INSTALLED"
echo "installed CUE $VERSION to $DEST/cue"
