#!/bin/bash
# Tests for worktree-guard.sh. Run: bash .claude/hooks/worktree-guard.test.sh
here=$(cd "$(dirname "$0")" && pwd)
H="$here/worktree-guard.sh"
MAIN=/Volumes/d8sst7/dev.decryptology.net/paddock
WT=/Volumes/d8sst7/dev-targets/wt/some-agent
fail=0
# who: agent|coord
run() { # who cwd command
  if [ "$1" = agent ]; then extra='{"agent_id":"a1","agent_type":"implementer"}'; else extra='{}'; fi
  jq -nc --arg c "$3" --arg d "$2" --argjson x "$extra" '{tool_name:"Bash",cwd:$d,tool_input:{command:$c}} + $x' | "$H" 2>/dev/null
}
expect() { # name want who cwd command
  out=$(run "$3" "$4" "$5")
  case "$out" in *'"deny"'*) got=deny ;; *) got=allow ;; esac
  if [ "$got" != "$2" ]; then echo "FAIL: $1 (want $2, got $got)"; fail=1; else echo "ok: $1"; fi
}
# --- unguarded cd (everyone)
expect "2026-11-12 shape: cd missing; git switch -c" deny agent "$WT" "cd /missing/worktree; git switch -c foo"
expect "2026-11-10 shape: cd train-wt; git merge" deny agent "$WT" "cd /Volumes/d8sst7/dev-targets/wt/train-wt; git merge fix/x"
expect "newline after cd is unguarded" deny coord "$MAIN" $'cd /x\ngit status'
expect "subshell ( cd X; ... ) is unguarded" deny agent "$WT" "( cd /x; git status )"
expect "cd X || echo; more is unguarded" deny agent "$WT" "cd /x || echo no; git status"
expect "cd X && ... is allowed" allow agent "$WT" "cd $WT && git status"
expect "cd X || exit 1; ... is allowed" allow agent "$WT" "cd $WT || exit 1; git status"
expect "cd X || return; ... is allowed" allow agent "$WT" "cd $WT || return; git status"
expect "a bare cd is allowed" allow agent "$WT" "cd $WT"
expect "cd X; with nothing after is allowed" allow agent "$WT" "cd $WT;"
expect "( cd X && ... ) is allowed" allow agent "$WT" "( cd $WT && git status )"
expect "a ; inside quotes is not a separator" allow agent "$WT" "cd $WT && echo 'a; b'"
expect "if cd X; then is not a bare cd" allow agent "$WT" "if cd $WT; then echo ok; fi"
expect "no cd at all" allow agent "$WT" "git status; git log -1"
# --- agent git writes in the main checkout
expect "agent git switch in main (cwd)" deny agent "$MAIN" "git switch -c foo"
expect "agent git commit via -C main" deny agent "$WT" "git -C $MAIN commit -m x"
expect "agent cd main && git merge" deny agent "$WT" "cd $MAIN && git merge fix/x"
expect "agent git push in main" deny agent "$MAIN" "git push origin main"
expect "agent git branch -D in main" deny agent "$MAIN" "git branch -D old"
expect "agent git worktree remove in main" deny agent "$MAIN" "git worktree remove /x"
expect "agent git tag v1 in main" deny agent "$MAIN" "git tag v1"
expect "agent fetch && reset in main" deny agent "$MAIN" "git fetch origin && git reset --hard origin/main"
expect "agent cherry-pick in main" deny agent "$MAIN" "git cherry-pick abc"
expect "agent git fetch in main is allowed" allow agent "$MAIN" "git fetch origin"
expect "agent git status/log/diff in main" allow agent "$MAIN" "git status && git log -3 && git diff HEAD~1"
expect "agent git worktree add in main" allow agent "$MAIN" "git worktree add /Volumes/d8sst7/dev-targets/wt/x -b b origin/main"
expect "agent git worktree list/lock in main" allow agent "$MAIN" "git worktree list && git worktree lock /x"
expect "agent git branch --list in main" allow agent "$MAIN" "git branch --list"
expect "agent git tag -l in main" allow agent "$MAIN" "git tag -l 'dyno-*'"
expect "agent git commit in its own worktree" allow agent "$WT" "git commit -m x"
expect "agent git -C own worktree commit" allow agent "$MAIN" "git -C $WT commit -m x"
expect "agent cd own worktree && merge" allow agent "$MAIN" "cd $WT && git merge x"
# --- the coordinator
expect "coordinator ff-merge in main" allow coord "$MAIN" "git merge --ff-only origin/main"
expect "coordinator push from main" allow coord "$MAIN" "git push origin main"
# --- fail open
out=$(echo 'not json' | "$H" 2>/dev/null); [ -z "$out" ] && echo "ok: bad input allows" || { echo "FAIL: bad input"; fail=1; }
out=$(jq -nc '{tool_name:"Bash",cwd:"/x",agent_id:"a",tool_input:{command:"git commit -m \"unterminated"}}' | "$H" 2>/dev/null); [ -z "$out" ] && echo "ok: odd quoting allows" || { echo "FAIL: odd quoting"; fail=1; }
# --- messages name the cause and the next step
out=$(run agent "$WT" "cd /missing; git status")
case "$out" in *"unguarded cd"*"cd /missing && ..."*) echo "ok: cd message" ;; *) echo "FAIL: cd message"; fail=1 ;; esac
out=$(run agent "$MAIN" "git commit -m x")
case "$out" in *"main checkout"*"your own worktree"*) echo "ok: main-checkout message" ;; *) echo "FAIL: main message"; fail=1 ;; esac
exit $fail
