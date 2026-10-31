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
# Ending processes: `pkill -f` on macOS and Linux; on Windows (Git bash has no pkill, and its ps sees only MSYS programs)
# PowerShell over Win32_Process command lines (.NET regex, which reads these patterns the same way). Afterwards the leak
# check lists any process that still matches one of the job's patterns (a ::warning with its pid and command line).
# Drivers that live outside the job's folders (tauri-driver in ~/.cargo/bin, safaridriver, WebKitWebDriver) cannot be
# scoped by a path and are never ended by name (another job may own one): the suite that starts one stops it, on ports it
# chose for the run (Debrief's wdio.conf.ts), and a pattern can still find it through an argument that names the job's
# folder (tauri-driver --native-driver <checkout>/.drivers/msedgedriver.exe).
# PADDOCK_PKILL, PADDOCK_PS and PADDOCK_POWERSHELL override those programs (tests).
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

windows=0
case "${RUNNER_OS:-$(uname -s)}" in Windows | MINGW* | MSYS* | CYGWIN*) windows=1 ;; esac
ps_bin="${PADDOCK_POWERSHELL:-powershell.exe}"

# end_matching <regex>: ends every process whose command line matches.
end_matching() {
  if [ "$windows" = 1 ]; then
    # The pattern travels in the environment, so PowerShell's own command line never matches it.
    # shellcheck disable=SC2016  # PowerShell code, expanded by PowerShell
    PADDOCK_KILL_PATTERN="$1" "$ps_bin" -NoProfile -NonInteractive -Command \
      'Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -and $_.ProcessId -ne $PID -and $_.CommandLine -match $env:PADDOCK_KILL_PATTERN } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }' \
      >/dev/null 2>&1
  else
    "$pkill_bin" -f "$1" 2>/dev/null
  fi
  return 0
}

# survivors <regex>: "<pid> <command line>" of every process that still matches.
survivors() {
  local all
  if [ "$windows" = 1 ]; then
    # shellcheck disable=SC2016  # PowerShell code, expanded by PowerShell
    PADDOCK_KILL_PATTERN="$1" "$ps_bin" -NoProfile -NonInteractive -Command \
      'Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -and $_.ProcessId -ne $PID -and $_.CommandLine -match $env:PADDOCK_KILL_PATTERN } | ForEach-Object { "{0} {1}" -f $_.ProcessId, $_.CommandLine }' \
      2>/dev/null | tr -d '\r'
  else
    # The listing is taken first, so the grep below is not in it.
    all="$("${PADDOCK_PS:-ps}" -axo pid=,command= 2>/dev/null)" || return 0
    printf '%s\n' "$all" | grep -E -- "$1" || true
  fi
}

scoped_patterns=()
for p in ${patterns[@]+"${patterns[@]}"}; do
  [ -n "$p" ] || continue
  if scoped="$(bash "$iso" pattern "$p" 2>/dev/null)"; then
    end_matching "$scoped"
    scoped_patterns+=("$scoped")
  else
    echo "::warning::host-cleanup: skipped the process pattern '$p': it is not anchored on a folder of this job ({tmp}, {build} or {slot})"
  fi
done
# The leak check: what this job started and the clean-up could not end.
for scoped in ${scoped_patterns[@]+"${scoped_patterns[@]}"}; do
  left="$(survivors "$scoped")"
  if [ -n "$left" ]; then
    echo "::warning::host-cleanup: processes of this job are still running after the clean-up (pattern $scoped):"
    printf '%s\n' "$left" | cut -c1-300 | sed 's/^/  /'
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
