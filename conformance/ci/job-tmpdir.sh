#!/usr/bin/env bash
# The job's own temp folder and socket folder: the one implementation (ER-22). The job-tmpdir action runs it in a job without
# job-isolation (a Linux container); job-isolation's acquire and release run it on the host runners. Nothing else sets TMPDIR,
# TMP or TEMP (conformance/test/ci-actions.test.mjs).
#
#   job-tmpdir.sh setup    PADDOCK_JOB_TEMP and TMPDIR (TMP and TEMP on Windows) = the runner's per-job folder, $RUNNER_TEMP,
#                          which the runner deletes after the job; on Unix also PADDOCK_SOCKET_BASE, a short private folder
#                          (/tmp/pd-<number of this job>, mode 700) for sockets whose path in the temp folder would be too long
#                          (Debrief's endpoints, debrief-ipc endpoint.rs). Written to $GITHUB_ENV. A second setup keeps them.
#   job-tmpdir.sh remove   removes the socket folder (the runner removes its own folder).
#
# Why: on a host runner the default temp folder is the runner user's own, which the developers' and agents' local runs on
# the same machine use too; a test or leak check there sees other runs' files (ER-14). PADDOCK_SOCKET_ROOT replaces /tmp in
# tests.
set -euo pipefail

mode="${1:-}"
t="${RUNNER_TEMP:?RUNNER_TEMP is not set (the per-job temp folder of the runner)}"
if command -v cygpath >/dev/null 2>&1; then t="$(cygpath -u "$t")"; fi
t="${t%/}"
# A number for this job, short enough for a socket path (about 100 bytes): the checksum of its per-job folder's path.
id="$(printf '%s' "$t" | cksum)"
base="${PADDOCK_SOCKET_ROOT:-/tmp}/pd-${id%% *}"

# Windows spellings, computed here: macOS' bash 3.2 cannot parse $(...) inside a case branch.
mixed="$t"
native="$t"
if command -v cygpath >/dev/null 2>&1; then
  mixed="$(cygpath -m "$t")"
  native="$(cygpath -w "$t")"
fi

case "$mode" in
  setup)
    [ -d "$t" ] || { echo "::error::job-tmpdir: the runner's per-job temp folder $t does not exist" >&2; exit 1; }
    {
      if [ "${RUNNER_OS:-}" = Windows ]; then
        echo "PADDOCK_JOB_TEMP=$mixed"
        echo "TMPDIR=$mixed"
        echo "TMP=$native"
        echo "TEMP=$native"
      else
        mkdir -p "$base"
        chmod 700 "$base"
        echo "PADDOCK_JOB_TEMP=$t"
        echo "TMPDIR=$t"
        echo "PADDOCK_SOCKET_BASE=$base"
      fi
    } >> "${GITHUB_ENV:?GITHUB_ENV is not set}"
    if [ "${RUNNER_OS:-}" = Windows ]; then echo "job-tmpdir: temp folders in $t" >&2; else echo "job-tmpdir: temp folders in $t; sockets in $base" >&2; fi
    ;;
  remove)
    [ "${RUNNER_OS:-}" = Windows ] || rm -rf "$base"
    ;;
  *)
    echo "::error::job-tmpdir: usage: job-tmpdir.sh setup or remove, not '$mode'" >&2
    exit 1
    ;;
esac
