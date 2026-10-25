#!/usr/bin/env bash
# Leaves nothing behind on the Mac host runner: stray test processes, temp dirs and (optionally) build outputs.
#   scripts/ci/macos-cleanup.sh [--build-outputs]
# Only matches Pitwall's own names; never touches other jobs' processes or the runner itself.
set -uo pipefail
pkill -x pitwall-server 2>/dev/null || true
pkill -x llama-server 2>/dev/null || true
pkill -f 'fake[-_]engine' 2>/dev/null || true
rm -rf "${TMPDIR:-/tmp}"/pitwall-* 2>/dev/null || true
if [ "${1:-}" = "--build-outputs" ]; then
  rm -rf "${CARGO_TARGET_DIR:-}/dist" "${CARGO_TARGET_DIR:-}/debug" 2>/dev/null || true
  rm -rf dist app/dist app/coverage app/playwright-report 2>/dev/null || true
fi
exit 0
