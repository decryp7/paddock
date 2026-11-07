#!/usr/bin/env bash
# Mechanics check of the benchmark runners (ADR 0030 section 5), for a VM or a clean machine: tiny sizes, one repetition, no speed
# asserted. Every kind must end with exactly one `result` line that is Done or Unavailable (with a reason), every number finite and
# positive, and the disk test must leave nothing in its folder. Usage: bench-smoke.sh <folder holding dyno-bench and dyno-bench-gpu>
# (for example the .app's Contents/MacOS, or target/debug). Exit 0 when all hold; otherwise one ::error:: line per failure.
set -u
dir="${1:?usage: bench-smoke.sh <folder holding dyno-bench and dyno-bench-gpu>}"
fail=0
err() { echo "::error::bench-smoke: $*"; fail=1; }

check() { # name, output
  local name="$1" out="$2" last
  last="$(printf '%s\n' "$out" | grep -v '"t":"heartbeat"' | tail -n 1)"
  [[ "$(printf '%s\n' "$out" | grep -c '"t":"result"')" == 1 ]] || err "$name: not exactly one result line"
  [[ "$last" == *'"t":"result"'* ]] || err "$name: the result is not the last line"
  if [[ "$last" == *'"state":"done"'* ]]; then
    printf '%s' "$last" | grep -Eq '"value":(-|0\.0[,}]|0[,}])' && err "$name: a number is zero or negative"
    printf '%s' "$last" | grep -Eq '"value":(NaN|null|inf)' && err "$name: a number is not finite"
    echo "ok: $name done"
  elif [[ "$last" == *'"state":"unavailable"'* && "$last" == *'"reason":"'?* ]]; then
    echo "ok: $name unavailable with a reason"
  else
    err "$name: ended neither Done nor Unavailable with a reason: ${last:0:200}"
  fi
}

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
fast=(--reps 1 --warmup-ms 0 --gap-ms 0 --millis 50)
check "cpu" "$("$dir/dyno-bench" cpu "${fast[@]}" 2>&1)"
check "memory" "$("$dir/dyno-bench" memory "${fast[@]}" 2>&1)"
check "disk" "$("$dir/dyno-bench" disk --disk-mib 16 --reps 1 --dir "$work" 2>&1)"
[[ -z "$(ls -A "$work")" ]] || err "disk: the scratch folder is not empty afterwards: $(ls -A "$work")"
check "gpu" "$("$dir/dyno-bench-gpu" --smoke 2>&1)"
exit "$fail"
