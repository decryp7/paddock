#!/bin/bash
# Tests for no-force-push.sh. Run: bash .claude/hooks/no-force-push.test.sh
here=$(cd "$(dirname "$0")" && pwd)
H="$here/no-force-push.sh"
fail=0
expect() { # name want command
  out=$(jq -nc --arg c "$3" '{tool_name:"Bash",cwd:"/x",tool_input:{command:$c}}' | "$H" 2>/dev/null)
  case "$out" in *'"deny"'*) got=deny ;; *) got=allow ;; esac
  if [ "$got" != "$2" ]; then echo "FAIL: $1 (want $2, got $got)"; fail=1; else echo "ok: $1"; fi
}
expect "git push -f" deny "git push -f origin x"
expect "git push --force" deny "git push --force origin x"
expect "git push origin x --force-with-lease" deny "git push origin x --force-with-lease"
expect "git push --force-with-lease=ref:sha" deny "git push --force-with-lease=x:abc origin x"
expect "git push --force-if-includes" deny "git push --force-if-includes origin x"
expect "git push origin +ref:ref" deny "git push origin +main:main"
expect "git -C dir push -f" deny "git -C /x push -f"
expect "env VAR=1 git push -f" deny "env GIT_SSH=x git push -f"
expect "push -f in a && chain" deny "cd /x && git push -f"
expect "push -uf bundle" deny "git push -uf origin x"
expect "plain push" allow "git push origin x"
expect "push -u" allow "git push -u origin x"
expect "push --set-upstream" allow "git push --set-upstream origin x"
expect "git status" allow "git status"
# The old hook joined lines with tr: a push on one line and -f of another command on a later one was refused.
expect "push on one line, -f of another command later" allow $'git push origin x\nrm -f /x/y'
expect "push, then git branch -f" allow $'git push origin x && git branch -f foo HEAD'
expect "push and ls -f in a list" allow "git push origin x; ls -f"
expect "heredoc text mentions the force flag" allow $'cat > /x/n.md <<\'EOF\'\ngit push -f origin x\nEOF'
expect "quoted text mentions the force flag" allow "echo 'git push -f origin x'"
expect "commit message mentions the force flag" allow "git commit -m 'never git push -f'"
out=$(echo 'not json' | "$H" 2>/dev/null); [ -z "$out" ] && echo "ok: bad input allows" || { echo "FAIL: bad input"; fail=1; }
out=$(jq -nc '{tool_name:"Bash",tool_input:{command:"git push -f"}}' | "$H"); case "$out" in *"<branch>-v2"*) echo "ok: message names the next step" ;; *) echo "FAIL: message"; fail=1 ;; esac
exit $fail
