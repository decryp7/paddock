# Performance record (benchmarks, budgets, results, bundle sizes and start-up)

Rule 35: every release records the initial chunk size, and the budgets in `app/angular.json` are measured size plus 10%.
Measured with `cd app && npx ng build` (production) and a headless Chromium load of `dist/app/browser` with the mock
backend (browser build; the desktop app loads no mocks).

This file is Debrief's single record of benchmark runs. The family guide, [docs/benchmarks.md](../../../docs/benchmarks.md),
says how to run a benchmark reproducibly and gives the template every new result uses. The sections below are: the
benchmark list, the budgets, the history of engine/store/memory results (newest first), then the older bundle and
start-up sections. Some measurements are long tables in ADRs and research notes. For those, the history here keeps the
headline numbers and links to the source instead of copying the whole table.

## Benchmarks

All corpora are synthetic: the invented `/samples` repeated with shifted times, or the generators named below. Never use a
user's logs (Paddock ER-14, docs/lessons.md "pasted logs are confidential"). Commands run from `apps/debrief` with
libduckdb set up (`eval "$(bash crates/debrief-store-duck/scripts/fetch-libduckdb.sh ~/.cache/debrief-libduckdb)"`, see
[development.md](development.md)).

### Engine and store (Rust)

| Benchmark | Measures | Command and env vars | Corpus (generator) | Budget asserted | CI |
|---|---|---|---|---|---|
| [`debrief-core/benches/engine.rs`](../crates/debrief-core/benches/engine.rs) (criterion) | Import throughput of each built-in importer (MiB/s); paging, facets, timeline, chart bucketing, exceptions and patterns on a loaded in-memory workspace | `cargo bench -p debrief-core --bench engine [-- --quick]`; `DEBRIEF_BENCH_IMPORT_LINES` (default 250 000), `DEBRIEF_BENCH_WORKSPACE_LINES` (default 100 000) | `/samples`, one file per importer, scaled up in the bench | none (criterion shows the change against the last run) | `debrief-bench.yml` job `bench` (`--quick`): pushes to main that touch the engine or store crates, nightly 03:47, dispatch. Artifact `criterion-report` |
| [`debrief-store-duck/tests/bench.rs`](../crates/debrief-store-duck/tests/bench.rs) | The whole pipeline at scale: corpus, import, index, reopen, then the interactive operations, each timed cold and warm, with the peak RSS of each phase | `DEBRIEF_BENCH_EVENTS=10000000 cargo test --release -p debrief-store-duck --test bench -- --nocapture`; `DEBRIEF_BENCH_DIR` (where the corpus is kept), `DEBRIEF_BENCH_STORE` (keep or reuse the store) | [`tests/common/corpus.rs`](../crates/debrief-store-duck/tests/common/corpus.rs): `/samples` written as CLEF and repeated, with shifted times and 20 renamed instances. 10M events is about 3.7 GB | **None.** 500 ms per operation is only printed (`INTERACTIVE_MS`). Import throughput is not checked | `debrief-bench.yml` job `store-bench`, 10M events, nightly and dispatch. Numbers only in the job log |
| [`debrief-store-duck/tests/bench_reads.rs`](../crates/debrief-store-duck/tests/bench_reads.rs) | Every `Query` and `Analysis` method on a large store, with the peak RSS while querying | `cargo test --release -p debrief-store-duck --test bench_reads -- --nocapture` with `DEBRIEF_BENCH_READS=<copies>` (28k events each; 360 is about 10M) or `DEBRIEF_BENCH_STORE=<folder>`; `DEBRIEF_BENCH_STRICT=1` fails over budget; `DEBRIEF_BENCH_ONLY=<a,b>` | `/samples` copies, or the store `bench.rs` kept | 0.5 s per interactive call and 300 MB query RSS, **only with `DEBRIEF_BENCH_STRICT=1`** | `store-bench`, reusing the 10M store, without `STRICT` (report only) |
| [`debrief-store-duck/tests/analysis_memory.rs`](../crates/debrief-store-duck/tests/analysis_memory.rs) | Memory added by anomalies, Moment and compare (RSS sampled every 20 ms) | `cargo test --release -p debrief-store-duck --test analysis_memory -- --nocapture`; `DEBRIEF_BENCH_EVENTS` (default 1M), `DEBRIEF_BENCH_STORE` | `tests/common/corpus.rs` | Growth of each analysis ≤ the DuckDB limit (64 MB) + 50 MB; no absolute RSS assertion (the allocator's retained pages after import vary from 200 to 290 MB between runners). Release builds only | `debrief.yml` job `rust-test`, step "Memory budget" (Linux, `continue-on-error` until Debrief 1.2 WP D-0) |
| [`debrief-store-duck/tests/memory.rs`](../crates/debrief-store-duck/tests/memory.rs) | Import peak RSS and ingest rate | `cargo test [--release] -p debrief-store-duck --test memory -- --nocapture`; `DEBRIEF_BENCH_COPIES=<n>` | `/samples` copies | Import ≤ 768 MB | `debrief.yml` `rust-test` and `rust-test-macos` (nextest, debug) |
| [`debrief-store-worker/tests/it/worker.rs`](../crates/debrief-store-worker/tests/it/worker.rs) | The shipped worker under the supervisor: import, idle and query memory; the app side stays flat | `cargo nextest run -p debrief-store-worker`; larger: `DEBRIEF_BENCH_COPIES=20 cargo test --release -p debrief-store-worker --test it worker:: -- --nocapture` | `/samples` copies | Import ≤ 768 MB (low memory ≤ 400); idle and query ≤ 320 MB (macOS 630); client growth ≤ 64 MB | `debrief.yml` `rust-test`, `rust-test-macos`; `debrief-bench.yml` `store-bench` with 20 copies, release |
| [`debrief-store-duck/tests/it/large_folder.rs`](../crates/debrief-store-duck/tests/it/large_folder.rs) | Live progress from the first moment, and per-phase timings of a very large folder | `cargo test -p debrief-store-duck --test it large_folder:: -- --nocapture`; `DEBRIEF_BENCH_FILES` (default 3 000; the timing run uses 100 000) | A tree of small files generated by the test | Progress is monotonic and the phases come in order (no time budget) | `debrief.yml` `rust-test` (default size) |
| [`debrief-store-duck/tests/it/layout_bakeoff.rs`](../crates/debrief-store-duck/tests/it/layout_bakeoff.rs) | Native DuckDB table against zstd Parquet segments (ADR 0014 R2b) | `DEBRIEF_BENCH_COPIES=20 cargo test --release -p debrief-store-duck --test it layout_bakeoff:: -- --ignored --nocapture` | `/samples` copies | none | no (ignored) |
| [`debrief-store-duck/tests/perf_idle.rs`](../crates/debrief-store-duck/tests/perf_idle.rs) | CPU and memory of the background phases and idle loops after an import. Prints `PHASE` lines for an outside sampler | `scripts/perf/perf-post.sh LABEL STORE [IDLE_S]` (macOS), or `DEBRIEF_PERF_STORE=<store> cargo test --release -p debrief-store-duck --test perf_idle -- --ignored --nocapture`; `DEBRIEF_PERF_IDLE_S` (default 60) | A store imported from `scripts/perf/gen_large.py` | none (the targets are in the research note's §4) | no. Its doc comment says the bench workflow runs it, but `debrief-bench.yml` does not |
| [`debrief-plugins/tests/it/perf_plugins.rs`](../crates/debrief-plugins/tests/it/perf_plugins.rs) | Idle cost of the built-in plugin processes | `cargo test --release -p debrief-plugins --test it perf_plugins:: -- --ignored --nocapture` under `scripts/perf/procsample.py`; `DEBRIEF_PERF_IDLE_S` | none | none | no (same mismatch as `perf_idle`) |
| [`debrief-plugins/tests/it/evtx_memory.rs`](../crates/debrief-plugins/tests/it/evtx_memory.rs) | Peak memory and import time of the Windows plugin on large `.evtx` files | `DEBRIEF_EVTX_MEASURE_MB=16,64,128 cargo test --release -p debrief-plugins --test it evtx_memory::measure -- --nocapture` | The MIT/Apache sample `security.evtx` chunk repeated to the size asked for | A 256 MB log within the 1024 MB plugin cap, peak < 512 MB. These two tests are `#[ignore]` ("fails today") | no |
| [`debrief-core/tests/it/stream_bench.rs`](../crates/debrief-core/tests/it/stream_bench.rs) | Time to first event: streaming import against listing everything first (ADR 0051) | `cargo test -p debrief-core --release --test it stream_bench:: -- --ignored --nocapture`; `DEBRIEF_BENCH_FILES` (default 100 000) | A generated tree | none | no |
| [`debrief-core/tests/it/names_perf.rs`](../crates/debrief-core/tests/it/names_perf.rs) | The name layers on 50 000 invented entities; `merge_scaling` | `cargo test -p debrief-core --release --test it names_perf:: -- --ignored --nocapture` | Invented names in the test | none | no |
| `debrief-core/src/spans/infer/tests.rs`, `evaluation_is_linear_and_skips_unrelated_events_cheaply` | Span-rule evaluation over many events | `DEBRIEF_BENCH_SPANS=1 cargo test --release -p debrief-core evaluation_is_linear` (10M events; otherwise 500k) | Generated in the test | < 120 s | `debrief.yml` `rust-test` (500k, debug) |
| [`debrief-profile/tests/it/perf.rs`](../crates/debrief-profile/tests/it/perf.rs) | The profile operations the UI calls most, on 1M samples over 50k functions | `cargo test --release -p debrief-profile --test it perf:: -- --nocapture` (debug: a fifth of the samples, budgets × 20) | Generated (seeded, skewed stacks) | Per operation, 100–2000 ms (see [profiling.md](profiling.md#performance)) | `debrief.yml` `rust-test` (debug) |
| `debrief-store-duck/src/fts_bench.rs` (branch `debrief/search-perf`, not on main) | Tantivy index build rate, footprint, segments and size; search p50/p95 | `DEBRIEF_FTS_BENCH_DOCS=2000000 DEBRIEF_FTS_BENCH_DIR=<disk> cargo test --release -p debrief-store-duck --lib fts::bench -- --ignored --nocapture`; `DEBRIEF_FTS_BENCH_HEAP_MB`, `_THREADS`, `_KEEP=1`, `_RUNS` (default 30) | Seeded log-like templates (600 templates, Zipf distributed, 3% stack traces) | none | no |

### Whole app and UI

| Benchmark | Measures | Command | Budget asserted | CI |
|---|---|---|---|---|
| [`scripts/ci/startup-bench.mjs`](../scripts/ci/startup-bench.mjs) | Median start-up marks (`windowCreated` … `firstInteractive`) over N everyday starts of the built app, with a throw-away data folder | `node scripts/ci/startup-bench.mjs <app> --runs 5 [--report-only] [--out timings.json]` | [`scripts/ci/startup-budgets.json`](../scripts/ci/startup-budgets.json), macOS only (325/400/790/915/920/930/940 ms) | `debrief.yml` job `tauri-smoke`, nightly 00:30 and dispatch: macOS enforced, Linux report only. Artifact `startup-timings-<os>` |
| [`app/scripts/window-weight.mjs`](../app/scripts/window-weight.mjs) | The JS a window loads before first paint (rule 35) | `cd app && npx ng build --stats-json && node scripts/window-weight.mjs main` (or `npm run check:window-weight`) | main ≤ 1355 kB, About ≤ 480 kB; pop-out reported | `debrief.yml` job `frontend`, every run |
| `app/angular.json` budgets | `ng build`'s initial chunks and component styles | `cd app && npx ng build` | initial warn 185 kB / error 192 kB; component style warn 16 / error 32 kB | `debrief.yml` `frontend` |

### Measurement scripts (macOS, manual)

[`scripts/perf/`](../scripts/perf):
- `python3 -I gen_large.py OUT_DIR SCALE` writes a synthetic folder shaped like `/samples` (scale 1.0 is about 8 GB, 56M events).
- `perf-import.sh LABEL CORPUS STORE` imports with the release CLI and worker under `procsample.py`.
- `perf-post.sh` drives `perf_idle`.
- `procsample.py` samples physical footprint, RSS, CPU and wake-ups every 250 ms through `proc_pid_rusage`.
- `perfsum.py` summarises the samples per phase.

Environment: `DEBRIEF_PERF_BIN`, `DEBRIEF_LIB_DIR`, `DEBRIEF_PERF_OUT`.

Not benchmarks, though the names suggest it: `debrief-plugin-profiling/tests/it/properties_perf.rs` is a property test of
the `perf script` importer; `e2e/scale-*.e2e.ts` check virtualisation (DOM row counts at 50M rows), not time;
`debrief-ai/tests/it/worker_memory.rs` checks that the AI worker's memory cap is enforced.

## Budgets

| Budget | Value | Owner | Enforced by |
|---|---|---|---|
| App process, one window, 10M events | < 250 MB | [ADR 0014](adr/0014-database-first-engine.md) §2 | nothing (gap) |
| Store worker importing | ≤ 768 MB (hard cap 2 GB); low-memory mode about 400 MB | ADR 0014 §2, [ADR 0031](adr/0031-performance-settings.md) | `memory.rs`, `worker.rs` |
| Store worker idle and querying | ≤ 300 MB. The test allows 320 MB on Linux since the names catalogue, and 630 MB on macOS since 1.1.1 | ADR 0014 §2, [notes/release-1.1.1-open.md](notes/release-1.1.1-open.md) | `worker.rs`, `analysis_memory.rs`, `bench_reads.rs` (`STRICT` only) |
| Interactive operation | < 0.5 s cold | ADR 0010, ADR 0014 | `bench_reads.rs` (`STRICT` only); `bench.rs` prints it only |
| Import throughput | ≥ 3 × 72.7k = 218k events/s on the same machine class | ADR 0014 §2 | nothing (gap) |
| Plugin memory cap | 1024 MB default; import ≤ 512 MB (proposed) | ADR 0031; [plugins research](../../../docs/research/2026-10-plugins-memory-and-manager.md) | `evtx_memory.rs` (ignored, fails today) |
| CPU, memory and idle gates (D7): idle ≤ 0.5% of a core and ≤ 1 wake-up/s; catalogue ≤ 512 MB; import ≤ 6 s and ≤ 32 CPU-s per GB; post-import ≤ 20 CPU-s/GB; name rebuild ≤ 1 s at 50k groups | as listed | [research/2026-10-28-cpu-memory.md](research/2026-10-28-cpu-memory.md) §4 | nothing yet (`bench/budgets.toml` and `regress_1_2.rs` are planned, not written) |
| Profile operations | 100–2000 ms each | [profiling.md](profiling.md#performance) | `debrief-profile` `perf.rs` |
| Start-up marks (macOS) | see `startup-budgets.json` | Paddock rule 16, [ADR 0058](adr/0058-startup-contract-and-window-watchdog.md) | `startup-bench.mjs` |
| First-paint JS | main 1355 kB, About 480 kB | Paddock rule 35 | `window-weight.mjs` |

## Results history: engine, store and memory (newest first)

Machines: "Mac mini" is the M4 Pro, 14 cores, 64 GB, shared with other agents' builds. "Windows VM" is the shared
24-core development VM used until 2026-10-22. Every run below was on a shared, loaded machine unless it says otherwise.
Read the numbers as indications and compare only within one run's A/B. Results recorded before the template existed miss
some fields; "n/s" means the source does not state them.

### 2026-10-30, `debrief report` under CPU contention: QoS background against utility (branch `fix/debrief-cli-report-hang`)

- **Commit:** A/B: base dfc9cdc94 (mining and full-text pools in QoS `BACKGROUND`) → change 1c2567566 (`UTILITY`)
- **Machine:** Mac mini M4 Pro, 14 cores (10 performance, 4 efficiency), 64 GB, macOS 27.2
- **Load:** 1-minute load about 20 before each run; 14 added `nice -n 20` CPU-bound processes (Perl busy loops, 150 s
  each) during the contended runs, which normal-priority threads preempt; other agents' builds ran too
- **Build:** debug (`cargo nextest run --no-run`), libduckdb 1.5.6; temporary per-phase `eprintln!` timing in
  `debrief-cli`'s `run.rs` and `tests/it/cli.rs` (not committed)
- **Corpus:** the test's own synthetic log (`write_log` in `crates/debrief-cli/tests/it/cli.rs`): 240 events, Serilog text
- **Command:** `TMPDIR=<53-character folder> PADDOCK_SOCKET_BASE=<folder> DYLD_LIBRARY_PATH=<libduckdb> target/debug/deps/it-<hash> --exact cli::the_report_is_written_as_html_and_never_overwritten_silently --nocapture`
- **Runs:** base 3 runs idle and 5 contended; change 3 contended; each run times 3 `report` executions (the fourth,
  refused, takes microseconds); ranges reported
- **Numbers:**

  | Measure | Base (`BACKGROUND`) | Change (`UTILITY`) | Budget | Verdict |
  |---|---|---|---|---|
  | Report build, idle | 0.16 s | not measured | none | — |
  | Report build, contended | 0.09–12.7 s | 0.09–0.33 s | none | contention no longer stretches it |
  | Import, contended | 0.23–0.35 s | 0.30–0.94 s | none | normal priority, not affected by the class |
  | Whole test, contended | 4.8–20.6 s | 1.5–2.7 s | nextest CI timeout 90 s | pass |

- **Memory measure:** not measured
- **Notes:** a timing on a shared, loaded machine: compare only within this A/B. CI history of the same test (macOS
  runner, jobs 21203, 21682, 21861, 22060): usually 3–9 s, 36 s, 63 s, then timeouts at 102 s and 90 s on busy runs;
  Linux always under 10 s. Raw output was not kept. Rule 52 and the lessons entry of 2026-10-30 quote these numbers.

### 2026-10-28, DuckDB server mode (Quack), research in progress

- Date, commit: 2026-10-28; branch `docs/research-duckdb-server-mode`, **uncommitted** in its worktree (`research/2026-10-28-duckdb-server-mode.md` and `harness/`).
- Machine, load: Mac mini, load 18–36 (each run waited for load < 40).
- Build: release, duckdb-rs 1.20000.0 / libduckdb 2.0.0; memory_limit 192 MB, 4 threads.
- Corpus: `tests/bench.rs` corpus, 3,000,000 events (1,095 MB CLEF), as Arrow IPC.
- Method: 3 interleaved rounds, medians.
- Numbers:
  - Write path: Parquet with 8 writers 4.38M events/s (footprint 734–814 MB); DuckDB table with 1 appender 200k/s; Quack with 4 writers and a 1 GB server 416k/s (client 2.5 GB + server 2.0 GB).
  - Queries are the same in process, over Quack and over the IPC model (for example facet 110 ms everywhere), except a bulk fetch of 100k rows: 94 / 128 / 220 ms cold.
- Notes: §3.3, §3.4 and the summary are unfilled placeholders. The prose and the bulk-fetch table disagree. Only macOS was measured.

### 2026-10-28, full-text search and Model2Vec (branch `debrief/search-perf`, eb8b0acb7)

- Machine, load: Mac mini, load 14–220; the note marks every number "indications only".
- Build: release, mimalloc. Bench: `fts_bench.rs`.
- Baseline:

| Docs | Build rate | Peak footprint | Index size |
|---|---|---|---|
| 200k (load 19) | 65k docs/s | 111 MB | 60 B/doc |
| 1M (load 31) | 50k docs/s | 137 MB | 56 B/doc |

- Search, p50 at 200k docs: terms 0.01–0.35 ms; prefix `t` 1.49 ms.
- The tokenizer takes 71% of the build samples.
- Model2Vec, potion-code-16M-v2: int8 is 16 MB with the same hit@5 as f32 (0.600; 0.700 with RRF).
- **Pending:** every per-change A/B row, the 10M runs, the writer thread/heap grid, the 2M end-to-end run and the Model2Vec throughput. Plan: `/Volumes/d7sst7/dev-targets/handoff/search-perf-bench.md`.
- Source: `research/2026-10-28-search-performance.md` on that branch.

### 2026-10-28, DuckDB 2.0 against 1.5.6 (branch `debrief/duckdb-2`, 08cf9f036, ADR 0014 amendment)

- Machine, load: Mac mini, load 33–65.
- Build: release CLI and worker.
- Corpus: `gen_large.py` scale 0.3, 16,847,461 events, 2.42 GB.
- Method: 3 interleaved pairs.
- Import:

| Version | Events/s | CPU (s) | Worker peak footprint |
|---|---|---|---|
| 1.5.6 | 687k–855k | 91.5–94.2 | 932–979 MB |
| 2.0 | 685k–991k | 79.1–81.2 | 905–945 MB |

- 2.0 uses about 13% less CPU. Both versions are **over** the 768 MB import budget. The footprint at the end of the import is about 190 MB higher on 2.0 (unexplained).
- Query settings (3M events, nine queries): 2.0 with `async_threads` 4 takes 0.56–0.66 s wall against 0.76–1.09 s on 1.5.6.
- `analysis_memory` on macOS, 2.0, 1M events: growth +48 / +6 / +14 MB (pass). The absolute 300 MB is a Linux budget: macOS starts at about 1,026 MB RSS.
- Worker (debug): import 356, query 219, low memory 268 MB (all pass).
- Notes: the "Query performance" section the branch adds to this file holds its details. The idle figure after a large import needs its own measurement.

### 2026-10-28, CPU and memory baseline (main 9b14d621e)

- Machine, load: Mac mini, load 3–25, ±30%.
- Build: release, libduckdb 1.5.6. Memory is physical footprint (`procsample.py`).
- Corpus: `gen_large.py`, scale 0.072 (581 MB, 4.04M events) and scale 1.0 (8.07 GB, 56.2M events).
- Import: 3.1 s and 43.4 s, 5.3–5.4 s/GB, 26.5–29.6 CPU-s/GB (**pass** against ≤ 6 s and ≤ 32 CPU-s per GB).
- Worker peak footprint: 805 / 1,268 MB (**fail**, budget 768).
- Catalogue and index after the import: 20 s / 297 s, peak 390 / 911 MB (**fail** at 8 GB, budget 512).
- Idle: the app stand-in with 12 plugins takes 116% of a core; the store takes 5.8–11.7% (**fail**, budget 0.5%).
- `merge_cooccurring` is quadratic: 8,000 groups take 44 s, so about 29 min at the 50k cap (**fail**, budget 1 s).
- Source: [research/2026-10-28-cpu-memory.md](research/2026-10-28-cpu-memory.md) (raw data outside the repo, in `research-data/results/perf-2026-10-28/`).

### 2026-10-27, very large import and DuckDB 2.0 CLI comparison

- Machine, load: Mac mini, load 20–90, wall times up to 2×.
- Build: release CLI and worker, libduckdb 1.5.6.
- Corpus: `gen_large.py`, 8.07 GB (56M events).
- Import: default settings 36–56 s (1.0–1.55M events/s), worker peak RSS 1,965–2,045 MB (3 MB under the 2 GB cap). Low-memory mode 66 s at 1,556 MB.
- Measured as footprint, the worker peaked at 0.9–1.05 GB against the 768 MB budget (**fail**). This led to the watcher switching to footprint (9d802de4c).
- DuckDB CLI 1.5.6 against 2.0.0 on the 56M store: ILIKE 3.5–4.2 s → 1.6–2.6 s; peak RSS 551 → 418 MB. Extra-key facets, ILIKE, regex and deep pages are over 0.5 s on both.
- Source: [docs/research/2026-10-duckdb-2-and-large-data.md](../../../docs/research/2026-10-duckdb-2-and-large-data.md) §2.4, §4, §5.

### 2026-10-27, plugin memory on large `.evtx`

- Bench: `evtx_memory.rs`; macOS, release (rows without a time are debug); machine n/s.
- The plugin peak grows about 11.3 MB per MB of log: 64 MB of log reaches 749–761 MB in 2.0 s; 256 MB reaches 2,892 MB in 7.7 s and is stopped at the 1024 MB cap.
- An idle plugin holds 11.3–11.5 MB resident.
- Source: [docs/research/2026-10-plugins-memory-and-manager.md](../../../docs/research/2026-10-plugins-memory-and-manager.md) §2–3.

### 2026-10-27, analysis memory over budget (plan D-0)

- `analyses_stay_within_the_query_budget` peaks at about 338 MB against 315, and `compare` runs out of DuckDB memory at 64 MB. Bisected to the entity catalogue (c299d685).
- Machine n/s.
- Source: [docs/plans/debrief-1-2.md](../../../docs/plans/debrief-1-2.md). The CI step is non-blocking until D-0 lands.

### 2026-10-24, 1.1.1 worker query memory on macOS

- Bench: `worker.rs` `imports_and_queries_within_the_budgets`, interleaved builds on the Mac mini.
- Query peak: v1.0.1 451–482 MB; 1.1.1 HEAD 503–533 MB. On the CI macOS host under load: 570 MB.
- Load moves the numbers by ±30 MB, and only one sample set was taken.
- The macOS budget became 630 MB. Linux arm64: 266–288 → 295–307 MB after the names catalogue, so Linux allows 320 MB.
- Source: [notes/release-1.1.1-open.md](notes/release-1.1.1-open.md).

### 2026-10-22, store memory (perf/store-memory)

- Machine, build: Windows VM, release.
- Corpus: 10,137,960 events (360 copies of `/samples`).
- Before → after: anomalies 65.7 s / 411 MB → 27–38 s / 257–291 MB; Moment 40.4 → 6.8 s; timeline with instance lanes 5.0 s → 71 ms.
- Low-memory import: 189–211 MB at 69–78k events/s.
- Source: [ADR 0014](adr/0014-database-first-engine.md), "Performance notes (perf/store-memory)".

### 2026-10-22, window capture

- Machine: Mac mini, 1440×900 pt, about 59,000 events; build n/s.
- The main thread blocks for about 5 ms; the whole command takes 0.2–0.9 s (it used to freeze the window for 3–8 s).
- Source: [ADR 0034](adr/0034-native-window-capture.md).

### 2026-10-21, allocator and parser (perf/rust)

- Machine: Windows VM, 24 cores, interleaved A/B, at least 2 runs each.
- mimalloc v3 with a 10 ms purge (shipped): 10M import in process 151–207k events/s against 28–79k with the system allocator.
- The parsed-map change raised 10M CLEF to 279–291k events/s in quiet pairs, which **meets** the 218k target in process. The 20-copy worker run stays at 52–60k events/s.
- `engine.rs` import, mimalloc against the system allocator: +60% to +175%.
- Source: ADR 0014, "Performance notes (perf/rust)".

### 2026-10-21, reads at 10M (6c)

- Machine, load: Windows VM, load about 80%, times vary up to 3×.
- Bench: `bench_reads.rs`, 10,137,960 events.
- Import 113 s (90k events/s) plus 73 s for the index.
- Under 0.5 s: summary, unfiltered pages, level and source facets, QL stats.
- Over 0.5 s: text search 1.1–1.5 s, timelines 3.4–4.3 s, field stats 2.8 s, Moment 23 s.
- Memory: 75–230 MB, except compare 309, Moment 316 and anomalies 424 MB.
- Source: ADR 0014, "Measured (6c …)".

### 2026-10-20, store bake-off and R2a/R2b/R2c

- Machine: Windows VM, release, ±30%.
- 20 copies (563,220 events): zstd Parquet segments write 261–409k events/s with 1 writer and 0.97–1.7M with 8, against 36.5k for the native table. This chose the layout.
- 10M (`bench.rs`): import 81 s (123k events/s), peak RSS 415 MB; store 787 MB; write-side reads under 0.5 s.
- Source: ADR 0014, sections R2a, R2b and R2c; bench `layout_bakeoff.rs`.
- See also: [research/2026-10-29-duckdb-write-paths.md](research/2026-10-29-duckdb-write-paths.md), the write paths compared again on DuckDB 2.0 and the Mac mini.

### 2026-10-20/22, mixed-source importers

- Bench: `engine.rs -- import`, 250k lines, Windows VM.
- Serilog text 32.2 → 28.6–29.2 MiB/s; log4net 28.3 → 25.5–29.6 MiB/s. Criterion calls both noise.
- Source: [ADR 0020](adr/0020-mixed-sources.md).

### 2026-10-18, store spike at 10M events (DuckDB against DataFusion)

- Machine n/s; corpus `/samples` scaled to 10M events (3.1 GB).
- DuckDB: ingest 72.7k events/s, reopen 23 ms, filtered page 88–127 ms, peak RSS 3.0 GB importing / 320 MB querying.
- The in-memory workspace needed about 1.9 KB per event.
- Source: [ADR 0010](adr/0010-persistent-store.md).

### 2026-10-17, process isolation and profile engine

- Out-of-process import: +14–16%; 1M access-log lines, loaded machine ([ADR 0002](adr/0002-microkernel-and-process-isolation.md)).
- Profile engine, release, Apple silicon, 1M samples: every operation within its budget, for example the flame layout in 40 ms against 300 ms ([profiling.md](profiling.md#performance)).

### Undated

- ADR 0051: time to first event on 100,000 files went from about 3.5 s to under 10 ms (`stream_bench.rs`; machine and build n/s).
- ADR 0050: rendering a dialog window went from about 345 to about 320 ms (production build in Chromium, median of 5, 2026-10-23).

## Results history: build and link times (newest first)

### 2026-10-29, incremental rebuild and link time (docs/link-time)

- **Commit:** cae1649f9 (origin/main) plus local, uncommitted experiment edits (crate type, a dev `dylib` crate); research
  note [2026-10-29-link-time.md](../../../docs/research/2026-10-29-link-time.md)
- **Machine:** Mac mini M4 Pro (Mac16,11), 14 cores, 64 GB, macOS 27.2, Xcode 27.1 (`ld-27037.1`); rustc 1.99.1 (LLVM 23.1.1)
- **Load:** 1-minute load 3.6-6.6 during the measured builds; other agents mostly idle; no other job of mine running
- **Build:** debug (`dev`: deps opt-level 2, `debug = "line-tables-only"`, `split-debuginfo = "unpacked"`), default features
- **Corpus:** none (the compiler is measured); synthetic one-line change in the app crate
- **Command:** Probe build: `perl` replaces `pub fn __link_probe() -> u32 { N }` at the end of `app/src-tauri/src/lib.rs`, then `CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=<private> CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=cc-timed.sh LINKMODE=ld|lld cargo build -p debrief-app [--features dynamic_linking] [--config 'profile.dev.package.debrief-app.debug=false|"full"']`; crate type switched by editing `[lib] crate-type`. [`cc-timed.sh`](../../../scripts/perf/link-time/cc-timed.sh) runs `cc` (with `--ld-path=<toolchain>/gcc-ld/ld64.lld` and `SDKROOT=MacOSX26.5.sdk` for lld) and logs each link's duration. Phase split: the same with `--timings`. The probe loop and round driver were throw-away shell scripts in `/Volumes/d7sst7/dev-targets/target/link-time-tools/` (machine-local); the linker wrapper is committed.
- **Runs:** 3 rounds, interleaved (every configuration once per round, each after an uncounted warm-up build); median (range) of wall time; link = median duration of the linker call
- **Numbers:**

  | Measure | Base (today: staticlib + cdylib + rlib, Apple ld) | Change | Budget | Verdict |
  |---|---|---|---|---|
  | Incremental rebuild, crate type rlib only | 5.88 s (5.83-6.19) | 4.40 s (4.13-4.66), -25% | none | adopt |
  | Incremental rebuild, dev `dylib` (option a), rlib only | 5.88 s | 3.99 s (3.79-3.99), -32% | none | not worth the hazards |
  | Incremental rebuild, lld (today's crate types) | 5.88 s | 6.68 s (6.41-6.69), +14% | none | reject |
  | Incremental rebuild, lld, rlib only / dylib | 4.40 / 3.99 s (ld) | 4.78 / 4.20 s | none | reject |
  | Incremental rebuild, app crate `debug = false` / `"full"` | 5.88 s | 5.60 s (-5%) / 6.55 s (+11%) | none | keep line-tables-only |
  | Binary link (`ld`) | 0.61 s (0.59-0.63) | rlib 0.64 s, dylib 0.30 s, lld 1.04 s | none | — |
  | cdylib link (`ld`) | 0.24 s (0.23-0.25) | gone with rlib only; lld 0.52 s | none | — |
  | App library unit (`--timings`, 2 runs) | 4.64-4.73 s | 3.05-3.35 s (rlib only) | none | — |
  | App binary unit (`--timings`, 2 runs) | 0.82-0.85 s | 0.85-0.87 s (rlib only) | none | — |
  | `libdebrief_app_lib.a` written per build | 1.0 GB | none (rlib only) | none | — |

- **Memory measure:** not measured (build time only)
- **Notes:** Cold build of the app with its dependencies: 7 min 45 s (465 s, `heavy.sh`, load 5-15). The lld runs used the MacOSX26.5 SDK because LLD 23 cannot read the macOS 27 SDK's `libSystem.tbd`. In the dylib configuration `debrief-alloc` stays out of the dylib (mimalloc's C symbols are not exported from a Rust dylib); the global allocator then does not cover the dylib's code (see the research note). Raw CSV and link logs: `/Volumes/d7sst7/dev-targets/target/link-time-tools/` (machine-local).

## Bundle size and start-up records (newest last)

## 2026-10-23, baseline before the Angular phase-1 migration (main at 67a51214)

| Measure | Value |
|---|---|
| Initial chunks (what `ng build` budgets) | 171.76 kB raw (styles 164.42 kB, `main` 6.80 kB, polyfills 0.5 kB); 15.77 kB transfer |
| Why so small | `main.ts` loads translations, then lazily imports `bootstrap`; the real startup cost is the lazy `bootstrap` chunk graph |
| `bootstrap` chunk (static closure, 122 chunks) | 1.52 MB raw |
| Largest single chunks | notices-generated 766 kB, a shared vendor chunk 678 kB, messages-pseudo-json 563 kB (only for the pseudo locale), bootstrap 466 kB |
| Browser startup (mock backend, to `networkidle`, 5 runs) | about 1.1 s, 170 requests, 3.69 MB raw (2.5 MB of it is the closure of the mock-backends chunk, browser only) |
| Total chunks | 3 initial + 285 lazy |

Budgets set from this: `initial` warn 190 kB / error 210 kB (measured + 10%, was 1.5 MB / 3 MB); `anyScript` warn 845 kB
(largest chunk + 10%) / error 1 MB.

## 2026-10-23, after Angular phase 1 (lazy Settings screen and diagnostics collector)

| Measure | Before | After |
|---|---|---|
| Initial chunks (budgeted) | 171.76 kB | 171.76 kB (unchanged: `main` only loads translations, then `bootstrap`) |
| Startup JS in the `bootstrap` static closure, raw | 2.08 MB (123 files, incl. 563 kB pseudo-locale messages loaded only for pseudo) | 1.87 MB (115 files) |
| Same, gzip | 602 kB | 544 kB |
| Browser startup bytes (mock backend, networkidle) | 3.69 MB, 170 requests, about 1.1 s | 3.52 MB, 169 requests, about 1.1 s |

The Settings screen (with its Storage, Parsers, Keyboard and other tabs) and the diagnostics collector dialog were
imported statically by the services that open them; their `open()` now loads them on first use. Views registered through
`component: () => import(...)` were already lazy. Largest remaining startup weight (stats-json, bytes in output):
dockview-core 479 kB, the mock backends 248 kB (browser only), Angular core 158 kB, echarts about 200 kB (pulled in by
chart cards and the Home view), the share/report, tools and AI folder-setup modules (statically imported by commands).
Next candidates: lazy dialog services for share/report, the format wizard and folder setup, and an `@defer` around echarts
consumers. How to measure: `npx ng build --stats-json`, then group `dist/app/browser-stats.json` outputs by input.

## 2026-10-24, what the main window loads before first paint (branch perf/startup-bundle)

`ng build`'s `initial` budget only sees `main`, polyfills and styles, because `main.ts` imports `bootstrap` dynamically and
`bootstrap` imports `app` and `app.config` dynamically. The real first-paint set is the static-import closure of the
chunks for `src/main.ts`, `src/bootstrap.ts`, `src/app/app.ts` and `src/app/app.config.ts` (for a pop-out `popout-shell.ts`
replaces `app.ts`; for a dialog window the dialog shell, its bootstrap and its content). `app/scripts/window-weight.mjs`
measures it from `dist/app/browser-stats.json` (`npx ng build --stats-json`, then `node scripts/window-weight.mjs main`;
`npm run check:window-weight` does both) and prints the heaviest chunks and packages. CI runs it after the frontend build.

| Measure | Before | After | Budget (measured + 10%) |
|---|---|---|---|
| Main window first-paint JS (static closure) | 1710.5 kB, 186 chunks | 1231.3 kB, 147 chunks | CI fails over 1355 kB |
| Pop-out window (reported, not enforced) | 1460.3 kB, 163 chunks | 1170.0 kB, 148 chunks | none (boots the whole `app.config`) |
| About dialog window | 441.2 kB, 30 chunks | 442.1 kB, 30 chunks | CI fails over 480 kB |
| Initial chunks (`ng build` budget) | 173.81 kB (styles 165.56, `main` 7.72, polyfills 0.5) | 173.81 kB | warn 185 kB, error 192 kB (was 190/210) |

What moved out of the first-paint set (all load when used, with the same behaviour): the dialogs, wizards and editors that
commands and services imported statically (format wizard, folder setup, report export, incident summary, redaction,
export, import range, add-tile, clock, updates, parser editor and import confirm, mark/marker editor, infer/extract/mask/drop,
note editor, knowledge editor, package review, layout dialogs, About) through
`DialogService.openLazy(() => import(...), options)`; What's new (now a document of the dock) and its generated changelog load with the view; the search-everything palette (fetched on first open); and, with
`@defer`, the filter bar (and the query field), side bar, command palette, import toast, partial-import banner and tour
overlay. Dialog windows no longer create the workspace facade, time cursor, live follow or live tail: `sharedUiState`
takes a scope (`display`, `summary`, `full`) and a dialog window asks for the narrowest one its backends allow.

What is left (about 780 kB of the 1231 kB is `src/app`, about 500 kB Angular, CDK and RxJS): the `provide*()` of about 50
features in `app.config.ts`, each of which statically pulls its stores, commands and start-up work (share 54 kB, popout 37,
profiling 30, AI 28, video 18, notes 15, dumps 11, playbooks 11 ...), the four built-in themes (27 kB, needed for the first
paint), and the dialog registry (17 kB). Getting a pop-out or the main window much lower needs lazy feature registration (a
manifest of command and view stubs per feature, the feature's code loaded on first use) and per-view provider sets for
pop-outs; that is a design change, proposed as an ADR rather than done here.

## 2026-10-24, start-up timings (branch perf/startup)

Every start logs one structured `Startup timings` line (`app/src-tauri/src/startup_timings.rs`): milliseconds since
the process start for `windowCreated`, `setupDone`, `pageLoad`, `firstPaint` (the page's first contentful paint),
`windowReady` (ADR 0058), `workspaceLoaded` and `firstInteractive` (the first frame after the load), plus `setup`'s
steps slowest first (`Steps`). The `startup_timings` command returns the same. The nightly `build` run starts the
debug app five times after a first run (`scripts/ci/startup-bench.mjs`) and checks the medians against
`scripts/ci/startup-budgets.json` (macOS; Linux reports only until its numbers are recorded).

Debug app on the Mac mini, empty data folder, median of 15 everyday starts, before → after this branch (interleaved
runs, load average 9–13 from other builds, so ±30 ms):

| Mark | Before (ms) | After (ms) |
|---|---|---|
| windowCreated | 217 | 215 |
| setupDone | 282 | 264 |
| pageLoad | 530 | 524 |
| firstPaint | 636 | 610 |
| windowReady | 637 | 611 |
| workspaceLoaded | 648 | 619 |
| firstInteractive | 653 | 624 |

Steps that left the main thread: stored settings (11–250 ms under load: applying a plugin's section starts its
process) and the tray icon (9–13 ms, now after the first paint); the store worker starts after the first paint
(about 0.5 s of background work the first query no longer waits for); the AI start-up log line no longer scans every
process. What remains before the first paint is the Tauri builder (`before setup`, ~115 ms in a debug build), creating
the main window's webview (~100 ms) and WebKit starting its content process after `setup` (~260 ms between
`setupDone` and `pageLoad`, outside Debrief's code). The native menu bar is built twice per start (the standard menus,
then the UI's once the workspace loaded), was 3–4 times.

The 1.4 s the user's 1.0.2 log showed between the window and the AI line was not reproduced with an empty data
folder (30 ms here); it depends on that Mac's state (stored settings with plugin sections, an AI model loading). The
`Steps` field of the next start's line names the step.

## Analysis memory on Linux, 2026-10-29

`analysis_memory` (1M events, release, paddock-ci:v2 under amd64 emulation on Apple silicon, DuckDB limit 64 MB): RSS before the
queries 273 MB; anomalies peak 377 MB (+104 MB), Moment +4 MB, compare +1 MB. In CI (run 7102) the RSS before was 224 MB and the
anomalies peak 330 MB. The absolute peak is reported, not asserted; compare used to run out of the 64 MB limit because both of
its statements were open at once.
