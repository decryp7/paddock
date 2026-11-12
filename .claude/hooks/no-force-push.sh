#!/bin/bash
# PreToolUse(Bash): refuse force pushes. History on shared branches is never rewritten;
# push a new branch instead (e.g. <branch>-v2) and say so in the report.
cmd=$(jq -r '.tool_input.command // ""')
if printf '%s' "$cmd" | tr '\n' ' ' | grep -Eq 'git([[:space:]]+-[[:alnum:]-]+([[:space:]]+[^[:space:]-][^[:space:]]*)?)*[[:space:]]+push([[:space:]]+[^;&|]*)?[[:space:]](-f|--force|--force-with-lease(=[^[:space:]]*)?|--force-if-includes)([[:space:]]|$)|git[[:space:]]+push[^;&|]*[[:space:]]\+[[:alnum:]_./-]+:'; then
  jq -n '{hookSpecificOutput:{hookEventName:"PreToolUse",permissionDecision:"deny",permissionDecisionReason:"Blocked: force pushes are never allowed (shared history is not rewritten). Push a new branch instead (for example <branch>-v2) and report it."}}'
fi
exit 0
