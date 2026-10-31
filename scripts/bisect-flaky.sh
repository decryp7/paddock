#!/usr/bin/env bash
# Payload for `git bisect run`: judges a commit by running a flaky test n times (rules 57 and 58, ER-28).
#
#   git bisect start --first-parent <bad> <good>          # first the batch merge, then once more inside it
#   git bisect run scripts/bisect-flaky.sh <n> <test cmd...>
#
# Exit 0: the test passed n times in a row (good). Exit 1: it failed once (bad; the loop stops at the first failure).
# Exit 125: the commit cannot be tested (git skips it): BISECT_BUILD_CMD, when set, failed.
# Exit 2: usage error (git aborts the bisect, which is right: nothing was judged).
#
# Pick n from the failure rate p so that (1-p)^n is small: n = 3/p gives about 95% (p = 0.1 -> 30 runs).
# Cost per bisect is steps x n x test time. Use a private CARGO_TARGET_DIR per bisect and CARGO_BUILD_JOBS=2 (rule 3).
# No sleeps: each run starts right after the last one ended; wait inside the test on the condition (ER-12).
set -u

if [ "$#" -lt 2 ]; then
  echo "::error::usage: bisect-flaky.sh <n> <test cmd...>; next step: pass the repeat count, then the command" >&2
  exit 2
fi
n=$1
shift
case "$n" in
  '' | *[!0-9]* | 0)
    echo "::error::bisect-flaky.sh: n must be a positive integer, got '$n'; next step: pass e.g. 30" >&2
    exit 2
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
  if ! "$@"; then
    echo "bisect-flaky: FAIL on run $i of $n at $(git rev-parse --short HEAD 2>/dev/null || echo '?'): $*" >&2
    exit 1
  fi
  i=$((i + 1))
done
echo "bisect-flaky: $n of $n runs passed at $(git rev-parse --short HEAD 2>/dev/null || echo '?')" >&2
exit 0
