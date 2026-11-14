#!/bin/bash
# PreToolUse(Bash): WARN (never block, ER-36) when `git commit` or `git push` output is piped into grep, head, tail or a similar
# filter. A hook rejection (commit-msg, pre-push) is printed by those commands; a filter hides it (3 times on 2026-11-13).
# Output: {"hookSpecificOutput":{"hookEventName":"PreToolUse","additionalContext":...}} when it matches, nothing otherwise; exit 0.
cmd=$(jq -r '.tool_input.command // ""')
if printf '%s' "$cmd" | tr '\n' ' ' | sed -e 's/[0-9]*>&[0-9]//g' -e 's/&>[^ ]*//g' -e 's/\\//g' | grep -Eq '(^|[^[:alnum:]_-])git([[:space:]][^|;&]*)?[[:space:]](commit|push)([[:space:]][^|;&]*)?\|[[:space:]]*(grep|egrep|fgrep|rg|head|tail|sed|awk|cut|sort|uniq|wc)([[:space:]]|$)'; then
  jq -n '{hookSpecificOutput:{hookEventName:"PreToolUse",additionalContext:"Warning: this command pipes `git commit` or `git push` output into a filter (grep, head, tail ...). A rejection by a hook (commit-msg, pre-push) can be hidden that way. Next step: run the git command unfiltered and read its whole output, then check `git log -1` or `git status` for the result."}}'
fi
exit 0
