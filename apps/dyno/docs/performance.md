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
| `cargo test --release -p dyno-agent --test disk_cache_bench -- --nocapture --include-ignored` (`crates/dyno-agent/tests/disk_cache_bench.rs`) | A saved disk map ([ADR 0029](adr/0029-saved-views-disk-map-cache.md)): the packed record stream raw and deflated (`flate2` level 1), saving into `cache.db`, the start-up listing, loading a first level, and the footprint of a loaded map against a live one | the command (the `--include-ignored` part adds the 5M-node tree) | A generated tree of 1M (and 5M) nodes with `Node`'s real fields (folders, files, refused and partly refused folders, loose totals), names of 8 to 40 bytes, seed fixed in the test | in release builds: start-up listing < 20 ms; first level of a 1M-node map < 300 ms from the request; saving 1M nodes < 2 s; loaded map within 10 % of a live one (debug builds run 30,000 nodes and assert only the round trip) | no | [Saved disk maps](#2026-11-03-a-saved-disk-map-of-1m-and-5m-nodes-in-cachedb-dyno-cached-views) |
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

### 2026-11-03, a saved disk-map tree of 1M nodes in SQLite: rows against packed chunks (docs/dyno-cached-views)

- **Commit:** a18e18801 (origin/main; design measurement for [ADR 0029](adr/0029-saved-views-disk-map-cache.md), no Dyno code)
- **Machine:** Mac mini M4 Pro, 14 cores, 64 GB, macOS 27.2, internal SSD
- **Load:** 1-minute load 19.5 at the start of the run (other agents building); the numbers are upper bounds
- **Build:** CPython 3.12.5 with its SQLite 3.45.3 (Dyno bundles SQLite through `libsqlite3-sys` 0.38.2); WAL,
  `synchronous = NORMAL`, as ADR 0008; `zlib` level 1 as the stand-in for `flate2`/`miniz_oxide`
- **Corpus:** [`perf/disk_cache_sqlite_bench.py`](perf/disk_cache_sqlite_bench.py) `1000000 <dir>`: 1,000,000 nodes, a
  random parent among the previous 5,000, invented names of 12 to 21 bytes, random sizes (seed 42); a 64-byte record per
  node plus the names (88.9 MB raw); synthetic
- **Command:** `nice -n 10 python3 -I disk_cache_sqlite_bench.py 1000000 .`
- **Runs:** single run, cold process, warm page cache (the file was just written)
- **Numbers:**

  | Measure | One row per node | Packed 4 MiB chunks | Chunks, zlib 1 | Budget | Verdict |
  |---|---|---|---|---|---|
  | Write 1M nodes (one transaction) | 1.24 s | 0.38 s (+0.13 s packing) | 0.52 s (+0.13 s) | < 2 s, off the UI and sampling threads | all pass |
  | File size | 74.2 MB | 89.0 MB | 27.3 MB | — | — |
  | Read the whole tree back | 0.89 s (Python row objects) | 0.042 s | 0.101 s | < 0.3 s to the first answer | chunks pass; rows fail in Python |
  | Decode the records (Python `struct`) | — | 0.093 s | 0.091 s | — | — |

- **Memory measure:** not measured (Python process; the Rust bench of WP-5 measures footprint, rule 26)
- **Notes:** Like for like (read and decode to node values): chunks 0.135 s (0.042 + 0.093) against rows 0.89 s, about
  6.5x; the chunk read alone is not comparable with the row read. The records here have fewer fields than `Node`
  (no `reason`, `unread`, `child_count`); WP-5 uses the real ones. Python's per-row cost dominates the row layout's read; rusqlite is faster per row but still builds 1M rows
  where the chunks are 22 SQLite rows. The names compress far better than real names would (they are regular), so the
  zlib size is optimistic; WP-5 repeats this in Rust with realistic name lengths and a 5M-node tree before D3 is final.
  Raw output was not kept beyond this block.
### 2026-11-03, a saved disk map of 1M and 5M nodes in cache.db (feat/dyno-cached-views)

- **Commit:** 74aa0c1c1 plus the bench (`disk_cache_bench.rs`) of the commit that adds this block
- **Machine:** Mac mini M4 Pro, 14 cores, 64 GB, macOS 27.2, internal SSD
- **Load:** 1-minute load 16.2 before the run and 17.3 after (other agents building); the numbers are upper bounds
- **Build:** release (`cargo test --release`), Rust 1.99, `flate2` 1.1.10 on the `zlib-rs` back end (the one `zip` already links), `rusqlite` 0.40
  with bundled SQLite, WAL and `synchronous = NORMAL`, `secure_delete = ON`
- **Corpus:** the generated tree of `crates/dyno-agent/tests/disk_cache_bench.rs`: 1,000,000 nodes (117.0 MB of nodes and names
  in memory) and 5,000,000 nodes (820.0 MB); about one node in nine a folder, 1 in 1,000 refused and 1 in 1,000 partly refused,
  loose totals in a quarter of the folders; the record stream is 85.1 MB (1M) and 425.6 MB (5M), cut into 4 MiB chunks; synthetic
- **Command:** `cargo test --release -p dyno-agent --test disk_cache_bench -- --nocapture --include-ignored`
- **Runs:** single run, cold process, warm page cache (the file was just written); the machine was busy, so read these as upper bounds
- **Numbers:**

  | Measure | 1M raw | 1M deflate 1 | 5M raw | 5M deflate 1 | Budget (1M) | Verdict |
  |---|---|---|---|---|---|---|
  | Serialise to 4 MiB chunks | 0.011 s | 0.011 s | 0.051 s | 0.051 s | — | — |
  | Stored size | 85.1 MB (1.00x) | 34.7 MB (2.46x) | 425.6 MB | 173.6 MB (2.45x) | — | — |
  | Encode all chunks | 0.005 s | 0.244 s | 0.022 s | 1.225 s | — | — |
  | Decode all chunks (inflate) | 0.001 s | 0.109 s | 0.007 s | 0.550 s | — | — |
  | Save into `cache.db` (one transaction, off the walk and sampling threads) | 0.329 s | 0.380 s | 1.659 s | 1.845 s | < 2 s | pass |
  | File size after the save | 85.4 MB | 34.8 MB | 426.6 MB | 174.2 MB | — | — |
  | Start-up listing of the saved maps | 0.2 ms | 0.2 ms | 0.2 ms | 0.2 ms | < 20 ms | pass |
  | Load the tree (read, inflate, parse, check) | 0.057 s | 0.155 s | 0.299 s | 0.780 s | — | — |
  | Load plus the first level (a request for a level of an unloaded map) | 0.058 s | 0.156 s | 0.300 s | 0.781 s | < 300 ms | pass |
  | Heap of the loaded map against a live one of the same size | 0.958x | 0.958x | 1.095x | 1.095x | within 10 % | pass (1M); 5M is at the edge |

- **Memory measure:** heap by capacity (`DiskIndex::heap_bytes`, nodes and names), not RSS: a loaded map is 112.0 MB against 117.0 MB
  live (1M) and 897.8 MB against 820.0 MB (5M; the load grows its arrays by doubling, as the walk does, and the 5M tree ends just
  past a doubling). Rule 26: this is the arena, not the process footprint
- **Decision (D3):** `flate2` level 1 stays on. The record stream shrinks 2.45x to 2.46x (the criterion was at least 2x) and inflating
  a million nodes takes 0.109 s (under 0.15 s); the load that the user waits for is 0.155 s against 0.057 s raw, inside the
  300 ms budget, and the file is 34.8 MB instead of 85.4 MB. Encoding costs 0.24 s per million nodes on the writer thread, which
  nobody waits for
- **Python against Rust:** the design measurement (ADR 0029, packed chunks 0.135 s to read and decode 1M nodes) is now 0.057 s raw
  and 0.155 s deflated in Rust, with every bound of the parser checked
- **Notes:** a save reads a finished snapshot (`Arc`) chunk by chunk and takes no lock on the walk's index, so queries on the same scan
  are not held up while it runs (a centre test parks the writer inside a save and queries the scan). 5M nodes need about 1.8 s to
  save and 0.8 s to load: bounded and off the UI thread, but the 5M budget of the ADR (none) is a measurement, not a check
- **Verdict:** all four budgets pass at 1M nodes; no regression baseline yet (first record)
### 2026-11-03, CPU per process in a macOS VM: baseline 1.0.0 and main, and the CPU fixes (perf/dyno-cpu)

Trigger: the user's report on Dyno 1.0.0, "High CPU use".

- **Commit:** baseline: the `dyno-v1.0.0` release dmg (dist profile). A/B: base `a18e18801` (origin/main) → change `74985a3c0`
  (perf/dyno-cpu), both `tauri build --bundles app` with the release profile from one worktree and target folder
- **Machine:** a fresh Tart clone of `vmgui-base-tahoe-4aaa751f` (macOS 26.6.2, 4 vCPU, 8 GB, paravirtual GPU), headless,
  with an Aqua session; host Mac mini M4 Pro, 14 cores, macOS 27.2
- **Load:** host 1-minute load 11-24 (other agents building; one other Tart VM running); the guest was otherwise idle (no Dyno:
  0.9-1.9 % of one core for the whole guest, window server 0.0)
- **Build:** release builds; the 1.0.0 baseline is the stripped dist build
- **Corpus:** none: the VM's own idle machine (about 400 processes); counters: Memory > Active, Free, Wired and Used By
  Compressor charted; disk map: a scan of Macintosh HD (9 GB counted, 165 folders unreadable)
- **Command:** in the guest, `cpu2.sh <label> <windows> <seconds>`: CPU time deltas from `ps -axo pid,time,comm` per
  process group (app, agent, `com.apple.WebKit.WebContent`, `.GPU`, `WindowServer`, other) in % of one core, plus the
  whole guest's busy time from `top -l` (it catches short-lived children such as `nettop` and `ioreg`). Pages are
  opened with the guest's AX driver (`axdrive press Dyno <page> AXLink`), the window is closed with its close button.
  Agent alone: `dyno-agent bench 60 none|summary`. The scripts are kept with the run notes, not in the repo.
- **Runs:** baseline: 5 windows of 60 s per state (3 for the reduced-motion pair); A/B: main, change, main, change, 2
  windows of 60 s per state each, so 4 windows per build and state; ranges are reported
- **Numbers** (% of one core; app = `dyno-app`, web = the WebContent process, GPU = WebKit's GPU process, WS = WindowServer;
  "guest" is the whole VM):

  | State | Process | 1.0.0 | main (A) | change (B) | Verdict |
  |---|---|---|---|---|---|
  | Dashboard | app | 1.3-1.7 | 1.1-1.4 | 0.8-0.9 | better |
  | Dashboard | web | 2.5-3.7 | 1.6-2.4 | 1.3-1.6 | better |
  | Dashboard | GPU | 5.2-8.1 | 4.0-6.8 | 3.0-4.2 | better |
  | Dashboard | WS | 5.4-6.4 | 4.3-6.0 | 2.6-3.6 | better |
  | Dashboard | agent | 0.7-0.8 | 0.9-1.0 | 0.9-1.0 | same |
  | Dashboard | guest | not taken | 17.6-22.4 | 12.2-15.6 | about -30 % |
  | Counters, 4 charted | app | 2.5-3.3 | 2.5-3.2 | 1.1-1.3 | better |
  | Counters, 4 charted | web | 5.4-6.7 | 5.2-7.1 | 2.3-2.8 | better |
  | Counters, 4 charted | GPU | 3.3-4.2 | 3.2-4.9 | 0.7-0.9 | better |
  | Counters, 4 charted | WS | 9.6-12.4 | 10.0-12.6 | 1.3-1.6 | better |
  | Counters, 4 charted | guest | not taken | 28.0-36.9 | 8.9-10.3 | about -70 % |
  | Window closed (tray only) | app | 0.28-0.30 | 0.35-0.38 | 0.20-0.23 | better |
  | Window closed (tray only) | agent | 0.73-1.00 | 0.85-0.97 | 0.57-0.73 | better |
  | Window closed (tray only) | WS | 2.1-2.5 | 2.6-3.3 | 2.1-2.5 | slightly better |
  | Window closed (tray only) | guest | not taken | 5.8-7.0 | 5.0-6.4 | about -10 % |
  | Network page | app / web / agent / WS | 0.7 / 0.4 / 0.9 / 3.4-4.3 | not re-measured | not re-measured | cheap already |
  | Disk map scan (9 GB) | agent | 15.9 in the first 30 s (the scan), then 0.8-1.0 | not re-measured | not re-measured | bounded, user-started |

- **Causes, measured:**
  - The Counters chart was the worst state: `dy-chart` replaces its options whole (`notMerge`) with animation on, so every
    2 s poll replayed the entry animation, about 18 canvas frames. Fixed: only the first draw animates (WS 10-12.6 → 1.3-1.6).
  - The dashboard's live numbers (`dy-animated-number`) repainted on every animation frame for 300 ms after each 1 s
    update. Confirmed with macOS Reduce Motion on the same 1.0.0 build (which turns the glide off): web 1.5-1.7 → 0.95-1.0,
    GPU 4.4-5.2 → 1.9-2.2, guest 17.0-18.0 → 13.6-16.0. Fixed: 6 steps on a timer.
  - With the window closed the app still subscribed at the summary level (the tray needs numbers), so the agent read the
    process table every 2 s: `dyno-agent bench 60` none 0.38-0.39 %, summary 0.72-0.74 % (two runs each, interleaved).
    Fixed: a `tray` level with the process table at the unwatched pace.
  - The menu bar items: WS is 0.0 with no Dyno and 2.1-3.3 with Dyno closed to the tray, so the icon redraws are the
    largest cost of an idle Dyno in this VM (inferred: nothing else draws then). Each update called `set_icon` then
    `set_icon_as_template` (two redraws) and the tooltip. Fixed in the measured change by sending only the changed parts,
    pixels and template in one call; on main this is now the tray branch's single-call paint (`feat/dyno-tray-per-metric`,
    `paint_ops`), plus no picture set when only the tooltip changed (`perf/dyno-cpu-2`). Most of the WS cost remains: one
    redraw per item per second (follow-up below). The tray branch's new item look was not measured here.
  - Ruled out: the Network page (agent 0.9, web 0.4) and the Disk map page after a scan are cheap; the 2 s network poll
    and its `nettop` read do not show above the noise. A disk scan costs the agent about 16 % of one core while it runs,
    bounded and started by the user. ECharts on the dashboard's history panels does not redraw per tick (no poll).
- **Memory measure:** not measured (CPU only)
- **Notes:** every number here is from the VM, which renders in software: the window server and the GPU process cost
  much more than on a real Mac, so read the shares between states and the before/after, not the absolute values. The
  menu bar fix was measured only together with the tray level (window closed: WS 2.6-3.3 → 2.1-2.5, app 0.36 → 0.21);
  the double draw was found in the code (`tray-icon` 0.25.1 `set_icon_as_template` sets the image again), not measured on
  its own. The user's own Mac shows about 1.4 % of one core for Dyno 1.0.0. Follow-ups,
  not done (each under 2 % here): a menu bar refresh setting with an Auto default (the remaining WS 2.1-2.5 with the
  window closed), the agent's unwatched floor (0.4 %; `roll_up` reads three `meta` rows from SQLite every tick, and the
  sampler thread spent about 2 % of its samples in `pread`), and the Counters page re-reading the whole 5-minute history
  every 2 s instead of appending.

### 2026-10-30, ld-prime against rust-lld for incremental debug rebuilds (ci/build-parallelism)

The family results block is in [docs/benchmarks.md](../../../docs/benchmarks.md#2026-10-30-linker-for-dynos-incremental-debug-rebuilds-ld-prime-against-rust-lld-cibuild-parallelism)
(Mac mini M4 Pro, load 5.7–10.8, 5 interleaved runs): touching dyno-agent 1.75 s with ld-prime against 2.20 s with
rust-lld, touching dyno-core 5.12 s against 5.43 s, both on SDK 26.5 (rust-lld cannot read the 27.2 SDK); the link alone
0.34 s against 0.42 s. Verdict: ld-prime not slower (indication, loaded; the noise floor between two SDKs with the same
linker was 39 %). It agrees with the 2026-10-29 block below: keep Apple's linker.

### 2026-10-30, agent threads at the gentle level (rule 52, fix/dyno-qos-utility-2)

- **Commit:** A/B: base 1af0b346e (only the disk walk lowered; every other agent thread at the default class) → change: the
  agent's own threads (sampler, network, hardware, issues, extras, unusual, network change, inventory, boot,
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


## Tray items: text rasterising and re-rendering (2026-11-03)

Why: the menu-bar items redraw every second and 32 items are allowed (ADR 0028); the first version made a font and a CoreText
line for every mask and every width, and rendered every item each tick whether or not its inputs had changed. Rule 51 / ER-20.

- **Machine and build:** this Mac (Apple silicon, macOS 27.2), debug profile, 2026-11-03, `nice -n 19`, load 7 to 11.
- **Command:** `cargo test -p dyno-app --lib measure_text -- --ignored --nocapture` (the ignored test `tray_text::tests::measure_text`).
- **Workload:** 100 ticks of 32 stacked-number items, two text masks and one width each (a label and a value that changes every tick,
  at @2x), measured on `CoreText::uncached()` and `CoreText::new()`.

| Variant | Per tick, 32 items |
|---|---|
| `uncached` (a font and a line made for every call, one measurement per mask) | 4.53 ms |
| `cached` (fonts per style, lines per text and style) | 0.16 ms |

So about 28 times less per tick. The "before" row is the new code with its caches off, not the code as it was: the first version
also made an `NSFont` per call and measured each line twice per mask, so it was slower still. Not measured: the native image set per
item (the macOS VM run: Dyno 1.08 % CPU with 32 items), and the saving of the render memo: an item whose value, state, history
and tooltip did not change returns its last frame without rendering (`an_item_whose_inputs_did_not_change_is_not_rendered_again`
counts the masks: none for unchanged items). The model alone, 32 graph items all changing every tick, took 0.21 ms per tick with
the former renderers (the entry in ADR 0028).
## Bad-IP check: building and querying the range index (`dyno_core::intel`)
## IP-list check: building and querying the range index (`dyno_core::intel`)

User lists of addresses, CIDR ranges and host names become one in-memory sorted-segment index (ADR 0031, amendment 1). A lookup
is one binary search; nothing is queried from a database per connection. The test `intel::tests::bench_index`
(`cargo test -p dyno-core --release --lib -- --ignored --nocapture bench_index`) builds the index from a deterministic
pseudo-random spread of /16 to /32 IPv4 ranges (nested now and then) in one list, then times 200,000 lookups.

- **Machine:** Mac mini M4 Pro, 14 cores, 64 GB, macOS 27.2; rustc 1.99.1; release build; load 15 to 22 from other agents
- **Memory** is a count from the element sizes (`IntelIndex::approx_bytes`), not an allocator reading.

| Entries | Build (single thread) | Segments | Memory (counted) | Lookup p50 | Lookup p99 |
|---|---|---|---|---|---|
| 100,000 (first design, one `u128` index) | 19 ms | 133,085 | about 39 MB | 208 ns | 875 ns |
| 1,000,000 (first design) | 283 ms | 1,601,926 | about 327 MB | 1,166 ns | 1,708 ns |
| 100,000 (compact) | 7 ms | 133,085 | about 6 MB | 125 ns | 458 ns |
| 1,000,000 (compact) | 77 ms | 1,601,926 | about 50 MB | 750 ns | 1,250 ns |

The compact index (rule 51, cheaper): one sorted array per address family (`u32` for IPv4, `u128` for IPv6), segment starts only
(a gap is a segment with no members), a 4-byte member offset per segment, members as `(slot, list bitmap)`, each distinct range
stored once with its list bitmap; notes only for the entries that have one; entry text is never resident (shown from the range
on demand). It is 6.5 times smaller and builds 3.7 times faster than the first design, with faster lookups (smaller arrays fit
the cache better). The build runs as a job: `IntelIndex::build_with_progress` reports the phase and count and stops when
cancelled.

Reading: 2,000 rows per look cost about 1.5 ms at the million-entry limit, so no extra thread is needed. The start-up rebuild
is 77 ms at 1,000,000 entries, under the 200 ms line for persisting a packed index, so none is persisted. Transient peak while
building is the lists themselves plus a hash map of the distinct ranges (not measured separately).
## Disk test, first real run (2026-11-03, ADR 0030 WP-B3)

One manual run of the rebuilt disk test to see that direct I/O reaches the device and not memory. It is not a benchmark of the
drive and not a test: the numbers move with whatever else the machine and the drive are doing.

- **Commit:** `feat/dyno-benchmark-2`, the commit that records this entry
- **Machine:** Mac mini M4 Pro, 14 cores, 64 GB, macOS 27.2; the scratch folder on the external SSD `/Volumes/d7sst7` (APFS)
- **Load:** 1-minute load about 8 (other agents), so the numbers are indications only
- **Build:** debug (`target/debug/dyno-bench`), which does not matter for I/O-bound work
- **Command:** `dyno-bench disk --disk-mib 256 --dir <folder on that drive>` (3 repetitions, ramp at most 1/8 of the file or 1 s); 73 s in all

| Pattern | Median of 3 | Samples |
|---|---|---|
| Sequential 1 MiB, 1 thread, read | 511 MB/s | 496, 511, 532 |
| Sequential 1 MiB, 1 thread, write | 241 MB/s (unsteady, CV 20 %) | 313, 241, 215 |
| Sequential 1 MiB, 8 threads, read | 508 MB/s | 508, 499, 550 |
| Sequential 1 MiB, 8 threads, write | 254 MB/s (unsteady, CV 33 %) | 301, 254, 150 |
| Random 4 KiB, 1 thread, read | 2,400 IOPS (unsteady) | 1,223, 2,756, 2,400 |
| Random 4 KiB, 1 thread, write | 1,082 IOPS | 1,141, 1,082, 958 |
| Random 4 KiB, 32 threads, read | 34,337 IOPS (140.6 MB/s) | 34,337, 32,096, 34,758 |

What it shows: reads are in the hundreds of MB/s and 4 KiB single-thread reads in the low thousands of IOPS, the device's order of
magnitude, where the 1.0 method read the file back from the page cache (memory speed). The `F_NOCACHE` path works on macOS. The
write medians are marked unsteady by the CV rule, as designed. **Not measured:** Windows and Linux direct I/O (to be checked on the
runners, WP-B3's remaining item), a spinning disk, a network share, 4 GiB files, and the write budget on a real drive (it is proved
against a scripted drive in `disk::tests`).

## CPU, memory and GPU kernels, first real runs (2026-11-03, ADR 0030 WP-B4 to B6)

Single runs to see that each kernel gives a plausible number and reaches the hardware. Not benchmarks of the machine and not tests;
see [Disk test, first real run](#disk-test-first-real-run-2026-11-03-adr-0030-wp-b3) for the setup.

- **Machine:** Mac mini M4 Pro, 14 cores, 64 GB, macOS 27.2, load average 8 to 20 from other agents; debug build of the runners
  (the kernels' dependencies are optimised in the dev profile, the runner crates are not, so CPU numbers from a debug build are
  lower than a shipped build's; they are recorded only to show the order of magnitude)
- **GPU, `dyno-bench-gpu --smoke`** (one repetition, Metal, Apple M4 Pro): compute 6,920 GFLOPS, copy 204 GB/s, fill 270 Gpixel/s,
  geometry 494 Mtri/s, 1.9 s in all. The fill figure is probably high: the off-screen target is discarded after each pass, which a
  tile-based GPU may skip writing back; the label says "fill rate", not "memory bandwidth". **Not measured:** the five-repetition run,
  discrete GPUs, DX12, Vulkan, the size of the bundle with wgpu (WP-B6 asks for it).

## Processes of every account through the helper (2026-11-04, helper-wiring WP-PROC)

The process list is a Live reading: the helper's sweep runs on its own worker every 5 s (`SWEEP_PERIOD`, paused 20 s after the
sampler last took the list), never on the sampling thread (ER-17). The sampling thread only merges the latest finished sweep into
its table. Pages: up to 16 of 1,024 entries (`MAX_SWEEP` 16,384). The helper reads the machine once per page request, so a machine
with fewer than 1,025 processes costs one sweep per period.

- **Commit:** `dyno/wp-proc` (the commit that adds this block)
- **Machine:** Mac mini M4 Pro, 14 cores, macOS 27.2, 1-minute load 3 to 8 from other agents (indications only)
- **Build:** release
- **Real sweep, `measure_a_real_sweep`** (`cargo test -p dyno-agent --release --test helper_processes -- --ignored --nocapture`;
  `SystemSweeper` then `build_page` for one page, three rounds, about 910 processes): sweep 42 to 51 ms, scoping and paging 8 ms.
- **Merge at the cap, `measure_merge_cost`** (`--lib --release -- --ignored --nocapture`; 16,384 helper entries with rates against
  an earlier sweep, merged with 8,192 local rows): rates 1.7 ms on the worker, merge 1.0 ms on the sampling thread.

Per sampling tick the sampler thread therefore pays about 1 ms at the largest allowed sweep, and no IPC. The worker pays about 50 ms
every 5 s on a machine like this one, 0.5 to 1 % of one core. **Not measured:** the Windows and Linux VMs (the VM rows of the
plan), a machine with several thousand processes, and the wire time of a 1,024-entry page (the IPC rig measures the encoding, not
the pipe). The period is a constant, not yet a Setting with an "Auto" default (the settings schema is not this package's file).
