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

# ---- the reviewer's cases (2026-11-12): every line must match
t() { # want who cwd name cmd
  if [ "$2" = agent ]; then x='{"agent_id":"a1"}'; else x='{}'; fi
  out=$(jq -nc --arg c "$5" --arg d "$3" --argjson x "$x" '{tool_name:"Bash",cwd:$d,tool_input:{command:$c}}+$x' | "$H" 2>&1)
  case "$out" in *'"deny"'*) g=deny;; *Traceback*) g=CRASH;; *) g=allow;; esac
  if [ "$g" = "$1" ]; then echo "ok: $4"; else echo "FAIL: $4 (want $1, got $g)"; fail=1; fi
}
t allow coord $MAIN "heredoc with cd lines" $'cat > /x/s.sh <<\'EOF\'\ncd /a\nmake\nEOF\nchmod +x /x/s.sh'
t allow coord $MAIN "unquoted heredoc with cd" $'cat > /x/s.sh <<EOF\ncd $HOME\nls\nEOF'
t allow coord $MAIN "git commit -m heredoc" $'git commit -m "$(cat <<\'EOF\'\nfix: x\n\ncd foo; bar\nEOF\n)"'
t allow coord $MAIN "cd in single quotes" "echo 'cd /x; ls'"
t allow coord $MAIN "comment mentions cd" $'ls # then cd /x; ls\necho done'
t allow coord $MAIN "sh -c with cd; (inner, FN accepted)" "sh -c 'cd /x; make'"
t allow coord $MAIN "bash -lc with cd && " "bash -lc 'cd /x && make'"
t allow coord $MAIN "cd X || { echo; exit 1; }" "cd /x || { echo no; exit 1; }; make"
t allow coord $MAIN "set -e then cd newline" $'set -e\ncd /x\nmake'
t deny  coord $MAIN "for loop cd; (rule says deny)" "for d in a b; do cd \$d; make; cd ..; done"
t deny  coord $MAIN "cd X && a; b  (b runs in wrong dir)" "cd /missing && git fetch; git merge x"
t allow coord $MAIN "pushd ... (not covered)" "pushd /x; make; popd"
t allow coord $MAIN "cd X 2>/dev/null || exit 1; ls" "cd /x 2>/dev/null || exit 1; ls"
t deny  coord $MAIN "if ...; then cd X; git merge; fi" "if true; then cd /x; git merge y; fi"
t allow coord $MAIN "python -c with ; and cd word" "python3 -c 'import os; os.chdir(\"/x\"); print(1)'"
t allow coord $MAIN "awk with cd text" "awk '/cd /{print}; END{print 1}' f"
t allow coord $MAIN "jq filter with ; and (" "jq -r '.a | (.b; .c)' f; echo"
t allow coord $MAIN "case ... ) inside" $'case $x in a) cd /x && ls ;; esac'
t allow coord $MAIN "arith ((  ))" "(( i++ )); cd /x && ls"
t allow coord $MAIN "\$(cd X && pwd)" "D=\$(cd /x && pwd); echo \$D"
t deny  coord $MAIN "\$(cd X; pwd) unguarded" "D=\$(cd /x; pwd); echo \$D"
t allow coord $MAIN "trailing comment after cd" "cd /x # go there"
t allow coord $MAIN "cd then newline comment only" $'cd /x\n# done'
t deny  agent $MAIN "agent: git -C quoted main commit" "git -C \"$MAIN\" commit -m x"
t deny  agent $MAIN "agent: env VAR=1 git push" "env GIT_SSH=x git push origin y"
t deny  agent $MAIN "agent: VAR=1 git merge (main cwd)" "GIT_TRACE=1 git merge x"
t deny  agent $MAIN "agent: git -c k=v merge" "git -c core.x=1 merge x"
t deny  agent $MAIN "agent: lowercase /volumes path" "git -C /volumes/d8sst7/dev.decryptology.net/paddock merge x"
t allow agent $MAIN "agent: git branch new (create)" "git branch newb origin/main"
t deny  agent $MAIN "agent: git update-ref (FN)" "git update-ref refs/heads/main HEAD~1"
t deny  agent $MAIN "agent: git branch with force flag" "git branch --force main HEAD~1"
t deny  agent $MAIN "agent: git -C main checkout in subshell" "( git -C $MAIN checkout x )"
t allow agent $MAIN "agent: cd wt && commit && status" "cd $WT && git commit -m x && git status"
t allow agent $MAIN "agent: git log in main" "git log --oneline -5"
t allow agent $WT   "agent: cd \$VAR && git merge (unknown, fail open)" "cd \$WT && git merge x"
t deny  agent $MAIN "agent: sudo git reset" "sudo git reset --hard"
t deny  agent $MAIN "agent: xargs git checkout (FN)" "echo a | xargs git checkout"
t allow coord $MAIN "unterminated quote" "echo 'abc; cd /x; ls"
t allow coord $MAIN "unbalanced paren" "echo ) ; cd /x && ls"

# --- fail open
out=$(echo 'not json' | "$H" 2>/dev/null); [ -z "$out" ] && echo "ok: bad input allows" || { echo "FAIL: bad input"; fail=1; }
out=$(jq -nc '{tool_name:"Bash",cwd:"/x",agent_id:"a",tool_input:{command:"git commit -m \"unterminated"}}' | "$H" 2>/dev/null); [ -z "$out" ] && echo "ok: odd quoting allows" || { echo "FAIL: odd quoting"; fail=1; }
# --- messages name the cause and the next step
out=$(run agent "$WT" "cd /missing; git status")
case "$out" in *"unguarded cd"*"cd /missing && ..."*) echo "ok: cd message" ;; *) echo "FAIL: cd message"; fail=1 ;; esac
out=$(run agent "$MAIN" "git commit -m x")
case "$out" in *"main checkout"*"your own worktree"*) echo "ok: main-checkout message" ;; *) echo "FAIL: main message"; fail=1 ;; esac

# ---- size cap and linear time (shared scanner)
big=$(python3 -c 'print("cp " + "a" * 300000 + " /x")')
out=$(run agent "$MAIN" "$big" 2>"${TMPDIR:-/tmp}/wg-err.$$"); case "$out" in *deny*) echo "FAIL: over 200 KB must allow"; fail=1 ;; *) grep -q "too large" "${TMPDIR:-/tmp}/wg-err.$$" 2>/dev/null && echo "ok: over 200 KB allows with a warning" || echo "ok: over 200 KB allows" ;; esac
rm -f "${TMPDIR:-/tmp}/wg-err.$$"
word=$(python3 -c 'print("echo " + "a" * 150000 + "; git status")')
s0=$(python3 -c 'import time;print(time.time())'); run agent "$MAIN" "$word" >/dev/null
ms=$(python3 -c "import time;print(int((time.time()-$s0)*1000))"); [ "$ms" -lt 1500 ] && echo "ok: a 150 KB token scans in ${ms} ms" || { echo "FAIL: 150 KB token took ${ms} ms"; fail=1; }
body=$(python3 -c 'print("\n".join("cd /x; mkdir y; echo %d" % i for i in range(5000)))')
s0=$(python3 -c 'import time;print(time.time())'); out=$(run agent "$MAIN" "cat > /x/f <<EOF
$body
EOF"); ms=$(python3 -c "import time;print(int((time.time()-$s0)*1000))")
case "$out" in *deny*) echo "FAIL: a heredoc body full of cd lines must be skipped"; fail=1 ;; *) [ "$ms" -lt 1500 ] && echo "ok: a 5000-line heredoc is skipped in ${ms} ms" || { echo "FAIL: heredoc took ${ms} ms"; fail=1; } ;; esac

# ---- a push after an unguarded commit (the commit-msg hook rejected it and the push still ran, twice on 2026-11-12)
expect "commit; push is refused" deny agent "$WT" "git commit -m x; git push origin b"
expect "commit newline push is refused" deny agent "$WT" $'git commit -m x\ngit push origin b'
expect "commit && x; push is refused" deny coord "$WT" "git commit -m x && echo ok; git push"
expect "commit && push is allowed" allow agent "$WT" "git commit -m x && git push origin b"
expect "commit || exit 1; push is allowed" allow agent "$WT" "git commit -m x || exit 1; git push origin b"
expect "commit || { ...; exit 1; }; push is allowed" allow agent "$WT" "git commit -m x || { echo no; exit 1; }; git push"
expect "a push alone is allowed" allow agent "$WT" "git push origin b"
expect "push before commit is allowed" allow agent "$WT" "git push origin a; git commit -m x"
expect "if git commit; then git push is allowed" allow agent "$WT" "if git commit -m x; then git push; fi"
expect "set -e then commit newline push is allowed" allow agent "$WT" $'set -e\ngit commit -m x\ngit push'
out=$(run agent "$WT" "git commit -m x; git push"); case "$out" in *"commit"*"&&"*) echo "ok: message names the next step" ;; *) echo "FAIL: push message"; fail=1 ;; esac

# ---- agents may not bypass the git hooks (lefthook)
expect "agent LEFTHOOK=0 git commit" deny agent "$WT" "LEFTHOOK=0 git commit -m x"
expect "agent env LEFTHOOK=0 git push" deny agent "$WT" "env LEFTHOOK=0 git push origin b"
expect "agent LEFTHOOK_EXCLUDE=x git push" deny agent "$WT" "LEFTHOOK_EXCLUDE=commit-msg git push"
expect "agent git commit --no-verify" deny agent "$WT" "git commit --no-verify -m x"
expect "agent git push --no-verify" deny agent "$WT" "git push --no-verify origin b"
expect "agent git -C wt commit -n --no-verify" deny agent "$WT" "git -C $WT commit -m x --no-verify"
expect "agent export-less LEFTHOOK=0 then git commit on its own line is not an env prefix" allow agent "$WT" $'LEFTHOOK=0\ngit commit -m x'
expect "agent plain commit is allowed" allow agent "$WT" "git commit -m x"
expect "agent LEFTHOOK=1 is allowed" allow agent "$WT" "LEFTHOOK=1 git commit -m x"
expect "coordinator LEFTHOOK=0 git commit is allowed" allow coord "$WT" "LEFTHOOK=0 git commit -m x"
expect "coordinator --no-verify is allowed" allow coord "$WT" "git commit --no-verify -m x"
expect "agent mentions --no-verify in a message" allow agent "$WT" "git commit -m 'never use --no-verify'"
out=$(run agent "$WT" "git commit --no-verify -m x"); case "$out" in *"Hooks must pass: cherry-pick the hook fix or stop and report (no bypass)"*) echo "ok: bypass message" ;; *) echo "FAIL: bypass message"; fail=1 ;; esac
exit $fail
