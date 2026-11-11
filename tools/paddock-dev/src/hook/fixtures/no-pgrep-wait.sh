#!/bin/bash
# PreToolUse(Bash): refuse waits that poll `pgrep -f` in a while/until loop.
# `pgrep -f PATTERN` matches the waiting shell's own command line, so the loop never ends.
cmd=$(jq -r '.tool_input.command // ""')
if printf '%s' "$cmd" | tr '\n' ' ' | grep -Eq '(while|until)[^;]*pgrep[[:space:]]+(-[[:alnum:]]*[[:space:]]+)*-[[:alnum:]]*f'; then
  jq -n '{hookSpecificOutput:{hookEventName:"PreToolUse",permissionDecision:"deny",permissionDecisionReason:"Blocked: don'"'"'t wait with pgrep -f (it matches its own command line). Wait on the job'"'"'s own completion notification, plock'"'"'s '"'"'<name> done'"'"' line in that job'"'"'s output, or `./dev wait --pid/--plock/--file/--ci-run ... --max <duration>`."}}'
fi
exit 0
