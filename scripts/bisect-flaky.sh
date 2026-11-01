#!/usr/bin/env bash
# Payload for `git bisect run`: judges a commit by running a flaky test n times (rules 57 and 58, ER-28).
#
#   git bisect start --first-parent <bad> <good>          # first the batch merge, then once more inside it
#   git bisect run scripts/bisect-flaky.sh <n> <test cmd...>
#
# Exit 0: the test passed n times in a row (good). Exit 1: it failed once (bad; the loop stops at the first failure).
# Exit 125: the commit cannot be tested (git skips it): BISECT_BUILD_CMD, when set, failed, or the test command exited
# 126/127 (cannot execute / not found at this commit, for example a script that does not exist yet).
# Exit 255: usage error, or the script was interrupted (INT/TERM). git bisect run aborts on 255 (any code 128..255), which is
# right: nothing was judged. Never exit 2 for this: git bisect run reads every code 1..124 except 125 as "bad".
#
# Pick n from rule 57's formula: n = max(1, ceil(ln 0.05 / ln(1-p))) with p measured on the failing run (p = 0.1 -> 29 runs).
# Cost per bisect is steps x n x test time. Use a private CARGO_TARGET_DIR per bisect and CARGO_BUILD_JOBS=2 (rule 3).
# No sleeps: each run starts right after the last one ended; wait inside the test on the condition (ER-12).
set -u
trap 'echo "::error::bisect-flaky.sh interrupted; next step: git bisect reset, then start again" >&2; exit 255' INT TERM

if [ "$#" -lt 2 ]; then
  echo "::error::usage: bisect-flaky.sh <n> <test cmd...>; next step: pass the repeat count, then the command" >&2
  exit 255
fi
n=$1
shift
case "$n" in
  '' | *[!0-9]* | 0)
    echo "::error::bisect-flaky.sh: n must be a positive integer, got '$n'; next step: pass e.g. 30" >&2
    exit 255
    ;;
esac

# A bare command name must exist now; a path may legitimately be missing at an old commit (127 below skips that commit).
case "$1" in
  */*) ;;
  *)
    if ! command -v "$1" >/dev/null 2>&1; then
      echo "::error::bisect-flaky.sh: test command '$1' not found; next step: pass an executable or a path" >&2
      exit 255
    fi
    ;;
esac

if [ -n "${BISECT_BUILD_CMD:-}" ]; then
  if ! sh -c "$BISECT_BUILD_CMD"; then
    echo "bisect-flaky: build failed at $(git rev-parse --short HEAD 2>/dev/null || echo '?'); skipping this commit" >&2
    exit 125
  fi
fi

i=1
while [ "$i" -le "$n" ]; do
  "$@"
  rc=$?
  if [ "$rc" -eq 126 ] || [ "$rc" -eq 127 ]; then
    echo "bisect-flaky: cannot run the test command at $(git rev-parse --short HEAD 2>/dev/null || echo '?') (exit $rc); skipping" >&2
    exit 125
  fi
  if [ "$rc" -ne 0 ]; then
    echo "bisect-flaky: FAIL on run $i of $n at $(git rev-parse --short HEAD 2>/dev/null || echo '?'): $*" >&2
    exit 1
  fi
  i=$((i + 1))
done
echo "bisect-flaky: $n of $n runs passed at $(git rev-parse --short HEAD 2>/dev/null || echo '?')" >&2
exit 0
