# Sourced by CI scripts that run phases which can hang (a file held open by a process left from an earlier job, antivirus on a large
# folder, git waiting on a lock): `bounded <label> <function>` runs the function in the background with stdout in $scratch/out and
# stderr in $scratch/err, logs "<tool>: <label> ...", then "<tool>: <label>: done in N s" (or "failed (exit N) in N s"), and returns its
# status. After $limit seconds it ends the function's process tree, prints ONE ::error:: line (ER-27: the phase, the last output (whole
# lines) and the next step) and exits 1; with BOUNDED_SOFT=1 it prints a ::warning:: line instead and returns 124 (a caller that must
# never fail its job). The caller sets tool, limit, scratch and hint (the next step) before the first call.
# kill_tree <pid>: ends the process and everything below it, each parent before its children: a shell ended after its child would
# see the child killed and write bash's `Killed: 9` note into the phase's error output (the warning's `Last output`).
kill_tree() {
  local p="$1" c kids=''
  if command -v pgrep >/dev/null 2>&1; then kids="$(pgrep -P "$p" 2>/dev/null || true)"; fi
  if [ "${RUNNER_OS:-}" = Windows ] && command -v taskkill >/dev/null 2>&1; then
    taskkill //F //T //PID "$(ps -p "$p" 2>/dev/null | awk 'NR == 2 { print $4 }')" >/dev/null 2>&1 || true
  fi
  kill -9 "$p" 2>/dev/null || true
  for c in $kids; do kill_tree "$c"; done
}

bounded() {
  local label="$1" fn="$2" start=$SECONDS pid rc=0
  echo "$tool: $label ..."
  "$fn" > "$scratch/out" 2> "$scratch/err" &
  pid=$!
  while kill -0 "$pid" 2>/dev/null; do
    # SECONDS counts whole seconds, so `-ge` could end a phase that ran only part of a second (one that started at 0.9 s reads 1 s
    # at 1.0 s); more than $limit whole seconds means at least $limit seconds really passed.
    if [ $((SECONDS - start)) -gt "$limit" ]; then
      kill_tree "$pid"
      if [ "${BOUNDED_SOFT:-0}" = 1 ]; then
        echo "::warning::$tool: '$label' did not finish within ${limit} s. Last output: $(grep -v '^[[:space:]]*$' "$scratch/err" | tail -n 4 | tr '\r\n' '  '). Next step: $hint"
        return 124
      fi
      echo "::error::$tool: '$label' did not finish within ${limit} s. Last output: $(grep -v '^[[:space:]]*$' "$scratch/err" | tail -n 4 | tr '\r\n' '  '). Next step: $hint"
      exit 1
    fi
    sleep 0.2
  done
  wait "$pid" || rc=$?
  echo "$tool: $label: $([ "$rc" = 0 ] && echo done || echo "failed (exit $rc)") in $((SECONDS - start)) s"
  return "$rc"
}
