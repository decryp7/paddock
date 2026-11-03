#!/usr/bin/env bash
# Checks that a built sidecar agent reports the expected version in its `--self-test` report.
#   scripts/demo/check-agent-version.sh <agent-binary> <version>
# The report (paddock_proc::self_test) is a header line `<name> <version> self-test` followed by one `ok <check>` line per
# check, so only the header's second field is the version. On a mismatch the script prints the path, the exit status and
# the whole report (ER-27), then exits 1.
set -uo pipefail
agent="${1:?agent binary}"
want="${2:?expected version}"
if [ ! -x "$agent" ]; then
  echo "::error::the agent is not an executable file: $agent (expected version $want)"
  ls -la "$(dirname "$agent")" 2>&1 | head -20 || true
  exit 1
fi
report="$("$agent" --self-test 2>&1)"
status=$?
got="$(printf '%s\n' "$report" | head -n 1 | awk '{print $2}')"
if [ "$status" -ne 0 ] || [ "$got" != "$want" ]; then
  echo "::error::$agent --self-test exited $status and reports version '$got', expected '$want'. Report:"
  printf '%s\n' "$report"
  exit 1
fi
echo "agent $agent reports $got"
