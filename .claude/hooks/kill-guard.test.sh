#!/bin/bash
# shellcheck disable=SC2016  # the commands are fed to the hook unexpanded, on purpose
# Tests for kill-guard.sh. Run: bash .claude/hooks/kill-guard.test.sh
# The proof cases start their own `sleep` children (ended through $! at exit) in fake worktrees under a temp folder, and stand
# in for the Claude process with KILL_GUARD_ROOT_PID=$$ (this shell).
here=$(cd "$(dirname "$0")" && pwd)
H="$here/kill-guard.sh"
fail=0
base=$(mktemp -d "${KILL_GUARD_TEST_TMP:-${TMPDIR:-/tmp}}/kill-guard.XXXXXX") || exit 1
base=$(cd "$base" && pwd -P)
own="$base/wt/own" other="$base/wt/other" main="$base/main"
mkdir -p "$own/sub" "$other" "$main/.git"
echo "gitdir: $main/.git/worktrees/own" >"$own/.git"
echo "gitdir: $main/.git/worktrees/other" >"$other/.git"
(cd "$own/sub" && exec sleep 300) & mine=$!
(cd "$other" && exec sleep 300) & theirs=$!
(cd "$main" && exec sleep 300) & inmain=$!
(exec sleep 300) & lone=$!  # stands in for another Claude session
disown -a  # no "Terminated" notes at exit
trap 'kill "$mine" "$theirs" "$inmain" "$lone" 2>/dev/null; rm -rf "$base"' EXIT
export KILL_GUARD_ROOT_PID=$$

expect() { # name want who cwd command
  if [ "$3" = agent ]; then x='{"agent_id":"a1"}'; else x='{}'; fi
  out=$(jq -nc --arg c "$5" --arg d "$4" --argjson x "$x" '{tool_name:"Bash",cwd:$d,tool_input:{command:$c}}+$x' | "$H" 2>/dev/null)
  case "$out" in *'"deny"'*) got=deny ;; *) got=allow ;; esac
  if [ "$got" != "$2" ]; then echo "FAIL: $1 (want $2, got $got)"; fail=1; else echo "ok: $1"; fi
}

# --- by name: never provable
expect "pkill by pattern" deny agent "$own" 'pkill -f lefthook'
expect "pkill by name" deny agent "$own" 'pkill cargo'
expect "killall" deny agent "$own" 'killall node'
expect "sudo killall" deny agent "$own" 'sudo killall -9 node'
expect "/usr/bin/pkill" deny agent "$own" '/usr/bin/pkill -9 lefthook'
expect "pgrep piped to xargs kill" deny agent "$own" 'pgrep -f lefthook | xargs kill'
expect "ps piped to xargs kill -9" deny agent "$own" "ps aux | grep cargo | awk '{print \$2}' | xargs kill -9"
expect "kill of a pgrep substitution" deny agent "$own" 'kill $(pgrep -f lefthook)'
expect "kill of a backtick substitution" deny agent "$own" 'kill -9 `cat pidfile`'
expect "kill of a variable from a substitution" deny agent "$own" 'p=$(pgrep x); kill $p'
expect "kill of a loop variable over pgrep" deny agent "$own" 'for p in $(pgrep x); do kill $p; done'
expect "kill inside bash -c" deny agent "$own" "bash -c 'kill 34522'"
# --- literal pids the hook cannot prove
expect "kill of an unrelated pid (launchd, pid 1)" deny agent "$own" 'kill 1'
expect "kill -9 of another agent's pid" deny agent "$own" "cd $own && kill -9 $theirs"
expect "kill of a pid in the main checkout" deny agent "$main" "kill $inmain"
expect "own pid, but the agent is in the main checkout" deny agent "$main" "kill $mine"
expect "kill of a pid that does not exist" deny agent "$own" 'kill 999999'
expect "kill of a variable read from elsewhere" deny agent "$own" 'kill $PID'
expect "kill -s TERM of another agent's pid" deny agent "$own" "kill -s TERM $theirs"
expect "kill of a process group" deny agent "$own" "kill -- -$mine"
expect "kill 0 (the whole group)" deny agent "$own" 'kill 0'
expect "kill -9 -1 (everything)" deny agent "$own" 'kill -9 -1'
expect "mixed: one own, one other" deny agent "$own" "kill $mine $theirs"
KILL_GUARD_ROOT_PID=$lone expect "own worktree, but not from this Claude session" deny agent "$own" "kill $mine"
# --- allowed: proven own processes and job references
expect "kill %1" allow agent "$own" 'kill %1'
expect "kill -9 %build" allow agent "$own" 'kill -9 %build'
expect "kill \$! in the command that started it" allow agent "$own" 'sleep 100 & kill $!'
expect "kill of a variable set from \$!" allow agent "$own" 'sleep 100 & pid=$!; kill -TERM "$pid"'
expect "kill of a descendant whose cwd is in the agent's worktree" allow agent "$own" "kill $mine"
expect "same, from a cd into the worktree" allow agent "$main" "cd $own && kill -9 $mine"
expect "same, from a subfolder of the worktree" allow agent "$own/sub" "kill -s KILL $mine"
expect "kill -0 is a probe, not a kill" allow agent "$own" 'kill -0 34522'
expect "kill -l lists signals" allow agent "$own" 'kill -l'
expect "./dev runs through paddock_proc" allow agent "$own" './dev wait --pid 34522 --max 5m'
expect "kill in a commit message is not a kill" allow agent "$own" 'git commit -m "fix: kill 34522 safely"'
expect "pkill in an echo is not a kill" allow agent "$own" 'echo pkill -f x'
# --- the coordinator (no agent_id) is not refused here
expect "coordinator may pkill" allow coordinator "$own" 'pkill -f my-server'
expect "coordinator may kill a pid" allow coordinator "$own" 'kill 1234'

# The refusal names the rule and the next step.
out=$(jq -nc --arg c 'pkill -f lefthook' '{tool_name:"Bash",agent_id:"a",tool_input:{command:$c}}' | "$H")
case "$out" in *"rule 53"*"Next step"*TaskStop*) echo "ok: message names rule 53 and the next step" ;; *) echo "FAIL: message ($out)"; fail=1 ;; esac
# A non-Bash tool and bad input are allowed (never fails closed).
out=$(printf 'not json' | "$H" 2>/dev/null); case "$out" in *deny*) echo "FAIL: bad input denied"; fail=1 ;; *) echo "ok: bad input allowed" ;; esac
exit $fail
