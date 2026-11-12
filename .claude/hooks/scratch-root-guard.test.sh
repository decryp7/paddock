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
expect "rm -rf of a root-level dir is refused" deny Bash command "rm -rf $S/old-run"
expect "rm -rf of a root-level dir with slash is refused" deny Bash command "rm -rf $S/old-run/"
expect "rm -rf inside own subfolder is allowed" allow Bash command "rm -rf $S/mine/old"
expect "mkdir in root is refused" deny Bash command "mkdir -p $S/newdir"
expect "mkdir of subfolder's child is allowed" allow Bash command "mkdir -p $S/mine/newdir"
expect "cp into root is refused" deny Bash command "cp a.txt $S/a.txt"
expect "reading the root is allowed" allow Bash command "ls $S/ && cat $S/notes.md"
expect "unrelated command is allowed" allow Bash command "git status"
expect "Write outside scratchpad is allowed" allow Write file_path /Volumes/d8sst7/dev-targets/x/a.txt
# The coordinator (no agent marker in the input) is refused too: the rule has no agent/coordinator split.
expect "coordinator has no exemption for the root" deny Write file_path "$S/c.md"
# The refusal names the next step.
out=$(run Write file_path "$S/notes.md")
case "$out" in *"your own subfolder"*) echo "ok: message names the subfolder" ;; *) echo "FAIL: message"; fail=1 ;; esac
exit $fail
