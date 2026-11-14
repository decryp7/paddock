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
# --- bypass shapes and false refusals: lib/shell-forms.tsv (run at the end); here only what needs this test's processes
expect "ANSI-C quoted kill of another agent's pid" deny agent "$own" "\$'\\x6bill' -9 $theirs"
expect "alias of kill on another agent's pid" deny agent "$own" "alias k=kill; k $theirs"
expect "heredoc fed to bash" deny agent "$own" "bash <<'X'
kill $theirs
X"
expect "ANSI-C quoted kill of an own process" allow agent "$own" "\$'\\x6bill' $mine"
# --- the real proof path: the session's process from CLAUDE_PID, and from the nearest `claude`-named ancestor
CLAUDE_PID=$$ KILL_GUARD_ROOT_PID='' expect "CLAUDE_PID: own descendant in the worktree" allow agent "$own" "kill $mine"
CLAUDE_PID=$lone KILL_GUARD_ROOT_PID='' expect "CLAUDE_PID: another session's process" deny agent "$own" "kill $mine"
# A symlink named `claude` to bash: the process is named after the link (a copy of a system bash is killed by macOS).
fake="$base/bin/claude"; mkdir -p "$base/bin"; ln -s "$(command -v bash)" "$fake"
check() { # name want got
  if [ "$3" = "$2" ]; then echo "ok: $1"; else echo "FAIL: $1 (want $2, got $3)"; fail=1; fi
}
viaclaude() { # want(mine|theirs): runs the hook under a process named `claude`, with its own child or another agent's pid
  out=$(KILL_GUARD_ROOT_PID='' CLAUDE_PID='' HOOK="$H" OWN="$own" THEIRS="$theirs" MINE="$mine" T="$1" "$fake" -c '
    if [ "$T" = theirs ]; then p=$THEIRS; elif [ "$T" = sibling ]; then p=$MINE; else (cd "$OWN" && exec sleep 300) >/dev/null 2>&1 & p=$!; fi
    jq -nc --arg c "kill $p" --arg d "$OWN" "{tool_name:\"Bash\",agent_id:\"a1\",cwd:\$d,tool_input:{command:\$c}}" | "$HOOK" 2>/dev/null
    [ "$T" = mine ] && kill "$p"
    :')
  case "$out" in *'"deny"'*) echo deny ;; *) echo allow ;; esac
}
check "claude ancestor: its own child in the worktree" allow "$(viaclaude mine)"
check "claude ancestor: another agent's process" deny "$(viaclaude theirs)"
# In the worktree but not under the nearest `claude` (it is the test shell's child): the nearest one is the session, not any above.
check "claude ancestor: the nearest claude is the session" deny "$(viaclaude sibling)"
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
# The shared shell-forms corpus: every scanner bypass and false refusal found so far, judged by every guard.
bash "$here/lib/shell-forms.sh" kill-guard "$H" || fail=1
exit $fail
