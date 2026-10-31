#!/usr/bin/env bash
# Runs an app's paddock.ci.json command list (one command per line; a `cd` line holds for the lines after it) and names the
# command that failed, with its exit code, so a red step says which gate it was. Stops at the first failure, like `bash -e`.
# Each command is a timed log group; the end of the step lists what ran, what failed and what was skipped after a failure
# (ci-log.sh: in the log and in the job summary). CI_SUMMARY_TITLE names that table (default: "Commands"); CI_MASK names
# variables whose values never appear in what this prints (pass secrets through the environment, never on a command line).
#
#   COMMANDS=$'cd apps/x\nnpm test' bash run-commands.sh        (or: bash run-commands.sh "$COMMANDS")
set -uo pipefail
# shellcheck source=conformance/ci/ci-log.sh
. "$(dirname "${BASH_SOURCE[0]}")/ci-log.sh"
commands="${1-${COMMANDS-}}"
failed=0
while IFS= read -r line || [ -n "$line" ]; do
  [ -n "${line//[[:space:]]/}" ] || continue
  if [ "$failed" -ne 0 ]; then
    ci_skip "\`$line\`" "an earlier command failed"
    continue
  fi
  case "$line" in
    cd\ *)
      echo "+ $(_ci_scrub "$line")"
      eval "$line" || {
        code=$?
        ci_error "command failed ($(ci_describe_exit "$code")): $line (from $PWD)" "check that the folder exists in this checkout (a sparse checkout, or a renamed app in paddock.ci.json?)"
        _ci_row "| \`$(_ci_cell "$line")\` | **failed** | |"
        failed=1
      }
      continue ;;
  esac
  echo "::group::$(_ci_scrub "$line")"
  start=$(_ci_now_ms)
  # Not `( ... ) && code=0 || code=$?`: a subshell in an && / || list runs with -e off, so `false; true` would pass. This
  # script runs without -e, so a failing subshell does not end it.
  ( set -e; eval "$line" )
  code=$?
  took=$(ci_duration $(($(_ci_now_ms) - start)))
  echo "::endgroup::"
  if [ "$code" -ne 0 ]; then
    ci_error "command failed ($(ci_describe_exit "$code"), after $took): $line (in $PWD)" "the group above shows what broke; reproduce with: (cd $(printf '%q' "$PWD") && $line)"
    _ci_row "| \`$(_ci_cell "$line")\` | **failed**, $(ci_describe_exit "$code") | $took |"
    failed=$code
  else
    echo "ci: ok in $took: $(_ci_scrub "$line")"
    _ci_row "| \`$(_ci_cell "$line")\` | ok | $took |"
  fi
done <<< "$commands"
ci_summary_flush "${CI_SUMMARY_TITLE:-Commands}"
exit "$failed"
