# Benchmarks

This page covers every benchmark, budget and recorded performance result in the Paddock family: what exists, how to run
it so the numbers mean something, which budgets it checks, the latest results, and what is missing. Each app's full list
and history is in its `docs/performance.md` (links below). Rule 16 says performance has budgets checked in the nightly
benchmark workflow. Rule 35 covers bundle size. Rule 26 says memory is real memory (footprint, PSS or private working
set), never RSS.

Inventory taken on 2026-10-28 from `origin/main` at 78d5b0f3c, plus the branches still in flight, which are named where
they are used.

## Running a benchmark reproducibly

1. **Use synthetic data only.** Use the invented `/samples`, a generator named in the tables below, or data a test
   generates itself. Never use a user's logs, folders or captures, and never put them in a corpus, fixture, doc or
   commit (ER-14; [lessons](lessons.md): pasted logs are confidential). If a real-world shape matters, invent data with
   the same shape.
2. **Use a quiet machine.** The Mac mini is shared by up to five agents. Record `uptime` before every run. Run only when
   the 1-minute load is below about 12 (the search-perf runbook's threshold) and nothing else is building or testing,
   and say so in the result. A number taken at load 30–90 is an indication. It can be compared only with an A/B run in
   the same session. Since 2026-10-22 the family has had no quiet machine for benchmarks. Most results below were taken
   under load.
3. **Use a release build** (`cargo test --release`, `cargo bench`), with the shipped allocator. Debrief's benches pull
   in `debrief-alloc`. A debug number only says that nothing blew up. Name the build in the result.
4. **Interleave A/B runs.** Run base, change, base, change (at least two pairs), one at a time, from prebuilt binaries
   (copy the test executable that `cargo test --no-run` prints). Drift then cancels out, and the change is not
   rebuilt between runs. Give ranges, not one number.
5. **Keep the build gentle.** Use `CARGO_BUILD_JOBS=2`, a private `CARGO_TARGET_DIR` per app per worktree, and at most
   one heavy job at a time through the machine's semaphore (ER-24, rule 41). Build before the measuring window, and
   never measure while compiling.
6. **Measure real memory.** Use physical footprint on macOS (`apps/debrief/scripts/perf/procsample.py`, or
   `paddock-proc`), PSS on Linux, and the private working set on Windows. Some older harnesses still sample RSS (see
   Gaps). When a result shows RSS, label it RSS.
7. **Record the run** with the [results template](#results-template) in the app's `docs/performance.md`, newest first.
   Do this even when the run first appears in an ADR, a research note or a pull request.

## Results template

Every benchmark write-up uses this block, in the app's `docs/performance.md` (newest first). An ADR or research note
that quotes a result links to the block instead of carrying the only copy.

```markdown
### YYYY-MM-DD, <what was measured> (<branch or tag>)

- **Commit:** <short sha> (A/B: base <sha> → change <sha>)
- **Machine:** <host, CPU, cores, RAM, OS and version> (for example Mac mini M4 Pro, 14 cores, 64 GB, macOS 27.2)
- **Load:** 1-minute load <min–max> during the runs; <other jobs running, if any>
- **Build:** release | debug; allocator; feature flags; library versions (for example libduckdb 1.5.6)
- **Corpus:** <generator and arguments> (for example `gen_large.py OUT 0.3`), <events, files, bytes>; synthetic
- **Command:** `<exact command, with every env var>`
- **Runs:** <n> runs, interleaved A/B | single; what is reported (median, range, p95)
- **Numbers:**

  | Measure | Base | Change | Budget | Verdict |
  |---|---|---|---|---|

- **Memory measure:** footprint | PSS | private working set | RSS (only for an older harness, say so)
- **Notes:** noise, anomalies, what is not measured, where the raw output is kept
```

## All benchmarks

The CI column names the workflow and job. "No" means nobody runs it unless they choose to. Details, env vars and
corpora are in each app's file.

### Shared crates, UI, conformance and scripts

| Benchmark | Measures | Command | Corpus | Budget asserted | CI | Results |
|---|---|---|---|---|---|---|
| [`crates/paddock-names/benches/search.rs`](../crates/paddock-names/benches/search.rs) | Catalogue build; search median, p95 and worst per query kind | `cd crates && cargo bench -p paddock-names`; `PADDOCK_BENCH_ENTRIES` (250 000), `PADDOCK_BENCH_BUDGET_MS` (50) | Names generated in the bench | Exits with an error when a full search takes over 50 ms (the palette budget) | No (compiled by `clippy --all-targets` only) | CHANGELOG 0.3.0 |
| [`crates/paddock-jobs/benches/walk.rs`](../crates/paddock-jobs/benches/walk.rs) | Parallel against sequential folder walk, warm cache | `cargo bench -p paddock-jobs`; `PADDOCK_BENCH_FILES` (60 000) | A tree generated in a temp folder | none (checks the same files are found) | No | [ADR 0008](adr/0008-long-operations.md) |
| [`crates/paddock-download/benches/segmented.rs`](../crates/paddock-download/benches/segmented.rs) | One stream against 2, 4 and 8 segmented connections, each connection rate-limited | `cargo bench -p paddock-download --features http`; `PADDOCK_BENCH_MIB` (32), `PADDOCK_BENCH_RATE_MIB` (8) | An in-process server; no network | none | No | [lessons](lessons.md) |
| [`crates/paddock-bench`](../crates/paddock-bench) + [`bench-nightly.yml` template](../.gitea/workflow-templates/bench-nightly.yml) | The harness for rule 16: `Recorder`, `bench/<app>.results.json`, `bench/budgets.toml` (limit, tolerance, regression), `paddock-bench check` | see [crates/README.md](../crates/README.md#paddock-bench-plan-x26) | — | Whatever an app's `budgets.toml` says | **No app uses it yet** | — |
| `perf/proc-per-pid`: `crates/paddock-proc/examples/watch_cpu.rs` (branch, not on main) | A process watcher's own CPU: whole process table against one pid | `cargo run --release -p paddock-proc --example watch_cpu -- 60` (Unix) | A `sleep` child | none (ADR 0024 D7: ≤ 0.5% of a core) | No | branch CHANGELOG |
| C29 window weight ([`conformance/src/window-weight`](../conformance/src/window-weight)) | JS each window loads before first paint (rule 35) | `ng build --stats-json`, then `paddock-conformance window-weight [kind] [--max kB] [--record]` | The build | `windowWeight` in `paddock.conformance.json`, **not configured anywhere** (C29 warns) | `ci.yml` conformance (warns) | — |
| Angular build budgets in `ui/angular.json` | Initial chunk size | `npm run build -w ui` (and each demo's build) | — | gallery 1.5 MB / 3 MB; shell-demo 500 kB / 1 MB; workbench-demo 580 kB / 1 MB (warn / error) | `ci.yml` ui jobs, through `ng build` | — |
| [`scripts/ci-timings.mjs`](../scripts/ci-timings.mjs) | CI wall time, queueing, runners, per-step and compile/test phases, critical path (Gitea API, read-only) | `GITEA_TOKEN=… node scripts/ci-timings.mjs [--repos …] [--limit 30] [--out docs/research/2026-10-ci-timings.md] [--json F]` | Gitea run history | none | No (manual) | [research/2026-10-ci-timings.md](research/2026-10-ci-timings.md) |

Not benchmarks, though the names suggest it:
- `ui/e2e/shell-demo-startup.spec.ts` checks the start-up contract (splash, ready, pings). Its 1.5 s busy loop simulates
  a frozen page.
- `markdown.spec.ts` `lastParsed` counts characters, not time.
- The timing bounds in Rust tests are hang and cancel guards, not budgets: Dyno IPC < 280 ms, Spotter engine 2–10 s,
  `paddock-assistant` stop within 100 ms.

### Per app

| App | Benchmarks | Where |
|---|---|---|
| Debrief | 16 Rust benches and measurement tests (engine criterion; store at 10M; reads; analysis, import and worker memory; idle CPU; plugin idle; evtx memory; streaming; names; spans; profile engine; layout bake-off; large folder; `fts_bench` on a branch), the start-up benchmark, first-paint JS, Angular budgets, the macOS `scripts/perf` toolkit | [apps/debrief/docs/performance.md](../apps/debrief/docs/performance.md) |
| Dyno | `dyno-agent bench` (the agent's own CPU and memory); Angular budgets. `dyno-bench` and the Benchmarks page are a **product feature** (benchmarking the user's machine), not a test of Dyno | [apps/dyno/docs/performance.md](../apps/dyno/docs/performance.md) |
| Pitcrew | Angular budgets; the auto-scale simulation (branch) | [apps/pitcrew/docs/performance.md](../apps/pitcrew/docs/performance.md) |
| Pitwall | Angular budgets. Its Benchmarks page and `pitwall-core/src/benchmarks.rs` are a **product feature** (model benchmarks) | [apps/pitwall/docs/performance.md](../apps/pitwall/docs/performance.md) |
| Spotter | Angular budgets | [apps/spotter/docs/performance.md](../apps/spotter/docs/performance.md) |
| Pacenotes | The embedding comparison of ADR 0001 (research harness, no code in the tree) | [apps/pacenotes/docs/performance.md](../apps/pacenotes/docs/performance.md) |
| Sighter | none (budgets only, in the proposal and the design review) | [apps/sighter/docs/performance.md](../apps/sighter/docs/performance.md) |
| shell-demo, workbench-demo | Angular budgets in `ui/angular.json` (above); no other benchmark | this page |

### Workflows that run benchmarks

| Workflow | Job and step | When | Output |
|---|---|---|---|
| [`debrief-bench.yml`](../.gitea/workflows/debrief-bench.yml) | `bench`: `cargo bench -p debrief-core --bench engine -- --quick` | Push to main touching `debrief-core`, `debrief-store-duck` or `debrief-store-worker`; nightly 03:47; dispatch | Artifact `criterion-report` |
| `debrief-bench.yml` | `store-bench`: `bench` and `bench_reads` at 10M events (report only), and the worker memory tests with 20 copies | Nightly and dispatch (skipped while main has not changed since the last green night) | Job log only |
| [`debrief.yml`](../.gitea/workflows/debrief.yml) | `rust-test`, step "Memory budget": `analysis_memory` (release, Linux, `continue-on-error`) | Every run | Job log |
| `debrief.yml` | `frontend`, step "First-paint JS budget": `window-weight.mjs main --max 1355`, `about --max 480` | Every run | Job log |
| `debrief.yml` | `tauri-smoke`, steps "Start-up benchmark" on macOS (enforced) and Linux (report only) | Nightly 00:30 and dispatch | Artifact `startup-timings-<os>` |
| `ci.yml` and each app's gates | `ng build` budgets; nextest runs the memory tests that are not ignored (Debrief `memory.rs`, `worker.rs`, profile `perf.rs` in debug) | Every run | Job log |
| [`bench-nightly.yml` template](../.gitea/workflow-templates/bench-nightly.yml) | `paddock-bench check` against `bench/budgets.toml` and the previous night | Not used by any app | — |

## Budgets and the ADRs that own them

| Budget | Value | Owner | Enforced by |
|---|---|---|---|
| Debrief store worker importing | ≤ 768 MB (hard cap 2 GB); low-memory mode about 400 MB | Debrief [ADR 0014](../apps/debrief/docs/adr/0014-database-first-engine.md) §2, [ADR 0031](../apps/debrief/docs/adr/0031-performance-settings.md) | `memory.rs`, `worker.rs` (CI) |
| Debrief worker idle and querying | ≤ 300 MB. The tests allow 320 MB on Linux and 630 MB on macOS | Debrief ADR 0014 §2; release-1.1.1 note | `worker.rs` (CI), `analysis_memory.rs` (CI, non-blocking), `bench_reads.rs` (`DEBRIEF_BENCH_STRICT=1` only) |
| Debrief interactive operation | < 0.5 s cold | Debrief ADR 0010, 0014 | `bench_reads.rs` with `STRICT` only; not in CI |
| Debrief import throughput | ≥ 218k events/s (3 × the spike) | Debrief ADR 0014 §2 | **nothing** |
| Debrief app process at 10M | < 250 MB | Debrief ADR 0014 §2 | **nothing** |
| Debrief CPU, memory and idle gates | idle ≤ 0.5% of a core and ≤ 1 wake-up/s; catalogue ≤ 512 MB; import ≤ 6 s and ≤ 32 CPU-s per GB; post-import ≤ 20 CPU-s/GB; name rebuild ≤ 1 s | Debrief [research/2026-10-28-cpu-memory.md](../apps/debrief/docs/research/2026-10-28-cpu-memory.md) §4; family [ADR 0024](adr/0024-background-work-contract.md) D7 | **nothing yet** (planned `bench/budgets.toml`) |
| Debrief plugin memory | cap 1024 MB; import ≤ 512 MB (proposed) | Debrief ADR 0031; [plugins research](research/2026-10-plugins-memory-and-manager.md) | `evtx_memory.rs` (ignored, fails today) |
| Debrief profile operations | 100–2000 ms each | Debrief [profiling.md](../apps/debrief/docs/profiling.md#performance) | `debrief-profile` `perf.rs` (CI, debug, budgets × 20) |
| Debrief start-up marks (macOS) | windowCreated 325 … firstInteractive 940 ms | Rule 16, Debrief ADR 0058 | `startup-bench.mjs` (nightly) |
| First-paint JS | Debrief main 1355 kB, About 480 kB; other apps not set | Rule 35 | `window-weight.mjs` (Debrief); C29 (not configured) |
| Angular initial bundle | Debrief 185/192 kB; Dyno 900 kB/1.5 MB; Pitcrew 1/2 MB; Pitwall 1.2/2 MB; Spotter 1/2 MB; gallery 1.5/3 MB; shell-demo 500 kB/1 MB; workbench-demo 580 kB/1 MB | Rule 35 | `ng build` in CI |
| Idle process | ≤ 0.5% of a core, ≤ 1 wake-up/s | [ADR 0024](adr/0024-background-work-contract.md) D7 | **nothing** shared; Debrief's gate is planned |
| Cancel latency | 250 ms target (`CANCEL_LATENCY_TARGET_MS`); progress at most every 100 ms | [ADR 0008](adr/0008-long-operations.md) | mechanism constants, no wall-clock test (by design) |
| Palette search | ≤ 50 ms over 250 000 names | `paddock-names` | the bench, when someone runs it |
| Frame budget, 100k-row and 5,000-lane tests | planned | [ADR 0018](adr/0018-bounded-loading-everywhere.md) | **nothing** |
| Dyno's own cost | about 1% of the machine; `sampling::CPU_BUDGET` = 1.0% of one core | Dyno [ADR 0002](../apps/dyno/docs/adr/0002-sampling-architecture.md) | adaptive intervals at run time; **no test** |
| Sighter latency and memory | for example locate p95 ≤ 250 ms on a GPU; server ≤ 1.5 GB with 3 models | Sighter [design review](../apps/sighter/docs/review/2026-10-design-review.md) §1.7, [proposal](../apps/sighter/docs/proposal.md) §7 | no code yet |
| Pacenotes retrieval quality | concept hit@5 ≥ 0.75, doc recall@5 +25% over BM25 | Pacenotes [ADR 0001](../apps/pacenotes/docs/adr/0001-embeddings.md) | research harness only |

## Latest results per app

One line each. The dated records are in the app files.

- **Debrief** (2026-10-28, Mac mini, under load):
  - Import: 5.3–5.4 s/GB and 26.5–29.6 CPU-s/GB on a synthetic 8 GB folder. **Pass.**
  - Worker import peak: 805–1,268 MB footprint against 768. **Fail.**
  - Idle: the app stand-in takes 116% of a core against 0.5%. **Fail.**
  - DuckDB 2.0 A/B (branch `debrief/duckdb-2`): about 13% less import CPU, the same footprint.
  - Start-up (2026-10-24, debug): firstInteractive 624 ms.
  - First-paint JS: 1231 kB.
  - Search (branch `debrief/search-perf`): only a baseline. The A/B is pending.
- **Shared crates:**
  - `paddock-names`: about 20 ms per search over 250k names (was 500 ms; 2026-10-25, machine not stated).
  - `paddock-jobs` walk: about 1.3× faster than a sequential walk on a warm cache.
  - `paddock-download`: 4 connections against 1, 11.0 s → 2.7 s for 64 MiB.
  - `paddock-proc` per-pid watcher (branch): 4.9% → 0.1% of a core.
  - `paddock-chromium` (2026-10-29, loaded Mac): a test browser exits 0.15 s (median) after `Browser.close` when alone,
    up to 11.7 s (p95 6.3 s) with about 20 at once, past the 5 s `CLOSE_GRACE` in 18 of 178 closes. See
    [the record](#2026-10-29-paddock-chromium-browserclose-exit-time-under-load-fixflaky-browser-close).
- **CI** (2026-10-24, before the monorepo): Paddock `ci.yml` median 3m05s. App builds 15–37 min median; Debrief's is
  24 min. See [ci-timings](research/2026-10-ci-timings.md). The monorepo has no re-run yet.
- **Dyno:** idle, nobody watching, the agent costs about 0.9–1.1% of one core and 30 MB (2026-10-30, release, load 21–61),
  the same with its threads at `UTILITY` as before; the finder is about 4% slower at `UTILITY` on a busy machine. Earlier:
  2.0–3.2% of one core and 24 MB (load 55–85, an upper bound, date not stated).
- **Pacenotes** (2026-10-25/28): Model2Vec potion-code-16M-v2 embeds 6,365 chunks/s on 1 thread and 55,499 on 13, with
  a 200 MB peak. BM25 plus potion-code meets the quality bar (0.750 / +28%).
- **Pitcrew** (branch `docs/pitcrew-autoscale`, simulation): auto-scaling 1–4 runners cuts the mean wait from 171 to
  9.7 min with no contention.
- **Pitwall, Spotter, Sighter, demos:** no recorded results beyond bundle budgets.

## Shared-crate results

Results of shared crates (rule 48: shared code records here), newest first.

### 2026-10-29, paddock-chromium: Browser.close exit time under load (fix/flaky-browser-close)

- **Commit:** 18360f145 (origin/main) with temporary probe output in `Chromium::close_gracefully` (not committed): the
  time from the `Browser.close` answer to the process's exit.
- **Machine:** Mac mini (Mac16,11), 14 cores, macOS 27.2; the pinned Chrome for Testing 156.0.8078.47 (macOS arm64).
- **Load:** single runs: 1-minute load 12–25. Parallel runs: 4 loops of the suite at once (about 20 test browsers),
  1-minute load 145–210 at the end of the runs, with other agents' jobs on the machine.
- **Build:** debug test build (`cargo test -p paddock-chromium --all-features --no-run`), `CARGO_BUILD_JOBS=2`.
- **Corpus:** none; the real-browser tests' own local pages (synthetic).
- **Command:** `CHROMIUM_TEST_PATH=<pinned Chrome for Testing> PADDOCK_REQUIRE_TEST_BROWSER=1 nice -n 10 <it binary>
  real_browser --nocapture`, 40 runs one at a time; then 4 such loops of 15 runs at once, twice (before and after the
  test fix).
- **Runs:** 120 closes alone; 178 and 180 closes in the two parallel sets. Reported: median, p95, max.
- **Numbers:**

  | Measure | Alone | 4 suites at once (1) | 4 suites at once (2) | Budget | Verdict |
  |---|---|---|---|---|---|
  | Exit after `Browser.close` answered, median | 0.15 s | 1.39 s | 0.94 s | `CLOSE_GRACE` 5 s | pass |
  | p95 | | 6.27 s | 3.31 s | 5 s | over in set 1 |
  | max | 1.55 s | 11.68 s | 6.25 s | 5 s | over |
  | Closes over 5 s | 0 of 120 | 18 of 178 | 4 of 180 | | |
  | `Browser.close` answered | 120 of 120 | all | all | | |

- **Memory measure:** not measured.
- **Notes:** the browser always answered `Browser.close` and closed its DevTools connection; under load it was slow to
  exit, not deaf. Tests that asserted the exit within `CLOSE_GRACE` failed on load alone (2 of 60 runs); they now wait
  on the exit under a 40 s hang guard. **For the product:** past the grace the tree is ended, which is safe for throwaway
  profiles (they are deleted); under heavy load about one close in ten ends by force. A kept profile
  (`Profile::Persistent`) gets 20 s (`CLOSE_GRACE_KEPT`), longer than the slowest exit measured, because a forced end
  during Chrome's shutdown can lose cookies or a sign-in. Raw output: the agent's scratch folder,
  not kept in the repository.

## Other family measurements

These measure tooling, not a product. They are kept where they are.
- [Monorepo spike](research/2026-10-monorepo-spike.md) (2026-10-25, load 18–35):
  - sccache hit rates: 100% only with the same checkout path and target folder; about 6.5% across worktrees.
  - moon against scripts.
  - Repository size: 703 MiB packed.
- [Nx against scripts](research/2026-10-nx-vs-scripts.md) (2026-10-27): a replay of 142 runs; cache hits would save
  25–37% of runner minutes, an upper band.
- [Rust tooling inventory](research/2026-10-rust-tooling-inventory.md) and [ADR 0022](adr/0022-rust-developer-and-ci-tool.md)
  §3.5 (2026-10-27, load 3–4): `node -e 0` 32 ms, a Rust binary 10 ms, conformance 2.9 s.
- [Package managers](research/2026-10-package-managers.md) (2026-10-25, load 11–20).
- [Dialog windows that open hidden](research/2026-10-28-dialog-hidden-windows.md) (2026-10-28): ready latency per OS
  and scenario.
- `paddock-proc` snapshot with start times ([ADR 0033](adr/0033-processes-by-identity-never-by-a-bare-pid.md), 2026-10-30,
  macOS release build, about 1,130 processes, load 17–23, synthetic: the machine's own process table, nothing recorded):
  `cargo run --release -p paddock-proc --example snapshot_cost`. A snapshot takes 13.6–14.7 ms, and the start times take
  0.53–1.77 ms of that. Windows is not measured.
- [Lessons](lessons.md): test-binary consolidation, 239 → 99 binaries, nextest 301 s → 196 s.

## Gaps

These are ordered by impact.

1. **Rule 16 is met only by Debrief, and only partly.**
   - The shared harness (`paddock-bench`, the `bench-nightly.yml` template) is used by no app.
   - No `bench/budgets.toml` or `*.results.json` exists anywhere.
   - The nightly 10M store benchmark prints its numbers to a job log, which expires.
   - Without stored results there is no history and no regression check.
2. **Budgets with no test:**
   - Debrief: import throughput ≥ 218k events/s; app process < 250 MB; every CPU, memory and idle gate of the 2026-10-28
     note (D7); the 0.5 s interactive budget in CI (`bench_reads` runs without `DEBRIEF_BENCH_STRICT`, and `bench.rs`
     only prints).
   - Family: ADR 0024's idle budget for every app.
   - ADR 0018's frame and scroll budgets.
   - Dyno's 1% overhead.
   - C29 window weight outside Debrief.
   - The palette and menu budget in [plans/workbench.md](plans/workbench.md).
3. **Benchmarks that are missing:**
   - Start-up for every app except Debrief, which rule 16 names first.
   - Idle CPU and wake-ups for Dyno, Spotter, Pitwall and Pitcrew, which run all day.
   - Scroll and frame smoothness.
   - Time to first event in CI (ADR 0051's result is a single manual run).
   - Pitwall request latency and queueing.
   - Spotter check throughput and Chrome memory: about 17 GB idle was seen on Linux, see the lessons.
   - Windows and Linux runs of anything except CI.
   - Memory is not checked outside Debrief.
4. **Results on loaded machines that need a quiet re-run:**
   - Nearly every Debrief result since 2026-10-20 (Windows VM at about 80% load; Mac mini at load 9–220).
   - Dyno's overhead (load 55–85; roadmap G13).
   - The search-perf baseline (load 14–220).
   - The DuckDB 2.0 A/B (33–65).
   - The monorepo spike (18–35).
   - Re-run each with the template once a quiet window exists. The search-perf runbook
     (`/Volumes/d7sst7/dev-targets/handoff/search-perf-bench.md`) already plans one for Debrief.
5. **Stale or doubtful numbers:**
   - CI timings predate the monorepo, and `ci-timings.mjs` still defaults to the old per-app repos.
   - Debrief ADR 0010's spike numbers are from DuckDB 1.5 on the Windows VM.
   - The R2a/R2b/R2c numbers use RSS and a different machine.
   - The `paddock-names`, `paddock-download` and `paddock-proc` figures give no machine or load.
   - The server-mode note's prose disagrees with its own bulk-fetch table.
   - Debrief `notes/1-2-baseline.md` (plan 0.1) does not exist yet.
6. **Measurement method:**
   - Debrief's `tests/common/rss.rs`, `bench.rs`, `memory.rs`, `analysis_memory.rs` and `worker.rs` sample RSS, which
     rule 26 forbids for budgets. On macOS, RSS was 1,965 MB where the footprint was 535 MB.
   - The macOS absolute query budget in `analysis_memory` cannot hold (about 1 GB RSS at the start).
   - `perf_idle.rs` and `perf_plugins.rs` say the bench workflow runs them; it does not.
   - The `debrief-bench.yml` comment says the import benchmark asserts the import budgets; it asserts none.
7. **Uncommitted or pending work:**
   - The DuckDB server-mode note and its harness are uncommitted in their worktree.
   - Search-perf's per-change A/B, the writer grid, the 10M runs and the Model2Vec throughput are pending.
   - Pitcrew ADR 0021's contention measurement (WP-0) is pending.
   - The Pacenotes measurement plan on real repositories is pending.
