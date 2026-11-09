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
| [`debrief-store-duck/tests/it/large_folder.rs`](../crates/debrief-store-duck/tests/it/large_folder.rs) | Live progress from the first moment, and per-phase timings of a very large folder | `cargo test -p debrief-store-duck --test it large_folder:: -- --nocapture`; `DEBRIEF_BENCH_FILES` (default 500; the timing run uses 100 000) | A tree of small files generated by the test | Progress is monotonic and the phases come in order (no time budget) | `debrief.yml` `rust-test` (default size) |
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

### 2026-10-31, store worker memory at 20 copies: the names catalogue (branch `fix/debrief-bench-query-rss`)

- **Commit:** main 09293bcde; change on `fix/debrief-bench-query-rss`
- **Why:** the nightly `store-bench` job (`debrief-bench.yml`, `worker::` with `DEBRIEF_BENCH_COPIES=20`, release) failed
  `imports_and_queries_within_the_budgets` on every scheduled run since it moved into Paddock; the push runs skip that job,
  so it had never passed. Querying 382–402 MB (PSS) against 320 MB:

| Run (date, commit) | Measure | Idle MB | Querying MB | Result |
|---|---|---|---|---|
| debrief 5976 (10-22, f826f4f4) | RSS | 196 | 303 | fail (budget 300) |
| debrief 6161 (10-23, eba53424) | RSS | 199 | 300 | pass |
| debrief 6233 (10-24, 9e4369c2, 1.0.2 head) | RSS | 214 | 322 | fail |
| debrief 6344 (10-25, e42bb85e, 1.1.1) | RSS | 321 | 430 | fail (idle) |
| debrief 6396 (10-26, e42bb85e) | RSS | 298 | 444 | fail (import 778) |
| debrief 6412 (10-27, e42bb85e) | RSS | 281 | 424 | fail |
| paddock 6451 (10-27, 2d3476a1, dispatch) | PSS | 264 | 400 | fail |
| paddock 6954 (10-29, 283e5067) | PSS | 294 | 382 | fail |
| paddock 7172 (10-30, dfc9cdc9) | PSS | 284 | 402 | fail |
| paddock 7260 (10-31, 09293bcd) | PSS | 245 | 399 | fail |

  The step to about +110 MB came with 1.1 (the names catalogue) and stayed; the switch from RSS to PSS (9d802de4c,
  10-27) took about 25 MB off. Not noise and not a regression since the move: the same failure each night.
- **Machine:** Mac mini, Linux amd64 container (`paddock-ci:v2` under Rosetta, 4 CPUs); 1-minute load 30–50 from other
  agents (the brief's load < 10 never came); every comparison interleaved in one container session.
- **Build:** release, libduckdb 1.5.6; temporary per-step tracing (`/proc/<pid>/smaps_rollup`, the catalogue's pending
  sources, `duckdb_memory()`) in `worker.rs`, not committed.
- **Corpus:** `/samples` × 20 (585,800 events), as the job.
- **Findings:**
  - The test measured "idle" and "querying" while the catalogue was still being built in the background: 600–760 of
    its sources pending at "idle"; it takes 85–120 s more for 20 copies. Idle and the query peak were a snapshot of
    how far it had got: idle 245–288 MB, query peak 351–407 MB, as on CI.
  - Waiting for the catalogue first: idle 397–458 MB, query peak 528–562 MB. A worker reopened on that store: idle
    132 MB, query peak 246–265 MB. So the queries themselves cost about 115–130 MB over idle; the rest is what the
    catalogue build leaves behind: DuckDB held 145 MB of committed catalogue rows (`duckdb_memory()`), never
    checkpointed because the store's `checkpoint_threshold` is 1 GB and only imports checkpoint.
  - `debrief_alloc::release()` on the catalogue thread alone changed nothing (idle 397 vs 442 MB, within noise).
  - By copies (worker after import, catalogue still running; reopened worker): 1 copy idle 205 / 120 MB, query peak
    263 / 186 MB; 5 copies 238 / 123, 346 / 216; 10 copies 274 / 136, 378 / 261; 20 copies 245–247 / 132,
    372–375 / 246–265.
- **Fix:** the catalogue checkpoints and releases freed pages when a run of builds ends, and reports itself built only
  after that; the worker test waits for the catalogue before it measures idle (a condition, bounded at 600 s).
- **After the fix:** not measured yet (Debrief paused for the Dyno, Spotter and Pitwall tags). The regression test
  `a_built_catalogue_is_saved_into_the_database_file` and the A/B at 20 copies (and copies=1) are still to
  run before this branch merges.
- **Budget:** unchanged (Linux 320 MB, macOS 630 MB).

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

## Query and import performance, DuckDB 1.5.6 against 2.0 (2026-10-28 and 29)

Every block says what it measured: machine (Apple M4 Pro, 14 cores, macOS), the load average when it ran (load over 10 is marked and
was not used for conclusions), the corpus (invented, same shape as the samples; never user logs), the command, and the build (release).
Reproduce with `/Volumes/d7sst7/dev-targets/duck2-bench` (`ab.sh`, `imem.sh`, `reads.sh`, `fmt/suite.py`); the decisions are in
ADR 0014's DuckDB 2.0 amendment.

### Real import pipeline, 1.5.6 against 2.0 (`ab.sh`)

Release CLI plus store worker, 16,847,461 events (2.42 GB: CLEF, log4net text, compressed), three interleaved pairs, load 33 to 65
(over 10: the speeds are lower bounds). Events per second 1.5.6 / 2.0: 707k / 685k, 687k / 738k, 855k / 991k. Worker CPU 91 to 94 s
against 79 to 81 s. Peak physical footprint 932 to 979 MB against 905 to 945 MB (both above the 768 MB budget; not new). End-of-import
footprint 319 to 344 MB against 452 to 536 MB (explained next).

### Memory after an import, and a bigger limit during it (`imem.sh`)

In-process import of 3,000,000 CLEF events (release, corpus cached, load 3 to 7), then 30 s idle, three runs per variant.

| Variant | Import | Peak footprint | DuckDB's own use right after | 30 s later | RSS 30 s later |
|---|---|---|---|---|---|
| A: shipped 192 MB limit | 4.2 to 5.3 s (570k to 717k events/s) | 756 to 816 MB | 10 to 12 MB | 172 to 176 MB | 1,092 to 1,133 MB |
| B: 512 MB during the import, 192 MB after | 4.2 to 4.3 s | 783 to 815 MB | 14 to 15 MB | 171 to 175 MB | 1,115 to 1,150 MB |
| C: 512 MB throughout | 4.1 to 4.3 s | 785 to 829 MB | 14 to 15 MB | 205 to 211 MB | 1,126 to 1,183 MB |
| D: 64 MB set after the import | 4.2 to 4.4 s | not sampled | 11 MB | 56 to 59 MB | 1,078 to 1,132 MB |
| E: 32 MB set after the import | 4.2 to 4.3 s | not sampled | 11 to 12 MB | 25 to 30 MB | 1,057 to 1,096 MB |

Explanation of the extra memory after an import: it is DuckDB's buffer pool. The import itself uses 10 to 15 MB of DuckDB memory (events go
to Parquet through arrow-rs, not through DuckDB); the background full-text index and catalogue builds then read the segments through DuckDB
and fill its pool up to `memory_limit` within 30 s (172 MB at the 192 MB limit, 205 to 211 MB at 512 MB). It is a cache bounded by the
limit, not a leak. A higher limit during the import gains nothing (A against B and C: same time, same peak). Setting a lower limit after
the import caps it (D, E): 115 to 145 MB less DuckDB memory; RSS moves little in this process because it also holds the allocator's
pages (RSS is not the right gauge; rule 26). Recommendation: do not raise the limit for imports; lower it to about 64 MB while the store
is idle after an import and restore 192 MB on the first query, if the idle budget needs the memory.

### Write stage against the number of writers (`layout_bakeoff`, 60 copies of the samples, 1.68M events, load 2.6 to 3.6)

| Writers | Events/s | Against one writer | Peak over baseline |
|---|---|---|---|
| 1 | 1.15M | 1.0x | 0 to 6 MB |
| 2 | 2.28M to 2.29M | 2.0x | 0 to 4 MB |
| 4 | 4.39M to 4.45M | 3.8x | 2 to 3 MB |
| 8 | 7.8M to 8.2M | 7.0x | 12 to 51 MB |
| 12 | 10.5M to 10.7M | 9.2x | 52 to 61 MB |
| Arrow appender into one native table | 737k to 745k | 0.64x | +175 to +470 MB |

Verdict: the write stage scales linearly with writers up to the core count (14 cores; the pipeline's real ceiling is parsing, 0.7 to
1.0M events/s in the pair table above, so writers beyond about the parse rate buy nothing). A single appender is one writer and tops out
at 0.74M events/s, below one Parquet writer. Appender with two writers was not measured (a native table has one appender per connection;
the test has no such variant).

### Query formats on the same 3M events (`fmt/suite.py`, load 3 to 5, 2 rounds of 7 repetitions, 192 MB limit, 4 threads)

p50 / p95 over the 14 repetitions of each query. File sizes: Parquet segments 165 MB; native table 1,681 MB (storage v2.0.0, 2.8 s to
create from the view) and 1,675 MB (v1.5.0, 4.0 s); Vortex 388 MB (13.4 s; `extra`, a MAP, is not supported and was written as JSON text).

| Query | Parquet + view | native v2.0.0 | native v1.5.0 | Vortex |
|---|---|---|---|---|
| count | 5 / 8 ms | 0 / 0 | 0 / 0 | 0 / 1 |
| level facet | 8 / 8 | 2 / 2 | 2 / 2 | 2 / 2 |
| minute x level | 18 / 19 | 7 / 9 | 7 / 7 | 7 / 8 |
| first page | 23 / 24 | 4 / 4 | 3 / 4 | 5 / 5 |
| extra facet | 119 / 120 | 59 / 64 | 57 / 62 | 87 / 98 |
| ILIKE | 40 / 41 | 45 / 46 | 43 / 44 | 23 / 23 |
| regex | 44 / 45 | 33 / 33 | 33 / 34 | 34 / 34 |
| by id | 2 / 3 | 0 / 1 | 0 / 1 | 1 / 1 |
| deep page | 77 / 80 | 57 / 58 | 56 / 57 | 57 / 58 |
| peak RSS | 125 to 132 MB | 294 to 326 MB | 302 to 304 MB | 898 to 930 MB |

The corpus is 20 shifted copies of the samples, which zstd compresses far better than DuckDB's native encodings would compress real logs,
so the 10x size gap is exaggerated; the direction is not. All runs are warm for the operating system's cache; "cold" here is only the
first run of a fresh process and is not separated. Native storage v2.0.0 (DICT_FSST) and v1.5.0 are the same in speed and size on this
data. Verdict on formats: a native table answers the scans 1.5 to 5x faster and uses 10x the disk and 2.3x the memory; at 3M events
every query is far under 0.5 s in all formats, so the Parquet default does not need to change on speed.

### Appender against the Parquet fan-out (answer to "is the fan-out needed?", from the write-stage and format blocks)

1. **Fan-out** (today): parse threads write segments in parallel; the write stage is never the limit (1.15M events/s per writer, 7 to 10M
   with 8 to 12); 165 MB per 3M events; catalogue of segments, compaction, deletion list and `union_by_name` are the code it costs.
2. **One Appender** into a native table: 0.74M events/s, so with 8 parse threads producing about 1M events/s it would be the bottleneck and
   needs a queue in front of it; peak +175 to +470 MB; 975 MB to 1.7 GB on disk for 1.7M to 3M events. It would delete the segment catalogue,
   compaction and `union_by_name`, but not the need for a deletion path or for the Tantivy index.
3. **Hybrid** (fan-out, then `INSERT INTO native SELECT * FROM view`): 2.8 s (v2.0.0) or 4.0 s (v1.5.0) for 3M events, about 1M events/s,
   doubling the disk and holding 1.2 GB of RSS while it runs.

Not measured, honestly: the real pipeline with the sink swapped (the importer's sink interface would need an Appender implementation; the
write-stage numbers above bound it), end-to-end time until search is ready for each, the compacted-Parquet, DuckLake, Quack, VARIANT and
scaling-with-size cells, and the cold-cache runs (they need a cache purge this account cannot run). They are listed in the ADR as open.

### Results blocks recorded after the fact (rule 48)

**Writer bench, first run (the 387k / 725k / 5.1M figures quoted in the ADR and reports).** Harness
`crates/debrief-store-duck/tests/it/layout_bakeoff.rs`; command
`DEBRIEF_BENCH_COPIES=20 DEBRIEF_THREADS=8 cargo test --release -p debrief-store-duck --test it layout_bakeoff:: -- --ignored --nocapture`;
corpus: 20 copies of the invented samples (about 0.56M events, 28k per copy); build: branch `debrief/duckdb-2` at 7b5653fdd plus the
progress relay later committed as 08cf9f036, DuckDB 2.0.0; machine: M4 Pro, 14 cores; load: not recorded (the machine was shared; the
later runs below were taken at load 2 to 4). Results: native table with one Arrow appender 387,515 events/s (peak +526 MB, 324.5 MB on
disk); one Parquet writer 725,432 events/s (peak +2 MB, 38.1 MB, 48 segments); eight writers 5,087,660 events/s (115 ms, peak +9 MB).
These are lower than the quiet runs of the writer table above for the appender (737k to 745k at load 2.6 to 3.6) and the one-writer
case (1.15M); the first run was taken under load, so treat the quiet figures as the reference.

**Quiet real import, 1.3 to 1.55 million events/s.** Not measured by this branch: it is the research note's figure
(`docs/research/2026-10-duckdb-2-and-large-data.md`, branch `docs/research-duckdb-2` c755e717d, measured at 355fd94ae with DuckDB 1.5.6):
56,152,132 events in 8.07 GB, the release CLI plus store worker (`debrief import`), 36 to 56 s, parse pool `min(cores - 1, 8)`, corpus from
`gen_large.py` scale 1.0 (invented), machine M4 Pro; load: not recorded beyond "quiet". This branch's own pair runs (load 33 to 65) reached
0.69M to 0.99M events/s on a 2.42 GB corpus and must not be read as the pipeline's speed. Re-measure on a quiet machine with
`ab.sh LABEL 156|200 large03` (scale 0.3) before quoting a number.

**More than one Appender (`DEBRIEF_APPENDERS=1,2,4`, 20 copies, load 2.1 to 2.8, three runs).** n appenders on n connections of one
database, each appending its share of the batches in its own transaction, one checkpoint at the end:

| Appenders | Events/s | Against one | Peak over baseline | On disk |
|---|---|---|---|---|
| 1 | 738k to 744k | 1.00x | +477 to +479 MB | 324.5 MB |
| 2 | 925k to 937k | 1.25x | +217 to +316 MB | 327.4 MB |
| 4 | 774k to 793k | 1.06x | +157 to +180 MB | 504 to 506 MB |

This settles the open conflict between "a second appender adds nothing" and "four in-process appenders gain 1.8x": in this harness a
second appender adds 25%, a fourth falls back to 1.06x and writes a 55% larger file (more, smaller row groups). Neither 1.0x nor 1.8x
holds as a rule; the gain is small, peaks at two writers and stays below one Parquet writer (1.15M events/s) on every count. The Quack
harness's 355k against 200k was not reproduced here (a different harness and corpus).

## Import of an 8 GB folder until search is ready (2026-10-29)

The question: how long from "import folder" until search works, for the typical case of a folder of 7+ GB of logs, and where
does the time go? Every block below gives the date, build, machine, load, corpus, command and numbers (rule 48).

- Machine: Apple M4 Pro, 14 cores, 64 GB, macOS; corpus on a Samsung T7 Shield (USB SSD, about 1 GB/s at best).
- Corpus: `large10`, 56,152,132 events in 8.07 GB, 618 files: 8 log4net text files of 587 MB, 6 CLEF files of 409 MB, 4 gzip
  files, 600 small Serilog text files. It comes from `gen_large.py` scale 1.0 (invented data, never user logs).
- Build: release. The harness `crates/debrief-store-duck/tests/import_ready.rs` is on branch `wip/debrief-duckdb2-prototypes`;
  the code measured is `debrief/duckdb-2` at 9e3bb4a7f. The harness runs the shipped `StoreOptions` in-process with the store
  worker's allocator and times, from the start: the import, the full-text index covering every segment, and the catalogue built.
- Command: `DEBRIEF_READY_CORPUS=…/large10 DEBRIEF_READY_STORE=<new folder> cargo test --release --test import_ready -- --nocapture`,
  through `/Volumes/d7sst7/dev-targets/duck2-bench/ready.sh`. Load is the 1-minute load at the start of each run; the run itself
  adds about 9.
- "Warm" means the corpus was read once just before (`cat > /dev/null`, 9.4 s for 7.2 GB from cold).

Blocks from 2026-10-31 on are newest first, above the 2026-10-29 baseline.

### 2026-10-31, ordering without the second sort (plan SR4, branch `debrief/1.2-wp3`)

- **Commit:** base 3310fa25b (window ordered by the key, then `ORDER BY pos`) → change: the SR4 commit (the ids sorted
  once in a subquery, numbered by `row_number() OVER ()`, `pos::insert_sql`).
- **Machine:** Mac mini M4 Pro, 14 cores, 64 GB, macOS 27.2; shared with other agents.
- **Load:** 1-minute load at the start of each run 4.8 to 9.5 (each run waited for < 10); the 5-minute load was 9 to 15.
- **Build:** the DuckDB 2.0.0 CLI (`v2.0.0 d54cbbe3df`, the version the store links), 4 threads like the store, the
  store's import memory (768 MB, `IMPORT_MEMORY_LIMIT`, which `Inner::reorder` gives the ordering), spilling to a
  temporary folder.
- **Corpus:** the catalog of a `large10` store (`gen_large.py` scale 1.0, invented; 56,152,132 events, 443 segments,
  2.81 GB) imported by `import_ready` with the base commit; each run works on a fresh copy of its `data.duckdb`.
- **Command:** `/Volumes/d7sst7/dev-targets/wp3-bench/pos.sh <store> 1 "A B C"` (`CHECK=1` adds the key-order check
  at 8 GB); A is the shipped statement, B the window without the second sort, C the change.
- **Runs:** one round of A, B and C, and one more of C with the check; rounds 2 and 3 were stopped by a pause for
  machine load (pending). Single runs, so differences under about 20% would be noise; this one is about 4x.
- **Numbers:**

  | Measure | A, base | B, window only | C, change | Budget | Verdict |
  |---|---|---|---|---|---|
  | Ordering, 56M events | 57.0 s | 49.9 s | 11.9 s; 17.7 s | at least 30% faster than A | pass (69 to 79% less) |
  | Rows with `pos <> rowid` | 0 | 0 | 0 | 0 | pass |
  | Positions out of key order (`lag` check) | not run (the check ran out of 768 MB) | not run | 0 | 0 | pass |

- **Memory measure:** none taken; DuckDB was held to 768 MB in every variant (the check alone ran at 8 GB).
- **Notes:** dropping only the second sort (B) saves about 12%; the window sort over one 56M-row partition is the
  cost, and one ordinary sort followed by a streaming row number avoids it. DuckDB documents that a window with an
  empty `OVER` clause keeps the order of its input (<https://duckdb.org/docs/2.0/sql/dialect/order_preservation.html>);
  the unit test `the_ordering_stores_positions_in_time_order_and_in_position_order` checks both orders on 1.5M
  shuffled rows at 4 threads and 64 MB (it fails when the subquery's order is wrong). In the same base import the
  store logged `import: pos 40.7 s` (load 9.9 to 13 during the run, the import itself 154.9 s); the in-import time
  with the change is pending, like rounds 2 and 3. Raw output: `/Volumes/d7sst7/dev-targets/wp3-bench/pos-runs.txt`.

### Baseline

| Run | Load at start | Import | Events/s | Index ready | Catalogue ready | Peak RSS (import / after) |
|---|---|---|---|---|---|---|
| cold | 9.7 | 47.6 s | 1.18M | 222.7 s | 444.6 s | 1,918 / 2,463 MB |
| cold, sampled with sample(1) | 9.7 | 54.6 s | 1.03M | 237.8 s | 433.1 s | 1,880 / 2,511 MB |
| warm | 5.2 | 39.7 s | 1.41M | 208.3 s | 376.4 s | 1,902 / 2,564 MB |
| warm | 9.96 | 46.1 s | 1.22M | 218.7 s | 386.3 s | 1,949 / 2,589 MB |

Store: 4.95 GB (segments 2.82 GB, Tantivy index 1.52 GB, `data.duckdb` 222 MB). Search is ready 5 to 10 times later than the import
ends, so the background builds are the long pole, not the import.

### Where the time goes (sample(1) call trees, self time; `sampcat.py`, `sampwho.py`)

Three windows of the sampled cold run: the import (1 to 47 s), index and catalogue (60 to 200 s), catalogue alone (240 to 420 s).

1. **Import, parse threads** (about 7.5 of 8 busy):
   - Regex 38%. 88% of that is `Regex::captures_at` in `LineParser::line`; the bounded backtracker resolves the groups.
   - Threads blocked in `read()` 19% (cold only).
   - memcpy 10%, other parsing 8%, zstd 6%, Parquet encoding 4%, Arrow batch building about 1%.

   Large files are already split into 64 MB chunks parsed with rayon work stealing, largest file first. Warm, the threads stay
   busy to the end. Cold, a 6 s near-idle stretch (16 to 22 s) is disk waits during the sequential newline pre-scans of the large
   files: each large file is read twice, the second time from the page cache.
2. **End of the import**: about 8 s at 3.5 cores is `pos::rebuild`, 20% of the warm import. Its plan is a window sort and then a
   second full `ORDER BY pos` sort. The sort keys, about 1.3 GB at 56M rows, exceed the 192 MB limit and spill.
3. **Index**: bound by Tantivy's 2 indexing threads (`WRITER_THREADS`, 64 MB heap), busy about 90%. The segment reader and the
   docs pool mostly wait. About 330k docs/s.
4. **Catalogue**: one thread, one source at a time.
   - Entity extraction 34%, DuckDB row fetch 26%, allocation 14%.
   - DuckDB also opens the segment files for every per-source query.
   - About 1 core for 3 to 6 minutes.
5. **No overlap**: the import holds the store's write lock for its whole length (`lib.rs`, `import_keeping`), so the index and
   the catalogue start only when it ends.

### Parse speed: capturing without a rest-of-line tail (microbenchmark)

`LineParser::line` spends 38% of the parse threads' time in `Regex::captures_at` (above). Almost every text format ends in a greedy
rest-of-line group, `(?P<message>.*)$`: the log4net-pattern regex and every output template. The `regex` crate resolves capture
groups with a bounded backtracker over the whole match, so that tail costs as much as the rest of the line.

For one line (no `\n`, no `\r`) the tail matches whatever its prefix matched. So the prefix alone decides every other group, in the
same leftmost-first order, and the tail group is the rest of the line. `debrief_core::text_format` detects the tail on the parsed
pattern and captures with the prefix; any other line, or pattern, uses the whole regex.

- Date and build: 2026-10-29, release, on `debrief/duckdb-2` at 092f59495.
- Machine: M4 Pro, single thread, at load 27 (a relative comparison inside one process; the absolute times are high).
- Corpus: the first 1,000,000 lines of `large10/web/WEB01/orders-web01.log` (invented, `gen_large.py`).
- Command: `rx-bench <file> 1000000` (`duck2-bench/rx`, on `wip/debrief-duckdb2-prototypes`).

| Capture | ns per line | MB/s | Lines matched |
|---|---|---|---|
| whole pattern, `captures` (shipped) | 915 to 925 | 123 to 124 | 909,102 |
| whole pattern, `is_match` only | 119 to 130 | 876 to 952 | 909,102 |
| prefix captures, tail = rest of line | 470 | 242 | 909,102 |

The split captures equal the whole pattern's on all 1M lines, the message lengths included. A one-pass DFA, which would be faster
still, does not build for this pattern ("not one-pass: conflicting transition"). Expected effect on a real import: about half of the
regex share, so roughly 15 to 20% less parse CPU on text logs. The end-to-end import has not been measured yet: the machine had no
quiet window.

### Index writer threads and heap: one round on a busy machine (2026-10-30)

- Build: release, `wip/debrief-duckdb2-prototypes` at 5bac7512c (`ready-w3`). That is index settings, words without counts,
  one UPDATE per step, the ordering fix and lowered threads at QoS utility, the same index code as `debrief/duckdb-2` at
  98afb89ee.
- Corpus: `large10`, warm.
- Command: `exp.sh e4` (`import_ready` with `DEBRIEF_READY_WAIT=index DEBRIEF_READY_INDEX=threads,total_heap_mb`).
- Load: the 1-minute and 5-minute load at the start, and the 1-minute load at the end. Runs that ended above 15 were run
  again; this is round 1 of 3, and rounds 2 and 3 were not run (the machine was busy).
- Index ready is counted from the start of the import; searches are p50 of 20, run right after the index was ready.
- Peak RSS is the resident set size of the harness process (`import_ready`, which runs the store in-process), sampled by
  the harness; it is not the physical footprint (rule 26) and not the store worker's.
- Every variant indexes words without counts; words with counts against without were not compared like-for-like, so
  no size difference is claimed.

| Threads x heap | Load start / end | Import | Index ready | Docs/s after import | Index | Segments | "timeout" p50 (3.7M hits) | Peak RSS (harness) |
|---|---|---|---|---|---|---|---|---|
| 2 x 32 MB (old default) | 9.9, 14.8 / 11.6 | 39.7 s | 225 s | 303k | 1,435 MB | 24 | 374 ms | 2,926 MB |
| 4 x 64 MB | 9.5, 12.0 / 11.2 | 42.1 s | 227 s | 304k | 1,440 MB | 27 | 204 ms | 2,694 MB |
| 4 x 128 MB | 9.8, 11.6 / 13.8 | 58.3 s | 237 s | 314k | 1,441 MB | 23 | 191 ms | 2,793 MB |
| 4 x 256 MB | 7.4, 7.3 / 6.7 | 54.4 s | 281 s | 248k | 1,422 MB | 19 | 198 ms | 2,871 MB |
| 6 x 64 MB | 9.9, 14.7 / 12.0 | 45.4 s | 252 s | 272k | 1,446 MB | 30 | 198 ms | 2,689 MB |
| 6 x 128 MB | 9.7, 10.7 / 13.8 | 51.9 s | 255 s | 276k | 1,439 MB | 36 | 188 ms | 2,710 MB |
| 6 x 256 MB | 9.1, 14.9 / 8.7 | 77.2 s | 327 s | 225k | 1,439 MB | 37 | 146 ms | 2,730 MB |
| 13 x 64 MB | 8.4, 14.7 / 11.4 | 42.3 s | 363 s | 175k | 1,466 MB | 54 | 132 ms | 2,729 MB |
| 13 x 128 MB | 9.3, 14.5 / 9.5 | 64.4 s | 535 s | 119k | 1,471 MB | 53 | 149 ms | 2,741 MB |
| 13 x 256 MB | 8.2, 11.3 / 9.2 | 58.4 s | 549 s | 115k | 1,456 MB | 52 | 137 ms | 2,811 MB |

What the round shows, single runs on a busy machine (the import times scatter from 40 to 77 s), so differences under
about 20% are noise:
- No variant indexes faster than about 315k documents/s (4 x 128 MB); 2 and 4 threads are equal within the noise.
- More threads are slower past 4: 13 threads take 1.6 to 2.4 times as long as 2 or 4.
- More threads also leave more index segments (52 to 54 with 13, 19 to 27 with 2 and 4), and common-word searches ran
  faster with more threads (13 threads 132 to 149 ms, 4 threads 191 to 204 ms, 2 threads 374 ms); why is not
  established.

So the writer threads are no longer the limit. The suspects, not profiled yet, are the pipeline in front of them: one
Parquet reader, a 4-thread docs pool, an in-order feeder, and a commit every 4 segments. Auto is capped at 4 until that is fixed (plan package SR1). An earlier
round with the writer at QoS `BACKGROUND` (efficiency cores only) was void and is not shown.

Pending, on a quiet machine: rounds 2 and 3 of this sweep, and the store worker's physical footprint (rule 26) for each
variant. Until then, these numbers support the Auto cap of 4 and nothing finer.

## Analysis memory on Linux, 2026-10-29

`analysis_memory` (1M events, release, paddock-ci:v2 under amd64 emulation on Apple silicon, DuckDB limit 64 MB): RSS before the
queries 273 MB; anomalies peak 377 MB (+104 MB), Moment +4 MB, compare +1 MB. In CI (run 7102) the RSS before was 224 MB and the
anomalies peak 330 MB. The absolute peak is reported, not asserted; compare used to run out of the 64 MB limit because both of
its statements were open at once.

## The store's waived suites under load, two at a time or all at once (2026-11-09)

Why: CI 7822 (macOS, three runner jobs on one Mac) stopped `differential::answers_like_the_workspace_with_{default,sample}_profile`
and `large_folder::progress_starts_at_once_and_the_found_count_grows` at their 180 s waiver; in CI on a6ec5b419 the same tests
took 95 to 114 s. The FTS merge wait on the same train was ruled out first: with main's `fts.rs` the three took the same
24 to 34 s locally (two runs) as with the train's.

Setup: Mac mini (14 cores), `cargo nextest run -p debrief-store-duck --profile ci`, `NEXTEST_TEST_THREADS=5` (one runner's
core share), ten busy-loop processes in their own process group beside it (stopped after each run), on a machine already
busy with other work (load average 55 before and after each run). Branch `fix/combined-ci-debrief`.

| run | slowest waived test | differential default / sample | analysis default / sample | crate total |
|---|---|---|---|---|
| `duck-heavy` (2 at a time), run 1 | 33.7 s (large folder) | 27.0 / 23.3 s | 19.3 / 17.9 s | 87.5 s |
| no group (all at once) | 47.7 s (large folder) | 32.9 / 28.0 s | 18.3 / 17.5 s | 77.1 s |
| `duck-heavy` (2 at a time), run 2 | 39.1 s (large folder) | 29.1 / 24.7 s | 17.9 / 16.7 s | 95.3 s |

Single runs on a loaded machine. The group takes 18 to 29% off the slowest test, and adds 10 to 18 s to the crate's
wall time (fewer of the heavy suites overlap). The 180 s limit is unchanged (ER-12).

## The store's slow tests made cheaper (2026-11-09)

Why: CI 7825 (macOS, `duck-heavy` group already in place, 8 test threads) stopped
`streaming_import::ids_positions_and_results_do_not_depend_on_workers_or_arrival_order` at 90 s, and the waived tests took
84 s (large folder), 142 s and 131 s (differential, default and sample profile) of their 180 s.

Where the time went, single runs on an idle-ish machine:
- streaming_import: each of its 8 runs imports into a new store, about 30 ms per file with one worker (60 small files); the
  big file's size did not matter (5,000 lines instead of 30,000: same open time).
- differential (default profile, 12.6 s): import of both stores 1.7 s; timelines 5.5 s (the reference Workspace answers a
  smart-search timeline in about 0.75 s, and every query ran at 10 and 400 buckets); facets 2.0 s (13 queries x 7 fields);
  pages 1.8 s; operators 1.1 s.
- large_folder: 3,000 small files and 3 x 200,000-line logs, 16.6 s of import.
- Every test store took the app's thread budgets (up to 8 parse workers, 4 DuckDB threads, 4 index writer threads), so a
  few stores at once oversubscribed a shared runner.

What changed (branch `fix/debrief-duck-macos-cost`): test stores take two parse workers, two DuckDB threads and one index
writer thread (`tests/common::test_options`); streaming_import uses 24 small files, 6 worker/jitter runs instead of 8, and
now compares every event (it compared the first 2,000); differential covers queries x bucket counts and queries x facet
fields pairwise instead of as the full cross product; large_folder defaults to 500 files and 3 x 20,000-line logs (ten
parser chunks each). No limit raised, no group added.

Setup: Mac mini (14 cores), `cargo nextest run -p debrief-store-duck --profile ci`, `NEXTEST_TEST_THREADS=8` (CI 7825's
value), `duck-heavy` group in place, ten busy-loop processes in their own process group beside it (stopped after each run),
on a machine also busy with other work (load averages in the table).

| run (load before) | streaming_import | differential default / sample | large_folder | analysis default / sample | crate |
|---|---|---|---|---|---|
| before 1 (9) | 14.8 s | 27.7 / 23.1 s | 62.9 s | 21.0 / 19.7 s | 117.3 s |
| before 2 (57) | 16.0 s | 32.4 / 27.2 s | 45.9 s | 26.6 / 25.8 s | 111.3 s |
| after 1 (23) | 7.2 s | 21.6 / 18.2 s | 6.0 s | 22.7 / 22.1 s | 56.4 s |
| after 2 (29) | 7.7 s | 22.5 / 20.5 s | 6.1 s | 22.3 / 19.8 s | 56.2 s |

Single runs on a loaded machine. streaming_import about halves, large_folder drops by a factor of 8 to 10, the crate by
half. The differential tests drop by about a quarter only: what is left is the import of the samples into both stores and
the reference Workspace's smart-search answers (pages, timelines, facets of the two smart-search queries), which this change
does not touch. Six repeated runs of the changed tests (profile ci, no extra load) passed in 15.3 to 16.3 s.
