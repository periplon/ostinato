#!/usr/bin/env bash
# Wait for CI, then squash-merge a pull request.
# Usage: scripts/auto-merge.sh <pr-number-or-url>
set -euo pipefail

PR="${1:?usage: auto-merge.sh <pr-number-or-url>}"
gh pr checks "$PR" --watch --interval 10
gh pr merge "$PR" --squash --delete-branch
