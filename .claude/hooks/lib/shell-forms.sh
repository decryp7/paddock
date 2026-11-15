#!/bin/bash
# The shared shell-forms corpus (ER-21: the scanner missed or misread shell forms four times): every guard on lib/shellscan.py
# runs every row of lib/shell-forms.tsv, so a scanner change is checked against all of them at once.
# Usage (from a guard's .test.sh): bash lib/shell-forms.sh <guard> <hook> [cwd]. Exit 1 when a row's verdict differs.
# A row: name, env (NAME=value words or -), command, then one column per guard: deny, warn (allowed with a warning in
# additionalContext: a hook that judges text only warns, ER-36), allow, or - (not judged by that guard).
# The command runs as an agent's (agent_id set), in <cwd> (default: a folder that is no checkout), with the variables the rows
# use unset unless the row's env sets them.
here=$(cd "$(dirname "$0")" && pwd)
guard=$1 hook=$2 cwd=${3:-/nonexistent/shell-forms}
col=$(grep -v '^#' "$here/shell-forms.tsv" | head -n 1 | tr '\t' '\n' | grep -nx -- "$guard" | cut -d: -f1)
[ -n "$col" ] || { echo "FAIL: shell-forms.tsv has no column for $guard"; exit 1; }
fail=0 n=0
while IFS=$'\t' read -r -a f; do
  [ "${f[0]}" = name ] && continue
  case "${f[0]}" in '#'* | '') continue ;; esac
  want=${f[$((col - 1))]}
  [ "$want" = - ] && continue
  envs=()
  [ "${f[1]}" = - ] || read -r -a envs <<<"${f[1]}"
  out=$(jq -nc --arg c "${f[2]}" --arg d "$cwd" '{tool_name:"Bash",agent_id:"a1",cwd:$d,tool_input:{command:$c}}' |
    env -u K -u KILLER -u CARGO -u NODE -u EDITOR -u PYTHON "${envs[@]}" "$hook" 2>/dev/null)
  case "$out" in *'"deny"'*) got=deny ;; *'"additionalContext"'*) got=warn ;; *) got=allow ;; esac
  n=$((n + 1))
  if [ "$got" != "$want" ]; then echo "FAIL: shell-forms ($guard): ${f[0]} (want $want, got $got)"; fail=1; fi
done <"$here/shell-forms.tsv"
echo "shell-forms ($guard): $n rows checked"
exit $fail
