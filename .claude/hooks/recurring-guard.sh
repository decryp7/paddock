#!/bin/bash
# Stop hook for the coordinator (main session): do not stop while a subagent report names a recurring class
# (`Recurring: <class>`, not `none`) and nothing after it asked the user or started work (ER-21).
# Input (stdin JSON): transcript_path, stop_hook_active, agent_id (present only inside a subagent).
# Output: {"decision":"block","reason":...} to keep Claude going; nothing to allow. Never fails closed: any problem
# reading the transcript allows the stop and warns on stderr. Reads only the tail of the transcript (fast on 20 MB).
TAIL_BYTES=${RECURRING_GUARD_TAIL_BYTES:-1000000}
input=$(cat)
[ "$(printf '%s' "$input" | jq -r '.stop_hook_active // false' 2>/dev/null)" = "true" ] && exit 0
[ -n "$(printf '%s' "$input" | jq -r '.agent_id // empty' 2>/dev/null)" ] && exit 0
tp=$(printf '%s' "$input" | jq -r '.transcript_path // empty' 2>/dev/null)
if [ -z "$tp" ] || [ ! -r "$tp" ]; then echo "recurring-guard: no readable transcript_path; allowing the stop" >&2; exit 0; fi
size=$(wc -c <"$tp")
if [ "$size" -gt "$TAIL_BYTES" ]; then
  body=$(tail -c "$TAIL_BYTES" "$tp" | tail -n +2)   # the first line of a tail is usually cut
else
  body=$(cat "$tp")
fi
# Every line must parse; one that does not means the answer cannot be trusted, so allow and say so.
if ! result=$(printf '%s\n' "$body" | jq -sR '
  def lines: split("\n") | map(select(length > 0));
  (lines | map(try fromjson catch "BAD")) as $e
  | if any($e[]; . == "BAD") then "BAD" else
    ($e | to_entries) as $all
    | ([ $all[] | select(.value.type == "user" and (.value.origin.kind // "") == "human") | .key ] | last // -1) as $start
    | def text: (.value.content // (.value.message.content // "")
                 | if type == "array" then map(.text? // "") | join("\n") else tostring end);
      [ $all[] | select(.key > $start)
        | select(.value.type == "queue-operation" or .value.type == "user")
        | select(((.value.type == "queue-operation" and .value.operation == "enqueue") or (.value.type == "user" and (.value.origin.kind // "") != "human")))
        | . as $x | ($x | text) as $t
        | select($t | test("\\[Subagent hand-back\\]|<agent-message"))
        | ($t | split("\n") | map(select(test("^\\s*Recurring:"))) | map(sub("^\\s*Recurring:\\s*"; ""))) as $r
        | select(($r | length) > 0)
        | { key: $x.key, value: ($r | map(select((ascii_downcase | gsub("[^a-z]"; "")) != "none")) | first // "") } ] as $reports
      | ($reports | map(select(.value != "")) | last) as $rep
      | if $rep == null then "" else
          ([ $all[] | select(.key > $rep.key and .value.type == "assistant")
             | .value.message.content[]? | select(.type == "tool_use" and (.name | IN("AskUserQuestion","Agent","SendMessage"))) ] | length) as $follow
          | if $follow > 0 then "" else $rep.value[0:80] end
        end
    end' 2>/dev/null); then
  echo "recurring-guard: could not read the transcript; allowing the stop" >&2; exit 0
fi
result=${result#\"}; result=${result%\"}
if [ "$result" = "BAD" ]; then echo "recurring-guard: a transcript line did not parse; allowing the stop" >&2; exit 0; fi
[ -z "$result" ] && exit 0
jq -n --arg c "$(printf '%s' "$result" | jq -Rr '.')" \
  '{decision:"block", reason:("A report named a recurring class (" + $c + ") — propose its fix now (ER-21): ask the user or start it under an earlier approval.")}'
exit 0
