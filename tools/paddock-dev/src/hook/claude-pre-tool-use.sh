#!/bin/sh
# Claude Code PreToolUse guard (ADR 0083): the installed paddock-dev decides; until it is installed only `./dev hooks install` passes.
b=$(git -C "${CLAUDE_PROJECT_DIR:-.}" rev-parse --path-format=absolute --git-common-dir 2>/dev/null)/paddock/hooks/paddock-dev
[ -x "$b" ] && exec "$b" hook pre-tool-use
j=$(cat); case $j in *'"command":"./dev hooks install"'*|*'"command":"./dev hooks install --local"'*) exit 0;; esac
printf '%s\n' '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"Blocked: the guard hook is not installed. Run `./dev hooks install` (a binary built from origin/main; --local for a branch build) first. Only that command is allowed until then."}}'
