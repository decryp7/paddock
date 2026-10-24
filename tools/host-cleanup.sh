#!/usr/bin/env bash
# Leaves nothing behind on a host runner (plan X47 / X10 host-cleanup action; Pitwall's macos-cleanup.sh): stray test
# processes, temp folders and (optionally) build outputs.
#
#   bash paddock/tools/host-cleanup.sh --id pitwall [--process pitwall-server --process llama-server ...]
#        [--process-pattern 'fake[-_]engine' ...] [--temp-prefix pitwall-] [--build-outputs]
#
# Only the names given are matched: `pkill -x <exact name>` for --process, `pkill -f <pattern>` for --process-pattern, and
# $TMPDIR/<prefix>* (default prefix: the id and a dash) for temp folders. It never touches other jobs' processes or the runner
# itself. --build-outputs removes $CARGO_TARGET_DIR/{dist,debug} (only when CARGO_TARGET_DIR is set) and dist, app/dist,
# app/coverage, app/playwright-report in the current folder. Always exits 0 (a cleanup must not fail a job).
# PADDOCK_PKILL overrides the pkill program (tests).
set -uo pipefail

id=""; build=0; prefix=""
procs=(); patterns=()
while [ $# -gt 0 ]; do
  case "$1" in
    --id) id="${2:-}"; shift ;;
    --process) procs+=("${2:-}"); shift ;;
    --process-pattern) patterns+=("${2:-}"); shift ;;
    --temp-prefix) prefix="${2:-}"; shift ;;
    --build-outputs) build=1 ;;
    *) echo "host-cleanup: unknown option $1" >&2; exit 2 ;;
  esac
  shift
done
[[ "$id" =~ ^[a-z][a-z0-9-]*$ ]] || { echo "host-cleanup: --id <app id> is needed (lower-case letters, digits, dashes)" >&2; exit 2; }
[ -n "$prefix" ] || prefix="$id-"
case "$prefix" in */* | "" | . | ..) echo "host-cleanup: the temp prefix must be a plain name start" >&2; exit 2 ;; esac
pkill_bin="${PADDOCK_PKILL:-pkill}"

for p in ${procs[@]+"${procs[@]}"}; do
  [ -n "$p" ] && "$pkill_bin" -x "$p" 2>/dev/null
done
for p in ${patterns[@]+"${patterns[@]}"}; do
  [ -n "$p" ] && "$pkill_bin" -f "$p" 2>/dev/null
done
tmpdir="${TMPDIR:-/tmp}"
for d in "$tmpdir/$prefix"*; do
  [ -e "$d" ] && [ ! -L "$d" ] && rm -rf "$d" 2>/dev/null
done
if [ "$build" = 1 ]; then
  if [ -n "${CARGO_TARGET_DIR:-}" ]; then rm -rf "$CARGO_TARGET_DIR/dist" "$CARGO_TARGET_DIR/debug" 2>/dev/null; fi
  rm -rf dist app/dist app/coverage app/playwright-report 2>/dev/null
fi
exit 0
