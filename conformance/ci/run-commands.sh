#!/usr/bin/env bash
# Runs an app's paddock.ci.json command list (one command per line; a `cd` line holds for the lines after it) and names the
# command that failed, with its exit code, so a red step says which gate it was. Stops at the first failure, like `bash -e`.
#
#   COMMANDS=$'cd apps/x\nnpm test' bash run-commands.sh        (or: bash run-commands.sh "$COMMANDS")
set -uo pipefail
commands="${1-${COMMANDS-}}"
while IFS= read -r line || [ -n "$line" ]; do
  [ -n "${line//[[:space:]]/}" ] || continue
  case "$line" in
    cd\ *)
      echo "+ $line"
      eval "$line" || { echo "::error::command failed (exit $?): $line"; exit 1; }
      continue ;;
  esac
  echo "::group::$line"
  ( set -e; eval "$line" )
  code=$?
  echo "::endgroup::"
  if [ "$code" -ne 0 ]; then
    echo "::error::command failed (exit $code): $line (in $PWD)"
    exit "$code"
  fi
done <<< "$commands"
