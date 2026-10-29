#!/usr/bin/env bash
# The job's own temp folder and socket folder: the one implementation (ER-22). The job-tmpdir action runs it in a job without
# job-isolation (a Linux container); job-isolation's acquire and release run it on the host runners. Nothing else sets TMPDIR,
# TMP or TEMP (conformance/test/ci-actions.test.mjs).
#
#   job-tmpdir.sh setup    PADDOCK_JOB_TEMP and TMPDIR (TMP and TEMP on Windows) = the runner's per-job folder, $RUNNER_TEMP,
#                          which the runner deletes after the job; on Unix also PADDOCK_SOCKET_BASE, a short private folder
#                          (mktemp -d <root>/pd-XXXXXXXX, mode 700, its .owner naming the job's folder) for sockets whose path in
#                          the temp folder would be too long (Debrief's endpoints, debrief-ipc endpoint.rs). Written to
#                          $GITHUB_ENV. A second setup in the job reuses $PADDOCK_SOCKET_BASE when it passes the folder check.
#   job-tmpdir.sh remove   removes $PADDOCK_SOCKET_BASE (the runner removes its own folder), after the same check.
#   job-tmpdir.sh sweep    removes the pd-* folders under the root whose owner folder is gone (a job killed before its
#                          remove); job-isolation's acquire runs it.
#
# The folder check: a real folder (not a link), owned by this user, directly under the root and named pd-*. Anything else
# fails. Why the script exists: on a host runner the default temp folder is the runner user's own, which the developers' and
# agents' local runs on the same machine use too; a test or leak check there sees other runs' files (ER-14).
# PADDOCK_SOCKET_ROOT replaces /tmp in tests. The script runs under macOS' bash 3.2 (see the CI script check in
# conformance/test/ci-actions.test.mjs).
set -euo pipefail

mode="${1:-}"
root="${PADDOCK_SOCKET_ROOT:-/tmp}"
root="${root%/}"

# ours <folder>: a folder of this mechanism that may be used or removed.
ours() {
  local b="$1" rest
  [ -n "$b" ] && [ -d "$b" ] && [ ! -L "$b" ] && [ -O "$b" ] || return 1
  case "$b" in
    "$root"/pd-*) ;;
    *) return 1 ;;
  esac
  rest="${b#"$root"/}"
  case "$rest" in
    */*) return 1 ;;
  esac
  return 0
}

job_temp() {
  local t="${RUNNER_TEMP:?RUNNER_TEMP is not set (the per-job temp folder of the runner)}"
  if command -v cygpath >/dev/null 2>&1; then t="$(cygpath -u "$t")"; fi
  printf '%s\n' "${t%/}"
}

cmd_setup() {
  local t mixed native base
  t="$(job_temp)"
  [ -d "$t" ] || { echo "::error::job-tmpdir: the per-job temp folder $t of the runner does not exist" >&2; exit 1; }
  if [ "${RUNNER_OS:-}" = Windows ]; then
    mixed="$t"
    native="$t"
    if command -v cygpath >/dev/null 2>&1; then
      mixed="$(cygpath -m "$t")"
      native="$(cygpath -w "$t")"
    fi
    {
      echo "PADDOCK_JOB_TEMP=$mixed"
      echo "TMPDIR=$mixed"
      echo "TMP=$native"
      echo "TEMP=$native"
    } >> "${GITHUB_ENV:?GITHUB_ENV is not set}"
    echo "job-tmpdir: temp folders in $t" >&2
    return 0
  fi
  base="${PADDOCK_SOCKET_BASE:-}"
  if [ -n "$base" ]; then
    if ! ours "$base"; then
      echo "::error::job-tmpdir: PADDOCK_SOCKET_BASE=$base is not a folder of this job (a real folder of this user under $root, named pd-*)" >&2
      exit 1
    fi
  else
    # Atomic and private: mktemp -d makes the folder with mode 700 under a name nobody else holds.
    base="$(mktemp -d "$root/pd-XXXXXXXX")"
    printf '%s\n' "$t" > "$base/.owner"
  fi
  {
    echo "PADDOCK_JOB_TEMP=$t"
    echo "TMPDIR=$t"
    echo "PADDOCK_SOCKET_BASE=$base"
  } >> "${GITHUB_ENV:?GITHUB_ENV is not set}"
  echo "job-tmpdir: temp folders in $t; sockets in $base" >&2
}

cmd_remove() {
  [ "${RUNNER_OS:-}" != Windows ] || return 0
  local base="${PADDOCK_SOCKET_BASE:-}"
  [ -n "$base" ] || return 0
  if [ ! -e "$base" ] && [ ! -L "$base" ]; then return 0; fi
  if ! ours "$base"; then
    echo "::error::job-tmpdir: will not remove $base: not a folder of this job (a real folder of this user under $root, named pd-*)" >&2
    exit 1
  fi
  rm -rf "$base"
}

cmd_sweep() {
  [ "${RUNNER_OS:-}" != Windows ] || return 0
  local b owner
  for b in "$root"/pd-*; do
    ours "$b" || continue
    [ -f "$b/.owner" ] || continue
    owner="$(head -n 1 "$b/.owner")"
    if [ -n "$owner" ] && [ ! -e "$owner" ]; then
      rm -rf "$b"
      echo "job-tmpdir: removed $b (the job folder $owner is gone)" >&2
    fi
  done
}

case "$mode" in
  setup) cmd_setup ;;
  remove) cmd_remove ;;
  sweep) cmd_sweep ;;
  *)
    echo "::error::job-tmpdir: usage: job-tmpdir.sh setup, remove or sweep, not $mode" >&2
    exit 1
    ;;
esac
