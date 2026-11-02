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

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

URL="https://github.com/cue-lang/cue/releases/download/${VERSION}/cue_${VERSION}_${OS}_${ARCH}.tar.gz"
echo "downloading $URL"
curl -fsSL "$URL" -o "$TMP/cue.tgz"
tar -xzf "$TMP/cue.tgz" -C "$TMP"

mkdir -p "$DEST"
install -m 0755 "$TMP/cue" "$DEST/cue"
"$DEST/cue" version
echo "installed CUE $VERSION to $DEST/cue"
