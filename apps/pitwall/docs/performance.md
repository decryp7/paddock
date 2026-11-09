# Performance record

Pitwall's benchmarks, budgets and recorded results. How to run a benchmark reproducibly and the results template are in
the family guide, [docs/benchmarks.md](../../../docs/benchmarks.md). Record every run there quoted (newest first) here.

The Benchmarks page (`app/src/app/features/benchmarks`, `crates/pitwall-core/src/benchmarks.rs`, `e2e/benchmarks.e2e.ts`)
is a product feature: it benchmarks the user's models. It is not a test of Pitwall's own speed.

## Benchmarks

| Benchmark | Measures | Command | Budget asserted | CI |
|---|---|---|---|---|
| `app/angular.json` budgets | The initial bundle | `cd app && npx ng build` | initial warn 1.2 MB / error 2 MB; component style warn 12 kB / error 24 kB | `ci.yml` Pitwall leg |
| `crates/pitwall-server/tests/llama_e2e.rs` | A real llama-server round trip (a hang guard, not a budget) | `PITWALL_LLAMA_E2E=1 cargo test -p pitwall-server --test llama_e2e` | none | No |

## Budgets

[ADR 0006](adr/0006-memory-policy.md) (memory policy) and [ADR 0005](adr/0005-queue-and-scheduling.md) (queue) set
behaviour, not measured budgets. Rule 16's budgets (start-up, request latency, queueing, memory) are not set.

## Gaps

- No start-up, request latency, queueing, idle CPU or server memory benchmark exists.
- The model-load memory Debrief depends on (about 7–8 GB for the 9B model, estimated in Debrief's
  [cpu-memory research](../../debrief/docs/research/2026-10-28-cpu-memory.md)) has never been measured.
- C29 window weight is not configured.

## Results (newest first)

### 2026-11-09, first starts of a freshly copied `pitwall-server` and the child-engine tests (fix/pitwall-e2e-engine-health)

Quoted by [ADR 0071](../../../docs/adr/0071-ready-or-exit-never-a-fixed-start-budget.md). Recorded from the raw output
the agent of `fix/pitwall-e2e-engine-health` kept. Figures that only its commit message gives (`81c3ac92f`: first starts
"up to 27.7 s", "an already-checked binary started in 0.06 s", "6-60 ms") have no kept output; they are not recorded and
nothing relies on them.

- **Commit:** stress runs 51592 and 2537 on the test code before `81c3ac92f` (200 polls 50 ms apart); run 93264 on
  `7a73cdc59` (ready-or-exit, `--version` warm-up). Branch `fix/pitwall-e2e-engine-health`.
- **Machine:** Mac mini M4 Pro, 14 cores, 64 GB, macOS 27.2 (the shared CI host).
- **Load:** not kept with the runs. Runs 51592 and 2537 had nice'd CPU burners (count not kept); run 93264 is not known
  to have had any. An indication only.
- **Build:** debug, `CARGO_TARGET_DIR=/Volumes/d7sst7/dev-targets/targets/pitwall-e2e-health`.
- **Corpus:** none; the fake engine (`pitwall-server --fake-engine`).
- **Command:** `stress.sh <iterations> <burners> <filterset of the child-engine tests>`, which copies `pitwall-server` to
  a new file before each iteration, so its first start is checked again, as after a fresh link:

  ```bash
  #!/bin/bash
  # stress.sh <iterations> <busy-procs> <filterset>
  set -u
  N=$1; BUSY=$2; FILTER=$3
  export CARGO_TARGET_DIR=/Volumes/d7sst7/dev-targets/targets/pitwall-e2e-health
  B=$CARGO_TARGET_DIR/debug/pitwall-server
  cd /Volumes/d7sst7/dev-targets/wt/pitwall-e2e-health/apps/pitwall
  pids=()
  for _ in $(seq 1 "$BUSY"); do nice -n 10 python3 -I -c 'import time
  t=time.time()+1800
  while time.time()<t: pass' & pids+=($!); done
  trap 'kill ${pids[@]+"${pids[@]}"} 2>/dev/null' EXIT
  pass=0; fail=0
  for i in $(seq 1 "$N"); do
    [ -n "${NOREFRESH:-}" ] || { cp "$B" "$B.new" && mv -f "$B.new" "$B"; }
    out=$(cargo nextest run -p pitwall-server --test e2e -E "$FILTER" --no-fail-fast --success-output immediate 2>&1 | sed $'s/\x1b\\[[0-9;]*m//g')
    echo "$out" > run-$i-$$.log
    if echo "$out" | grep -q "^ *FAIL"; then fail=$((fail+1)); echo "$out" > fail-$i-$$.log; else pass=$((pass+1)); fi
  done
  echo "summary: $pass passed, $fail failed of $N (busy=$BUSY)"
  ```

  CPU burners on the shared host are now only a bounded fallback (ADR 0041 §5).
- **Runs:** kept failure logs: run 51592 iterations 1, 3, 5, 6, 7, 9; run 2537 iterations 4, 6, 7 (the number of
  iterations, 10 per run in the commit message, was not kept). Run 93264: 8 iterations, all logs kept.
- **Numbers:**

  | Measure | Before the fix (51592, 2537) | After (93264) | Budget | Verdict |
  |---|---|---|---|---|
  | Failing iterations | 6 (51592) and 3 (2537) failure logs, each with both tests failing at 10.66-10.89 s ("did not become healthy") | 0 of 8 | none | the 200-poll budget fails on a fresh binary; the fix does not |
  | First start of the fresh binary (the warm-up's printed duration) | not printed by the old code | 0.49, 0.69, 0.69, 1.06, 1.44, 6.61, 7.40, 10.08 s | none | first starts of the same binary vary 20× between iterations |
  | The filtered tests, whole run | - | 3.67-10.52 s; 37.0 s in the 10.08 s iteration ("5 slow") | none | a slow first start slowed the tests and did not fail them |

- **Memory measure:** not measured; the quantity is time to readiness, which memory does not bear on.
- **Notes:** raw output in `/Volumes/d7sst7/dev-targets/scratch/pitwall-e2e-health/` (scratch, not kept). One more
  failure log (`fail-20-9408`, both tests failing at 20.02 s) belongs to an intermediate version of the fix and is not
  counted. Why the check of this binary took up to 10 s here and about 15 s on CI is not measured.

None recorded beyond CI timings ([docs/research/2026-10-ci-timings.md](../../../docs/research/2026-10-ci-timings.md),
2026-10-24: `build.yml` with bundles, median 20m38s).
