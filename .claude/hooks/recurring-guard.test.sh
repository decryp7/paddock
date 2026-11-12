#!/bin/bash
# Tests for recurring-guard.sh with invented transcripts of the real shape. Run: bash .claude/hooks/recurring-guard.test.sh
here=$(cd "$(dirname "$0")" && pwd)
H="$here/recurring-guard.sh"
T=${TMPDIR:-/Volumes/d8sst7/dev-targets/scratch-guard}/recurring-guard-test.$$
mkdir -p "$T"; trap 'rm -rf "$T"' EXIT
fail=0
human='{"type":"user","origin":{"kind":"human"},"message":{"role":"user","content":"do the batch"}}'
report() { jq -nc --arg c "<agent-message from=\"a1\">
[Subagent hand-back] The text below is the final report.
  Done. Branch pushed.
  Recurring: $1
</agent-message>" '{type:"queue-operation",operation:"enqueue",content:$c}'; }
tool() { jq -nc --arg n "$1" '{type:"assistant",message:{role:"assistant",content:[{type:"tool_use",id:"t1",name:$n,input:{}}]}}'; }
say() { jq -nc '{type:"assistant",message:{role:"assistant",content:[{type:"text",text:"ok"}]}}'; }
check() { # name want transcript-file [extra-json]
  extra=${4:-"{}"}
  out=$(jq -nc --arg p "$3" --argjson extra "$extra" '{transcript_path:$p,hook_event_name:"Stop"} + $extra' | "$H" 2>/dev/null)
  case "$out" in *'"block"'*) got=block ;; *) got=allow ;; esac
  if [ "$got" != "$2" ]; then echo "FAIL: $1 (want $2, got $got)"; fail=1; else echo "ok: $1"; fi
}
{ echo "$human"; report none; say; } >"$T/none.jsonl"
{ echo "$human"; report "none."; say; } >"$T/none2.jsonl"
{ echo "$human"; report "flaky waits via sleep; earlier: commit abc, lesson 12"; say; } >"$T/class.jsonl"
{ echo "$human"; report "scratch-root writes"; tool AskUserQuestion; } >"$T/asked.jsonl"
{ echo "$human"; report "scratch-root writes"; tool Agent; } >"$T/agent.jsonl"
{ echo "$human"; report "scratch-root writes"; tool SendMessage; } >"$T/send.jsonl"
{ echo "$human"; report "old class"; echo "$human"; say; } >"$T/oldspan.jsonl"
{ echo "$human"; report "class"; say; echo '{"type":"assistant", oops'; } >"$T/bad.jsonl"
check "Recurring: none allows" allow "$T/none.jsonl"
check "Recurring: none. allows" allow "$T/none2.jsonl"
check "a class with no follow-up blocks" block "$T/class.jsonl"
check "a class followed by AskUserQuestion allows" allow "$T/asked.jsonl"
check "a class followed by Agent allows" allow "$T/agent.jsonl"
check "a class followed by SendMessage allows" allow "$T/send.jsonl"
check "a class before the last user message allows" allow "$T/oldspan.jsonl"
check "stop_hook_active true allows" allow "$T/class.jsonl" '{"stop_hook_active":true}'
check "a subagent stop (agent_id) allows" allow "$T/class.jsonl" '{"agent_id":"a1"}'
check "a malformed line allows" allow "$T/bad.jsonl"
check "a missing transcript allows" allow "$T/none-such.jsonl"
out=$(jq -nc --arg p "$T/class.jsonl" '{transcript_path:$p}' | "$H")
case "$out" in *"flaky waits via sleep"*"propose its fix now (ER-21)"*) echo "ok: reason names the class" ;; *) echo "FAIL: reason: $out"; fail=1 ;; esac
# Speed: a 20 MB transcript with the report at the end.
big="$T/big.jsonl"; : >"$big"
line=$(jq -nc '{type:"attachment",pad:("x" * 2000)}')
yes "$line" | head -n 10000 >"$big"; { echo "$human"; report "big class"; say; } >>"$big"
s=$(date +%s%N 2>/dev/null || python3 -c 'import time;print(int(time.time()*1e9))')
check "a 20 MB transcript blocks" block "$big"
e=$(date +%s%N 2>/dev/null || python3 -c 'import time;print(int(time.time()*1e9))')
ms=$(( (e - s) / 1000000 )); echo "info: ${ms} ms on $(wc -c <"$big") bytes"
[ "$ms" -lt 500 ] || { echo "FAIL: too slow (${ms} ms)"; fail=1; }
exit $fail
