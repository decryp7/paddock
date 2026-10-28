#!/usr/bin/env bash
# Leaves nothing of this job behind on a host runner (plan X47 / X10 host-cleanup action; Pitwall's macos-cleanup.sh): stray
# test processes, temp folders and (optionally) build outputs. Job isolation (.gitea/actions/README.md): it touches only
# this job's own processes and folders, so any number of jobs can run on the host at once.
#
#   bash paddock/tools/host-cleanup.sh --id pitwall [--process-pattern '{build}[/\\]debug[/\\]pitwall-server' ...]
#        [--temp-prefix pitwall-it-] [--build-outputs]
#
# --process-pattern: a `pkill -f` pattern anchored on a folder of this job: {tmp} (the job's own temp folder, the runner's
# per-job $RUNNER_TEMP), {build} (its build folder) or {slot} (its job slot, in a kept folder's name), filled in by
# conformance/ci/job-isolation.sh. A pattern that names none of them, or whose folder this job does not have, could match
# another job's processes: it is skipped with a warning. There are no exact process names (they match every job's).
# Temp folders: <job temp>/<prefix>* (default prefix: the id and a dash), only in the job's own temp folder
# (PADDOCK_JOB_TEMP, set by the job-isolation action); without one the sweep is skipped. --build-outputs removes
# $CARGO_TARGET_DIR/{dist,debug} (only when CARGO_TARGET_DIR is set) and dist, app/dist, app/coverage,
# app/playwright-report in the current folder. Always exits 0 (a cleanup must not fail a job).
# PADDOCK_PKILL overrides the pkill program (tests).
set -uo pipefail

id=""; build=0; prefix=""
patterns=()
while [ $# -gt 0 ]; do
  case "$1" in
    --process-pattern) patterns+=("${2:-}"); shift ;;
    --temp-prefix) prefix="${2:-}"; shift ;;
    --build-outputs) build=1 ;;
    --id) id="${2:-}"; shift ;;
    *) echo "host-cleanup: unknown option $1" >&2; exit 2 ;;
  esac
  shift
done
[[ "$id" =~ ^[a-z][a-z0-9-]*$ ]] || { echo "host-cleanup: --id <app id> is needed (lower-case letters, digits, dashes)" >&2; exit 2; }
[ -n "$prefix" ] || prefix="$id-"
case "$prefix" in */* | "" | . | ..) echo "host-cleanup: the temp prefix must be a plain name start" >&2; exit 2 ;; esac
pkill_bin="${PADDOCK_PKILL:-pkill}"
iso="$(dirname "$0")/../conformance/ci/job-isolation.sh"

for p in ${patterns[@]+"${patterns[@]}"}; do
  [ -n "$p" ] || continue
  if scoped="$(bash "$iso" pattern "$p" 2>/dev/null)"; then
    "$pkill_bin" -f "$scoped" 2>/dev/null
  else
    echo "::warning::host-cleanup: skipped the process pattern '$p': it is not anchored on a folder of this job ({tmp}, {build} or {slot})"
  fi
done
tmpdir="${PADDOCK_JOB_TEMP:-}"
if [ -n "$tmpdir" ] && [ -d "$tmpdir" ]; then
  for d in "$tmpdir/$prefix"*; do
    [ -e "$d" ] && [ ! -L "$d" ] && rm -rf "$d" 2>/dev/null
  done
else
  echo "host-cleanup: no temp folder of this job (PADDOCK_JOB_TEMP); temp folders are left to the runner"
fi
if [ "$build" = 1 ]; then
  if [ -n "${CARGO_TARGET_DIR:-}" ]; then rm -rf "$CARGO_TARGET_DIR/dist" "$CARGO_TARGET_DIR/debug" 2>/dev/null; fi
  rm -rf dist app/dist app/coverage app/playwright-report 2>/dev/null
fi
exit 0
