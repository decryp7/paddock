#!/bin/bash
# Tests for no-pgrep-wait.sh (copied unchanged). Run: bash .claude/hooks/no-pgrep-wait.test.sh
here=$(cd "$(dirname "$0")" && pwd)
H="$here/no-pgrep-wait.sh"
fail=0
expect() { # name want command
  out=$(jq -nc --arg c "$3" '{tool_name:"Bash",tool_input:{command:$c}}' | "$H" 2>/dev/null)
  case "$out" in *'"deny"'*) got=deny ;; *) got=allow ;; esac
  if [ "$got" != "$2" ]; then echo "FAIL: $1 (want $2, got $got)"; fail=1; else echo "ok: $1"; fi
}
expect "while loop on the dash-f pattern" deny 'while pgrep -f "cargo build"; do sleep 5; done'
expect "until loop on the dash-f pattern" deny 'until ! pgrep -f "x y"; do sleep 2; done'
expect "multi-line loop" deny $'while pgrep -f "x"\ndo sleep 1\ndone'
expect "bundled flags" deny 'while pgrep -fl "x"; do sleep 1; done'
expect "one-shot lookup is allowed" allow 'pgrep -f "cargo build"'
expect "exact-name loop is allowed" allow 'while pgrep -x cargo; do sleep 1; done'
expect "dev wait is allowed" allow './dev wait --pid 123 --max 5m'
out=$(jq -nc '{tool_name:"Bash",tool_input:{command:"while pgrep -f x; do sleep 1; done"}}' | "$H"); case "$out" in *"dev wait"*) echo "ok: message names the next step" ;; *) echo "FAIL: message"; fail=1 ;; esac
exit $fail
