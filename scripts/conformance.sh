#!/usr/bin/env bash
# Compare the Rust evaluator with official CUE on the conformance corpus.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
export CUE_BIN="${CUE_BIN:-cue}"
export TZ=UTC

if ! "$CUE_BIN" version >/dev/null 2>&1; then
  echo "official CUE not found. Run scripts/install-cue.sh v0.17.1" >&2
  exit 1
fi

VERSION="$("$CUE_BIN" version | head -1)"
MAJOR_MINOR="$(echo "$VERSION" | sed -E 's/.*v([0-9]+)\.([0-9]+)\.[0-9]+.*/\1.\2/')"
case "$MAJOR_MINOR" in
  0.17|0.18|0.19|0.2[0-9]) ;;
  *)
    echo "CUE oracle too old ($VERSION); require >= v0.17.1" >&2
    exit 1
    ;;
esac

cargo test -p ostinato --test conformance -- --nocapture
