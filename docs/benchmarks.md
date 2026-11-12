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

## Results of shared tooling

### 2026-11-08, `./dev gates check` (feat/gates-c100)

- **Commit:** the tip of `feat/gates-c100` before the app clippy fixes (see the branch)
- **Machine:** Mac mini, Apple M4 Pro, 14 cores, 64 GB, macOS 27.2
- **Load:** 1-minute load 41 to 47 during the runs (other agents' builds were running)
- **Build:** release (`cargo build --release` in `tools/paddock-dev`); yaml-rust2 0.13, toml 1
- **Corpus:** this repository: 12,328 tracked files, 3,192 `.rs` files, 25 workflow and action files, 9 app contracts, 328 checks
- **Command:** `target/release/paddock-dev gates check --root . --base HEAD` (includes one `node conformance/bin/paddock-conformance.mjs list --json` and two `git` calls)
- **Runs:** 5 runs, single; wall time as `time -p` reports it
- **Numbers:**

  | Measure | Result | Budget | Verdict |
  |---|---|---|---|
  | C100's own report (without start-up) | 1,248 to 1,586 ms | a few seconds (ADR 0060 section "Parallelism and cost") | within |
  | wall time | 1.25 to 2.03 s | | within |
  | debug build, the same command | 8 to 18 s | | not what CI runs |

- **Notes:** one thread, no parallelism needed at this size; the cheaper scan (a byte-string test before parsing Rust and TypeScript, one parse per YAML file) is in. The debug figure is for a debug `./dev` on this loaded machine. How the wrapper builds `./dev` in CI (profile, cold cache) is not measured here.

### 2026-11-12, full against sparse worktree checkout (fix/worktree-gates-profile)

- **Commit:** e2a318345 (origin/main when measured); the tool is the `./dev` built from main before the change
- **Machine:** Mac mini, Apple M4 Pro, 14 cores, 64 GB, macOS 27.2; the worktree root on the external d8 disk
- **Load:** 1-minute load 9.0 to 9.8 at the start and end of the runs (other agents' builds were running)
- **Build:** the `./dev` wrapper's binary, unchanged by the measurement
- **Corpus:** this repository: 13,163 tracked files; the cone is `crates,ui,scripts,docs,tools,apps/spotter`
- **Command:** `HEAVY_LOCK_DIR=/Volumes/d8sst7/dev-targets/heavy-slots engineering-rules/scripts/heavy.sh ./dev worktree add <name> --base origin/main -b <branch> --owner implementer:meas --root /Volumes/d8sst7/dev-targets/wt --agent <id> [--sparse crates,ui,scripts,docs,tools,apps/spotter]`, then `du -sk <folder>`
- **Runs:** 3 runs of each, interleaved; median and range of the wall time as `date` differences; disk is the same every run
- **Numbers:**

  | Measure | Sparse cone | Full checkout | Verdict |
  |---|---|---|---|
  | wall time, median (range) | 1.2 s (1.2 to 1.3) | 2.7 s (2.6 to 7.1) | full costs about 1.4 s more at median |
  | disk | 203 MiB | 544 MiB | full costs about 340 MiB more |

- **Notes:** the full checkout's slowest run (7.1 s) came while the load was high. An earlier run on a quieter machine gave 3.1 s and 1.9 s (two runs each, disk the same). Decision: a branch worktree is a full checkout (`--profile gates`), because conformance, `scripts/affected.mjs`, `./dev gates check` and `./dev plan` read most folders and a cone made them fail with ENOENT; agents added folders by hand at least four times on 2026-11-11. `--sparse` stays for `--detach` review copies. Not measured: build output, which is the same for both.

### 2026-11-12, SVG drift check: font metrics (ci/svg-drift-check)

- **Commit:** 8a3c3111a (the branch merged with origin/main 0b736ac0b, which has train 7's Pitwall re-export c8057ff13)
- **Machine:** Mac mini, Apple M4 Pro, 14 cores, 64 GB, macOS 27.2; the fresh exports ran in `paddock-ci:v2` as `linux/amd64` (emulated), one container, `--rm --cpus 4 --memory 12g`
- **Load:** 1-minute load 15 to 37 during the run (other agents' builds were running)
- **Build:** draw.io desktop 32.6.10 (`drawio-amd64-32.6.10.deb`, SHA-256 fc96faaf...af83c) under `xvfb-run`, `--no-sandbox`; the committed SVGs were exported by the authors with draw.io desktop on macOS (the Mac's installed app is 32.6.10)
- **Corpus:** 143 committed SVGs with a `.drawio` source (150 sources; 7 kit templates have no export on purpose), including Sighter's 8 under `docs/images/diagrams/svg/`
- **Command:** `node scripts/svg-drift.mjs --keep <dir>` in the container, then a script that compares each fresh export with its committed file after `normalise()` and lists every differing number with its attribute
- **Runs:** 1 run (an export is deterministic once the random root id is normalised)
- **Numbers:**

  | Measure | Result |
  |---|---|
  | identical after normalising (PNG label copies, random root id, fallback text wrapping) | 132 of 143 |
  | differ only in root width and a sideways translation of every x coordinate | 10 (Debrief use-cases extend, investigate, operate, report; Dyno use-cases power; Pitcrew use-cases automatic, maintenance, runners; Pitwall state pairing, use-cases clients) |
  | maximum root width difference | 12 px (Pitwall `uml-use-cases-clients`); the others 2, 2, 2, 3, 8, 8, 9, 9, 10 |
  | maximum sideways translation | 9 px (Pitcrew maintenance, runners); every x coordinate moves by the same amount, no y, size, font size or path control point differs |
  | stale committed SVG (the source changed, the SVG did not) | 1: `apps/debrief/docs/diagrams/svg/c4-3-research-browser.svg` (label "[paddock_proc::Supervised child]" in the source, "[Supervised child]" in the SVG) |

- **Notes:** `ROOT_TOLERANCE` in `scripts/svg-drift.mjs` is 12, the measured maximum; everything else, font sizes included, is compared exactly. Cause: a label that overflows its box in the host's font changes the canvas extent. Only two hosts were compared (macOS committed, Linux amd64 fresh); a Windows export was not measured. The check takes about 5 minutes under emulation on this loaded machine; native amd64 CI is not measured.

## All benchmarks

The CI column names the workflow and job. "No" means nobody runs it unless they choose to. Details, env vars and
corpora are in each app's file.

### Shared crates, UI, conformance and scripts

| Benchmark | Measures | Command | Corpus | Budget asserted | CI | Results |
|---|---|---|---|---|---|---|
| [`crates/paddock-names/benches/search.rs`](../crates/paddock-names/benches/search.rs) | Catalogue build; search median, p95 and worst per query kind | `cd crates && cargo bench -p paddock-names`; `PADDOCK_BENCH_ENTRIES` (250 000), `PADDOCK_BENCH_BUDGET_MS` (50) | Names generated in the bench | Exits with an error when a full search takes over 50 ms (the palette budget) | No (compiled by `clippy --all-targets` only) | CHANGELOG 0.3.0 |
| [`crates/paddock-ipc/examples/os_endings.rs`](../crates/paddock-ipc/examples/os_endings.rs) | How a connection ends on the OS: a close with unread input, a pipe closed with `CloseHandle`, a listener closed with a client in its backlog; ordered-close time and refusals per second (ADR 0072 WP-0) | `cargo run --release -p paddock-ipc --example os_endings` (macOS, Linux, Windows) | Synthetic bytes | No (a baseline for ADR 0072, not a budget) | No | [2026-11-09](#2026-11-09-how-a-connection-ends-on-each-os-unread-input-pipe-closehandle-listener-backlog-ordered-close-baseline-feat-ipc-endings-m0) |
| [`crates/paddock-jobs/benches/walk.rs`](../crates/paddock-jobs/benches/walk.rs) | Parallel against sequential folder walk, warm cache | `cargo bench -p paddock-jobs`; `PADDOCK_BENCH_FILES` (60 000) | A tree generated in a temp folder | none (checks the same files are found) | No | [ADR 0008](adr/0008-long-operations.md) |
| [`crates/paddock-download/benches/segmented.rs`](../crates/paddock-download/benches/segmented.rs) | One stream against 2, 4 and 8 segmented connections, each connection rate-limited | `cargo bench -p paddock-download --features http`; `PADDOCK_BENCH_MIB` (32), `PADDOCK_BENCH_RATE_MIB` (8) | An in-process server; no network | none | No | [lessons](lessons.md) |
| [`crates/paddock-bench`](../crates/paddock-bench) + [`bench-nightly.yml` template](../.gitea/workflow-templates/bench-nightly.yml) | The harness for rule 16: `Recorder`, `bench/<app>.results.json`, `bench/budgets.toml` (limit, tolerance, regression), `paddock-bench check` | see [crates/README.md](../crates/README.md#paddock-bench-plan-x26) | — | Whatever an app's `budgets.toml` says | **No app uses it yet** | — |
| `perf/proc-per-pid`: `crates/paddock-proc/examples/watch_cpu.rs` (branch, not on main) | A process watcher's own CPU: whole process table against one pid | `cargo run --release -p paddock-proc --example watch_cpu -- 60` (Unix) | A `sleep` child | none (ADR 0024 D7: ≤ 0.5% of a core) | No | branch CHANGELOG |
| C29 window weight ([`conformance/src/window-weight`](../conformance/src/window-weight)) | JS each window loads before first paint (rule 35) | `ng build --stats-json`, then `paddock-conformance window-weight [kind] [--max kB] [--record]` | The build | `windowWeight` in `paddock.conformance.json`, **not configured anywhere** (C29 warns) | `ci.yml` conformance (warns) | — |
| `./dev hook pre-tool-use` (RT-4, [ADR 0083](adr/0083-agents-cannot-write-outside-their-own-worktree.md) §2) | Per-call time of the installed guard binary on a PreToolUse call, the shim around it, and the binary's size growth | 300 runs of `<git-common-dir>/paddock/hooks/paddock-dev hook pre-tool-use` with a JSON call on stdin, as described in the section below (the script is not kept) | A short, a 6 KB and a malformed call | none yet (the plan expects about 4 to 6 ms) | No | [results](#hook-per-call-cost-and-binary-size-rt-4-2026-11-10) |
| `./dev gates check` (C100, [ADR 0060](adr/0060-every-check-runs-somewhere.md)) | Run time of the meta-check: 328 checks, the CI model, 3,192 Rust files scanned, the docs | `./dev gates check` (release build of `tools/paddock-dev`) | This repository | none (ADR 0060: quote it only from here; a few seconds at most) | `ci.yml` conformance job, `nightly.yml` c100 job | [results](#2026-11-08-dev-gates-check) |
| Angular build budgets in `ui/angular.json` | Initial chunk size | `npm run build -w ui` (and each demo's build) | — | gallery 1.5 MB / 3 MB; shell-demo 500 kB / 1 MB; workbench-demo 580 kB / 1 MB (warn / error) | `ci.yml` ui jobs, through `ng build` | — |
| `./dev ci timings` ([`tools/paddock-dev/src/ci_runs/timings.rs`](../tools/paddock-dev/src/ci_runs/timings.rs); the script [`scripts/ci-timings.mjs`](../scripts/ci-timings.mjs) is its oracle until it is deleted, [known-copies](known-copies.toml) `ci-timings`) | CI wall time, queueing, runners, per-step and compile/test phases, critical path (Gitea API, read-only) | `GITEA_TOKEN=… ./dev ci timings [--repos paddock] [--limit 30] [--cache DIR] [--out docs/research/2026-10-ci-timings.md] [--json-file F]` (default repo: `paddock`; the script defaulted to the old per-app repos) | Gitea run history | none | No (manual) | [research/2026-10-ci-timings.md](research/2026-10-ci-timings.md) |

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
| Dyno | `dyno-agent bench` (the agent's own CPU and memory); the file finder and the maintenance status (`finder_bench`, `maintenance_bench`); Angular budgets. `dyno-bench` and the Benchmarks page are a **product feature** (benchmarking the user's machine), not a test of Dyno | [apps/dyno/docs/performance.md](../apps/dyno/docs/performance.md) |
| Pitcrew | Angular budgets; the auto-scale simulation (branch) | [apps/pitcrew/docs/performance.md](../apps/pitcrew/docs/performance.md) |
| Pitwall | Angular budgets. Its Benchmarks page and `pitwall-core/src/benchmarks.rs` are a **product feature** (model benchmarks) | [apps/pitwall/docs/performance.md](../apps/pitwall/docs/performance.md) |
| Spotter | Angular budgets | [apps/spotter/docs/performance.md](../apps/spotter/docs/performance.md) |
| Pacenotes | The embedding comparison of ADR 0001 (research harness, no code in the tree) | [apps/pacenotes/docs/performance.md](../apps/pacenotes/docs/performance.md) |
| Sighter | none (budgets only, in the proposal and the design review) | [apps/sighter/docs/performance.md](../apps/sighter/docs/performance.md) |
| shell-demo, workbench-demo | Angular budgets in `ui/angular.json` (above); no other benchmark | this page |

### Workflows that run benchmarks

| Workflow | Job and step | When | Output |
|---|---|---|---|
| [`debrief-bench.yml`](../.gitea/workflows/debrief-bench.yml) | `bench`: `cargo bench -p debrief-core --bench engine -- --quick` | Push to main touching `debrief-core`, `debrief-store-duck` or `debrief-store-worker`; nightly at 22:03 local (UTC+8; [ci-schedules.md](ci-schedules.md)); dispatch | Artifact `criterion-report` |
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
- **Dyno** (2026-11-03, macOS VM, A/B, branch `perf/dyno-cpu`): the whole guest with Dyno on the Counters page
  28–37% → 9–10% of one core, on the dashboard 18–22% → 12–16%, with the window closed 5.8–7.0% → 5.0–6.4% (agent
  0.85–0.97% → 0.57–0.73%). Causes: chart animations replayed on every poll, number glides painting every frame, the
  tray-only app subscribing at the summary level, menu bar icons drawn twice per update. See
  [the record](../apps/dyno/docs/performance.md#2026-11-03-cpu-per-process-in-a-macos-vm-baseline-100-and-main-and-the-cpu-fixes-perfdyno-cpu).
- **Dyno:** idle, nobody watching, the agent costs about 0.9–1.1% of one core and 30 MB (2026-10-30, release, load 21–61),
  the same with its threads at `UTILITY` as before; the finder is about 4% slower at `UTILITY` on a busy machine. Earlier:
  2.0–3.2% of one core and 24 MB (load 55–85, an upper bound, date not stated).
- **Dyno:** the agent costs about 2.0–3.2% of one core and 24 MB (load 55–85, an upper bound, date not stated).
- **Dyno:** the maintenance status takes 0.03 ms instead of 6–7 ms with 14 windows, by jumping to window edges (2026-10-30, load 20–30, A/B).
- **Pacenotes** (2026-10-25/28): Model2Vec potion-code-16M-v2 embeds 6,365 chunks/s on 1 thread and 55,499 on 13, with
  a 200 MB peak. BM25 plus potion-code meets the quality bar (0.750 / +28%).
- **Pitcrew** (branch `docs/pitcrew-autoscale`, simulation): auto-scaling 1–4 runners cuts the mean wait from 171 to
  9.7 min with no contention.
- **Pitwall, Spotter, Sighter, demos:** no recorded results beyond bundle budgets.

## Shared-crate results

Results of shared crates (rule 48: shared code records here), newest first.

### 2026-11-12, first-paint JS of the shell's contribution registry: `shell/contributions` against `shell` (fix/paddock-ui-light-shell-entry)

- **Commit:** A/B: base c5de1b427 (train/2026-11-11-5) -> change 461cc6128 for Dyno and Pitwall; Debrief: 0c52ca0c1 (feat/debrief-m1-contributions) with and without its six registry imports switched to the new entry (not committed to Debrief)
- **Machine:** Mac mini M4 Pro, 14 cores, 64 GB, macOS 27.2.1
- **Load:** 1-minute load 10-18 during the builds (CI running on the same machine)
- **Build:** production `ng build --stats-json` (Angular application builder, esbuild), `ui/dist` rebuilt from the tree under test with `npm run build`
- **Corpus:** none (bundle sizes)
- **Command:** `npm run build -w ui`, then in `apps/<app>/app` `npx ng build --stats-json --output-path <scratch>/<name>`, then
  `node ui/scripts/first-paint-weight.mjs <scratch>/<name>/browser-stats.json src/main.ts` (Debrief: add `src/bootstrap.ts src/app/app.ts src/app/app.config.ts`).
  The script is the static closure of the entry chunks through `measure()` of `conformance/src/window-weight/measure.mjs`; Material, CDK and paddock-ui kB are the `bytesInOutput` of their inputs in those chunks
- **Debrief files switched for the "change" build** (6, from `apps/debrief/app/src/app/`): `extensions/command-context.ts`, `extensions/command-registry.ts`,
  `extensions/context.ts`, `extensions/contributions.ts`, `extensions/lazy-registration.ts` and `testing/contributions.ts`: `@decryp7/paddock-ui/shell` became `@decryp7/paddock-ui/shell/contributions`
- **Runs:** single (a build is deterministic)
- **Numbers:** (kB of JS loaded before first paint, minified, not compressed)

  | Window | Measure | Base | Change | Verdict |
  |---|---|---|---|---|
  | Debrief M1 main | JS total | 1242.5 | 1189.4 | -53.1 |
  | Debrief M1 main | `@angular/material` | 56.4 | 14.5 | -41.9 (the root's `material/menu`) |
  | Debrief M1 main | `@angular/cdk` | 148.7 | 135.3 | -13.4 |
  | Debrief M1 main | paddock-ui | 19.8 | 10.4 | -9.4 |
  | Dyno main | JS total | 719.5 | 719.8 | no change (+0.3: chunking) |
  | Dyno main | Material / CDK / paddock-ui | 157.1 / 166.7 / 208.8 | 157.2 / 166.5 / 208.9 | no change |
  | Pitwall main | JS total | 702.5 | 702.4 | no change |
  | Pitwall main | Material / CDK / paddock-ui | 158.4 / 168.0 / 201.2 | 158.4 / 168.0 / 201.1 | no change |

- **Memory measure:** n/a (bundle sizes, no process measured)
- **Budget:** none asserted for these windows (rule 35 budgets: Debrief main 1355 kB, see Budgets); Debrief M1's main window at 1189.4 kB is under its 1355 kB
- **Notes:** Dyno and Pitwall main windows already import the paddock-ui root and Material elsewhere (the other files that import `shell` also use the
  frame, so they stay on `shell`), so switching their registry-only files gains nothing there; they gain only the cleaner import. A first Debrief
  "before" build ran against a `ui/dist` that already contained the new entry (its `shell` re-exports it) and showed no difference: compare only against a `ui/dist` built from the base tree.
  The static test `ui/scripts/light-entries.test.mjs` keeps the entry free of the root and Material.

### 2026-11-08, conformance C99: macOS dependency graph probe and source scan (pitwall/1.1-c99b)

- **Commit:** the head of `pitwall/1.1-c99b` (run on c4da1ca25 plus the C99 follow-up commits)
- **Machine:** Mac mini M4 Pro, 14 cores, macOS 27.2, fast home network
- **Load:** 1-minute load 4.3 to 5.5 during the runs; nothing else building
- **Build:** not a build: `cargo fetch`, `cargo metadata` and `cargo tree` (cargo 1.99.1) for the 5 app locks, the 2 demo locks and `crates/`, on both Apple triples, `--all-features` for the apps
- **Corpus:** this repository's real lockfiles; 8 workspaces
- **Command:** `RUSTC_WRAPPER= node conformance/bin/paddock-conformance.mjs --app paddock --root . --only C99`
- **Runs:** warm: 2 runs; cold: 1 run (empty `CARGO_HOME`, registry download of about 1.5 GB)
- **Numbers:**

  | Measure | Result | Budget | Verdict |
  |---|---|---|---|
  | warm cargo cache | 13.0 s, 13.1 s | none set | recorded |
  | cold cargo cache (download) | 34.3 s | none set | recorded |
  | CI runner (Linux container, runner cache) | to measure on the first CI run | none set | pending |

- **Memory measure:** not measured
- **Notes:** measured on macOS, not on the CI image. Check on the first CI run that the Linux host's
  `cargo tree --target aarch64-apple-darwin` prints the same line shape; a different shape shows as "check broken: the
  cargo tree output has a line this probe cannot read", never as a pass. No budget until the CI number exists (rule 48).
### 2026-11-09, paddock-proc: the orphan guardian's cost per child (fix/chromium-orphan-guard)

- **Commits:** A, the first shape (21abc8d71: a keeper and a guardian `sh` per child). B, the Rust guardian (69205f941:
  one guardian per process, one `cat` keeper per child). C, B hardened after the security review (the owner check, the
  folder marker, one write per line). Each measured with a temporary timing test in the guardian's tests (not committed).
- **Machine:** Mac mini, Apple M4 Pro, 14 cores, 64 GB, macOS 27.2.
- **Load (1-minute):** A 8.8 to 10.3; B 72 to 93; C 10.0 to 11.5. A and C ran at a similar load; B did not, and its
  numbers are not comparable with A's.
- **Build:** debug, `cargo nextest run -p paddock-proc --no-capture`.
- **Runs:** 3 runs each. A: 60 iterations of start `/bin/sleep 5` in its own group, `Guard::start`, drop. B and C: 61
  iterations of start `/bin/sleep 5` in its own group, `guardian::register`, end the child, drop the registration; the
  first registration of the process (which starts the guardian) reported apart. B registered a folder; C both without a
  folder and with one (which writes the folder's marker). p50 / p95 per run.
- **Numbers (p50 of each run; p95 in brackets):**

  | Measure | A (load 9-10) | B (load 72-93) | C, no folder (load 10-12) | C, with a folder (load 10-12) |
  |---|---|---|---|---|
  | the child's own spawn | 0.30, 0.45, 0.39 ms | 0.13, 0.11, 0.11 ms | 0.17, 0.14, 0.13 ms | 0.14, 0.14, 0.13 ms |
  | guarding one child | 5.8, 7.0, 6.1 ms (7.9, 13.0, 7.6) | 0.16, 0.14, 0.15 ms (1.41, 0.19, 0.21) | 0.26, 0.20, 0.20 ms (0.67, 0.44, 0.25) | 0.35, 0.31, 0.29 ms (0.75, 0.41, 0.38) |
  | ending the guard of one child | 0.33, 0.34, 0.32 ms | 0.05, 0.04, 0.04 ms | 0.08, 0.05, 0.05 ms | 0.08, 0.06, 0.06 ms |
  | the first child (starts the guardian) | (none shared) | 0.66, 0.44, 0.59 ms | 0.99, 0.53, 0.55 ms | (started already) |
  | processes for N guarded children | 2N | N + 1 | N + 1 | N + 1 |

- **Memory:** resident size (`ps -o rss`), once, shape B: the guardian of a 5 MB debug test binary 6.9 MB, a `cat`
  keeper 1.3 MB. Not measured: an app-sized release binary as its guardian.
- **Notes:** A's start was mostly the wait for the guardian's starting shell to exit; B and C start the guardian once,
  and a registration is a keeper spawn plus one line on a pipe (C: plus the marker file when a folder is given).

### 2026-11-03, paddock-widgets: grid engine, 32 placements (feat/widgets-grid)

- **What:** `cargo bench -p paddock-widgets` (`crates/paddock-widgets/benches/grid.rs`): ADR 0044 §7 budgets, a 32-widget board of 3 by 4 cells; each line is the fastest of 15 batches (the machine is shared).
- **Machine and load:** Mac mini, release build, run through the plock queue (load about 4 to 20), single thread.
- **Before** (first version, mean of 200 runs): a drop 4.3 us (budget 50); the derived 6- and 1-column arrangements plus the moves of all 32 placements 1,538 us against the ADR's 200 us budget: 320 gestures were each applied to a clone of the board.
- **After** (rule 51, cheaper): a drop 2.4 us; a nudge 2.4 us; both derived arrangements 4.0 us; the moves of all 32 placements 58.6 us; the whole view computation 63.2 us (budget 200). What changed: ids are `Arc<str>` (a board clone allocates nothing); one validation and one room check for all placements; a gesture that has somewhere to go is known to change the arrangement when no push can run past the last row (the proptest `moves_match_applying_every_gesture` checks the shortcut against applying each of the ten gestures, 1,000 random boards), so only End is applied to be answered; a drop settles with indices instead of looking ids up. Intermediate steps on the way: 236 us with the slice-based probes alone, 225 us with the index-based settle.
- **After the lone-half rule** (2026-11-04, derive widens a half-width widget alone in its rows to the whole row; M-series Mac, `cargo bench -p paddock-widgets --bench grid`): both derived arrangements 4.4 us (was 4.0); the whole view computation 64.6 us (budget 200); a drop 2.4 us; the moves of all 32 placements 61.2 us.

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

### 2026-11-09, CI share on the shared Mac (three runners)

Quoted by `conformance/ci/build-jobs.sh` (the macOS layout) and the fix/main-after-7761 report (CI run 7775).
- **Commit:** b51c72509 (origin/main); test binaries built by CI for dyno-app (`dyno_app_lib-65bdd936c62ef7f2`) and
  dyno-bench, copied to a scratch folder and run there with TMPDIR in it.
- **Machine:** Mac mini M4 Pro, 14 cores (10 performance, 4 efficiency), 64 GB, macOS 27.2; the three CI runners idle.
- **Load:** 1-minute load 2.7–17 (agents on the same Mac).
- **Build:** debug, as CI builds the tests.
- **Corpus:** none; CPU-bound probe = `link::model::the_machine_keeps_every_invariant_in_every_interleaving_up_to_the_ci_bound`
  (about 11.5 CPU-seconds on up to 15 threads); busy threads = `yes > /dev/null` processes.
- **Command:** `TMPDIR=<scratch> /usr/bin/time -p ./dyno_app_lib-… --exact <probe>` beside the stated load.
- **Runs:** single runs, three for the probe beside three test runs; wall seconds.
- **Numbers:**

  | Measure | Result |
  |---|---|
  | Probe alone | 2.6–4.1 s |
  | Probe beside 11 / 23 / 41 busy threads | 3.2 / 4.0 / 5.9 s |
  | Probe beside 3 dyno lib test runs at 14 threads, or at 4 threads | 2.6–2.7 s, 2.6–2.8 s |
  | Probe beside dyno-bench's two all-core tests (cpu kernels, memory sweep) | 3.6–3.9 s |
  | dyno lib tests (439, without the model tests) at 14 / 8 / 4 test threads | 20.8 / 15.7 / 47.8 s wall, about 1 s CPU |
  | The probe in CI, from job logs of runs 7690–7782 | 7–27 s; 60 s TIMEOUT in run 7775 (job 32410) |

- **Used for:** the macOS class defaults in `conformance/ci/build-jobs.sh`: 3 jobs per host, 2 cores kept (4 build
  jobs per job) and 8 test threads (`NEXTEST_TEST_THREADS`), the fewest threads that matched 14 in the dyno lib run.
- **Notes:** 3 jobs × 4 rustc plus about 10 agent threads is 22 busy threads, which slowed the probe by about 1.5 times, not
  the 20 times seen in run 7775; so the share is not what timed it out, and that timeout is not reproduced here. In the
  job logs the probe took 15–26 s every time its run overlapped dyno-bench's all-core tests (4 of 4) and about 7 s when it
  did not (3 of 3); run beside them here it did not slow down, so that correlation is not a confirmed cause (ER-28). The
  dyno lib tests mostly wait, so 4 test threads instead of 14 made that run 2.3 times longer, while 8 was the fastest.
- **Per run** (wall seconds; 1-minute load from `sysctl vm.loadavg` before the run):
  - probe alone: 2.66 (load 2.7; the other model test, `a_diverging_search_…`, took 2.99); 2.70 (2.8); 4.13, 2.66 (4.1)
  - probe beside 11 / 23 / 41 busy threads: 3.15 (load 3.5), 3.99 (5.0), 5.91 (5.0)
  - probe ×3 beside 3 dyno lib runs at 14 threads: 2.72, 2.65, 2.62 (load 9.8 before, 6.7 after); at 4 threads: 2.59,
    2.75, 2.64 (6.7 before, 6.0 after)
  - probe beside dyno-bench's cpu and memory tests: 3.85, 3.55 (load 4.1 before, 8.1 after)
  - dyno lib tests without the model tests: 14 threads 20.77 (load 4.3), 4 threads 47.77 (5.9), 8 threads 15.65 (3.8)
  - The job-log figures (7–27 s, the overlaps) are from the logs of runs 7690–7782 on Gitea.
### 2026-11-09, the first start of a freshly linked program on macOS (docs/adr-0071-ready-or-exit)

Quoted by [ADR 0071](adr/0071-ready-or-exit-never-a-fixed-start-budget.md) (context and "Parallelism and cost").
- **Commit:** b51c72509 (origin/main); no Paddock code, a synthetic program.
- **Machine:** Mac mini M4 Pro, 14 cores, 64 GB, macOS 27.2; the shared CI host (three runners), agents working on it.
- **Load:** 1-minute load 8.9 before, 6.6-6.8 after (5-minute 11.5-14.1). An indication only.
- **Build:** each program is a new one-line C file (`puts` of a fresh `time_ns()` value, so every binary is new to the
  OS) linked with `cc -O0`; Python 3.12.5.
- **Corpus:** none.
- **Command:** `python3 -I measure.py <empty dir> 8`, then three times `python3 -I measure_conc.py <empty dir> 6`:

  ```python
  # measure.py: times the first and second start of N freshly linked tiny programs, one after another.
  import subprocess, sys, time, os
  d, n = sys.argv[1], int(sys.argv[2])
  rows = []
  for i in range(n):
      src = os.path.join(d, f"p{i}.c"); exe = os.path.join(d, f"p{i}_{time.time_ns()}")
      open(src, "w").write(f'#include <stdio.h>\nint main(){{puts("{time.time_ns()}");return 0;}}\n')
      subprocess.run(["cc", "-O0", "-o", exe, src], check=True)
      t = time.perf_counter(); subprocess.run([exe], capture_output=True); first = time.perf_counter() - t
      t = time.perf_counter(); subprocess.run([exe], capture_output=True); second = time.perf_counter() - t
      rows.append((first, second)); os.remove(exe); os.remove(src)
      print(f"{i}: first {first*1000:.0f} ms, second {second*1000:.1f} ms", flush=True)
  ```

  ```python
  # measure_conc.py: links N unique tiny programs, starts all N at once (each one's first start), then all N again.
  import subprocess, sys, time, os, concurrent.futures as cf
  d, n = sys.argv[1], int(sys.argv[2])
  exes = []
  for i in range(n):
      src = os.path.join(d, f"c{i}.c"); exe = os.path.join(d, f"c{i}_{time.time_ns()}")
      open(src, "w").write(f'#include <stdio.h>\nint main(){{puts("{time.time_ns()}");return 0;}}\n')
      subprocess.run(["cc", "-O0", "-o", exe, src], check=True); os.remove(src); exes.append(exe)
  def run(p):
      t = time.perf_counter(); subprocess.run([p], capture_output=True); return time.perf_counter() - t
  for label in ("first", "second"):
      with cf.ThreadPoolExecutor(n) as ex: r = sorted(ex.map(run, exes))
      print(f"{label}: n={n} min {r[0]*1000:.0f} median {r[n//2]*1000:.0f} max {r[-1]*1000:.0f} ms", flush=True)
  for p in exes: os.remove(p)
  ```

- **Runs:** 8 programs one after another; 3 rounds of 6 programs started together.
- **Numbers:**

  | Measure | First start | Second start | Budget | Verdict |
  |---|---|---|---|---|
  | One at a time (8 programs) | 258-2,864 ms, median 304 ms | 2.4-6.6 ms, median 2.7 ms | none | a new binary's first start carries a check its second start does not, about 100× the start itself on this host |
  | Six at once (3 rounds) | 322-325, 274-279, 266-340 ms per round | 2-5 ms | none | six concurrent first starts took about as long as one: the checks of different binaries ran in parallel here, they did not queue |

- **Memory measure:** not measured: the quantity is start latency, which memory does not bear on.
- **Notes:** the tiny programs do not reproduce the 10-17 s first starts seen with `pitwall-server` (Pitwall's
  [performance record](../apps/pitwall/docs/performance.md), the CI block below); what makes a large binary's check slow
  on the shared host is not measured.

### 2026-11-09, test durations around the first-start failures of 2026-11-08 (CI job logs)

Quoted by [ADR 0071](adr/0071-ready-or-exit-never-a-fixed-start-budget.md). CI observations, not a benchmark: what the
job logs say, so the ADR can quote them.
- **Commit:** the jobs' own heads (Pitwall run 7775 job 32436 and job 32380; Debrief job 32417; the WebKit leg job 32097).
- **Machine:** the macOS CI host, Mac mini M4 Pro (runners `d7-mac-mini`, `-2`, `-3` on one machine since 2026-11-08).
- **Load:** not recorded by the jobs.
- **Build:** debug test builds (`cargo nextest run --workspace --profile ci`); Playwright 1 worker.
- **Corpus:** none.
- **Command:** extraction from the downloaded job logs:
  `sed $'s/\x1b\\[[0-9;]*m//g' <job>.log | grep -E '^\S+\s+(PASS|FAIL) \[' | grep 'pitwall-cli::cli'` (and the same
  with `debrief-plugin-e2e`, `Summary \[`, `shell-demo-webkit`).
- **Runs:** one job each.
- **Numbers:**

  | Measure | Value | Budget | Verdict |
  |---|---|---|---|
  | Job 32436: `pitwall-cli::cli` tests (each the first start of the fresh `pitwall`) | 10 of 12 ended at 16.83-16.88 s within 57 ms of each other; the other two at 17.84 and 19.35 s | none | consistent with every start of one binary waiting for one slow check of that binary |
  | Job 32380 (same day, earlier): the same tests | 10 of 12 at 1.391-1.434 s; the other two at 2.40 and 3.98 s | none | the same tests without the delay |
  | Job 32436: the two failed `pitwall-server::e2e` child-engine tests | 10.56 and 11.06 s | the double's 200 polls × 50 ms | failed |
  | Job 32417: the two failed `debrief-plugin-e2e` store-worker tests | 29.97 and 17.14 s ("did not connect within 15 s") | the product's 15 s connect window | failed |
  | Job 32097: first `shell-demo-webkit` test; the next one; the rest of the a11y file | 5.4 s (failed); 6.3 s; 0.33-0.66 s | the 5 s expect timeout | failed |

- **Memory measure:** not applicable (test durations from logs).
- **Notes:** the job logs are kept in the coordinator's scratch folders (`main-after-7761/logs`, `pitwall-e2e-health`),
  not in the repository; Gitea keeps them for its retention period.
### 2026-11-09, test-double survey: scan and shipped feature trees

Quoted by [ADR 0073](adr/0073-no-test-doubles-in-a-shipped-build.md) ("Parallelism and cost").
- **Commit:** b51c72509 (origin/main).
- **Machine:** Mac mini M4 Pro, 14 cores, 64 GB, macOS 27.2.
- **Load:** 1-minute load 7.3–7.5 at start and end (5-minute 14.8–18.1); other agents were building.
- **Build:** release; the survey tool (a generalisation of Dyno's `no_ungated_doubles.rs`, `syn` 2.0.119), committed as
  text in [research/2026-11-09-test-doubles-survey.md](research/2026-11-09-test-doubles-survey.md) (WP-0 of ADR 0073
  builds the real check); `cargo tree` from the pinned toolchain.
- **Corpus:** the repository's own Rust sources (no user data): `crates/`, `apps/debrief`, `apps/dyno`, `apps/pitwall`,
  `apps/spotter`, `apps/pitcrew`.
- **Command:** `fake-scan <workspace> .` per workspace (it runs `cargo metadata --no-deps --offline`, then parses every
  lib and bin target); `cargo tree --offline --locked -p <pkg> -e normal --prefix none --target all -f '{p} {f}'` per
  shipped package.
- **Runs:** 3 rounds of the scan over the six workspaces, one after another; 3 runs of Dyno's six trees, 1 run of
  Debrief's four.
- **Numbers:**

  | Measure | Rounds 1 and 2 | Round 3 | Budget | Verdict |
  |---|---|---|---|---|
  | Scan, `apps/debrief` | 0.81, 0.82 s | 35.1 s | – | indication |
  | Scan, `apps/dyno` | 0.42, 0.43 s | 50.5 s | – | indication |
  | Scan, `crates` | 0.45, 0.48 s | 6.8 s | – | indication |
  | Scan, `apps/pitwall`, `apps/pitcrew`, `apps/spotter` | 0.10–0.13 s | 2.7–3.3 s | – | indication |
  | `cargo tree`, Dyno's 6 shipped packages in a row | 2.65, 1.40, 1.33 s | – | – | indication |
  | `cargo tree`, Debrief's 4 shipped packages in a row | 1.05 s | – | – | indication |

- **Memory measure:** not measured.
- **Notes:** round 3 was about 15 to 120 times slower in every workspace at once (2.7 to 50.5 s against 0.10 to 0.82 s). The cause was not diagnosed; most likely
  `cargo metadata` waited on a Cargo lock held by another agent's build. These are indications under load, not budgets.
### 2026-11-09, C58 prototype: unit walk and mode b (ADR 0075)

Quoted by [ADR 0075](adr/0075-copy-waivers-are-data-bound-to-the-landing-unit.md) (Parallelism and cost).

- **Commit:** register and main-equivalent at `origin/main` `b51c72509`; ranges as in the table (all in git history)
- **Machine:** Mac mini M4 Pro, 14 cores, 64 GB, macOS 27.2; git 2.56.0; Python 3.12.5
- **Load:** 1-minute load 18.8–26.6 for the three timed passes; 22–41 for the sweep; up to five agents building
- **Build:** a throwaway Python prototype (not in the repository): the §2 walk, ADR 0034 §2 counted lines with comment and
  `#[cfg(test)]` masking by line patterns, `item:` spans by a brace walk, judging with trailers honoured; one `git`
  process per diff and blob, serial
- **Corpus:** the repository's own history (no user data); the `rev-list` of the main-equivalent is built before timing
- **Command:** `python3 -I units.py <repo> --main b51c72509 [--base <B>] <tip>`; the sweep is `--sweep 2026-11-02`
- **Runs:** 3 runs per range, one after another; range reported. Sweep: 1 run
- **Numbers:**

  | Range | Units | Commits | Time |
  |---|---|---|---|
  | `4772dc8ce..2e9ffb7a3` (one-commit topic) | 1 | 1 | 0.12–0.13 s |
  | `b51c72509..63b1a9016` (handshake topic) | 1 | 7 | 0.30–0.33 s |
  | `84bcb9732^1..84bcb9732` (one landing merge) | 1 | 1 | 0.13–0.17 s |
  | `origin/train/2026-11-08-1` from its merge-base | 9 | 83 | 3.65–3.81 s |
  | `96c2a16ae..e20ae50c3` (`pitwall/1.1-b1b-b6`) | 36 | 206 | 10.1–11.5 s |
  | sweep: 78 branches reaching registered paths | 1–50 each | — | 161 s total |

  The independent reviewer's raw `git diff -U0` loop over register paths per first-parent commit (no walk, no masking,
  no judging), same day at load 32: the train 0.42–0.67 s, 90 first-parent commits of `origin/main` 1.46–1.53 s.
- **Memory measure:** not measured (neither the prototype's nor git's memory was sampled)
- **Notes:** taken under load; an indication only. The time is in per-commit `git diff` and `git show` processes of the
  two large union units of the Pitwall line (52 and 56 commits); the ADR's WP-3 batches them, and the same ranges are
  re-measured (time and memory) after WP-3, as a new block here. Raw output in the change's
  scratch folder, not kept in the repository.
### 2026-11-09, a syn scan of every spawn site in the monorepo (docs/adr-0076-owned-background-work)

Quoted by [ADR 0076](adr/0076-owned-background-work.md) ("Parallelism and cost", the cost of check C42).
- **Commit:** b51c72509 (origin/main), read from a worktree; the prototype is scratch code, not committed
  (`/Volumes/d7sst7/dev-targets/scratch/adr-0076/scan`, syn 2.0.119 with `full` and `visit`, proc-macro2 span locations).
- **Machine:** Mac mini M4 Pro, 14 cores, 64 GB, macOS 27.2.
- **Load:** 1-minute load 5.2 (5-minute 5.1) at the start; other agents were working. An indication.
- **Build:** release, `CARGO_BUILD_JOBS=2`, rustc 1.99.1.
- **Corpus:** the repository's own sources: 3,222 `.rs` files under `crates/`, `apps/` and `tools/` (no `target`,
  `node_modules` or dot folders). No user data.
- **Command:** `spawnscan <worktree>` (one pass to find `#[cfg(test)] mod x;` files, one pass that parses every file and
  visits every spawn call), output to a file.
- **Runs:** 3, one after the other, after one untimed warm-up run.
- **Numbers:** 5.90, 5.95, 6.04 s wall; about 4.9 s user and 1.0 s system per run; 647 sites found (615 spawn sites plus 32 `thread::scope` calls, ADR 0076 revision 2), 0 parse failures. No
  budget; verdict: cheap enough for CI over the whole tree, and for the push check over the changed files only.
- **Memory:** not measured.

### 2026-11-09, host-runner checkout: plain checkout plus stable-workspace against the kept clone (ci/kept-clone-sparse-checkout)

What: the checkout and stable-workspace steps of ci.yml's host jobs (`crate`, `app`, `paddock-dev`) on the real runners, from the
48 h of job data of the speed research R2 (`/Volumes/d7sst7/dev-targets/scratch/speed-research/data/jobs`, successful host jobs
with both steps) against dispatched affected runs 7808 (commit a3ad3d78f, 12 host jobs) 7816 (commit 6d0eb0824), 7818 (commit 4ebe7db09) and 7819 (commit 0fcc299fb). The tree: 12,367 files, 540 MiB; the
lean profile checks out 10,314 files. Synthetic data only; step times from the Gitea API (`steps[].started_at/completed_at`).
Script: `measure.py` in the scratch folder `/Volumes/d7sst7/dev-targets/scratch/ci-cheap-checkout`.

| Step (seconds) | before p50 | before p90 | run 7808 (a3ad3d78f) |
|---|---|---|---|
| Windows checkout | 50 | 63 | 5 to 14 (bootstrap) |
| Windows stable-workspace | 51 | 61 | kept-clone 62 to 127 (see below) |
| Windows both | 102 | 123 | 76 to 133 |
| macOS checkout | 24 | 40 | 1 to 8 (bootstrap) |
| macOS stable-workspace | 14 | 70 | kept-clone 16 to 59 |
| macOS both | 41 | 130 | 17 to 67 |

Reading run 7808, per phase (kept-clone log lines): on Windows the clone is created in 1 to 7 s, the fetch takes 0 to 2 s and the
checkout 3 to 5 s, but `clean` took 52 to 111 s: `reset --hard` ran before the sparse patterns were set and wrote the whole tree of
the new clone's branch. On macOS the slots held clones of the old layout (plain shallow checkouts), which were reused and fetched
without a partial filter: fetch 12 to 50 s, clean 2 to 10 s. Both were fixed in 5544158b2.

**Run 7816 (commit 6d0eb0824, with that fix), Windows, every clone cold** (seconds; the CI VM runs three slots, s1 to s3; bootstrap
checkout 6 to 7 s in each):

| Job (slot) | create the clone | fetch | check out | kept-clone total | checkout + kept-clone |
|---|---|---|---|---|---|
| crate (s2) | 14 | 1 | 25 | 43 | 49 |
| paddock-dev (s1) | 19 | 1 | 30 | 52 | 58 |
| app dyno (s3) | 17 | 0 | 29 | 49 | 56 |
| app pitcrew (s1) | 36 | 1 | 27 | 67 | 72 |
| app pitwall (s2) | 17 | 1 | 27 | 48 | 54 |
| app spotter (s3) | 1 | 0 | 29 | 34 | 40 |

Against the 102 s p50 (123 s p90) of the plain checkout plus stable-workspace, a cold clone costs 40 to 72 s (about 55 % of p50,
the 25 to 30 s being the blobs of the 10,314 lean files). **No clone was warm**: the kept folder was named after the job
(`<app>-<job>-<os>-<slot>`), so each of the six jobs made its own clone in its slot; the one that was 1 s to create (spotter, s3)
reused a folder of the same name left by run 7808. The clone is now one per slot, shared by every job (`paddock-checkout-<os>`),
which a warm job should turn into a fetch of the delta (the fetch phase was 0 to 1 s) plus a checkout of the changed files. That is
**not measured yet**. The 3-slot VM means three cold clones at first and warm ones after.

**Run 7818 (commit 4ebe7db09, one clone per slot), Windows** (seconds; slots s1 to s3, `paddock-checkout-Windows-s<n>`):

| Job (slot) | bootstrap checkout | create | clean | fetch | check out | kept-clone total | checkout + kept-clone |
|---|---|---|---|---|---|---|---|
| crate (s2), cold | 6 | 0 | - | 1 | 26 | 28 | 35 |
| paddock-dev (s1), cold | 6 | 1 | - | 0 | 25 | 28 | 34 |
| app dyno (s3), cold | 6 | 1 | - | 0 | 25 | 28 | 34 |
| app pitcrew (s1), warm | 14 | - | 4 | 1 | 2 | 10 | 24 |
| app pitwall (s1), warm | 9 | - | 1 | 2 | 1 | 7 | 16 |
| app spotter (s2), warm | 6 | - | 1 | 2 | 1 | 7 | 13 |

Against 102 s p50 (123 s p90): cold 34 to 35 s (a third), warm 13 to 24 s (an eighth to a quarter). Caveat on "warm": the slot's
clone was already at the same commit (the same run), so fetch and check out had no delta to carry; a warm clone one commit
behind adds the changed files. The six jobs repeated only slots s1 and s2, the three-slot VM's first run. The warm bootstrap
checkout (6 to 14 s) is now the biggest single piece left; the cold check out (25 s) is the blobs of the lean files.

**macOS did not run the clone in run 7816 or 7818.** All six macOS jobs failed in job-isolation, before kept-clone, in both
runs: paddock-dev has to be built from the bootstrap (a cached `./dev` binary hid this in run 7808), and cargo loads the whole
workspace around its path dependencies. 7816 lacked `crates/paddock-testkit` (a dev-dependency), 7818 lacked the other members
of the `crates/` workspace (`failed to load manifest for workspace member crates/cargo-paddock-plugin`, `no matching package
named paddock-jobs`). The bootstrap is now the whole `crates/` folder (13.7 MiB). Before the next run it was proved locally (ER-9):
a sparse partial clone of exactly the bootstrap folders, the real `job-isolation.sh acquire` against a scratch CI home with a
fresh build folder and no cached binary, which built paddock-dev, took slot s1 and released it.

**Run 7819 (commit 0fcc299fb), macOS** (seconds; slots s1 to s3, `paddock-checkout-macOS-s<n>`; the first run per slot is cold):

| Job (slot) | bootstrap checkout | create | check out | kept-clone total | checkout + kept-clone |
|---|---|---|---|---|---|
| crate (s1), cold | 2 | 1 | 25 | 27 | 29 |
| app dyno (s3), cold | 3 | 1 | 20 | 22 | 25 |
| app pitwall (s2), cold | 6 | 1 | 17 | 18 | 24 |
| paddock-dev (s1), warm | 5 | - | 0 | 2 | 7 |
| app pitcrew (s3), warm | 6 | - | 0 | 2 | 8 |
| app spotter (s1), warm | 4 | - | 1 | 2 | 6 |

Against 41 s p50 (130 s p90): cold 24 to 29 s, warm 6 to 8 s (same caveat as Windows: the warm clone was already at this commit).
Windows in the same run, warm: kept-clone 5 to 10 s, checkout + kept-clone 15 to 20 s (bootstrap 8 to 12 s); the Windows
slots that repeated were s1 and s3.

Failures in runs 7818 and 7819 that are not the checkout: macOS `paddock-dev` in 7819 passed its 99 tests and failed at the end on
sccache (`No such file or directory` in the job's temp folder, the holder of s1 ended before the job did), the same error as in
run 7808 and the older run 7774; `crate` Windows (`paddock-mcp` bridge test timeout), `paddock-dev` Windows (two
timeouts, `golden::live_plan_matches_affected_mjs_on_sample_change_sets` and `wrapper::compiler_flags_are_part_of_the_hash...`;
the golden live tests also timed out or failed on Windows in runs 7812 and 7803 of other branches, and the wrapper test works in a
temporary fixture repository, so load on the three-slot VM is the likelier cause, not shown).

### 2026-11-09, CI baseline: queues, critical path, setup, cache and failures (docs/adr-ci-speed)

Quoted by [ADR 0080](adr/0080-ci-at-the-speed-of-the-critical-path.md).
- **Commit:** none built; CI history of `decryp7/paddock` runs 7480 to 7825 (main at `b51c72509`, train
  `train/2026-11-09-2` at `6e57be619`).
- **Machine:** the CI runners as the Gitea API names them: `Gitea Action Runner (d-gtr9)` (Linux containers,
  `paddock-ci:v2`), `decryp7ology-dev` (Windows VM, 16 vCPU, 32 GB; 1 job at a time until the afternoon of 2026-11-09,
  3 after), `d7-mac-mini`, `-2`, `-3` (one Mac mini M4 Pro, 14 cores, 64 GB, macOS 27.2, shared with agents; `-2` and
  `-3` since 2026-11-09 01:03 and 01:16 +08). The analysis ran on the same Mac.
- **Load:** not controlled: a busy development week (trains, release candidates, the planner port). Run outcomes in the
  window: 64 success, 87 failure, 63 cancelled, 2 running.
- **Build:** none.
- **Corpus:** the API's run, job and step timestamps for 216 runs (3,624 jobs) from 2026-11-04 05:00 to 2026-11-09
  18:38 +08, and 174 job logs of runs 7696, 7697, 7810, 7813, 7821, 7822 plus the failed jobs of 7774, 7776, 7795,
  7796, 7797, 7803, 7812 and 7819. No user data; logs kept outside the repository.
- **Command:** `GET /api/v1/repos/decryp7/paddock/actions/runs?limit=50&page=1..6`, `.../runs/{id}/jobs?limit=100`
  (paged where `total_count` exceeded the page), `.../actions/jobs/{id}/logs`; parsed with a throwaway Python script (the
  method of `scripts/ci-timings.mjs` plus log parsing; WP-0 of ADR 0080 makes it `./dev ci timings`).
  "Ready" is the end of `plan` (or of `rust format + clippy` for Debrief's smoke and bundle jobs, per their `needs`);
  "queue" is start minus ready. Step phases are classified by step name (checkout, npm, browser, tool set-up, upload,
  clean-up, work). Compile, nextest and Playwright phases come from cargo's `Finished ... in`, nextest's `Summary [ N s]`
  and Playwright's `N passed (T)` lines. Concurrency is sampled every 30 or 60 s from job start and end.
- **Runs:** one pass over history, fetched from the API on 2026-11-09 before 18:39 +08 (the job lists of runs 7824 and
  7825 at 18:38, while they were still running; a later fetch counts more jobs for them, which explains small
  differences in job counts).
- **Numbers:**

  | Measure | Value |
  |---|---|
  | `ci.yml` wall, 48 completed runs (not cancelled) | median 45.1 min, p90 127.4 min |
  | `ci.yml` no-queue critical path (plan end + longest job) | median 19.6 min, p90 41.4 min |
  | `ci.yml` queue of the last job | median 33.8 min, p90 113.4 min |
  | `debrief.yml` wall / no-queue path, 13 runs | median 62.1 / 23.4 min |
  | Run 7821 (`ci.yml`, train) / 7822 (`debrief.yml`) | 47.4 / 21.8 min; Linux 26 jobs, 104 job-min / 11 jobs, 55 job-min; last Linux job of 7821 queued 31.5 min |
  | Run 7824 at 16.7 min | 19 jobs queued: 17 Linux, 2 macOS |
  | `plan` job | median 34 s, p90 211 s; in 7821 2.9 min (55 s before the first step, 100 s checkout) |
  | Linux job time by phase (6 runs) | Playwright 211 of 453 min (47 %), compile 70 (15 %), nextest run 16 (3 %) |
  | macOS / Windows job time by phase | compile 50 % / 54 %; nextest run 18 % / 13 %; Playwright 5 % / 0 % |
  | Set-up share of job wall (5.5 days) | Linux 20 %, Windows 43 %, macOS 31 % |
  | Checkout median / p90 | Linux 10 / 24 s; Windows 105 / 211 s; macOS 57 / 268 s (before the kept clone) |
  | Linux jobs under 2 min | 54 % of 2,405 |
  | Concurrency, last 24 h, share of time at each level | Linux: 0 jobs 35 %, 4 jobs 47 %, more than 4 3 %; Windows: 1 job 59 %, 3 jobs 8 %; macOS: 1 job 37 %, 3 jobs 26 % |
  | Queue wait, last 72 h, p50 / p90 | Linux 7.3 / 35.1 min; Windows 20.7 / 79.9 min; macOS 19.7 / 72.5 min |
  | Mean busy lanes, last 72 h | Linux 1.31, Windows 0.53, macOS 0.70 |
  | Linux lanes while a Linux job waited, last 72 h (1,523 such minutes) | all 4 busy in 74 %; two or more runs in play in 56 % |
  | Linux host idle, last 72 h | 61 % of the time; 62 % between 00:00 and 08:00 +08, 61 % between 08:00 and 24:00 |
  | sccache hits / misses (6 runs) | Linux 34,027 / 3,217 (91.4 %); macOS 587 / 1,836 (24.2 %); Windows 583 / 1,580 (27.0 %) |
  | Misses of the same job on two OS (7821 `crate`) | Linux 78, macOS 79 |
  | Cold slot (7810 `app dyno (windows)`, `sccache-s3`) | 1,020 misses, 2 hits |
  | Linux compile requests in 7821 (10 Rust jobs) | 12,729 |
  | `cargo-about` via `taiki-e/install-action` | 277, 279, 276 s in 7821/7822 (8 × HTTP 504 each); 1 to 19 s in 5 other runs |
  | Runner minutes by run outcome | Linux 9,547 (failed runs 59 %, cancelled 20 %, green 20 %); Windows 3,839 (64 / 15 / 21 %); macOS 4,211 (59 / 24 / 16 %) |
  | Failed `ci.yml`/`debrief.yml` runs: minutes in failed jobs | 1,394 of 5,724 (24 %); median 9 % of jobs fail |
  | Slowest tests | `debrief-store-duck` suites on macOS 60 to 126 s, three 180 s timeouts in 7822; `paddock-dev` golden 196 s (Windows); tests over 10 s: 150 of 403 test-min |
  | 49 failed jobs in 12 runs | 7 on static checks (seconds locally), 2 on the job-isolation holder, the rest on tests (load- or OS-dependent on macOS and Windows, plus goldens) |
  | Jobs in the sample | 5,005 listed, 3,957 not skipped, 3,624 with a start and an end (the base of every count here) |
  | Host test time by class (file of each test; macOS 347, Windows 103 test-min) | adapter package 15 / 34 %; `cfg(os)` file 13 / 11 %; file with a listed OS-touching API 35 / 14 %; unknown package 2 / 13 %; generic 34 / 29 % |
  | Packages with any `cfg(os)` code | 75 of 134; 73 % (macOS) and 80 % (Windows) of host test time |
  | Debrief `rust tests (macOS)` (7697, 7822, 7813) | wall 8.3 / 19.1 / 46.0 min; 208 test-min: generic 34 %, `cfg(os)` files 13 %, OS-touching API 51 %, adapters 2 %; 26 of 83 test binaries only generic (2 % of its test time) |
  | `crate` doc tests and features-off build on hosts (5.5 days) | macOS 83 + 97 min, Windows 71 + 68 min; failed alone: features-off 11 times on Linux, once on macOS, never on Windows; doc tests once on macOS |
  | Duplicated or input-independent work (5.5 days) | `dyno-release.yml` gate jobs 903 job-min: 165 on commits with a green `ci.yml` run, 485 on commits with no `ci.yml` run, 254 on commits whose `ci.yml` was not green; WebKit legs in `ci.yml` 664: 173 on `main`, 491 on `dyno/1.1.0-rc`; notices, publish dry run, plugin SDK templates about 290; tiny static jobs 117 and commit lint 71 |
  | Host job-min of `ci.yml` and `debrief.yml` by head | 3,133 on `main`, train, steward and release-candidate heads; 1,867 on other branches |

- **Memory:** not measured.
- **Notes:** the `ci.yml` and `debrief.yml` figures use those workflows' `needs`; the 72-hour queue figures include the
  release and nightly workflows with "ready" taken as the end of their plan job or the run's creation, which can overstate
  their queue. A busy development week; compare later blocks in pairs (the 2026-10 note: load changes timings by up to
  60 %). The Windows capacity changed during the window, so its queue figures mostly describe the old one-lane set-up.

### 2026-11-03, starving a test process by OS priority on macOS

Quoted by [ADR 0041](adr/0041-deterministic-timing-in-tests.md) §5 (the starved run).
- **Commit:** 9da2ccf59 (origin/main); no code, a synthetic loop.
- **Machine:** Mac mini M4 Pro, 14 cores (10 performance, 4 efficiency), 64 GB, macOS 27.2.
- **Load:** 1-minute load 16.4 before and 26.3 after; other agents and CI were working on the machine. An indication only.
- **Build:** none; Python 3.12.5 (`python3 -I`).
- **Corpus:** none. The work is a fixed loop: `x += i*i` for `i` in `range(6_000_000)`, timed with `time.perf_counter()`
  inside the script.
- **Command:** `python3 -I work.py` (normal), `taskpolicy -b python3 -I work.py` (`PRIO_DARWIN_BG`), `taskpolicy -c
  utility python3 -I work.py` (a utility QoS clamp); three rounds, interleaved.
- **Runs:** 3 per set-up.
- **Numbers:** normal 0.33-0.36 s; utility clamp 0.33-0.36 s; `PRIO_DARWIN_BG` 3.03-24.73 s (24.73, 6.93, 3.03), so 9× to
  73× the normal time. No budget; verdict: `taskpolicy -b` starves a process without adding load, with a large and
  load-dependent spread, while a utility clamp does not starve it at this load.
- **Memory:** not measured (not relevant to this measurement).

### 2026-10-30, linker for Dyno's incremental debug rebuilds: ld-prime against rust-lld (ci/build-parallelism)

- **Commit:** ddf0becda (origin/main); no code change, the linker is chosen per run.
- **Machine:** Mac mini M4 Pro, 14 cores, 64 GB, macOS 27.2, Xcode 27.2 (ld-prime `ld-27106.1`), rustc 1.99.1 (LLVM 23.1.1).
- **Load:** 1-minute load 5.7–10.8 at the start of each rebuild run (up to 13.9 at the end of one) (each started only when the 5-minute load was below 10,
  aborted above 15; none was); 11–20 during the link-only runs, so those are an indication, compared only with each other.
  Other agents were working on the machine throughout.
- **Build:** debug (`dev` profile: workspace crates at opt-level 0, dependencies at 2, `debug = "line-tables-only"`), no
  sccache, `CARGO_INCREMENTAL=1`, `CARGO_BUILD_JOBS=13` (cores − 1), under `nice -n 10`. Three warm build folders, one per
  linker set-up: `ldprime27` (the default: ld-prime, SDK 27.2), `ldprime` (ld-prime, SDK 26.5) and `lld` (rust-lld from the
  toolchain's sysroot through `-C link-arg=-fuse-ld=lld -C link-arg=-B<sysroot>/lib/rustlib/aarch64-apple-darwin/bin/gcc-ld`,
  SDK 26.5). rust-lld cannot link against the 27.2 SDK at all: its `libSystem.tbd` lists an `arm64e.x1-macos` target that
  LLVM 23.1's TAPI reader rejects ("unknown target"), so the like-for-like pair is `ldprime` against `lld` on SDK 26.5. The
  `LC_BUILD_VERSION` of each binary names its linker (tool 3 = ld, tool 4 = lld).
- **Corpus:** none (a build). Scenarios: `touch crates/dyno-agent/src/rollup.rs` (a leaf of the agent) or
  `touch crates/dyno-core/src/forecast.rs` (the core, which every other crate depends on), then build.
- **Command:** `env -u RUSTC_WRAPPER CARGO_TARGET_DIR=<dir> CARGO_BUILD_JOBS=13 CARGO_INCREMENTAL=1 [SDKROOT=<26.5 SDK>]
  [CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS=<lld flags>] nice -n 10 cargo build -q -p dyno-agent -p dyno-cli` in
  `apps/dyno`; an untimed build before each touch. Link only: rustc's own linker command (`cargo rustc ... -- -C save-temps
  --print link-args`) rerun with and without the lld flags, output to a scratch file.
- **Runs:** 5 rebuilds per scenario and set-up, interleaved (ldprime27, ldprime, lld per round); 7 interleaved link-only
  pairs per binary. Medians and ranges.
- **Numbers:**

  | Measure | ld-prime, SDK 27.2 (default) | ld-prime, SDK 26.5 | rust-lld, SDK 26.5 | Budget | Verdict |
  |---|---|---|---|---|---|
  | Rebuild after touching dyno-agent | 2.43 s (1.72–3.89) | 1.75 s (1.43–2.77) | 2.20 s (1.87–3.01) | > 20 % faster to adopt | ld-prime not slower (indication, loaded) |
  | Rebuild after touching dyno-core | 4.97 s (4.36–6.95) | 5.12 s (4.08–7.01) | 5.43 s (3.84–7.67) | > 20 % faster to adopt | ld-prime not slower (indication, loaded) |
  | Link of `dyno-agent` alone (33–35 MB) | – | 0.34 s (0.25–0.43) | 0.42 s (0.40–0.46) | – | ld-prime not slower (indication, loaded) |
  | Link of `dyno` (the CLI) alone | – | 0.12 s (0.08–0.14) | 0.145 s (0.13–0.17) | – | ld-prime not slower (indication, loaded) |

- **Memory measure:** not measured (a build).
- **Notes:** noise floor: the same linker (ld-prime) on two SDKs, which should take the same time, differs by 39 % in the
  dyno-agent medians (2.43 s against 1.75 s), so differences below that are noise at this load. Linking is 0.1–0.4 s of a
  1.5–5 s rebuild; the rest is rustc on the touched crate and its dependants, so no
  linker could make the rebuild 20 % faster, and rust-lld was not faster in any measure. This agrees with Dyno's
  2026-10-29 link-time study (rust-lld +16 % on the app's rebuild, [performance.md](../apps/dyno/docs/performance.md)). An earlier pass at the same load
  timed with a 0.5 s polling clock and is discarded. mold and sold were not tried: mold produces no Mach-O any more, and
  sold (the commercial macOS fork) was archived in November 2024, its README pointing to Apple's new linker. Linux and
  Windows were not measured: on `x86_64-unknown-linux-gnu` rustc already links with rust-lld (default since 1.90; the CI
  image's rustc 1.99.1 passes `-fuse-ld=lld` and `-B<sysroot>/.../gcc-ld`, checked with `--print link-args` in the image),
  and the repo's `.cargo/config.toml` already sets `linker = "rust-lld.exe"` for `x86_64-pc-windows-msvc`.
  Raw output: `/Volumes/d7sst7/dev-targets/linker-bench/` (`results.tsv`, `link-only*.tsv`).

### VM run outcomes, Dyno 1.1.0 rounds, 2026-11-05 to 11-08

A count, not a timing: the step outcomes of the local VM release checks, quoted by ADR 0042's amendment of 2026-11-08 as
the baseline for its WP-V9 targets.
- **Input:** every `20*-*.json` report under a `local-*` run folder in `/private/tmp/claude-501/**` and
  `/Volumes/d7sst7/dev-targets/scratch/**` written 2026-11-05 to 2026-11-08, 111 files (list:
  `/Volumes/d7sst7/dev-targets/scratch/adr-vm-harness/runs.txt`).
- **Command:** `node scripts/vm-smoke/round-stats.mjs --list-unclassified $(cat runs.txt)` on `tools/vm-smoke` `7735f1bbd`.
- **Machine and load:** not relevant (a count of existing reports).

| OS, runs | Runs | Steps | check-broken (all) | not-measured | check-broken, harness-caused | check-broken, precondition |
|---|---|---|---|---|---|---|
| Windows, full | 20 | 587 | 87 | 23 | 66 (11.2 %) | 18 |
| Windows, partial | 16 | 246 | 28 | 8 | 25 | 3 |
| macOS, full | 26 | 700 | 42 | 39 | 2 (0.3 %) | 39 |
| macOS, partial | 24 | 167 | 9 | 10 | 3 | 6 |
| Linux, full | 10 | 248 | 15 | 22 | 10 (4.0 %) | 5 |
| Linux, partial | 15 | 192 | 6 | 10 | 6 | 0 |

The harness and precondition columns split the 183 required steps that ended check-broken by keyword (reports before the
kit carry free text); 5 reasons stayed unclassified.

### 2026-11-09, how a connection ends on each OS: unread input, pipe CloseHandle, listener backlog, ordered-close baseline (feat/ipc-endings-m0)

WP-0 of ADR 0072 (`docs/adr/0072-every-connection-end-says-why-in-order.md` on branch `docs/adr-0072-ordered-close`; the measurements it asks for in "M0 plan" item 5). The
program uses only the OS's own APIs, not paddock-ipc, so it measures the platform: `cargo run --release -p paddock-ipc --example os_endings`
([source](../crates/paddock-ipc/examples/os_endings.rs)). Each case runs 200 times (timing: 500); the orchestration waits on the data
itself (`MSG_PEEK` until the unread bytes arrived), nothing sleeps. A "client" is the side that wrote 4 KiB the "server" never reads.
- **Commit:** base a03a24725 (origin/train/2026-11-08-1); the example is added by the change that adds this block.
- **Machines:** macOS 27.2 on a Mac mini M4 Pro (14 cores, 64 GB), the program native; Linux: Debian in a native arm64 container on
  the same Mac (OrbStack kernel 7.2.12, rustc 1.99.1, run as root with `--ulimit nofile=1024`); Windows: the Paddock test VM
  (Windows 11 Pro on Arm build 26300, 4 vCPU, 8 GB, VMware Fusion, nothing else running in it), the program cross-built for
  `aarch64-pc-windows-msvc` with `cargo xwin`.
- **Load:** macOS 1-minute load 5 to 8 during the macOS and Linux runs (other agents' builds); the VM idle (the program is one thread).
- **Build:** release. **Corpus:** none (synthetic bytes: 4 KiB of `0x07`, the line `REASON`).
- **Runs:** single run per OS, 200 repetitions per case (500 for timing). Raw output is kept below; counts are the whole result.
- **Numbers:**

  | Case (what the client read) | macOS | Linux | Windows |
  |---|---|---|---|
  | TCP, server closes with unread input | reason, then `ConnectionReset` (200/200) | reason, then `ConnectionReset` (200/200) | **no reason**, `ConnectionReset` (200/200) |
  | TCP, server writes the reason, half-closes, drains, then closes | reason, then EOF; 4096 bytes drained (200/200) | the same (200/200) | the same (200/200) |
  | AF_UNIX, server closes with unread input | reason, then EOF (200/200) | reason, then **`ConnectionReset`** (200/200) | not applicable (std has no AF_UNIX on Windows) |
  | AF_UNIX, ordered close | reason, EOF, 4096 drained (200/200) | the same | not applicable |
  | Named pipe, server `CloseHandle` with unread input | not applicable | not applicable | **reason, then broken pipe (109)** (200/200) |
  | Named pipe, server `DisconnectNamedPipe` then `CloseHandle` | not applicable | not applicable | **no reason**, `ERROR_NO_DATA` (233) (200/200) |
  | TCP listener closed, client connected and not accepted | client's write fails (`BrokenPipe` 139, `ConnectionReset` 3) or succeeds (58), then `ConnectionReset`, no reason | write `ConnectionReset`, then EOF, no reason (200/200) | write `ConnectionReset`, then `ConnectionReset`, no reason (200/200) |
  | AF_UNIX listener closed, client connected and not accepted | write `BrokenPipe`, then EOF, no reason (200/200) | write `BrokenPipe`, then **`ConnectionReset`**, no reason (200/200) | not applicable |
  | Named pipe: the one pending instance closed, client connected and not accepted | not applicable | not applicable | write fails (232), read broken pipe (109), no reason (200/200) |
  | Ordered close to release (reason, half-close, client reads and closes, server drains), TCP | p50 18 us, p99 42 us, max 67 us | p50 2 us, p99 9 us, max 12 us | p50 32 us, p99 105 us, max 230 us |
  | Ordered close to release, AF_UNIX / named pipe (reason, client reads and closes, server reads to the end) | p50 1 us, p99 4 us, max 7 us | p50 0 us, p99 1 us, max 5 us | pipe: p50 14 us, p99 46 us, max 58 us |
  | Refusals per second (connect, read the reason to EOF, drain, close), TCP | 20,712 | 154,804 | 5,022 |
  | Refusals per second, AF_UNIX / named pipe | 227,260 | 391,903 | pipe: 27,035 |

- **What it decides.** (1) **D4's pipe option is confirmed:** a named pipe closed with `CloseHandle` keeps what was written
  before it readable by the client (200/200), so on Windows pipes the server may release right after the reason and skip the
  drain; `DisconnectNamedPipe` discards it (200/200 without the reason), so it must never be called while the client may still
  read. (2) The unread-input reset is **not the same on every OS**: Linux resets AF_UNIX too (macOS does not), and Windows TCP
  loses the reason altogether, so a test that passes on the developer's Mac says little about Linux and Windows (ER-9). (3) The
  reset arrives after the reason on macOS and Linux loopback in this orchestration, but the 2026-11-07 field case quoted in
  `crates/paddock-mcp/src/loopback.rs` and the harness's own MCP scenario (reason lost on Linux, either order on macOS) show
  that the order varies under real timing; the harness therefore reports "reason lost to a reset" as one fault
  (`Expect::unread_input`). (4) The ordered close costs tens of microseconds on loopback; the ADR's bounds (2 s flush cap, 250 ms
  reason deadline, 2 s drain cap) are for a peer that stopped reading, not for the normal case.
- **Not measured:** the Windows AF_UNIX case (not in std); an x86_64 Windows host (the VM is Arm; the OS API is the same);
  loaded machines for this program (the ending tests are rerun under load separately, see the lessons entry); the Dyno helper's
  system endpoint (it is the same `endpoint` code).
- **Raw output** (macOS and Linux as printed; Windows from `C:\paddock-test\measure.txt`):

  ```text
  macOS   RESULT tcp-close-with-unread-input reps=200 reason+error(ConnectionReset)=200
  macOS   RESULT tcp-ordered-close-with-unread-input reps=200 reason+eof+drained4096=200
  macOS   RESULT tcp-listener-closed-with-client-in-backlog reps=200 write-BrokenPipe:error(ConnectionReset)-without-reason=139 write-ConnectionReset:error(ConnectionReset)-without-reason=3 write-ok:error(ConnectionReset)-without-reason=58
  macOS   RESULT tcp-close-to-release reps=500 p50_us=18 p99_us=42 max_us=67
  macOS   RESULT tcp-refusals-per-second reps=500 per_second=20712
  macOS   RESULT unix-close-with-unread-input reps=200 reason+eof=200
  macOS   RESULT unix-ordered-close-with-unread-input reps=200 reason+eof+drained4096=200
  macOS   RESULT unix-listener-closed-with-client-in-backlog reps=200 write-BrokenPipe:eof-without-reason=200
  macOS   RESULT unix-close-to-release reps=500 p50_us=1 p99_us=4 max_us=7
  macOS   RESULT unix-refusals-per-second reps=500 per_second=227260
  Linux   RESULT tcp-close-with-unread-input reps=200 reason+error(ConnectionReset)=200
  Linux   RESULT tcp-ordered-close-with-unread-input reps=200 reason+eof+drained4096=200
  Linux   RESULT tcp-listener-closed-with-client-in-backlog reps=200 write-ConnectionReset:eof-without-reason=200
  Linux   RESULT tcp-close-to-release reps=500 p50_us=2 p99_us=9 max_us=12
  Linux   RESULT tcp-refusals-per-second reps=500 per_second=154804
  Linux   RESULT unix-close-with-unread-input reps=200 reason+error(ConnectionReset)=200
  Linux   RESULT unix-ordered-close-with-unread-input reps=200 reason+eof+drained4096=200
  Linux   RESULT unix-listener-closed-with-client-in-backlog reps=200 write-BrokenPipe:error(ConnectionReset)-without-reason=200
  Linux   RESULT unix-close-to-release reps=500 p50_us=0 p99_us=1 max_us=5
  Linux   RESULT unix-refusals-per-second reps=500 per_second=391903
  Windows RESULT tcp-close-with-unread-input reps=200 error(ConnectionReset)-without-reason=200
  Windows RESULT tcp-ordered-close-with-unread-input reps=200 reason+eof+drained4096=200
  Windows RESULT tcp-listener-closed-with-client-in-backlog reps=200 write-ConnectionReset:error(ConnectionReset)-without-reason=200
  Windows RESULT tcp-close-to-release reps=500 p50_us=32 p99_us=105 max_us=230
  Windows RESULT tcp-refusals-per-second reps=500 per_second=5022
  Windows RESULT pipe-closehandle-with-unread-input reps=200 reason+broken-pipe(109)=200
  Windows RESULT pipe-disconnectnamedpipe-with-unread-input reps=200 win32(233)-without-reason=200
  Windows RESULT pipe-listener-closed-with-client-not-accepted reps=200 write-win32(232):broken-pipe(109)-without-reason=200
  Windows RESULT pipe-close-to-release reps=500 p50_us=14 p99_us=46 max_us=58
  Windows RESULT pipe-refusals-per-second reps=500 per_second=27035
  ```

- **Notes:** the VM was started for this run only, used through `scripts/vm-lock-run.sh` and `vm-win.sh`, stopped and reverted to
  `clean-baseline` afterwards (139 s with the lock held). The macOS numbers were taken with other builds running (load 5 to 8): the
  counts do not depend on load, the microsecond figures do; they are a baseline for "before", not a budget.

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
   - CI build parallelism on Windows: the host is being resized to 16 vCPU and 32 GB with runner `capacity: 3`
     (2026-11-09). `build-jobs.sh` declares the class default of 3 jobs per host: 5 build jobs ((16 - 0) / 3; RAM allows 6)
     and 8 test threads. Chosen, not measured. Open: an A/B of job durations for the share (3 x 5 rustc against fewer jobs
     of more), and an A/B of the test threads (8 against the core share of 5), as macOS found 8 matched 14 while the
     core share ran 2.3 times longer. The 1.5 GB per rustc that `PADDOCK_CI_RUSTC_MB` assumes is still an estimate. The
     earlier numbers (8 cores, 16,316 MB, run 7209: Debrief's Windows bundle 18.6 min with 4 against 14.5 min with 8,
     run 7195, different commits and caches) are not evidence.
   - CI build parallelism on Linux: `conformance/ci/build-jobs.sh` leaves `CARGO_BUILD_JOBS` unset on the Linux runner
     (cargo uses every core in each of up to four concurrent containers, as before 2026-10-30) and logs the host's cores:
     16 cores and 127,941 MB in a container on d-gtr9 (run 7211, job 23231).
     Its share (cores / 4 or cores / 2, `PADDOCK_CI_JOBS_PER_HOST` in d-gtr9's `runner.envs`) waits for one A/B of Gitea
     job durations of the Rust jobs (crate, crates, app legs, Debrief's rust tests), the same commits with and without it.
   - CI build parallelism: a clean, CI-shaped Dyno build (`cargo test --no-run --workspace --exclude dyno-app`, no sccache,
     no incremental) with `CARGO_BUILD_JOBS=4` (the Mac leg's share) against 13 (cores − 1). Tried on 2026-10-30; the
     5-minute load never fell below 10 in an hour, so nothing was recorded (`conformance/ci/build-jobs.sh` keeps 4 on the
     shared Mac until it is measured).
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
   - CI timings predate the monorepo, and `./dev ci timings` defaults to `paddock`; only the script (until deleted) defaults to the old per-app repos.
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

## `./dev check run` against the JS conformance host (RT-5, 2026-11-10)

Question: is the Rust host of C39 and C53 faster than `paddock-conformance.mjs` on this tree? Same two checks, same tree
(3466 tracked `.rs` files), release build of `tools/paddock-dev`.

| Host | Command | Wall time |
|---|---|---|
| Rust | `./dev check run --only C39,C53` (OS file cache warm) | 0.16 s |
| Rust | the same, cold file cache | 0.51 s |
| JS | `node conformance/bin/paddock-conformance.mjs --app paddock --only C39,C53 --root .` | 0.6 s |

- **Machine:** Mac mini M4 Pro, macOS 27.2. **Source:** the independent review of `feat/rt5-check-host` (40b7faf8f), single runs
  each, not interleaved; the load was not recorded. Treat the ratio (about 3.6 warm) as the figure, not the seconds.
- The debug build the tests use is slower; no budget is asserted. The shadow release (RT-5 wiring) can record CI timings.

## `./dev` cold build (planner port, 2026-11-08)

Question: what does a cache miss of the `./dev` wrapper cost a plan job? `cargo build --release --locked` of
`tools/paddock-dev`, cold target folder, `CARGO_BUILD_JOBS=2`.

| Where | Result |
|---|---|
| Mac mini (arm64, native), machine load about 30, through `heavy.sh` | 103 s wall (105 s user) for the cold release build; the binary is 4.2 MB |
| `paddock-ci:v2` image (linux/amd64) under emulation on that Mac, `--cpus=2` | did not finish in 25 minutes; not a measure of a CI host |

A real Linux CI host was not available, so the CI number is still open: measure the first cold plan job once the prebuilt
binary per hash (R0b) or the kept mount is in use. (Update: the prebuilt binary is measured in the next section, and the CI
plan steps call `./dev plan` on the `ci/dev-prebuilt` branch.)

## `./dev` prebuilt download (ADR 0022 R0b, 2026-11-08)

Question: what does a cache miss cost with the prebuilt binary compared with building the tool? Cold = `cargo build --release
--locked` of `tools/paddock-dev`, fresh target folder, `RUSTC_WRAPPER` unset, `CARGO_BUILD_JOBS=2`. Download = `./dev --version`
with an empty cache and `PADDOCK_DEV_DOWNLOAD=1`, which includes the wrapper's own work (git, `rustc -vV`, hashing).

| Where | Cold build | Download path |
|---|---|---|
| Mac mini (arm64), load about 33 | 194 s wall (101 s user), 4.2 MB binary | 0.6 to 1.0 s for the whole `./dev` call, 3 runs, from a local HTTP server on loopback (so no network cost; the 4.2 MB file is not a model of the real link) |
| CI, `paddock-ci:v2` container on a Linux runner (run 7721, `dev-tool.yml`) | 16 s for the build step: a true cold compile of 146 crates on a 16-core host with all 16 cores (`CARGO_BUILD_JOBS` unset), fresh target folder and `RUSTC_WRAPPER` unset by the publish script, so sccache was not used (warm or cold is moot); 5.4 MB binary | plan job of ci.yml run 7786: `dev: downloaded paddock-dev 2e607335... for x86_64-unknown-linux-gnu in 2 s, checksum and embedded hash verified` (whole seconds; the 5.4 MB file came from the private store, upload took 4 to 5 s) |
| CI, Windows host runner (run 7723) | `dev: built paddock-dev ... in 9 s` with the host's kept cache | not measured: the download got HTTP 401 (no read token reached the wrapper); fixed in this branch |

The Linux download is measured above (one run). Not measured: a Windows or macOS download (nothing is published for them yet), a cold build on a CI-like Linux host with an
empty compile cache, and the `paddock-ci:v2` image under emulation on the Mac (the earlier note here: it did not finish in 25
minutes; this time its test run, including the tool's cold test build, finished in under 8 minutes at `--cpus` default).
The cold numbers differ by 12x between this Mac (2 jobs, load 33) and the CI step (16 jobs), so the saving per CI job is about
16 s plus checkout of the crates on a 16-core Linux runner, more on hosts with fewer cores; the ADR's 100 s figure is a 2-job
figure. The CI download figure needs a measurement once the package is populated.

The first publish attempts failed for two reasons found on the way: `curl --data-binary` makes Gitea answer 500 (fixed with `-T`),
and a binary published for one hash is a miss after any commit that touches `tools/paddock-dev` (a test file changed between
publish and consume): the hash is per commit of the tool's inputs, so each such commit needs its own publish.

## Hook per-call cost and binary size (RT-4, 2026-11-10)

Question: what does the guard hook cost per tool call, and what does it add to `paddock-dev`? The hook is `paddock-dev hook
pre-tool-use` run as the installed copy (`<git-common-dir>/paddock/hooks/paddock-dev`, put there by `./dev hooks install
--local` in a throw-away repository), never through the `./dev` wrapper. Each run is a new process fed one JSON call on
standard input by a Python `subprocess.run`, so every figure includes the spawn (`/usr/bin/true` is the floor). 300 timed runs
after 20 warm-up runs, release build with `CARGO_BUILD_JOBS=2`, Mac mini (arm64), load average 15 to 16 (a busy machine).

| Per call | min | median | p90 | max |
|---|---|---|---|---|
| `/usr/bin/true` (the spawn floor) | 1.65 ms | 2.12 ms | 2.59 ms | 5.47 ms |
| Installed hook, an allowed Bash call (`cd /x && git status --short \| head -5; ls -la`) | 4.07 ms | 5.07 ms | 5.69 ms | 9.14 ms |
| Installed hook, a denied call (a `while` loop that polls `pgrep -f`) | 4.00 ms | 4.89 ms | 5.58 ms | 14.69 ms |
| Installed hook, a 6 KB commit command (heredoc in `$(...)`, then a push) | 4.04 ms | 4.96 ms | 9.85 ms | 15.15 ms |
| Installed hook, malformed input (allowed, one stderr line) | 4.35 ms | 7.05 ms | 10.72 ms | 27.41 ms |
| The first shim (`sh`, `git rev-parse`, then `exec` of the installed hook), an allowed call | 13.38 ms | 15.54 ms | 29.42 ms | 66.06 ms |
| A `brush-parser` 0.4.0 probe binary that only parses one `while` loop (no rules) | 2.29 ms | 4.24 ms | 5.81 ms | 10.29 ms |

The hook itself costs about 3 ms over the spawn floor, inside the plan's 4 to 6 ms range, and a longer command costs no more at the
median (every loop of the lexer has a limit: 1 MiB of text, 16 levels, 10,000 nested pieces, 4 MiB of `((` scanning; before the
last one 1 MiB of `(` took 256.7 s of CPU and 224 MB, after it 0.01 s: `hook pre-tool-use < 1 MiB of (`, release build, measured
with `/usr/bin/time -p`; the reviewer's `dos.py` set of 15 worst cases ran 0.2 to 2.0 s wall each, at a machine load of 15 to 60, including
Python's own spawn and a 1.05 to 8.39 MB input). Of the first shim's 15.5 ms, about 10 ms was the `git rev-parse` that finds the
common folder. The shim now finds it with shell builtins only (it walks up to `.git`; for a linked worktree it reads the
`gitdir:` line of the `.git` file and cuts `/worktrees/<name>`; the deny-all fallback and the install-command exception are
unchanged). Second run, same method, 300 runs, after that change (load average 18 to 20, busier than the first run, so its
spawn floor is higher too):

| Per call, second run | min | median | p90 | max |
|---|---|---|---|---|
| `/usr/bin/true` (the spawn floor) | 1.86 ms | 3.00 ms | 4.19 ms | 8.21 ms |
| Installed hook, an allowed Bash call | 4.04 ms | 5.87 ms | 7.99 ms | 17.56 ms |
| Installed hook, a 6 KB commit command | 3.65 ms | 5.20 ms | 6.56 ms | 10.99 ms |
| The shim without `git`, an allowed call | 6.80 ms | 8.55 ms | 9.23 ms | 19.59 ms |

The shim path falls from 15.5 ms to 8.6 ms at the median (about 2.7 ms over the hook alone, which is the `sh` start). The
binary of this second run is 7,684,640 bytes (the rule changes added 688 bytes).

| Binary size (`cargo build --release --locked`, `tools/paddock-dev`) | Bytes |
|---|---|
| `origin/train/2026-11-09-3` (before RT-4) | 7,567,536 |
| With `hook` and `hooks install` (hand lexer, two rules, installer) | 7,683,952 (+116,416, +1.5%) |
| `brush-parser` 0.4.0 probe alone (a hello-world that parses one command) | 871,216, with 94 new crates in its lockfile (`bon`, `darling`, `cached`, a second `syn` major) |

Decision recorded with the numbers: the lexer is hand written (`tools/paddock-dev/src/hook/lex.rs`, one file, 21 tests). The
brush-parser probe's start cost is not the problem (its parse-only run costs the same as the whole hook); the 94 crates
(proc-macro crates, a second `syn` major, licence and supply-chain review, every change to the lockfile changing the tool's input
hash) and an AST walk for what the rules read as a flat command list are. Reverse it if a later rule needs a real grammar.
Not measured: Linux and Windows hosts, and a run at the load of 53 that ADR 0083 quotes.

## Approval signature verification (ADR 0097, 2026-11-10)

Question: what does checking a user's signature cost per approval, so a gate can verify every record on every tag?
Throw-away software `ssh-ed25519` keys made in the agent's scratch folder (no real key, no keychain, no ssh-agent:
`HOME` in scratch, `GIT_CONFIG_GLOBAL=/dev/null`, `GIT_CONFIG_NOSYSTEM=1`, `SSH_AUTH_SOCK` unset), a five-commit repository
in the same folder, 200 calls per round, three rounds. macOS 27.2, Mac mini (Mac16,11, 14 cores), git 2.56.0, the system
OpenSSH 10.5p1. Load 8.8 before and 13.5 after: an indication, not a budget.

| Operation | Round 1 | Round 2 | Round 3 |
|---|---|---|---|
| `git verify-commit HEAD` with `gpg.format=ssh` and `gpg.ssh.allowedSignersFile` (one process for git, one for `ssh-keygen`) | 21.9 ms | 51.5 ms | 23.3 ms |
| `ssh-keygen -Y verify` of a detached signature over a 2-line record, own namespace | 11.6 ms | 7.9 ms | 8.9 ms |

History cost on the repository itself, `docs/gates.toml` at `origin/train/2026-11-09-3` (`c151422a4`; 1 023 lines, 14
commits touch it), same isolation, 5 calls per round, three rounds, load 15.9 to 20.4 (an indication):

| Operation | Round 1 | Round 2 | Round 3 |
|---|---|---|---|
| `git -c blame.ignoreRevsFile= blame --porcelain` of the whole file | 15 ms | 24 ms | 31 ms |
| `git log -L1016,1024:docs/gates.toml` (one record's history) | 44 ms | 52 ms | 52 ms |
| `git log --first-parent` of the file | 53 ms | 43 ms | 48 ms |

All are dominated by process start. Not measured: a security-key (`sk-ssh-ed25519`) or Secure Enclave signature (no
device was used, by design), an in-process Rust verifier, and the runners. At these rates 200 approval commits checked
with one `git verify-commit` each cost 4.4 to 10.3 s (200 x 21.9 to 51.5 ms); a verifier that checks each distinct commit
once is bounded by the number of approval commits, and the history walk adds about 50 ms per record.

## Kit shim default (2026-11-12)

`conformance/ci/kit.sh` gives the tool's own first build (before `./dev ci build-jobs` has written the job's real share) a default of
4 build jobs (`CARGO_BUILD_JOBS`). The cost on Linux, the one class that left cargo at every core before: a cold `cargo build
--release --locked` of `tools/paddock-dev` (what `./dev` runs when it can neither find nor download its binary) in the paddock-ci
image, `docker run --rm --cpus 6 --memory 12g`, linux/amd64 emulated on an arm64 Mac (so the absolute times are slow; the ratio is
the finding), an empty target folder each, one run each:

| CARGO_BUILD_JOBS | wall time |
| --- | --- |
| 4 (the shim's default) | 131 s |
| 6 (every core of the container) | 96 s |

So the default costs about 35 s (36 %) on a 6-core container, and only on a cold build: a job with the read credential downloads
the prebuilt tool (1-2 s) or finds it in the kept folder, and never builds. The default is kept because a build before `build-jobs`
has no share to go by, and on the shared Mac 4 is the share (ER-24). Not measured: a 16-core Linux runner (the cost there is larger
by the same ratio, bounded by the tool's compile graph).

## `./dev check --pre-push` run times (2026-11-12, feat/ci-in-rust-wp2)

One run of `./dev check --pre-push --base origin/main` on the branch's tree (66 changed files: the shared crate `paddock-proc`, paddock-dev,
workflows, docs), so the plan reaches every app. Machine: the shared Mac mini (arm64) at a 1-minute load of 25 (one gate at a time, as
`workers()` chooses from a load of 20 or more), `CARGO_BUILD_JOBS=2`, every compile through `heavy.sh`, warm build folders, Docker not
asked for (see below). The gate's budget is 15 minutes; one run each, so these are what happened, not a distribution.

| Gate | Seconds |
| --- | ---: |
| fmt, one per workspace (13 workspaces) | 1.4 to 6.5 each |
| lockfile `cargo metadata --locked`, one per workspace | 0.1 to 0.6 each |
| conformance (ci.yml's line) | 7.6 |
| commit-lint | 0.1 |
| Pitwall script tests, Pitwall `cargo deny` | 0.5, 1.9 |
| C102 `ipc-endpoints` | 4.3 |
| host Clippy `apps/pitcrew` | 285.8 |
| host Clippy `apps/dyno` | 173.6 |
| host Clippy `apps/debrief` | 395.5 |

Result: 30 gates passed in about 14 minutes; 12 were "not measured" because the budget ran out (Clippy of Pitwall, the demos, Spotter,
`crates`, `tools/*`, the Windows Clippy of `crates`, Dyno and Pitwall, and the Linux Clippy). Exit 6. So: a change to a shared crate on a
loaded machine does not fit in 15 minutes when the gates run one at a time; the cheap gates (about 30 s together) finish at once, and
the host Clippy of each app is the cost. Not measured: the same run on a quiet machine (three gates at a time), the Windows Clippy and
the Linux Clippy times (neither ran), and the cost of a change that reaches one workspace. Test durations of paddock-dev under
`cargo nextest run`: 863 tests in 115 s at a load of 18 to 25 (the same tests took over 23 minutes in one `cargo test` run that hung).

### Second measurement, after the review round (2026-11-12, feat/ci-in-rust-wp2)

The same branch tree as a pre-push hook run: `./dev check --pre-push --refs-from-stdin --base origin/main` with the pushed commit
checked out in a throw-away worktree (an untracked file in the main tree forced it), three gates at a time, `heavy.sh` on PATH,
`CARGO_BUILD_JOBS=2`, the shared Mac mini at a 1-minute load of 11 (falling to 8), warm build folders. The change reaches 13
workspaces (it changes the shared crate `paddock-proc` and paddock-dev); only the ones with a changed file of their own are
linted (`crates`, `tools/paddock-dev`); the nine dependents are listed as CI's. One run.

| Gate | Seconds |
| --- | ---: |
| host Clippy `crates` | 91.3 |
| Windows Clippy `crates` (xwin) | 94.0 |
| host Clippy `tools/paddock-dev` | 96.5 |
| fmt `crates`, `tools/paddock-dev` | 1.4, 1.8 |
| lockfile `cargo metadata --locked`, 11 workspaces | 0.1 to 0.3 each |
| conformance, commit-lint, Pitwall scripts, Pitwall deny, C102 | 7.3, 0.1, 0.5, 1.4, 3.1 |

Result: 21 gates passed, 0 not measured, exit 0, in about 100 s of wall time (the three Clippy runs overlapped). The first measurement
above (every app linted, one gate at a time at a load of 25) took over 14 minutes and left 12 gates unmeasured: restricting the hook
to workspaces with their own changes is what makes a shared-crate push fit the 15 minute budget. Not measured: the same run at a
load of 25 or more (the load was 11 at the start; with one gate at a time the three Clippy runs would add up to about 5 minutes
at these speeds, not measured), and the opt-in Linux Clippy (`--linux`).

## sccache and cargo 1.100's build-dir layout on Windows (2026-11-13, fix/spotter-win-cmdline-1-100)

### 2026-11-13, rustc spawn under sccache with and without the legacy build-dir layout (fix/spotter-win-cmdline-1-100)

- **Commit:** 4c581a781 (the fix: `__CARGO_TEMPORARY_BUILD_DIR_NEW_LAYOUT_OPT_OUT=1` beside `RUSTC_WRAPPER=sccache` on Windows)
- **Machine:** the local Windows 11 Arm test VM (VMware Fusion on the Mac mini, `scripts/vm-win.sh`), from `clean-baseline`, under the
  test-machines lock (231 s), reverted afterwards
- **Load:** host 1-minute load 9 to 34 during the session (other agents' builds); a pass/fail result, not a timing
- **Build:** debug; rustc and cargo 1.100.0 (`aarch64-pc-windows-msvc` dist components, checksums verified), sccache 0.19.0
  (`sccache-v0.19.0-aarch64-pc-windows-msvc.zip`, sha256 66d20f1e...), local disk cache, `CARGO_BUILD_JOBS=2`, `CARGO_INCREMENTAL=0`
- **Corpus:** a synthetic workspace of 450 leaf crates `c000..c449`, 20 middle crates `h00..h19` (each depends on a slice of the
  leaves), `hub` (depends on the 20) and `top` (depends on `hub`): every leaf is an indirect dependency of `hub` and `top`, so
  cargo 1.100 passes one `-L dependency=<build-dir>\debug\build\<pkg>\<hash>\out` per leaf. No build scripts or proc macros (no
  linker needed). Build folder under `C:\Users\paddock-test\paddock-cl-repro\a-deliberately-long-folder-like-the-ci-build-folder`.
  On the Mac the same shape with 400 leaves gave 420 `-L` and a 44,916-character rustc line for `top` with the new layout, 1 `-L`
  and 862 characters with the opt-out
- **Command:** `cargo build --offline -v -p top --target-dir <folder>\t-<case>`, once per case, each in a fresh target folder
- **Runs:** single run per case (deterministic pass/fail)
- **Numbers:**

  | Case | `RUSTC_WRAPPER` | opt-out variable | Result |
  |---|---|---|---|
  | before the fix | sccache | unset | exit 101: `sccache: error: failed to spawn Command` ... `The filename or extension is too long. (os error 206)` (crate `hub`) |
  | with the opt-out | sccache | `1` | builds (exit 0) |
  | without sccache | unset | unset | builds (exit 0): cargo's own `@argfile` fallback works |

- **Memory measure:** not measured
- **Notes:** reproduces CI run 8027, job 37099 (spotter_app_lib: 307 `-L`, about 37,000 characters). The cause is sccache 0.19
  expanding cargo's `@argfile` (mozilla/sccache#2787; fixes #2789 and #3026 open). Raw output and the generator were kept outside
  the repository (the session's build folder on d7), not committed.
