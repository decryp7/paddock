# Performance record

Dyno's benchmarks, budgets and recorded results. How to run a benchmark reproducibly and the results template are in the
family guide, [docs/benchmarks.md](../../../docs/benchmarks.md). Record every run there quoted (newest first) here.

Dyno has two things called "bench":
- **`dyno-bench` and the Benchmarks page** (`crates/dyno-bench`, `app/src/app/features/benchmarks`,
  [ADR 0015](adr/0015-benchmarks-network-and-ip-lookup.md)) are a product feature. They measure the user's machine (CPU,
  memory, disk). `crates/dyno-bench/tests/process.rs` tests the feature's states, not its speed.
- **`dyno-agent bench`** measures Dyno itself and is the benchmark this file covers.

## Benchmarks

| Benchmark | Measures | Command | Corpus | Budget asserted | CI | Results |
|---|---|---|---|---|---|---|
| `dyno-agent bench [seconds] [none\|summary\|processes]` (`crates/dyno-agent/src/cli.rs`) | The agent's own CPU (share of one core) and memory with nobody watching, a summary subscriber or a processes subscriber; JSON output | `cargo run --release -p dyno-agent -- bench 25 processes` | The live machine (read-only) | none | No. `scripts/check-bundle.sh` (`dyno-release.yml`) runs `dyno-agent once`, a smoke test with no budget | below; [architecture.md](architecture.md#dynos-own-overhead) |
| `cargo test --release -p dyno-agent --test finder_bench -- --ignored --nocapture` (`crates/dyno-agent/tests/finder_bench.rs`) | The file finder (large and duplicate files) on a generated tree, by the number of threads reading it | the command | 400 folders, 50,000 small files and 300 files of about 1 MiB (100 files with two copies each), made in a temp folder | none (a measurement; the checks are the finder's unit tests) | no | [File finder](#file-finder-2026-10-29) |
| `cargo test --release -p dyno-platform --test maintenance_bench -- --ignored --nocapture` (`crates/dyno-platform/tests/maintenance_bench.rs`) | The Maintenance view's status (when the windows close or open next) in the worst case, with the OS zone and a POSIX zone with summer time | the command | 14 windows, all closed now, the next opening almost a week ahead (generated in the test) | none (a measurement; `jumping_to_window_edges_finds_what_scanning_every_minute_finds` checks the answers) | no | [Maintenance status](#maintenance-status-2026-10-30) |
| `app/angular.json` budgets | The initial bundle | `cd app && npx ng build` | — | initial warn 900 kB / error 1.5 MB; component style warn 16 kB / error 32 kB | `ci.yml` Dyno leg | — |

## Budgets

| Budget | Value | Owner | Enforced by |
|---|---|---|---|
| Dyno's own cost | about 1% of the machine; the sampler's `sampling::CPU_BUDGET` = 1.0% of one core | [ADR 0002](adr/0002-sampling-architecture.md) | Adaptive intervals stretch at run time when the budget is exceeded; **no test** |
| Idle process | ≤ 0.5% of a core, ≤ 1 wake-up/s | family [ADR 0024](../../../docs/adr/0024-background-work-contract.md) D7 | nothing |

## Gaps

- [Roadmap](roadmap.md) G13: the overhead was measured once, under heavy load, on one machine. There is no CI artifact
  per OS and no regression check. Run `bench` on release builds on a quiet runner per OS, record the load next to the
  numbers, and fail on a regression (through `paddock-bench`).
- The lessons ask to "put the poll cost in the benchmark budgets" (a068435). No benchmark does this yet.
- Memory is RSS, not footprint or PSS (rule 26).

## Results (newest first)

### 2026-10-30, ld-prime against rust-lld for incremental debug rebuilds (ci/build-parallelism)

The family results block is in [docs/benchmarks.md](../../../docs/benchmarks.md#2026-10-30-linker-for-dynos-incremental-debug-rebuilds-ld-prime-against-rust-lld-cibuild-parallelism)
(Mac mini M4 Pro, load 5.7–10.8, 5 interleaved runs): touching dyno-agent 1.75 s with ld-prime against 2.20 s with
rust-lld, touching dyno-core 5.12 s against 5.43 s, both on SDK 26.5 (rust-lld cannot read the 27.2 SDK); the link alone
0.34 s against 0.42 s. Verdict: ld-prime not slower (indication, loaded; the noise floor between two SDKs with the same
linker was 39 %). It agrees with the 2026-10-29 block below: keep Apple's linker.

### 2026-10-30, agent threads at the gentle level (rule 52, fix/dyno-qos-utility-2)

- **Commit:** A/B: base 1af0b346e (only the disk walk lowered; every other agent thread at the default class) → change: the
  agent's own threads (sampler, network, hardware, issues, extras, unusual, network change, inventory, boot, backup check,
  counters, volume list and probes, traffic and name lookups) and the finder's thread and workers at `lower_current_thread`
  (`QOS_CLASS_UTILITY` on macOS); `dyno-server`, its connections and the benchmark thread not lowered
- **Machine:** Mac mini M4 Pro (Mac16,11), 14 cores, macOS 27.2; rustc 1.99.1
- **Load:** busy, other agents building: 1-minute load 21-61, 5-minute 22-37 (the finder runs started at 5-minute 36, above
  the usual gate of 30), so the numbers are noisy and only compare like for like
- **Build:** release (`cargo build --release -p dyno-agent`, `cargo test --release --test finder_bench --no-run`), each
  commit in its own worktree and target folder
- **Corpus:** idle: the live machine, read-only, nobody subscribed; finder: `finder_bench`'s generated tree (400 folders,
  50,000 small files, 300 files of about 1 MiB)
- **Command:** idle: `dyno-agent bench 150 none` (in-process agent, data in a temp folder, no window and no launch agent)
  with `top -l 25 -s 5 -pid <pid> -stats pid,cpu,power` and one `ps -M -p <pid>` while it ran; finder: the
  `finder_bench` binary with `--ignored --nocapture` under `nice -n 10`
- **Runs:** alternating base, change, base, change; the idle runs 150 s each; the finder bench reports best and median of
  its own repeats
- **Numbers:**

  | Measure | Base | Change | Budget | Verdict |
  |---|---|---|---|---|
  | Idle agent CPU, `bench` (% of one core, 2 runs) | 0.98, 0.90 | 1.06, 0.95 | about 1% (ADR 0002), 0.5% idle (ADR 0024 D7) | the same within noise; over the 0.5% idle budget before and after |
  | Idle agent CPU, `top` mean of 24 samples (2 runs) | 0.92, 0.83 | 0.97, 0.88 | — | the same within noise |
  | Idle energy impact, `top` POWER mean of 24 samples (2 runs) | 0.92, 0.83 | 0.97, 0.88 | — | equal to the CPU column (see the notes) |
  | Resident memory, `bench` | 29.7, 29.8 MB | 30.0, 30.1 MB | — | the same |
  | Agent threads by scheduling priority (`ps -M`) | all at 31 (default) | 8 at 20 (utility), the rest at 31 | — | as intended |
  | Finder, 1 thread, median (2 runs) | 0.81, 0.82 s | 0.85, 0.85 s | none | about 4% slower on a busy machine |
  | Finder, 4 threads, median (2 runs) | 0.25, 0.25 s | 0.26, 0.26 s | none | about 4% slower |
  | Finder, 8 threads, median (2 runs) | 0.18, 0.16 s | 0.19, 0.19 s | none | about 10% slower, within its spread |

- **Memory measure:** resident set size from `bench` (`sysinfo`)
- **Notes:** the POWER row is not a copy of the CPU row: `top` printed the same value in its `%CPU` and `POWER` columns in
  every one of the 100 samples (4 runs × 25), so for this process its energy-impact score followed CPU time alone and adds
  nothing to the CPU row. Why `top` gives equal values is not documented in `top(1)` (not verified further). The means
  leave out `top`'s first sample of each run, which is always 0.0 (it has no earlier sample to compare with); the first
  version of this entry counted it. Lowering changes when the agent runs, not how much work it does, so its idle CPU and energy stay the same; a
  lowered search yields to other work on a busy machine, which is the few percent above. Not measured: the agent under
  launchd (`ProcessType Standard` instead of `Background`), which needs the installed app; that is for the VM checks before
  the tag, with `ps -M` on the running agent (every thread was at 4 under `Background`).

### 2026-10-29, release identity of the rlib-only crate type (docs/link-time)

- **Commit:** A/B: base cb3329e0f (template crate types) → change 451948bbb (`crate-type = ["rlib"]`): pre-rebase cb3329e0f (parent cae1649f9) → 451948bbb, same content as 784264b1d's crate-type diff; both kept on the branch `wip/link-time-ab`; research note
  [2026-10-29-link-time.md](../../../docs/research/2026-10-29-link-time.md#release-identity)
- **Machine:** Mac mini M4 Pro (Mac16,11), 14 cores, 64 GB, macOS 27.2, Xcode 27.1 (`ld-27037.1`); rustc 1.99.1
- **Load:** 1-minute load 16-43 (other agents building)
- **Build:** release, `dist` profile (thin LTO, one codegen unit, strip); unstripped copies with `--config profile.dist.strip=false`; Apple `ld`; default features
- **Corpus:** none (the build output is compared); synthetic
- **Command:** `CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/Volumes/d7sst7/dev-targets/target/link-time-verify cargo build -p dyno-app --profile dist` at each commit, then `cargo clean -p dyno-app --profile dist` and the same again; `cargo test -p dyno-app --profile dist` at each commit
- **Runs:** A, B, A after clean, B after clean (single builds); tests once per commit
- **Numbers:**

  | Measure | Base | Change | Budget | Verdict |
  |---|---|---|---|---|
  | `dyno-app` SHA-256 (first build / rebuild) | 27afd223... / 27afd223... | 7bce7a2e... / 7bce7a2e... | reproducible | each reproducible, A ≠ B |
  | Size, `__text` | 11,205,808 B, 7,776,740 B | 11,205,824 B, 7,778,404 B | none | +16 B, +1,664 B |
  | Functions with different instructions (unstripped, by demangled name) | 10,694 named | 10,692 named; 19 differ (addresses, `TypeId` constants, inlining) | — | not semantic |
  | `cargo test -p dyno-app --profile dist` | 74 passed, 1 failed | 74 passed, 1 failed (the same test) | — | same |
  | App unit after `cargo clean -p dyno-app` | 84.2 s | 69.8 s | none | indication only (load 16-25) |

- **Memory measure:** not measured
- **Notes:** the failing test (`native_feel::tests::debug_windows_keep_reload_and_devtools_but_never_zoom`) asserts a
  debug build and fails in any release profile. Analysis: [`scripts/perf/link-time/classify.py`](../../../scripts/perf/link-time/classify.py) and
  [`disfn.sh`](../../../scripts/perf/link-time/disfn.sh); their outputs are machine-local in `/Volumes/d7sst7/dev-targets/target/link-time-tools/`.

### 2026-10-29, incremental rebuild and link time (docs/link-time)

- **Commit:** cae1649f9 (origin/main) plus local, uncommitted experiment edits (crate type, a dev `dylib` crate); research
  note [2026-10-29-link-time.md](../../../docs/research/2026-10-29-link-time.md)
- **Machine:** Mac mini M4 Pro (Mac16,11), 14 cores, 64 GB, macOS 27.2, Xcode 27.1 (`ld-27037.1`); rustc 1.99.1 (LLVM 23.1.1)
- **Load:** 1-minute load 3.7-6.0 during the measured builds; other agents mostly idle; no other job of mine running
- **Build:** debug (`dev`: deps opt-level 2, `debug = "line-tables-only"`, `split-debuginfo = "unpacked"`), default features
- **Corpus:** none (the compiler is measured); synthetic one-line change in the app crate
- **Command:** Probe build: `perl` replaces `pub fn __link_probe() -> u32 { N }` at the end of `app/src-tauri/src/lib.rs`, then `CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=<private> CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=cc-timed.sh LINKMODE=ld|lld cargo build -p dyno-app [--features dynamic_linking] [--config 'profile.dev.package.dyno-app.debug=false|"full"']`; crate type switched by editing `[lib] crate-type`. [`cc-timed.sh`](../../../scripts/perf/link-time/cc-timed.sh) runs `cc` (with `--ld-path=<toolchain>/gcc-ld/ld64.lld` and `SDKROOT=MacOSX26.5.sdk` for lld) and logs each link's duration. Phase split: the same with `--timings`. The probe loop and round driver were throw-away shell scripts in `/Volumes/d7sst7/dev-targets/target/link-time-tools/` (machine-local); the linker wrapper is committed.
- **Runs:** 3 rounds, interleaved (every configuration once per round, each after an uncounted warm-up build); median (range) of wall time; link = median duration of the linker call
- **Numbers:**

  | Measure | Base (today: staticlib + cdylib + rlib, Apple ld) | Change | Budget | Verdict |
  |---|---|---|---|---|
  | Incremental rebuild, crate type rlib only | 1.91 s (1.74-1.94) | 1.19 s (1.15-1.20), -38% | none | adopt |
  | Incremental rebuild, dev `dylib` (option a), rlib only | 1.91 s | 1.08 s (1.03-1.17), -43% | none | not worth it |
  | Incremental rebuild, lld (today's crate types) | 1.91 s | 2.21 s (2.03-2.29), +16% | none | reject |
  | Incremental rebuild, lld, rlib only / dylib | 1.19 / 1.08 s (ld) | 1.25 / 1.09 s | none | reject |
  | Incremental rebuild, app crate `debug = false` / `"full"` | 1.91 s | 1.78 s (-7%) / 2.22 s (+16%) | none | keep line-tables-only |
  | Binary link (`ld`) | 0.21 s (0.21-0.24) | rlib 0.22 s, dylib 0.13 s, lld 0.34 s | none | — |
  | cdylib link (`ld`) | 0.14 s (0.14-0.16) | gone with rlib only; lld 0.26 s | none | — |
  | App library unit (`--timings`, 2 runs) | 1.37-1.54 s | 0.60-0.76 s (rlib only) | none | — |
  | App binary unit (`--timings`, 2 runs) | 0.34-0.35 s | 0.34 s (rlib only) | none | — |
  | `libdyno_app_lib.a` written per build | 364 MB | none (rlib only) | none | — |
  | `dist` binary (`--profile dist`, 1 build each, load 8-44) | 11,205,808 B, `__text` 7,776,740 B, SHA-256 af25a650... | rlib only: 11,205,824 B, `__text` 7,778,404 B, SHA-256 13b39130...; not byte-identical (built with the probe edit; see the release-identity block above) | none | verified above |

- **Memory measure:** not measured (build time only)
- **Notes:** Cold build: 5 min 15 s (load 9-45, other agents building). Dist identity check of the crate-type change: see the research note. Raw CSV and link logs: `/Volumes/d7sst7/dev-targets/target/link-time-tools/` (machine-local).

### Date not stated, the agent's own overhead

- **Commit:** not stated
- **Machine:** Mac mini M4 Pro, 14 cores
- **Load:** 1-minute load about 55–85, so the numbers are upper bounds
- **Build:** not stated
- **Command:** `dyno-agent bench 25 none|summary|processes`
- **Numbers:**

  | Scenario | CPU (one core) | Resident memory |
  |---|---|---|
  | Idle, nobody watching | about 2.0% | about 24 MB |
  | Summary subscriber | about 2.7% | about 24 MB |
  | Processes subscriber | about 3.2% | about 24 MB |

- **Notes:** 2% of one core on 14 cores is about 0.14% of the machine. A quiet re-run on a release build on each OS is
  needed. Source: [architecture.md](architecture.md#dynos-own-overhead).

## File finder (2026-10-29)

Rule 48: the finder was sequential (one thread walked, then hashed); P1-4 of the review (ER-20, rule 51) moved it to
`paddock_jobs::par_walk` and `par_map_ordered` (4 to 8 threads on an SSD, 2 on a network share, 1 on a spinning disk; the results
come back in input order, so the answer does not depend on the count, which a unit test checks).

**Corpus:** 400 folders, 50,000 files of 5 bytes, 300 files of 1 MiB + 17 bytes (100 distinct, each with two copies in other
folders), in a temp folder on the internal APFS SSD of the Mac mini, mode "Both", the 1 MiB floor. Release build, `nice -n 10`.
**Conditions:** the machine was **busy** (load average 25 to 44 from other agents' builds), so the numbers are noisy; the first
sequential run was disturbed most (median 4.37 s). Three runs per row; best and median in seconds. Like for like: the same
tree, the same query and the same machine within one session.

| Code | Threads | Best | Median |
|---|---|---|---|
| before (commit 6c73efd75, sequential walk and hashing) | 1 | 1.04 to 1.63 | 1.51 to 4.37 |
| after | 1 | 1.00 to 1.18 | 1.04 to 1.24 |
| after | 2 | 0.58 to 0.70 | 0.60 to 0.83 |
| after | 4 | 0.32 to 0.37 | 0.38 to 0.40 |
| after | 8 | 0.23 to 0.33 | 0.25 to 0.37 |

So 4 threads take about a third of the time and 8 about a quarter to a third of it, on a corpus that sits in the OS file cache.
The corpus is small and warm; a large cold folder spends its time waiting for the disk, where parallel reads help more on an
SSD, and not at all on a spinning disk (hence one thread there). **Not measured:** a spinning disk and a network share (none to
hand); their thread counts follow ER-20's table and the disk map's.

## Maintenance status (2026-10-30)

Rule 51 (review P2 of ADR 0024): the status looked for the next change by testing every minute of the next eight days, with
two zone lookups per window per minute. It now jumps from window edge to window edge (`time::starts_of`, so a time the clocks
skip or repeat is handled) and does one zone lookup per test. The answers are the same: the benchmark prints them (9,535 and
9,955 minutes to the next opening, before and after), and a unit test compares the two ways around both of 2026's clock changes.

- **Commit:** A/B on `fix/dyno-today-grouping`: before = the minute scan of 794c3cd70 (with this round's other edits), after =
  the edge jump of the commit that records this entry
- **Machine:** Mac mini M4 Pro, 14 cores, 64 GB, macOS 27.2; rustc 1.99.1
- **Load:** 1-minute load 20 to 22, 5-minute 29 to 30 (other agents and the nightly CI); an indication, compared only within
  this A/B
- **Build:** release (`cargo test --release --no-run`), the two test binaries copied and run in turn, `nice -n 10`
- **Corpus:** 14 windows on Mondays from 09:00 to 10:00 (start minutes 0 to 13 apart), now Monday 2026-10-26 10:05 UTC; synthetic
- **Runs:** before, after, three pairs, interleaved; 50 calls each

| Zone | Before, best | Before, mean | After, best | After, mean |
|---|---|---|---|---|
| The OS zone (`SystemZone`, Asia/Singapore on this machine) | 6.06 to 6.20 ms | 6.37 to 6.61 ms | 0.029 to 0.030 ms | 0.052 to 0.067 ms |
| POSIX `CET-1CEST,M3.5.0,M10.5.0/3` (`TzZone`) | 6.27 to 6.78 ms | 7.23 to 7.41 ms | 0.034 to 0.035 ms | 0.035 to 0.037 ms |

So about 200 times less work per status. It was never slow enough to notice once, but the status is asked for each time the
Maintenance view opens and the minute scan grew with the number of windows. **Memory:** not measured (a vector of at most 504
instants).

