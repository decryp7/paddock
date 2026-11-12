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
#                          On macOS, when the runner's work folder is on an external volume (diskutil: Internal=false) the
#                          folder is $PADDOCK_INTERNAL_TMP_ROOT (default $HOME/gitea-runner/tmp)/<the runner's job hash> on the
#                          internal disk instead: TCC (Removable Volumes) denies processes that are their own responsible
#                          process (disclaimed children, `launchctl submit` jobs) the job folder on a USB disk, and Full Disk
#                          Access of the runner does not help (docs/lessons.md). The script logs the folder and the reason.
#   job-tmpdir.sh remove   removes $PADDOCK_SOCKET_BASE (the runner removes its own folder), after the same check, and the
#                          internal job folder (the runner does not know it).
#   job-tmpdir.sh sweep    removes the pd-* folders under the root whose owner folder is gone (a job killed before its
#                          remove), and the internal job folders (and their .owner files) whose runner folder is gone;
#                          job-isolation's acquire runs it.
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

internal_root() { printf '%s\n' "${PADDOCK_INTERNAL_TMP_ROOT:-${HOME:-/tmp}/gitea-runner/tmp}" | sed 's|/*$||'; }

# volume_is_external <folder>: the one rule, in boot-volume.sh (also used by Dyno's check-bundle.sh and paddock_testkit).
# shellcheck source-path=SCRIPTDIR source=boot-volume.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/boot-volume.sh"

# On a macOS host with an external work folder: the per-job folder on the internal disk (see the header). Prints the folder.
internal_job_temp() {
  local t="$1" iroot hash d
  iroot="$(internal_root)"
  hash="$(basename "$(dirname "$t")")"
  d="$iroot/$hash"
  mkdir -p "$iroot" && chmod 700 "$iroot" && mkdir -p "$d" && chmod 700 "$d" || {
    echo "::error::job-tmpdir: cannot make the internal job folder $d; make $iroot writable (or set PADDOCK_INTERNAL_TMP_ROOT)" >&2
    exit 1
  }
  printf '%s\n' "$t" > "$iroot/$hash.owner"
  printf '%s\n' "$d"
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
  if [ "${RUNNER_OS:-}" = macOS ] && volume_is_external "$t"; then
    local ext="$t"
    t="$(internal_job_temp "$ext")"
    echo "job-tmpdir: temp folder $t on the internal disk, not $ext: that volume is external and macOS (TCC, Removable Volumes) denies disclaimed processes and launchctl jobs there" >&2
  elif [ "${RUNNER_OS:-}" = macOS ]; then
    echo "job-tmpdir: temp folder $t is the runner's own: its volume is internal (or diskutil could not tell)" >&2
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
  local base="${PADDOCK_SOCKET_BASE:-}" j="${PADDOCK_JOB_TEMP:-}" iroot
  iroot="$(internal_root)"
  if [ -n "$j" ] && [ "${RUNNER_OS:-}" = macOS ] && [ -d "$j" ] && [ ! -L "$j" ] && [ "$(dirname "$j")" = "$iroot" ] && [ "$j" != "${RUNNER_TEMP:-}" ]; then
    # conformance: scoped-cleanup — $j is guarded above: a real directory, not a symlink, directly under the internal root and not RUNNER_TEMP
    rm -rf "$j" "$iroot/$(basename "$j").owner"
    echo "job-tmpdir: removed the internal job folder $j" >&2
  fi
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
  local b owner iroot f d
  iroot="$(internal_root)"
  for f in "$iroot"/*.owner; do
    [ -f "$f" ] || continue
    owner="$(head -n 1 "$f")"
    d="${f%.owner}"
    if [ -n "$owner" ] && [ ! -e "$owner" ]; then
      if [ -d "$d" ] && [ ! -L "$d" ]; then rm -rf "$d"; fi
      rm -f "$f"
      echo "job-tmpdir: removed the internal job folder $d (the runner folder $owner is gone)" >&2
    fi
  done
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
