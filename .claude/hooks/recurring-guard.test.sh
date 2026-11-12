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
  case "$out" in *'additionalContext'*) got=block ;; *) got=allow ;; esac
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
check "a class with no follow-up gives the notice" block "$T/class.jsonl"
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
check "a 20 MB transcript gives the notice" block "$big"
e=$(date +%s%N 2>/dev/null || python3 -c 'import time;print(int(time.time()*1e9))')
ms=$(( (e - s) / 1000000 )); echo "info: ${ms} ms on $(wc -c <"$big") bytes"
[ "$ms" -lt 500 ] || { echo "FAIL: too slow (${ms} ms)"; fail=1; }

# ---- the reviewer's cases (2026-11-12): every line must match
R=${TMPDIR:-/Volumes/d8sst7/dev-targets/scratch-guard}/recurring-guard-rv.$$
mkdir -p "$R"; trap 'rm -rf "$T" "$R"' EXIT
rep() { jq -nc --arg c "<agent-message from=\"a1\">
[Subagent hand-back] The text below is the final report.
$1
</agent-message>" '{type:"queue-operation",operation:"enqueue",content:$c}'; }
tool() { jq -nc --arg n "$1" '{type:"assistant",message:{role:"assistant",content:[{type:"tool_use",id:"t",name:$n,input:{}}]}}'; }
t() { # want name file [extra]
  out=$(jq -nc --arg p "$3" --argjson x "${4:-null}" '{transcript_path:$p}+($x//{})' | "$H" 2>/dev/null)
  case "$out" in *additionalContext*) g=block;; *) g=allow;; esac
  if [ "$g" = "$1" ]; then echo "ok: $2"; else echo "FAIL: $2 (want $1, got $g)"; fail=1; fi
}
mk() { f="$R/$1.jsonl"; shift; { echo "$human"; for x in "$@"; do echo "$x"; done; say; } > "$f"; echo "$f"; }
t allow "Recurring: none new. (84/148 real none-lines look like this)" "$(mk a "$(rep '  Recurring: none new. The fix is local.')")"
t allow "Recurring: no new instance this round" "$(mk b "$(rep '  Recurring: no new instance this round.')")"
t allow "Recurring: none (first time seen)" "$(mk c "$(rep '  Recurring: none (first time seen)')")"
t allow "Recurring: n/a" "$(mk d "$(rep '  Recurring: n/a')")"
t allow "template quoted in a code block, real line none" "$(mk e "$(rep $'  The brief asks for:\n  ```\n  Recurring: <class name plus earlier instances>\n  ```\n  Recurring: none')")"
t block "**Recurring:** bold markdown class (FN)" "$(mk f "$(rep '  **Recurring:** flaky sleeps again (lesson 12)')")"
t block "- Recurring: list item class (FN)" "$(mk g "$(rep '  - Recurring: flaky sleeps again')")"
t block "class on the next line (FN)" "$(mk h "$(rep $'  Recurring:\n  flaky sleeps again')")"
t allow "class, then unrelated Agent call counts as follow-up" "$(mk i "$(rep '  Recurring: flaky sleeps')" "$(tool Agent)")"
t block "class, then only a Bash tool call" "$(mk j "$(rep '  Recurring: flaky sleeps')" "$(tool Bash)")"
t allow "same class second stop (stop_hook_active)" "$R/j.jsonl" '{"stop_hook_active":true}'
# Report arrives also as a peer user message (real transcripts carry both)
peer=$(jq -nc --arg c "[Subagent hand-back] report
  Recurring: flaky sleeps" '{type:"user",origin:{kind:"peer"},message:{role:"user",content:$c}}')
t block "peer-origin user message form" "$(mk k "$peer")"
# 2 MB attachment after the report pushes it out of the 1 MB tail
pad=$(jq -nc '{type:"attachment",pad:("x" * 1100000)}')
t block "report followed by 1.1 MB line (FN: outside tail)" "$(mk l "$(rep '  Recurring: flaky sleeps')" "$pad")"

# A transcript with no human message within the scan limit is too large to scan: allow with a warning.
pad=$(jq -nc '{type:"attachment",pad:("x" * 100000)}'); { for i in 1 2 3 4 5 6; do echo "$pad"; done; rep '  Recurring: flaky sleeps'; } > "$R/nohuman.jsonl"
out=$(jq -nc --arg p "$R/nohuman.jsonl" '{transcript_path:$p}' | RECURRING_GUARD_MAX_SCAN=300000 "$H" 2>"$R/err.txt")
case "$out" in *additionalContext*) echo "FAIL: too large to scan must allow"; fail=1 ;; *) grep -q "too large to scan" "$R/err.txt" && echo "ok: too large to scan allows with a warning" || { echo "FAIL: no warning"; fail=1; } ;; esac

# Advisory only: the output never carries a block decision, and names the class and the next step.
out=$(jq -nc --arg p "$T/class.jsonl" '{transcript_path:$p}' | "$H")
if printf '%s' "$out" | jq -e 'has("decision") | not' >/dev/null && printf '%s' "$out" | jq -e '.hookSpecificOutput.hookEventName == "Stop" and (.hookSpecificOutput.additionalContext | test("flaky waits via sleep") and test("ER-21"))' >/dev/null; then echo "ok: advisory notice, no block decision"; else echo "FAIL: notice shape: $out"; fail=1; fi
exit $fail
