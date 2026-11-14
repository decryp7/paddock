#!/bin/bash
# Tests for git-output-filter-warn.sh. Run: bash .claude/hooks/git-output-filter-warn.test.sh
here=$(cd "$(dirname "$0")" && pwd)
H="$here/git-output-filter-warn.sh"
fail=0
expect() { # name want(warn|quiet) command
  out=$(jq -nc --arg c "$3" '{tool_name:"Bash",tool_input:{command:$c}}' | "$H" 2>/dev/null); rc=$?
  case "$out" in *'"deny"'*|*'"decision"'*) echo "FAIL: $1 (the hook must never block)"; fail=1; continue ;; esac
  case "$out" in *'Warning'*) got=warn ;; *) got=quiet ;; esac
  if [ "$got" != "$2" ] || [ "$rc" != 0 ]; then echo "FAIL: $1 (want $2, got $got, exit $rc)"; fail=1; else echo "ok: $1"; fi
}
expect "commit piped to grep" warn 'git commit -m "x" | grep -v skip'
expect "commit with stderr merged, piped to tail" warn 'git commit -m x 2>&1 | tail -5'
expect "push piped to head" warn 'git push origin main | head -3'
expect "git -C commit piped to grep" warn 'git -C /tmp/wt commit -q -m x | grep error'
expect "chained after a cd" warn 'cd /x && git add -A && git commit -m x 2>&1 | grep -iE "error|reject"'
expect "multi-line" warn $'git push origin b \\\n  | tail -2'
expect "plain commit" quiet 'git commit -m "x"'
expect "plain push" quiet 'git push origin b'
expect "log piped to head" quiet 'git log --oneline | head -3'
expect "status piped to grep" quiet 'git status --short | grep M'
expect "or-chain is not a pipe" quiet 'git commit -m x || echo failed'
expect "commit then a separate grep" quiet 'git commit -m x && git log -1 | grep x'
expect "redirect to a file" quiet 'git commit -m x > /tmp/out.txt 2>&1'
expect "tee is not a filter" quiet 'git push origin b 2>&1 | tee /tmp/push.log'
exit $fail
