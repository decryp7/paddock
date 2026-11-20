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

### 2026-11-20, bulk delete and the embeddings cache: operation counts per call (feat/pitwall-bulk-wp3, ADR 0108 WP-3)

Quoted by [ADR 0108](../../../docs/adr/0108-bulk-apis-one-call-for-many-items.md) (PW-1, PW-2) and the CHANGELOG. These are counts of statements, commits,
scans and cache calls, read by the budget tests themselves (SQLite's own trace and commit hook, the library's scan counter, the
cache's call counter); they do not depend on the machine or its load, so there is no timing here.

- **Commit:** base `5740e7ea3~1` (before WP-3) against `5740e7ea3` and the review fixes on `fix/pitwall-bulk-wp3-review`.
- **Machine:** the Mac mini M4 Pro (arm64, macOS 27.2). Counts only; no timing was taken.
- **Load:** not relevant to counts.
- **Build:** debug.
- **Corpus:** synthetic: 60 vectors of 16 bytes; 5 to 6 inputs per request; 4 ids per delete call.
- **Command:** `cargo nextest run -p pitwall-server -E 'test(/bulk|caching|cache_db|library/)'`
- **Runs:** single (deterministic counts, asserted with `==`).
- **Numbers:**

  | Measure | Before | After | Budget (asserted) |
  |---|---|---|---|
  | 60 stores, commits | 60 | 1 | `== 1` (`a_batch_of_stores_is_one_commit`) |
  | 60 stores, statements | 120 | 63 (60 to 66 allowed) | `60..=66` |
  | 60 stores, `SUM(size)` over the table | 60 | 0 after the first | `== 0` (`stores_never_sum_the_table_again`, floor `>= 1` at open) |
  | 60 reads, commits and statements | 60 and 120 | 1 and 1 read plus 1 commit for the hit counts | `== 1` each |
  | Embeddings request of N inputs, cache reads and writes | N and N | 1 and 1 | `== 1` each (`an_embeddings_request_reads_and_writes_the_disk_cache_once`, N = 6) |
  | Embeddings request, all hits | N reads | 1 read, 0 writes | `== (1, 0)` |
  | `delete_models` of 4 ids, scans of the models folder | 4 | 1 | `== 1` (`delete_models_scans_once_and_answers_one_result_per_id`) |

- **Notes:** the "before" statement and commit counts for 60 stores and reads are the author's figures from the WP-3 commit message
  (`5740e7ea3`), measured by the same spy on the per-key code; the per-request and per-delete "before" counts follow from the old
  code's loop (one call per input or id). The review round showed each budget test red against the old shape: the per-key loop put
  back in `http.rs` fails the request-path test (N reads and N writes against 1 and 1).

### 2026-11-11, Whisper small Q8_0 on CPU: Windows ARM64 VM and Linux container (docs/pitwall-speech-spike, WP-6 second revision)

Quoted by the [speech spike note](research/speech-spike-2026-11-11.md), sections 5 and 6 (the user's choice of Whisper
small Q8_0 on whisper.cpp; the 0.5-day Windows and Linux measurement before WP-7).

- **Commit:** none of Pitwall's code runs; repository at `896171397`. Engine: whisper.cpp `v1.9.5` (`d1be6fde1`), the
  official binaries of the `b5454` release on that commit, SHA-256 checked: `whisper-bin-win-cpu-arm64.zip`
  (`28c37e7b598c…`), `whisper-bin-ubuntu-arm64.tar.gz` (`6b95ebfc6044…`). Model `ggml-small-q8_0.bin` (`49c8fb02b65e…`).
- **Machines:** Windows: the Paddock test VM (VMware Fusion on the Mac mini M4 Pro), Windows 11 ARM64 build 26300.9457,
  4 vCPUs, 8 GB, reverted to `clean-baseline`, plus Microsoft's VC++ redistributable (arm64), without which the server
  does not start (note, section 6); stopped and reverted after each run. Linux: `docker run --rm --cpus 4 --memory 12g
  train5-linux-dev:2` (Debian 13, aarch64) on the same Mac. **No x64 CPU was measured.**
- **Load:** the Mac was shared with building agents: 1-minute load 7–13 during the Linux runs, 14–45 during Windows run 1,
  5–20 during Windows run 2. An indication only.
- **Build:** official release binaries; the Windows arm64 build reports `DOTPROD = 1 | MATMUL_INT8 = 1`, the Linux arm64
  build only `NEON = 1 | ARM_FMA = 1`.
- **Corpus:** the spike's seven synthetic English clips (4.7–38.2 s), its 300 s English-and-German clip, and a new
  300 s English-only clip made from the same clips (texts invented; note, appendix).
- **Command:** `whisper-server -m ggml-small-q8_0.bin -l en -t 4`; `/inference` with `temperature=0`
  (`audio_ctx = min(1500, 50 × seconds + 64)` in the sized variant). Harness: the spike's `bench.py` adapted for `/proc`
  (Linux), a PowerShell 5.1 port with `curl.exe` (Windows); both in the note's appendix.
- **Runs:** 3 requests per clip, median; each 300 s clip once; Windows twice from a fresh revert. Start = process start
  to `/health` 200.
- **Numbers** (WER on synthetic speech, small sample):

  | Measure | Windows ARM64 VM, run 1 / run 2 | Linux arm64 container | Budget | Verdict |
  |---|---|---|---|---|
  | Latency, 4.7 / 8.7 / 13.5 / 24.7 / 38.2 s clips | 2.92 / 1.86 / 2.43 / 3.21 / 6.05 s; 1.91 / 1.91 / 2.03 / 2.64 / 4.92 s | 2.45 / 3.10 / 3.35 / 4.32 / 10.06 s | none | about 2–3 s for a short dictation |
  | Real-time factor, 7 clips | 0.13–0.63; 0.11–0.74 (0.74: one noisy clip in a load peak) | 0.17–0.52 | spike's "too slow": worse than about 0.3 | within 0.3 for clips of 8.7 s or more; the 4.7 s clip is above it on both systems (30 s window floor) |
  | 300 s English clip | 104.7 s (RTF 0.35, load peak); 33.4 s (RTF 0.11) | 36.0 s, RTF 0.12 | none | - |
  | 300 s English + German clip (`-l en`) | 45.5 s; 48.5 s | 50.6 s | none | - |
  | With `audio_ctx` sized, clips up to 24.7 s | 0.70–10.0 s (one load peak); 0.43–2.71 s, RTF 0.08–0.11 | 0.56–3.28 s, RTF 0.11–0.19 | none | the 30 s window floor goes |
  | WER, 7 English clips (mean) | 0.3%; 0.3% | 0.4% | none | - |
  | WER, 300 s English / English + German | 0.7% / 11.8% (both runs) | 0.7% / 11.8% | none | German is not transcribed (English only) |
  | Memory | working set 352 MB idle, peak 472 MB (short), 575–587 MB (300 s); private bytes 928–933 MB | RSS 350 MB idle, peak 500 MB (short), 573 MB (300 s) | none | - |
  | Start to ready | first server after boot 2.52 s; **50.9 s**; later 2.28–5.30 s | 1.53 s first, 0.16–0.18 s later | none | the first start can be long; not explained |

- **Memory measure:** Windows `Get-Process` `WorkingSet64`, `PeakWorkingSet64`, `PrivateMemorySize64` after the run;
  Linux `/proc/<pid>/status` `VmRSS` and `VmHWM`.
- **Notes:** on the clean VM without the VC++ runtime, `whisper-server --help` and llama.cpp `b11669`'s
  `llama-server --version` both exit with `0xC0000135` (note, section 6; seen in both runs). Outliers of 9.4–52.7 s per
  request in run 1 coincide with host load peaks. Raw output in
  `/Volumes/d7sst7/dev-targets/scratch/pitwall-speech-measure/out/` (scratch, not kept).

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
