#!/usr/bin/env sh
# Points git at the repository's own hooks directory.
#
# `core.hooksPath` is per-clone and never committed, so this is opt-in and
# reversible: `git config --unset core.hooksPath` turns it off again.
set -eu
cd "$(dirname "$0")/.."
git config core.hooksPath .githooks
chmod +x .githooks/* 2>/dev/null || true
echo "pre-push hook enabled — it runs: sh scripts/ci.sh --quick"
echo "disable with: git config --unset core.hooksPath"
