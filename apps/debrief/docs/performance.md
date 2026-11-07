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

### 2026-11-07, DuckLake trial 2 (branch `docs/debrief-ducklake-trial-2`)

Quoted by [research/2026-11-07-ducklake-trial-2.md](research/2026-11-07-ducklake-trial-2.md). Runs taken 2026-11-07 10:26
to 15:57, plus three targeted catalogue runs from 16:21 to 16:37.

- **Commit:** store at 19aff3b83 (`debrief/1.2-int` plus the DuckDB 2.0 storage format) with the WP-1 page fix
  cherry-picked (c149483e9 and 13e165c75 from `origin/perf/debrief-grid-query`), plus a test-only patch selecting the
  variants (`DEBRIEF_TRIAL_STORE`, `DEBRIEF_TRIAL_ROW_GROUP`; copy:
  [trial2-store-patch.diff.txt](research/2026-11-07-ducklake-trial-2/trial2-store-patch.diff.txt),
  [trial_lake.rs.txt](research/2026-11-07-ducklake-trial-2/trial_lake.rs.txt)) and the harness
  ([ducklake_trial.rs.txt](research/2026-11-07-ducklake-trial-2/ducklake_trial.rs.txt)). None of it is in the product.
- **Machine:** Mac mini M4 Pro, 14 cores, 64 GB, macOS 27.2; data and build on d7 (APFS).
- **Load and CI:**
  - **Start gate:** every run started only at a 1-minute load of 12 or less with no macOS CI job in progress (`run2.sh`,
    checked before and again inside the lock).
  - **Peak loads:** the highest load during each run is in [runs.txt](research/2026-11-07-ducklake-trial-2/runs.txt).
    10M and layout runs: 3.6 to 7.0, except `rg-10m-16384-3` at 12.53. 16 of the 23 counted 56M store runs peaked at 12.5
    to 32 from other agents' jobs. They are counted and flagged, with a flag column in runs.txt; replacements at
    15:37–15:57 were no quieter (16.2 and 16.4), so the time box stopped them. The one clean run per variant: B0 170.9,
    B1 92.0, B2 98.6, B3 118.6 ms page p50. Import, searchable and during-import times are load-sensitive.
  - **CI overlap:** `f-56m-base-1` overlapped macOS CI (7657/30879) and is excluded. Earlier runs of the day (`t2-*`,
    `q-*`) are not counted: they ran before the WP-1 fix, or while the CI probe was blind, and most overlapped macOS CI.
- **Build:** release, `CARGO_BUILD_JOBS=2`, mimalloc, duckdb-rs 1.20000.0 with libduckdb 2.0.0, `ducklake`
  `417a9117f5` loaded by path. Every store variant uses the store's own connection settings: 4 DuckDB threads, 192 MB,
  768 MB while an import compacts and orders. B3's lake UPDATE ran at 768 MB without insertion order (at 192 MB it ran
  out of memory). The layout runs used 4 threads and 192 MB for queries, 768 MB for writes.
- **Corpus:** as trial 1 (invented; 10M: `gen_large.py OUT 0.18`; 56M: `large10`), two imports, the same query.
- **Commands:**
  - `all2.sh` (layouts with `lrun.sh`, store variants and the row-group sweep with `run2.sh`, at 10M then 56M);
  - `post2.sh` (1-hour buckets, worker imports through `wrun.sh`, one replacement run);
  - `repl2.sh` (replacements).

  The scripts are in [research/2026-11-07-ducklake-trial-2/](research/2026-11-07-ducklake-trial-2/) as `.txt` copies.
- **Runs:** 3 per cell (at 56M: B0 runs 2–5 and B1 runs 1–4 counted). The worker run and the 1-hour bucket runs were single passes and 3 failed
  passes respectively.
- **Numbers and verdicts against the budgets:**

  | Measure (56M unless stated) | B0 | B1 pruned | B2 mirror | B3 native | Budget | Verdict |
  |---|---|---|---|---|---|---|
  | Page query quiet, p50 | 168–176 ms | 91–108 ms | 91–100 ms | 117–123 ms | interactive < 0.5 s cold | met warm (B1 and B2 about 1.8x faster than B0); cold not measured |
  | Page query during import 2, p50 / max | 295–1,900 / 3,265–4,542 ms | 174–1,737 / 410–4,484 ms | 172–1,769 / 3,112–4,981 ms | 208–1,674 / 3,698–4,244 ms | < 0.5 s | all over at p50 in some runs; at max in every run but one B1 run (410 ms) |
  | Plain field filter, quiet p50 | 1,621–1,649 ms | 1,616–1,777 ms | 1,687–1,745 ms | 1,730–1,758 ms | < 0.5 s | all over |
  | Import 1 (54.4M) | 30.3–41.7 s | 35.6–43.3 s | 40.9–45.4 s | 34.8–42.9 s | ≥ 218k events/s | all met (1.2–1.8M/s) |
  | Searchable end to end | 306–332 s | 317–329 s | 275–290 s | 274–287 s | none (exploratory) | B2 and B3 faster: the catalogue's per-source reads (targeted runs: catalogue B0 270 s, B2 239 s, B0 with per-source file lists 249 s; index 98–120 s) |
  | Import time per GB (7.32 GB) | 4.1–5.7 s/GB | 4.9–5.9 s/GB | 5.6–6.2 s/GB | 4.8–5.9 s/GB | D7: ≤ 6 s/GB | met, except B2 over in some runs |
  | Catalogue memory | not separable | not separable | not separable | not separable | D7: catalogue ≤ 512 MB | not applicable here: the background peaks (1.6–1.9 GB at 56M) mix the catalogue with the index build in one process |
  | Undo, searchable again | 5.22–5.55 s | 4.97–5.77 s | 5.71–8.20 s | 5.41–6.16 s | none (exploratory) | — |
  | Time shift (6.4M rows), searchable again | 33.5–41.3 s | 32.3–37.9 s | 35.2–42.8 s | 29.8–37.0 s | none (exploratory) | — |
  | Disk after shift | 4,902–4,976 MB | 4,894–5,034 MB | 4,874–4,966 MB | 5,567–5,696 MB | none (exploratory) | — |
  | Moved folder answers the same | yes | yes | yes | no | correctness | B3 fails |
  | Peak footprint, import phase (in-process) | 1,263–1,581 MB | 1,383–1,487 MB | 1,234–1,282 MB | 1,461–1,521 MB | worker ≤ 768 MB importing (2 GB cap) | not comparable in-process; see the worker row |
  | Worker peak footprint, import 1 (`debrief import`) | 1,388 MB | not measured | check broken | not measured | ≤ 768 MB, cap 2 GB | B0 over the budget, under the cap |
  | `tests/it` (95) | 95 pass | 95 pass | 93 pass | 88 pass | all pass | B2 and B3 fail |

  10M, page query quiet p50:

  - **B0:** 78.4–78.9 ms;
  - **B1:** 64.7–67.0 ms;
  - **B2:** 71.3–73.7 ms;
  - **B3:** 89.3–91.2 ms.

  Undo searchable 0.79–0.93 s and shift searchable 4.5–8.0 s for all. At 10M every query met the 0.5 s budget warm, the plain
  filter included (332–393 ms).

  C2, rows per row group, 56M page query quiet p50: the budget is interactive < 0.5 s, met warm by all four; 65,536 is
  tied with 32,768 at 56M, so keep the default.

  - **16,384:** 249.9–273.4 ms;
  - **32,768:** 172.9–189.2 ms;
  - **65,536:** 167.6–175.5 ms;
  - **131,072:** 200.9–206.6 ms.

  At 10M the order was 61.6–61.9, 57.7–63.4, 78.4–78.9 and 120.1–125.8 ms.

  C1, C3 and C4 at the SQL level, 56M (the page's SQL shape, standard window p50; 10 runs):

  - **seg (all files):** 112.3–114.1 ms;
  - **seg_pruned (ts min/max):** 24.2–24.8 ms;
  - **lake (DuckLake adopt):** 52.7–56.7 ms;
  - **sorted (C3):** 9.6–12.4 ms;
  - **b10 (C4):** 11.8–14.7 ms, and 8.7–8.8 ms after its merge.

  Budget: none (exploratory). Costs of the rewritten layouts:

  - **sorted rewrite:** 74.5–87.7 s, peak 1.3–1.6 GB;
  - **b10 write:** import 1 160.4–189.4 s, import 2 1.8–3.3 s, producing 58 late files; the merge took 21.2–21.8 s;
  - **b60 (1-hour buckets):** out of memory at 768 MB at both sizes, so every `lay-*` run ended with rc 101 at that step;
    the earlier layouts completed. 10M: 3 of 3 with insertion order on. 56M: 3 of 3 on and 3 of 3 off.

  C1 per 45 s window, 56M segments: 18.3 files and 22.8 row groups by range, against 12.3 files and 12.7 row groups
  holding rows (sorted: 1.6 / 1.7; b10: 1.6 / 6.3).

- **Memory measure:** physical footprint (`ri_phys_footprint`): in-process 20 ms phase peaks and the `procsample.py`
  whole-run peak; for the worker run, `procsample.py` over the CLI and its worker process.
- **Notes:**
  - B2 and B3 are measured with the correctness failures listed in the note.
  - The worker run for B2 is "check broken": the worker does not inherit `DEBRIEF_TRIAL_STORE`. A 10M check made no lake.
  - C1, C3 and C4 are SQL-level only. The store's page path adds about 65 ms over its SQL shape at 56M (B1 91–108 ms
    against 24 ms), which is not attributed.
  - The committed harness copy is the latest version: insertion order off for bucket writes, the index/catalogue split
    and `TRIAL_STOP_AFTER_READY`. The `lay-*` runs used the version before it.
  - Raw output is in `/Volumes/d7sst7/dev-targets/scratch/ducklake-trial/out2/`.

### 2026-11-06, DuckLake trial (branch `docs/debrief-ducklake-trial`)

Quoted by [research/2026-11-06-ducklake-trial.md](research/2026-11-06-ducklake-trial.md). Runs taken 2026-11-07 01:15 to
03:20.

- **Commit:** store at `origin/feat/debrief-duckdb2-storage-format` 19aff3b83 (`debrief/1.2-int` plus the DuckDB 2.0
  storage format) with the trial harness `tests/ducklake_trial.rs` added, not committed to the crate (copy:
  [ducklake_trial.rs.txt](research/2026-11-06-ducklake-trial/ducklake_trial.rs.txt))
- **Machine:** Mac mini M4 Pro, 14 cores, 64 GB, macOS 27.2; data and build on d7 (APFS)
- **Load:** the 1-minute load at the start and end of every run and its highest 5-second sample are in
  [runs.txt](research/2026-11-06-ducklake-trial/runs.txt). Counted runs: highest 1-minute load 3.0 to 9.5, except
  `l10m-sqlite-1..3` (13.1 to 14.7: flagged, replaced by `l10m-sqlite-4..6` at 3.6 to 4.5). macOS CI: the probe used during
  these runs could not read the Gitea API (a Python certificate error it reported as "none"), so the check was blind at
  the time (ER-33). Reconstructed on 2026-11-07 from the job start and end times of every macOS job
  ([ci-mac-jobs.txt](research/2026-11-07-ducklake-trial-2/ci-mac-jobs.txt), runs 7630 onwards, `overlap.py`): none of the
  77 runs overlapped a macOS CI job ([trial1-ci-overlap.txt](research/2026-11-07-ducklake-trial-2/trial1-ci-overlap.txt)). A first 56M pass used a query window that matched 37
  rows; it was discarded and rerun (one of its store runs peaked at load 70.9). The review runs (768 MB with 4 threads,
  4-thread raw and adopt queries, 03:31 to 03:36) ran at a highest 1-minute load of 1.9 to 3.8.
- **Build:** release (`cargo test --release --test ducklake_trial --no-run`, `CARGO_BUILD_JOBS=2`), mimalloc (the store
  worker's allocator), duckdb-rs 1.20000.0 with the official prebuilt libduckdb 2.0.0 (osx-arm64), `ducklake` extension
  `417a9117f5` (spec 1.1) and `sqlite_scanner` for DuckDB v2.0.0, loaded by path with autoinstall and autoload off. Store
  defaults: 8 parse threads, 4 DuckDB threads, DuckDB limit 192 MB (768 MB while an import compacts and orders). DuckLake
  runs: 8 DuckDB threads, no memory limit unless stated.
- **Corpus:** invented. 10M: `gen_large.py OUT 0.18` (1.45 GB), 10,108,925 events in 126 files. 56M: the kept `large10`
  (`gen_large.py OUT 1.0`, 7.2 GB), 56,152,132 events in 618 files. Each split into two imports: everything except
  `clef/WEB12` (9,801,191 and 54,441,640 events), then `clef/WEB12` (307,734 and 1,710,492 events, one CLEF file).
- **Command:** each run went through plock (normal lane) via `run.sh LABEL ENV...`. The series were `series.sh 10m`,
  `extra.sh 10m 1791936600000 1791936645000`, `series.sh 56m` and `NO768=1 extra.sh 56m 1791936600000 1791936645000`,
  then for the review `run.sh l<size>-fair768-<n> TRIAL_MODE=lake TRIAL_ZSTD=1 TRIAL_MEMORY_LIMIT=768MB TRIAL_THREADS=4
  TRIAL_NO_ORDER=1 ...`, `run.sh r56m-t4-<n> TRIAL_MODE=raw TRIAL_THREADS=4 ...` and `run.sh l56m-adopt-t4-<n>
  TRIAL_MODE=lake TRIAL_LAKE_IMPORT=adopt TRIAL_THREADS=4 ...`
  (scripts kept as `.txt` copies in [research/2026-11-06-ducklake-trial/](research/2026-11-06-ducklake-trial/)). Modes:
  - `TRIAL_MODE=store`: the store imports import 1, then import 2, each with a query thread every 250 ms; waits for the
    index and the catalogue; runs `remove_source` on import 2 (undo); runs `set_source_offset` +1 h on the largest
    sources whose share is nearest 10% of the rows (11.3% here); waits for the index again; runs `compact`.
  - `TRIAL_MODE=lake`: a DuckLake table is filled from the first store run's kept segments (INSERT, or
    `ducklake_add_data_files` for "adopt"), with the same query thread. Then a DELETE of import 2's sources, an UPDATE of
    `ts` on the same sources as the store, and `ducklake_rewrite_data_files`, `_merge_adjacent_files`,
    `_expire_snapshots` and `_cleanup_old_files`.
  - `TRIAL_MODE=raw`: the same SQL as the lake runs, over the store's segment files.
  - Query: events from 2026-10-14 00:10:00 to 00:10:45 UTC where field `Command` = `Reserve` (205 rows; 138 after the
    undo).
- **Runs:** 3 per cell; each cell gives the range over its 3 runs.
- **Numbers (10M):**

  | Measure | Store (today) | DuckLake insert, snappy | DuckLake insert, zstd | DuckLake adopt | DuckLake, SQLite catalog |
  |---|---|---|---|---|---|
  | Import 1 (9.8M) | 5.52–6.23 s, 1.57–1.77M events/s, parsing included | write only: 1.92–5.14 s, 1.91–5.09M/s | write only: 1.91–1.94 s, 5.06–5.12M/s | register 80 files: 0.20–0.27 s | write only: 1.97–4.72 s |
  | Import 2 (0.31M) | 1.73–3.03 s | 0.16–0.20 s | 0.22 s | 0.01–0.02 s | 0.17–0.25 s |
  | Query during import 1, p50 / p95 / max | page: 70.6–88.9 / 141.8–282.9 / 183.8–465.1 ms | SQL: 22.6–25.2 / 34.1–108.2 / 83.9–192.3 ms | 30.5–45.5 / 103.7–115.1 / 103.7–115.1 ms | 15.8–19.4 ms (1 query each) | 23.5–26.8 / 33.8–83.5 / 33.8–110.1 ms |
  | Quiet query, p50 | page: 146.0–151.3 ms; same SQL over the segments (raw): 19.2–19.4 ms | 25.4–25.7 ms | 32.0–32.2 ms | 17.5–27.9 ms | 26.4–29.9 ms |
  | Disk after the imports | segments 504.4–504.9 MB in 82–84 files, data.duckdb 100.7–131.9 MB (full-text index 257.2–258.0 MB, needed by both) | 749.1 MB in 12 files, catalog 0.1 MB | 426.5–426.9 MB in 12 files | no new data (the segments, hard-linked), catalog 0.2 MB | 749.1 MB, catalog 0.2 MB |
  | Index and catalogue after the import | 46.3–50.7 s | n/a (not part of DuckLake) | | | |
  | Undo import 2 | 0.98–1.40 s (space freed at once) | 0.038–0.041 s (DELETE; no space freed) | 0.036–0.038 s | 0.021–0.037 s | 0.044–0.045 s |
  | Time shift, 1,147,009 rows (11.3%) | 4.04–7.54 s; searchable again after 6.94–11.08 s | 0.418–0.874 s (UPDATE; no index or positions) | 0.449–0.494 s | 0.395–0.429 s | 0.444–0.490 s |
  | Compaction | 0.034–0.037 s (nothing left to merge) | 0.41–1.43 s | 0.36 s | 2.22–4.97 s (rewrites the adopted segments as snappy) | 0.42–0.45 s |
  | Disk after compaction | segments 484.6–485.1 MB in 79–81 files, data.duckdb 147.9–149.2 MB | 820.6 MB in 19 files | 467.2–467.6 MB in 19 files | 751.1–751.3 MB in 4 files | 820.6 MB in 19 files |
  | Peak footprint, whole run | 993–1,129 MB (import phase 916–941 MB) | 1,703–1,767 MB | 1,621–1,653 MB | 1,516–1,726 MB (registration 33–36 MB) | 1,669–1,738 MB |

  With zstd and a 768 MB DuckDB limit:
  - **8 threads, DuckDB's defaults:** the write took 2.53–3.33 s and the peak was 1,348–1,432 MB, over 3 runs. One of
    the 3 (`l10m-zstd768-2`, rc=101) then failed with "Out of Memory Error" in `ducklake_merge_adjacent_files`; its later
    cells are missing.
  - **4 threads and `preserve_insertion_order = false`** (`l10m-fair768-1..3`, all passed): the write took 2.79–2.81 s,
    the query during the import 48.5–49.4 / 121.5–125.0 / 121.5–125.0 ms and the quiet query 42.1–43.1 ms. Disk was
    426.2–427.0 MB, undo 0.031–0.034 s, shift 0.600–0.634 s and compaction 0.589–0.601 s, leaving 466.8–467.6 MB. The
    peak was 940–986 MB.

- **Numbers (56M):**

  | Measure | Store (today) | DuckLake insert, snappy | DuckLake insert, zstd | DuckLake adopt | DuckLake, SQLite catalog |
  |---|---|---|---|---|---|
  | Import 1 (54.4M) | 27.92–37.24 s, 1.46–1.95M events/s, parsing included | write only: 10.65–13.42 s, 4.06–5.11M/s | write only: 18.35–20.22 s, 2.69–2.97M/s | register 428 files (2.5 GB): 1.04–1.05 s | write only: 16.75–28.21 s, 1.93–3.25M/s |
  | Import 2 (1.7M) | 6.13–6.40 s | 0.59–0.70 s | 0.88–1.06 s | 0.05–0.06 s | 1.27–1.51 s |
  | Query during import 1, p50 / p95 / max | page: 176.9–483.4 / 819.1–884.5 / 2,108.5–2,171.1 ms | SQL: 25.4–25.6 / 58.6–126.2 / 67.6–131.6 ms | 28.1–43.6 / 79.5–110.9 / 84.6–118.9 ms | 18.2–18.9 ms (4 queries each) | 23.9–25.8 / 47.6–73.3 / 92.5–96.7 ms |
  | Query during import 2, p50 | page: 1,423.0–1,458.6 ms (p95 up to 3,074.1 ms) | 25.2–83.7 ms | 51.4–97.2 ms | 18.5–20.7 ms | 32.6–37.6 ms |
  | Quiet query, p50 | page: 476.0–484.5 ms; raw SQL over the segments: 54.3–55.2 ms | 32.4–34.8 ms | 38.6–43.1 ms | 22.2–22.3 ms | 29.7–33.0 ms |
  | Disk after the imports | segments 2,808.2–2,816.7 MB in 441–446 files, data.duckdb 389.6–496.3 MB (index 1,422.7–1,423.6 MB) | 4,150.0–4,150.1 MB in 58 files, catalog 0.4 MB | 2,376.0–2,376.5 MB in 58 files | no new data, catalog 1.0 MB | 4,150.0–4,150.1 MB, catalog 0.2 MB |
  | Index and catalogue after the import | 266.1–268.9 s | n/a | | | |
  | Undo import 2 | 5.07–5.13 s | 0.100–0.138 s (no space freed) | 0.098–0.103 s | 0.070–0.074 s | 0.101–0.110 s |
  | Time shift, 6,369,152 rows (11.3%) | 22.23–23.77 s; searchable again after 38.27–40.62 s | 2.39–2.44 s (no index or positions) | 3.60–3.96 s | 2.19–3.60 s | 4.37–5.11 s |
  | Compaction | 0.157–0.164 s (nothing left to merge) | 1.70–1.74 s | 3.92–4.50 s | 8.44–17.42 s | 5.32–7.12 s |
  | Disk after compaction | segments 2,698.0–2,706.6 MB in 422–428 files, data.duckdb 629.4–630.2 MB | 4,576.3–4,576.7 MB in 84 files | 2,608.2–2,608.3 MB in 82 files | 3,907.3–3,908.2 MB in 70 files | 4,575.7–4,575.9 MB in 84 files |
  | Peak footprint, whole run | 1,361–1,671 MB (import phase 1,268–1,287 MB) | 1,936–2,043 MB | 1,880–1,943 MB | 2,562–2,599 MB (in compaction; registration 126–137 MB) | 1,903–1,958 MB |

  With zstd and a 768 MB DuckDB limit at 56M:
  - **8 threads, defaults:** run only in the discarded first pass, where 3 of 3 failed with "Out of Memory Error" in the
    time-shift UPDATE (`w8s/out/l56m-zstd768-*`).
  - **4 threads and `preserve_insertion_order = false`** (`l56m-fair768-1..3`, all passed): the write took
    14.95–15.04 s, the query during the import 48.4–52.7 / 147.3–154.2 / 156.7–161.8 ms, and during import 2 a p50 of
    55.1–68.3 ms. The quiet query took 53.6–54.2 ms. Disk was 2,374.6–2,375.3 MB, undo 0.115–0.119 s, shift
    3.46–3.51 s and compaction 2.87–2.96 s, leaving 2,606.0–2,607.0 MB. The peak was 1,206–1,269 MB.

  At 4 DuckDB threads, the quiet query over the store's 441–446 segments took 62.6–63.8 ms as plain SQL
  (`r56m-t4-1..3`) and 31.4–31.5 ms through DuckLake adopting the same files (`l56m-adopt-t4-1..3`). The adopt runs at 4
  threads (no memory limit) peaked at 1,371–1,568 MB, in the time shift and compaction.

- **Budgets and verdicts** (`docs/performance.md`, Budgets):

  | Measure | Budget | Verdict |
  |---|---|---|
  | Events-grid query | interactive < 0.5 s (ADR 0010, 0014) | store page: met quietly (146–151 ms at 10M, 476–485 ms at 56M, close to the limit); over during an import at 56M (p50 1,423–1,459 ms during import 2, max 2.1 s during import 1). DuckLake and raw SQL: met everywhere (under 200 ms) |
  | Store worker importing | ≤ 768 MB (hard cap 2 GB) | not comparable: this harness runs the import, index, catalogue and queries in one test process. Store 916–1,287 MB in the import phase; DuckLake uncapped 1.6–2.6 GB (over the 2 GB hard cap in adopt's compaction at 56M); DuckLake with DuckDB capped at 768 MB and 4 threads 940–1,269 MB |
  | Import throughput | ≥ 218k events/s | store 1.46–1.95M events/s: met. DuckLake end to end: not measured |
  | Other measures (disk, undo, time shift, compaction, file counts) | none, exploratory | none |

- **Memory measure:** physical footprint (`ri_phys_footprint`), sampled every 20 ms in the process for the phase peaks
  and every 0.5 s by `procsample.py` for the whole-run peak.
- **Notes:**
  - The DuckLake "write only" cells start from rows that are already parsed into Parquet segments. They leave out
    parsing, and also the ordering into time positions and the catalogue, which a store built on DuckLake would still
    need. The store column includes all of that, so the "write only" cells are not end-to-end import rates.
  - The store's query goes through `Query::page` (the events grid); the DuckLake and raw cells run one SQL statement.
    Compare the raw cells with the DuckLake cells for the storage layer, and use the page cells for what the app shows
    today.
  - DuckLake's DELETE and UPDATE leave the old rows in their files, and the default `ducklake_rewrite_data_files`
    rewrote nothing here, so the disk after compaction is larger than after the import. A lower delete threshold was not
    tried.
  - An Appender into a DuckLake table works (1,000 rows into a probe table); a path that parses straight into an
    Appender was not measured.
  - The page query runs on the store's connection with 4 DuckDB threads and a 192 MB limit; the raw and DuckLake SQL ran
    with 8 threads (4 in the cells marked so) and no limit unless stated. That confound is the first hypothesis for the
    page-against-SQL gap.
  - DuckLake against the store: undo 22–39 times faster at 10M and 37–52 times at 56M; shift 5–19 times at 10M and 4–11
    times at 56M. The store's times include compaction, positions and, for the shift, the index; how much each phase
    accounts for was not measured.
  - Unexplained spreads, given as ranges: `l10m-insert-1` 1.92 s against 4.86–5.14 s; `s56m-1` 37.24 s against
    27.92–28.36 s; `l56m-sqlite-*` 16.75–28.21 s.
  - The adopt "query during the import" cells are 1 query at 10M and 4 queries at 56M, taken during a registration of
    0.2 s and 1 s.
  - Raw output is in `/Volumes/d7sst7/dev-targets/scratch/ducklake-trial/out/`, and the discarded first 56M pass is in
    `w8s/`.

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
