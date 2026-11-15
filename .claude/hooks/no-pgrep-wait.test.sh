#!/bin/bash
# Tests for no-pgrep-wait.sh (copied unchanged). Run: bash .claude/hooks/no-pgrep-wait.test.sh
here=$(cd "$(dirname "$0")" && pwd)
H="$here/no-pgrep-wait.sh"
fail=0
TMP=$(mktemp -d "${TMPDIR:-/tmp}/no-pgrep-wait.XXXXXX") || exit 1
trap 'rm -rf "${TMP:?}"' EXIT
# No `dev wait` runs unless a case says so: the running-waits table is this empty file.
: >"$TMP/ps-none"
export NO_PGREP_WAIT_PS_FILE="$TMP/ps-none"
expect() { # name want(deny|warn|allow) command
  out=$(jq -nc --arg c "$3" '{tool_name:"Bash",tool_input:{command:$c}}' | "$H" 2>/dev/null); rc=$?
  case "$out" in *'"deny"'*) got=deny ;; *'"additionalContext"'*) got=warn ;; *) got=allow ;; esac
  if [ "$got" != "$2" ] || [ "$rc" != 0 ]; then echo "FAIL: $1 (want $2, got $got, exit $rc)"; fail=1; else echo "ok: $1"; fi
}
expect "while loop on the dash-f pattern" deny 'while pgrep -f "cargo build"; do sleep 5; done'
expect "until loop on the dash-f pattern" deny 'until ! pgrep -f "x y"; do sleep 2; done'
expect "multi-line loop" deny $'while pgrep -f "x"\ndo sleep 1\ndone'
expect "bundled flags" deny 'while pgrep -fl "x"; do sleep 1; done'
expect "one-shot lookup is allowed" allow 'pgrep -f "cargo build"'
expect "exact-name loop is allowed" allow 'while pgrep -x cargo; do sleep 1; done'
expect "dev wait is allowed" allow './dev wait --pid 123 --max 5m'
expect "a loop written into a heredoc is text, not a wait" allow $'cat > /x/notes.md <<\'EOF\'\nwhile pgrep -f x; do sleep 1; done\nEOF'
expect "multi-line grep poll warns" warn $'while ! grep -q ok /x/log\ndo\n  sleep 3\ndone'
out=$(jq -nc '{tool_name:"Bash",tool_input:{command:"while pgrep -f x; do sleep 1; done"}}' | "$H"); case "$out" in *"dev wait"*) echo "ok: message names the next step" ;; *) echo "FAIL: message"; fail=1 ;; esac
out=$(jq -nc '{tool_name:"Bash",tool_input:{command:"while ! grep -q ok /x/log; do sleep 1; done"}}' | "$H")
case "$out" in *'./dev wait --file /x/log --line'*) echo "ok: the grep warning names the dev wait that replaces it" ;; *) echo "FAIL: grep warning text: $out"; fail=1 ;; esac
out=$(jq -nc '{tool_name:"Bash",tool_input:{command:"while ! curl -s localhost:8080 | grep -q ok; do sleep 1; done"}}' | "$H")
case "$out" in *'--file localhost'*|*'--file -s'*) echo "FAIL: a curl pipe is no file: $out"; fail=1 ;; *'bounded wait'*) echo "ok: a grep on a curl pipe suggests a bounded wait, not --file" ;; *) echo "FAIL: curl warning text: $out"; fail=1 ;; esac
# A `dev wait` already running for the same target (from `ps`) warns; one for another target does not.
printf '%s\n' '  4100 /d/paddock-dev wait --ci-run 8112 --max 1h' '  4101 /d/paddock-dev wait --file /x/other --line y --max 5m' >"$TMP/ps-one"
NO_PGREP_WAIT_PS_FILE="$TMP/ps-one" expect "a second wait for a ci run already waited on warns" warn './dev wait --ci-run 8112 --max 1h'
NO_PGREP_WAIT_PS_FILE="$TMP/ps-one" expect "a wait for another file is quiet" allow './dev wait --file /x/log --line y --max 5m'
out=$(jq -nc '{tool_name:"Bash",tool_input:{command:"./dev wait --ci-run 8112 --max 1h"}}' | NO_PGREP_WAIT_PS_FILE="$TMP/ps-one" "$H")
case "$out" in *'pid 4100'*) echo "ok: the running wait is named by pid" ;; *) echo "FAIL: running-wait text: $out"; fail=1 ;; esac
# The process table cannot be read: the hook stays quiet about running waits and never blocks.
NO_PGREP_WAIT_PS_FILE="$TMP/missing" expect "an unreadable process table allows" allow './dev wait --ci-run 8112 --max 1h'
bash "$here/lib/shell-forms.sh" no-pgrep-wait "$H" || fail=1
exit $fail
