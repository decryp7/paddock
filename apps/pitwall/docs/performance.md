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

### 2026-11-11, speech to text: llama.cpp + Qwen3-ASR against whisper.cpp (docs/pitwall-speech-spike, WP-6)

Quoted by the [speech spike note](research/speech-spike-2026-11-11.md) (ADR 0023 Decision 5, plan WP-6).

- **Commit:** none of Pitwall's code runs; repository at `896171397`. Engines: llama.cpp `b11669` (`620d2225c`), the
  official `llama-b11669-bin-macos-arm64.tar.gz` that `llama-runtimes.json` pins (SHA-256 checked); whisper.cpp
  `v1.9.5` (`d1be6fde1`) built from source (the release has no macOS CLI asset).
- **Machine:** Mac mini M4 Pro, 14 cores, 64 GB, macOS 27.2 (the shared host; other agents were building).
- **Load:** 1-minute load 7–42 at the start of the runs (printed per run in the note's table). An indication only: no run
  had the machine to itself.
- **Build:** llama.cpp release binary (Metal); whisper.cpp `cmake -DCMAKE_BUILD_TYPE=Release -DGGML_METAL=ON
  -DWHISPER_SDL2=OFF`, `cmake --build -j 2 --target whisper-cli whisper-server`. Models: `ggml-org/Qwen3-ASR-0.6B-GGUF`
  Q8_0 + its Q8_0 mmproj; `ggerganov/whisper.cpp` `ggml-{tiny,base,small,small-q8_0,large-v3-turbo-q5_0}.bin`, all
  SHA-256 checked against Hugging Face's LFS ids.
- **Corpus:** synthetic: 9 clips made with macOS `say -r 150` (voices Daniel, Samantha, Anna, Thomas), converted with
  `afconvert -f WAVE -d LEI16@16000 -c 1`; 4.7–38.2 s, English, German, French, two with pink noise mixed in by ffmpeg
  (`anoisesrc=color=pink:amplitude=0.06`), one 300 s concatenation. Texts invented; listed in the note.
- **Command:** the note's harness, `python3 -I bench.py <run> llama|whisper <port> <engine args>`; llama-server
  `--temp 0 -c 8192 -np 1 --no-webui -m <model> --mmproj <mmproj>` (`-c 4096`/`6144` and `--device none -t 4 -tb 4` in
  the variants), request `input_audio` with `cache_prompt: false`; whisper-server `-m <model> -l auto -t 4` (`-ng` for
  CPU), `/inference` with `temperature=0`.
- **Runs:** 3 requests per clip on a loaded server, median reported; the 300 s clip once. Cold start = process start to
  `/health` 200, 3 starts back to back with a warm file cache.
- **Numbers** (Metal unless marked CPU; WER on synthetic speech, small sample):

  | Measure | Qwen3-ASR 0.6B Q8 | whisper base | whisper small | Budget | Verdict |
  |---|---|---|---|---|---|
  | Latency, 24.7 s clip | 0.78 s (CPU 4 threads 2.51 s) | 0.39 s (CPU 0.73 s) | 0.48 s (CPU 1.83 s) | none | all well under 1 s on Metal |
  | Real-time factor, 4.7–38 s clips | 0.028–0.047 (CPU 0.080–0.102) | 0.016–0.033 (CPU 0.030–0.089) | 0.019–0.052 (CPU 0.068–0.424) | none | - |
  | 300 s clip | 9.1 s, RTF 0.030 (CPU 74.7 s, RTF 0.25) | - | 4.45 s, RTF 0.015 | none | - |
  | WER, 7 English clips (mean) | 0.3% | 1.2% | 0.3% | none | Qwen = small > base |
  | WER, German / French clip | 0% / 0% | 10.5% / 12.5% | 0% / 0% | none | base too weak outside English |
  | WER, 300 s mixed English + German | 0.7% (`max_tokens` 2048) | - | 11.8% (`-l auto` dropped every German passage) | none | - |
  | Physical footprint | 1,105 MB (`-c 4096`), 1,247 MB (`-c 6144`), 1,548 MB (`-c 8192`); CPU 1,931–2,232 MB | 405 MB | 802 MB (q8_0: 551 MB) | none | - |
  | Start to ready (warm cache) | 0.53–0.56 s | 0.12–0.17 s | 0.22–0.23 s | none | - |

- **Memory measure:** physical footprint (`footprint -p <pid>`) of the server after the run.
- **Notes:** first starts on this loaded host varied 1.4–11.6 s (Qwen) and 20–33 s (whisper.cpp's first Metal library
  compile); not repeatable, not a cold-boot figure. Tiny and large-v3-turbo results and the per-clip rows are in the
  note. Not measured: Windows (any backend), Linux, Intel or AMD CPUs, a real voice or real microphone. Raw output in
  `/Volumes/d7sst7/dev-targets/scratch/pitwall-speech-spike/out/` (scratch, not kept).

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
