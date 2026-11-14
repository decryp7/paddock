#!/bin/bash
# Runs every .claude/hooks/*.test.sh (ci.yml's tools job, `./dev ci run tools hooks`; report-only until its first green run, docs/gates.toml).
# PADDOCK_DEV is the built paddock-dev (the parity test runs it); the wrapper builds or fetches it when the job has not already.
# Every failing test prints one ::error:: line with the next step; the exit code is the number of failed test files, capped at 1.
here=$(cd "$(dirname "$0")" && pwd)
repo_root=$(cd "$here/../.." && pwd)
if [ -z "${PADDOCK_DEV:-}" ]; then
  PADDOCK_DEV=$("$repo_root/dev" --print-binary 2>/dev/null | tail -n 1) && [ -x "$PADDOCK_DEV" ] || {
    echo "::error::the hooks tests need paddock-dev and ./dev --print-binary named none. Fix: run ./dev once in $repo_root (it builds the tool), or set PADDOCK_DEV to a built binary."
    exit 1
  }
fi
export PADDOCK_DEV
fail=0 ran=0
for t in "$here"/*.test.sh; do
  ran=$((ran + 1))
  start=$SECONDS
  if bash "$t"; then echo "passed: ${t##*/} ($((SECONDS - start)) s)"
  else
    echo "::error::${t##*/} failed (output above). Fix: run 'bash .claude/hooks/${t##*/}' in the repository; a FAIL line names the case."
    fail=1
  fi
done
[ "$ran" -gt 0 ] || { echo "::error::no .claude/hooks/*.test.sh found in $here. Fix: the hooks folder moved; update .claude/hooks/run-tests.sh and ci.yml's hooks step."; exit 1; }
echo "hooks tests: $ran files, $([ $fail = 0 ] && echo all passed || echo some failed)"
exit $fail
