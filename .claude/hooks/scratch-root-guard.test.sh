#!/bin/bash
# Tests for scratch-root-guard.sh. Run: bash .claude/hooks/scratch-root-guard.test.sh
here=$(cd "$(dirname "$0")" && pwd)
H="$here/scratch-root-guard.sh"
S=/private/tmp/claude-501/-Volumes-x-proj/00000000-aaaa/scratchpad
fail=0
run() { jq -n --arg t "$1" --arg k "$2" --arg v "$3" '{tool_name:$t,tool_input:{($k):$v}}' | "$H"; }
expect() { # name want(deny|allow) tool key value
  out=$(run "$3" "$4" "$5")
  case "$out" in *'"deny"'*) got=deny ;; *) got=allow ;; esac
  if [ "$got" != "$2" ]; then echo "FAIL: $1 (want $2, got $got)"; fail=1; else echo "ok: $1"; fi
}
expect "Write to root file is refused" deny Write file_path "$S/notes.md"
expect "Edit of root file is refused" deny Edit file_path "$S/notes.md"
expect "Write to a subfolder is allowed" allow Write file_path "$S/implementer-x/notes.md"
expect "redirect into root is refused" deny Bash command "echo hi > $S/out.txt"
expect "append into root is refused" deny Bash command "echo hi >> $S/out.txt"
expect "redirect into subfolder is allowed" allow Bash command "echo hi > $S/mine/out.txt"
expect "rm -rf of a root-level DIRECTORY is allowed (an agent's own subfolder)" allow Bash command "rm -rf $S/old-run"
expect "rm -rf of a root-level dir with slash is allowed" allow Bash command "rm -rf $S/old-run/"
expect "rm -rf inside own subfolder is allowed" allow Bash command "rm -rf $S/mine/old"
expect "mkdir -p of a root-level folder is allowed (first step of every agent)" allow Bash command "mkdir -p $S/newdir"
expect "mkdir of subfolder's child is allowed" allow Bash command "mkdir -p $S/mine/newdir"
expect "cp into root as a file is refused" deny Bash command "cp a.txt $S/a.txt"
expect "rm of a root-level file is refused" deny Bash command "rm $S/a.txt"
expect "touch in root is refused" deny Bash command "touch $S/a.txt"
expect "reading the root is allowed" allow Bash command "ls $S/ && cat $S/notes.md"
expect "unrelated command is allowed" allow Bash command "git status"
expect "Write outside scratchpad is allowed" allow Write file_path /Volumes/d8sst7/dev-targets/x/a.txt
# The coordinator (no agent marker in the input) is refused too: the rule has no agent/coordinator split.
expect "coordinator has no exemption for the root" deny Write file_path "$S/c.md"
# The refusal names the next step.
out=$(run Write file_path "$S/notes.md")
case "$out" in *"your own subfolder"*) echo "ok: message names the subfolder" ;; *) echo "FAIL: message"; fail=1 ;; esac

# ---- the reviewer's cases (2026-11-12): every line must match
RS=/private/tmp/claude-501/-Volumes-x-proj/00000000-aaaa/scratchpad
t() { # want name tool key value
  out=$(jq -n --arg t "$3" --arg k "$4" --arg v "$5" '{tool_name:$t,tool_input:{($k):$v}}' | "$H" 2>&1)
  case "$out" in *'"deny"'*) g=deny;; *) g=allow;; esac
  if [ "$g" = "$1" ]; then echo "ok: $2"; else echo "FAIL: $2 (want $1, got $g)"; fail=1; fi
}
t allow "Write deep subfolder" Write file_path "$S/foo/bar/baz.txt"
t allow "NotebookEdit in subfolder" NotebookEdit notebook_path "$S/foo/n.ipynb"
t deny  "NotebookEdit in root" NotebookEdit notebook_path "$S/n.ipynb"
t allow "mkdir -p own subfolder (needed first step)" Bash command "mkdir -p $S/reviewer-x"
t allow "mkdir -p own subfolder with slash" Bash command "mkdir -p $S/reviewer-x/"
t allow "mkdir -p own subfolder/child" Bash command "mkdir -p $S/reviewer-x/out"
t allow "rm -rf own subfolder at cleanup" Bash command "rm -rf $S/reviewer-x"
t allow "cp FROM a root file into own subfolder" Bash command "cp $S/old.txt $S/mine/old.txt"
t allow "diff root file > own subfolder" Bash command "diff $S/a $S/b > $S/mine/d.txt"
t allow "grep root file | tee own subfolder" Bash command "grep x $S/a.log | tee $S/mine/x.log"
t allow "write into subfolder with quotes" Bash command "echo hi > \"$S/mine/out.txt\""
t deny  "write root in quotes" Bash command "echo hi > \"$S/out.txt\""
t deny  "write root 2>" Bash command "make 2> $S/err.log"
t deny  "write root &>" Bash command "make &> $S/all.log"
t deny  "tee -a root" Bash command "echo x | tee -a $S/a.log"
t deny  "cd root && echo > x (relative, FN)" Bash command "cd $S && echo hi > notes.txt"
t deny  "S var then redirect (FN)" Bash command "S=$S; echo hi > \$S/notes.txt"
t deny  "/tmp spelling (FN)" Bash command "echo hi > /tmp/claude-501/-Volumes-x-proj/00000000-aaaa/scratchpad/n.txt"
t deny  "Write via /tmp spelling (FN)" Write file_path "/tmp/claude-501/-Volumes-x-proj/00000000-aaaa/scratchpad/n.txt"
t allow "heredoc body mentions a root write" Bash command $'cat > '"$S"$'/mine/s.sh <<\'EOF\'\nmkdir -p '"$S"$'/x\nEOF'
t allow "docker --rm then reading root file" Bash command "docker run --rm img cat $S/notes.md"
t allow "git rm unrelated + cat root" Bash command "git rm -q old.txt; cat $S/notes.md"
t allow "ls root" Bash command "ls -la $S"
t allow "empty input fields" Write file_path ""

exit $fail
