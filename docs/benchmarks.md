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

## Timing benchmarks on a busy host

A benchmark that gives a verdict on the shared Mac does not gate on the 1-minute load average (ER-24: it counts OrbStack and
build threads, and run 8175 was red for starts that were not slowed), and it is never refused for a busy host (user, 2026-11-17).
The one shared mechanism is `scripts/bench/busy-host.mjs` (tests: `busy-host.test.mjs`); Debrief's `scripts/ci/startup-bench.mjs`
uses it, results in
[Debrief performance.md](../apps/debrief/docs/performance.md#start-up-benchmark-on-a-busy-host-2026-11-16-and-17-featbench-busy-host).

- It is always taken, and the host CPU idle (`iostat`) and 1-minute load are recorded with every start.
- **Fails** when the CPU time (user + sys) of the started process tree is over its limit (a tight one when the median host CPU idle was
  high enough, a coarse one on a busy host; between the two on a busy host is a note), or a wall mark is over its ceiling
  (a factor of its baseline that only a broken start reaches), on a train and a tag alike. Thresholds are a recorded baseline plus
  a measured noise margin (ER-12), in the app's budgets file with the results block that measured them.
- **Notes, never a failure:** a wall mark over its noise threshold but under the ceiling, a high spread of the starts, the ratio of
  start-up to a fixed reference workload, host idle and load. A reference workload cannot scale the wall time on the Mac (it slows
  2 to 5 times under loads that leave the app's start unmoved).
- **Not measured** only when the app gave no timings (it did not start, or logged no "Startup timings" line): `::error::` and exit 6
  on every run (ER-33).
- A new timing benchmark imports `verdict`, `annotations`, `readTreeCpuMs`, `readCpuIdle` and `hostSummary` from it and records its
  thresholds the same way. Other load gates in the repo are admission gates (they decide whether to start work, they give no
  verdict) and are not part of this: `scripts/vm-win.sh`, `scripts/vm-win/prepare.sh`, `scripts/vm-smoke/run.mjs` and
  `tools/macos-vm-smoke.sh` (do not start a VM above a load), `tools/heavy` (the heavy-slot gate) and `./dev`'s queue reason
  (`pre_push/exec.rs`).

## Results of shared tooling

### 2026-11-27, the tool's own build through sccache (ci/dev-selfbuild-sccache)

- **Commit:** origin/main 7da64dedb plus this branch (the `dev` launcher's `selfbuild_cached`). Evidence: CI run 9248 (the plan job's cold `./dev` build took 90 s on Linux, 183 s on the Windows paddock-dev leg, 47 s on macOS; the Mac and Windows sccache "0%" is a kept build folder, not a fault: 3 requests executed, 96 to 101 non-cacheable).
- **Machine (local numbers):** Mac mini, 14 cores, macOS 27.2, `CARGO_BUILD_JOBS=6`, rustc 1.100.0, sccache 0.20.1 with its own empty `SCCACHE_DIR` and port, `RUSTC_WRAPPER=` empty for the plain variants (the worktree's shim would otherwise answer from the shared cache), each build in a fresh target folder, through `./dev throttle`. Load 9 to 15 (the first 2 plain builds ran at 18 to 35); wall time is an indication, CPU time (user + sys of `time -p`, the compile server's included for the cached builds) is the measure.
- **Command:** `PADDOCK_DEV_HASH=bench cargo build [--release] --locked --quiet --config build.target-dir=<fresh>` in `tools/paddock-dev`. Variants: **a** as today (release, no wrapper); **b** `RUSTC_WRAPPER=sccache CARGO_INCREMENTAL=0`, the dependencies in the cache and the tool's own inputs changed (a new `PADDOCK_DEV_HASH`: the tool's own crate misses, 1 miss of 313 requests); **c1** release with `opt-level = 1`; **c3** `opt-level = 0`; **c2** the dev profile; **d** b with `opt-level = 1`. Follow-on: `paddock-dev plan origin/main --explain` and `gates check` on the built binary, CPU time.
- **Runs:** 2 builds each (a, c1, c2, c3), interleaved with the others; b and d twice (rep 1 cold cache, rep 2 changed inputs).
- **Numbers (macOS):**

  | Variant | Build wall | Build CPU | `plan` CPU | `gates check` CPU |
  |---|---|---|---|---|
  | a today (release) | 89 s, 67 s | 384 s, 347 s | 1.1 s | 5.5 s |
  | b sccache, deps cached, tool inputs changed | 44 s | 179 s (client 21 + server 158) | 1.1 s | not run (same binary as a) |
  | b sccache, cold cache (empty) | 57 s | not measured (the server's CPU was not timed) | | |
  | c1 opt-level 1 | 56 s, 56 s | 287 s, 291 s | 1.1 s | 6.2 s (+13%; user time +19%) |
  | c3 opt-level 0 | 26 s, 26 s | 102 s, 104 s | 4.8 s (x4) | 17.3 s (x3) |
  | c2 dev profile | 40 s, 36 s | 179 s, 170 s | 5.6 s (x5) | 19.9 s (x3.6) |
  | d opt 1 + sccache, deps cached, inputs changed | 35 s | 142 s | not run | not run |

- **Reading:** the cheaper profiles lose: opt-level 0 and the dev profile cut the build by 70% but make every later `./dev` command 3 to 5 times dearer, and the binary serves every job of the run (about 100 commands), so they cost more than they save; opt-level 1 saves 16 to 37% of the build wall (25% of its CPU) and costs 6 to 13% more CPU on every command. The compile cache keeps the follow-on cost at zero and halves the build CPU, since the dependencies come from the cache and the tool's own crate is the only miss (about 35 s of the 44 s). The key is stable only for one build folder path (the same crate built in another folder missed 83 of 313 requests: OUT_DIR is in the keys of build-script crates), which is why the launcher builds in the one folder `<tool folder>/target/rustc-<compiler>` and sets `SCCACHE_BASEDIRS`. Chosen: b (ER-44, ER-20).
- **Linux runner (CI trial of ci/dev-selfbuild-bench, a measurement job beside the plan job, never landed):** a trial of `ci/dev-selfbuild-bench` (run 9274, the tools job; a measurement step on the real runner d-gtr9: 30 cores, `CARGO_BUILD_JOBS=5`, other jobs running beside it, the S3 two-level store from setup-rust for the sccache variants; the same variants as above; wall time and the client's CPU time, so the cached builds' CPU is the client's only). Rep 1 is the one that shows the cache (rep 2 reuses rep 1's entries: all hits):

  | Variant (rep 1; rep 2 in brackets for the plain ones) | Build wall | Build CPU (user + sys) | `plan` CPU |
  |---|---|---|---|
  | a today | 148.7 s (132.7 s) | 557 s (530 s) | 0.94 s to 1.0 s |
  | b sccache, first build (cache holds some dependencies from other jobs) | 122.2 s | 15.6 s (client) | |
  | b sccache, dependencies cached, tool inputs changed | 36.2 s | 12.8 s (client) | 0.81 s to 0.86 s |
  | c1 opt-level 1 | 78.6 s (109.7 s) | 344 s (470 s) | 1.0 s to 1.45 s (+6% to +50%) |
  | c3 opt-level 0 | 35.7 s (48.7 s) | 117 s (170 s) | 5.5 s to 9.0 s (x6 to x9) |
  | d opt-level 1 and sccache, inputs changed | 36.6 s | 15.1 s (client) | not run |

  On the runner the cached build with changed inputs is 3.7 to 4.1 times faster than today (36 s against 133 to 149 s), a cheaper profile reaches 79 to 110 s (opt-level 1) or 36 to 49 s (opt-level 0, with a `plan` that costs 6 to 9 times more CPU), and opt-level 1 adds nothing once the cache is there (d 36.6 s against b 36.2 s).
- **Plan job's "built ... in N s" line (trial of this branch):** run 9278 (cache folder empty): `dev: compile cache of the self-build (/sccache/paddock-dev-build-cache): 0 hits, 319 misses.` and `built paddock-dev ... in 129 s` (the bench's plain build on the same runner under load: 133 to 149 s; run 9248 on a quieter day: 90 s); run 9282 (the next tool hash, the folder filled by run 9278): `318 hits, 1 misses` and `built paddock-dev ... in 55 s`. The tool's cache key (the hash) is the same with the cache on or off (`the_compile_cache_does_not_change_the_hash`). The macOS leg of run 9278 printed the new label: `sccache-stats: warm build folder: 1 executed, all changed first-party crates; the rest fresh in the kept folder (97 non-cacheable calls are not compiles). A hit rate of 0%25 here is not a fault.` (the `%25` is GitHub's annotation escape of the percent sign in the raw log; the UI shows `%`).
- **Not measured:** the Windows and macOS legs through the cache (they keep their build folders warm and are not changed); the cache's growth over many runs (limit 5 GB, sccache evicts); a hit rate with two jobs of different hashes at once (one build at a time under the lock).

### 2026-11-27, trials yield to train runs; planned legs count as Mac and Windows (fix/trials-yield-to-trains)

- **Commit:** origin/main 7da64dedb plus this branch's change. Not a new run: the saving is read from the recorded timeline of train run 9248 against trial run 9247 (CI replay of the two runs), so it is an expected figure.
- **Machine:** the CI runners: d-gtr9 (6 Linux slots, `conformance/ci/runner-slots.json`) and the Windows runner (3 slots).
- **Method:** trial 9247 (a member trial of `ci.yml`) was dispatched while train run 9248 of the same workflow was queued. The run lists and job timestamps of both runs were read. `classes_of` found no Mac or Windows class in `ci.yml` (a planned matrix is not a literal value), so neither `ci dispatch` nor `runner-wait` guarded those slots.
- **Numbers:**

  | Measure | Before | Expected after |
  |---|---|---|
  | 9248 `plan` job queued | 87 s (behind 9247's Linux jobs) | 0 s of trial load: the trial waits for the train run to finish |
  | 9248 `crate (windows-latest)` queued | 315 s (9247 held 2 of 3 Windows slots) | the Windows slots are free of the trial |
  | Critical-path cost of the trial | about 117 s or more | 0 s |
  | `ci.yml` runner classes seen by `classes_of` | none | macOS and Windows |

- **Reading:** a trial now waits (at most its `--max`) while a `train/*` run of the same workflow is queued or running, so the train's `plan` and Windows legs start at once. The cost moves to the trial, which is not on a train's critical path. A trial that waits for the whole `--max` ends with an `::error::` naming the train run. Tests: `ci_runs::dispatch::tests::a_trial_waits_while_a_train_run_of_its_workflow_is_queued` and the two beside it. Not measured: the real saving. The next train run with a trial dispatched beside it gives it; record it here.

### 2026-11-27, train priority in throttle (feat/throttle-train-priority)

- **Commit:** origin/main 696f472f4 plus this branch's change (a debug build of tools/throttle, the binary the tests ran).
- **Machine:** Mac mini, 14 cores, macOS 27.2; 1-minute load 6 at the start of the runs; each run in its own empty lock folder (`THROTTLE_DIR`), `THROTTLE_SLOTS=1`, `THROTTLE_POLL=1`, `run --no-gate` of `sleep` commands, so the real queue was not touched. Times are whole seconds from `date +%s`, one run per case (the order, not the second, is the result).
- **Method:** two shells' worth of runs started by hand, as an agent and an integrator would. Case A: H (normal, `sleep 12`) holds the slot; N (normal, `sleep 2`) starts at t+2 and queues; T (`--high`, `sleep 2`) starts at t+4. Case B: H2 (`sleep 10`) holds the slot; N2 (normal, `sleep 1`) queues at t+2; T2 (`--high`) queues at t+3 and is killed with `kill -9` at t+5.
- **Numbers:**

  | Case | Result |
  |---|---|
  | A: order of the two waiters after H ends (t+12) | T (the later, train) finished at t+15, N (the earlier, normal) at t+17: train went first |
  | A: what N said while it waited | `normal waiting 2s: waiting behind train work: train (pid <pid>)` |
  | B: N2 after the killed T2 | H2 ended at t+10, N2 finished at t+11 (ran at once, no wait for the dead marker); high markers left in the folder: 0 |

- **Reading:** a train step goes before an earlier normal waiter, normal work goes on as soon as no train marker is left, a SIGKILLed train waiter blocks nobody (its marker is removed by the next poll), and the wait line says who it waits behind. Fake-clock tests for the same four facts: `tools/throttle/tests/it/priority.rs`. Not measured: a train step among several slots, or on Windows. Method of the run: tools/throttle/README.md, "Train priority".

### 2026-11-26, slot locks on kernel file locks against directory locks (feat/throttle-rename-filelock)

- **Commit:** origin/main 202d0a181 (directory locks, the release binary of that tree) against the file-lock tree of this branch (a debug build: the handoff time is poll-bound, not code-bound)
- **Machine:** Mac mini, Apple M4 Pro, 14 cores, 64 GB, macOS 27.2; 5-minute load 11 to 17 during the runs (other agents' builds); each run in its own empty lock folder, no shared state
- **Method:** `heavy run --no-gate` of `sh` commands, `HEAVY_SLOTS=1`, `PLOCK_POLL=1`, 5 interleaved pairs of runs per binary, times read with `perl -MTime::HiRes` in the commands; the script is not kept (twelve lines of shell)
  1. *Two agents contend for one slot:* A holds the slot for 2 s, B starts 0.5 s later and queues; handoff = B's command start minus A's command end.
  2. *A holder killed with SIGKILL:* K runs `sleep 8`; 2 s in, `kill -9` on K's heavy (its command goes on for 6 s more); then W starts `heavy run`; time = W's command start minus the kill.
- **Numbers:**

  | Measure | Directory locks (main) | File locks (this branch) |
  |---|---|---|
  | handoff to a queued waiter, 5 runs | 0.59 to 0.61 s | 0.57 to 0.61 s |
  | next run after the holder is SIGKILLed, 5 runs | 6.26 to 6.37 s (it waited for the orphaned command to end) | 0.02 to 0.04 s |

- **Notes:** the handoff is the poll interval (1 s here; the waiter looks again once a poll), so the lock kind does not change it. The SIGKILL case changes from "as long as the orphan lives" to "at once": the kernel frees the lock when heavy's handle closes. The price, stated in tools/heavy/README.md: the orphaned command is no longer counted against the slot, so the next step starts beside it. Not measured: Linux and Windows, a loaded host with two real gates (the live gates of other agents were not touched), a network file system (refused by design).

### 2026-11-26, shared npm and ui/dist cache (feat/npm-ui-cache)

- **Commit:** `feat/npm-ui-cache` over `origin/main` `8333e185f`; the change under test is `./dev deps` (`tools/paddock-dev/src/deps_cache.rs`)
- **Machine:** Mac mini, Apple M4 Pro, 14 cores, macOS 27.2.1, APFS; worktrees and the cache on the same volume (`<dev-targets>`); other agents shared the machine
- **Load:** the 1-minute load was 20 to 50 during every run below (5-minute load 25 to 35), so the absolute times are inflated; the A/B pairs ran one after the other
- **Workload:** the root workspace of this commit: 541 `node_modules` folders (43,087 files, 582 MiB; 6 workspace `node_modules` folders), `npm ci --no-audit --no-fund` (npm 11.19.1, Node 24.22.0, registry reachable, npm's own http cache warm), then `npm run build -w ui` (`ui/dist` 9 MiB, css 124 KiB)
- **Before:** the two commands above, after removing `node_modules`, `ui/dist` and `ui/projects/paddock-ui/css`. **After, cold:** `./dev deps ui` with no cache entry (the same two commands, then a store of each result). **After, warm:** `./dev deps ui` with both entries present
- **Runs:** before and warm: pairs, interleaved (the second pair below); cold: once (it needs an empty cache). Not repeated more than that: the effect is an order of magnitude
- **Numbers (wall seconds):**

  | Step | Before (`npm ci` + ui build) | After, cold (`./dev deps ui`) | After, warm (`./dev deps ui`) |
  |---|---|---|---|
  | Pair 1, load 50 | 26.0 | | 1.48 |
  | Pair 2, load 44 | 13.4 | | 1.45 |
  | Separate run, load 20 to 25 | 6.3 + 10.6 = 16.9 | | 1.9 to 2.2 (three runs, `node_modules` 0.8 to 1.0 s, ui 1.0 to 1.3 s) |
  | Separate run, load 48 | | 52.0 (npm ci 22.1, build 26.1; the two stores cost 1.9 and 0.3) | |

  The cold run costs about 2 s more than the plain commands (the store, a clone of each result); every later worktree costs 1.5 to 2 s
  instead of 13 to 26 s, whatever the load, because a hit runs no install and no build.
- **Cheaper operation (ER-20):** the clone is one `clonefile(2)` of the whole folder on macOS. `cp -cRP` of the same 43,087 files took
  5.0 s (the first version of this code, 5.3 s for `node_modules`) and `clonefile(2)` 0.46 s; removing a clone is 1.7 to 2.0 s. A
  clone shares the file data until a file is written, so a cached entry plus N worktrees hold about one copy on disk. Parallelism does
  not apply: a hit is one call, a miss is one `npm ci` that must not run twice.
- **Checks made on a cloned tree:** `npm run build -w ui` (Angular, 18 s), `npm run test:tokens` (Vitest, 532 tests) and the e2e build guard
  (`staleBuilds(ui, 'ui-dist')` empty) all pass; workspace links in a clone are relative (`node_modules/@decryp7/paddock-conformance ->
  ../../conformance`), so they point into the clone's own worktree.
- **Not measured:** other disks (a clone across volumes is a plain copy); Linux reflinks; the apps' own builds.

### 2026-11-26, what the two whole-tree gate rows read (ADR 0116, declared input sets for `conformance-tests` and `member-audits`)

- **What:** a one-off trace of every `open`, `stat`, `lstat`, `opendir` and `fopen` under the checkout during one green run of each row, by a
  throw-away `DYLD_INSERT_LIBRARIES` interposer (macOS, non-SIP binaries only: node and the cargo-built test binaries; `git` and `/bin/sh` children
  are not seen). Distinct paths, `node_modules`, `.git` and build folders left out. Not kept in the repository (a measurement tool, not a check).
- **Host and tree:** Mac mini, load 28 to 50 during the runs (a trace, not a timing); tree `247afe10e`.
- **Results:**

| Row | Run | Distinct paths read | Where |
|---|---|---|---|
| `conformance-tests` (`node test/run-tests.mjs`, 1203 tests) | 78 s | 13,675 | apps/* 5,900, conformance 2,600, ui 1,100, tools/paddock-dev 1,000, `.gitea` 190, crates/* about 1,500, docs, scripts, icons, templates, eslint |
| `member-audits` (83 tests) | 117 s | 36,087 | apps/* 15,800, tools/paddock-dev 5,700, icons 2,400, ui 3,900, crates/* about 2,500, conformance 670, docs, `.gitea` |

- **Reading:** both rows read files in every top-level folder of the tree, the apps and the shared crates included. Any declared input set that
  is sound is the whole tree minus build output, so a folder-level declared key reuses no more than the whole-tree key does today. A tighter key
  needs a per-run read set (hash the files a green run read, check them on the next run; Bazel's and ccache's dynamic dependency
  tracking), which needs a tracer on every OS (dyld interposition here, `LD_PRELOAD` on Linux, nothing equivalent on Windows without a driver) and
  gives a key that no author can review. The rows also read the base ref (the growth tests) and the clock (waivers), which the whole-tree key
  already carries. Decision recorded in ADR 0116 (amendment 2026-11-26): no `Class::Declared`; `member-audits` can join `reuse::class_of`'s whole-tree class
  (one line, the gate owner's file) if the exact-tree repeat is worth it.
- **Not measured:** the child processes (`git`, shells, `./dev` started by tests), Linux and Windows, a trace of a failing run.

### 2026-11-26, test-result cache: an unchanged Cargo package reuses its pass (feat/adr-0116-test-result-cache)

- **What:** `./dev ci test-cache run --workspace crates --only paddock-limits,paddock-names,paddock-metrics` (nextest `ci` profile,
  `--all-features`, debug build) with the local store (`<git common dir>/paddock-pre-push/test-cache`). Three small packages, 171 tests.
- **Host:** Mac mini, macOS arm64, 5-minute load about 20 before the runs (1-minute 8 to 12), nothing else of mine running. Single runs,
  not interleaved A/B pairs: an indication, not a range (rule 4 above). Debug build, because the quantity is "run or skip".
- **Runs (worktree A, then worktree B at another commit, each with its own build folder):**

| Run | Targets | Reused | Ran | nextest time |
|---|---|---|---|---|
| A, empty store (cold build folder) | 3 | 0 | 3 | 96.4 s |
| B, same inputs, other worktree and commit, shadow mode | 3 | 0 (3 would be) | 3 | 68.8 s (cold build folder) |
| B again with `--reuse` | 3 | 3 | 0 | 0.0 s |
| B after a one-line edit to `paddock-names`, committed, `--reuse` | 3 | 2 | 1 (`paddock-names`) | 1.9 s (warm build) |

- **Reading:** the key is the same in both worktrees and commits (the graph's node hash is content, not commit), so a second
  worktree, and a train after a branch gate, skip an unchanged package. A one-line change reran only the changed package here (the
  other two do not depend on it). The cold runs are dominated by compiling the `crates` workspace once per build folder; the cache
  removes the tests and their per-package build, not a cold workspace build.
- **Not measured:** a full 58-package run, CI runner classes, the shared S3 store (bucket and keys not available when this ran), the
  wall time of a train round, interleaved A/B.

### 2026-11-24, pre-push row reuse on an unchanged tree (feat/check-result-reuse-v2)

- **Commit:** `285c68695` (one commit over train 2026-11-25-b `cfe8a8592`); the change under test is paddock-dev only
- **Machine:** Mac mini, Apple M4 Pro, 14 cores, macOS 27.2.1; other agents' builds and pre-push runs shared the machine
- **Load:** 1-minute load about 30 to 130 during runs A and B (heavy's load gate held the compiles); queued time is reported apart
- **Build:** the dev binary `./dev` built for that commit (release); `CARGO_BUILD_JOBS=2`
- **Command:** in the branch's worktree, clean tree, one after the other: A `./dev check --pre-push --no-reuse` (before), B
  `./dev check --pre-push` (runs every row and stores the green ones), C `./dev check --pre-push` again (after)
- **Runs:** single, not repeated
- **Numbers:**

  | Row | A, reuse off (run / queued) | B, stores (run / queued) | C, unchanged tree |
  |---|---|---|---|
  | lockfile, fmt tools/paddock-dev | 0.2 s, 2.5 s | 1.1 s, 8.7 s | reused |
  | clippy tools/paddock-dev | 265.4 s / 194.1 s | 0.8 s / 650.2 s (warm build folder) | reused |
  | clippy-linux tools/paddock-dev | 24.3 s / 947.5 s | 32.7 s / 5.2 s | reused |
  | conformance, gates-register | 25.5 s, 4.8 s | 62.0 s, 12.7 s | reused |
  | commit-lint, locks-guard | 0.1 s, 4.0 s | 0.2 s, 12.2 s | ran again: 0.2 s, 4.9 s (never reused: they read commits and the base) |
  | **Whole command** | **1,439 s** | **794 s** | **9 s** |

- **Notes:** most of A and B is waiting for the heavy slot under the load gate, so the saving on a quiet machine is smaller: the
  row run times of B add up to about 130 s, all but about 5 s of which C did not repeat. C's 9 s include the planner, the two rows
  that always run, the version probes (`cargo --version`, `rustc -vV`, `docker --version`, `node --version`) and hashing the dev
  binary. The records are `<git common dir>/paddock-pre-push/reuse/<key>.json`, beside `step-history.jsonl`. Not measured: a push
  through the hook (a throw-away worktree of the same commit; the key leaves the checkout path and the build folder out, which
  `the_checkout_path_and_build_folder_are_not_in_the_key` checks), a member gate, and other machines. Raw output was kept only in the
  agent's scratch folder.

### 2026-11-25, member gate reuse on a train (feat/train-gate-reruns-overlaps)

- **What:** how many of a train's member-gate rows can rest on the members' own green rows, now that the `tests` and `tests-linux`
  rows are keyed too (they were never reused before: the shared store held 447 records, none of them a tests row).
- **Part 1, from the history (no run):** for every member merge of trains 2026-11-26-f, -g, -h and -i, whether the member's head and
  the merge agree on each touched Cargo workspace's inputs (the workspace, `crates` for an app, `.cargo`, `lint`,
  `rust-toolchain.toml`, `ui/projects/paddock-ui`, `.gitea`) and on `tools/paddock-dev` (the `dev` binary in every key). Script:
  `git diff --quiet <merge>^2 <merge> -- <inputs>` per workspace.
- **Numbers (part 1):** 26 member merges that touch a Cargo workspace (75 workspace checks); 0 workspaces whose inputs match, and the `dev` differs in all 26.
  Every member was branched from an older main than its train, and main changed `crates`, the apps and `tools/paddock-dev` in
  between (13 of the 18 merges of trains f and g changed `tools/paddock-dev`), so the member's green rows are of another tree.
- **Notes:** with these inputs the reuse would have rerun every row on these trains, whatever the key held for `dev`. Two changes
  follow (user, 2026-11-25): a member runs its final member gate after merging origin/main (AGENTS.md, "Before hand-back"), and the
  `dev` hash keys only the rows `dev` computes itself, so a member that changes `tools/paddock-dev` no longer reruns the Cargo,
  Clippy and tests rows of the others.

### 2026-11-16, unit suites with the test leak detector (feat/test-leak-detector)

- **Commit:** the tip of `feat/test-leak-detector`; base `3585bcab1` (train 13) plus `fix/debrief-startup-loads-drain`
- **Machine:** Mac mini, Apple M4 Pro, 14 cores, 64 GB, macOS 27.2
- **Load:** base runs at about 40 to 46% CPU idle; the runs after at about 56% idle (other agents' jobs were on the machine; a run at 0% idle was discarded)
- **Build:** the Angular unit-test builder with Vitest 5.0.5 (jsdom), `ui/dist` built once
- **Corpus:** the apps' and the ui's own specs (no data files)
- **Command:** `npx ng test --watch=false` in each `apps/<app>/app`; `npx ng test <project> --watch=false` in `ui`
- **Runs:** one run before and one or two after (not interleaved), Vitest's `Duration`; the fail-mode runs of Pitcrew, Pitwall and Spotter were repeated three times (no red)
- **Numbers:**

  | Suite (tests) | Before | After | Mode now | Verdict |
  |---|---|---|---|---|
  | Debrief (4989, 4992 after) | 33.19 s | 32.1 s to 38.1 s (five runs) | fail | no measurable change (the review timed it alternating with the base) |
  | Dyno (1652) | 17.80 s | 16.16 s and 16.41 s | fail (follow-up) | within 10% |
  | Pitcrew (642, 646 after) | 9.90 s | 9.34 s | fail | within 10% |
  | Pitwall (959, 962 after) | 9.01 s | 6.61 s | fail | within 10% (noise: the earlier run was loaded) |
  | Spotter (496, 500 after) | 6.20 s | 3.97 s | fail | within 10% (noise) |
  | ui paddock-ui (3036, 3065 after) | 11.70 s | 8.01 s | fail | within 10% (noise) |
  | ui workbench-demo (2) | 0.71 s | 0.61 s | fail | within 10% |

- **Notes:** the detector wraps `setTimeout`/`setInterval` once per worker and reads `new Error().stack` (limit 12 frames)
  per call that is not Vitest's own; no cost shows beyond noise. Detector findings for Debrief and Dyno (report mode, lines
  `[leak-detector] <test>: <kind>: <what>` on stderr): the `Duration` figures above carry them. Raw logs are in the
  agent's scratch folder only.

#### Findings left, by file and line (none: Debrief and Dyno are in failing mode)

Debrief: zero findings after `fix/debrief-test-leaks` (about 310 before), in failing mode, green in three runs (32.1 s,
33.2 s, 38.1 s against 33.19 s before the detector; the slowest run overlapped other agents' jobs). Fixed there: the
mock clock (`systemMockClock` tracks its timers and sleeps; the setup stops them after every test and halts it between tests),
the stores and services that kept a timer past their injector (`query.store`, `profile-draft`, `query-session`,
`engine-settings.store`, `recovery`, `session.service`, the copy-label resets in `shared/path-text`, `context-section` and
`dock/angular-panel`, the dialog shell, the extractor editor, the tour's wait), `OpenRequests.start` (two callers overwrote the
timer handle of the first), the retry timer of the start-up read, the pop-out backend's poll, the dock service's cue
elements (detached with its injector), mock job state (drained after every test), the focus retry, and the body nodes some dock specs
left. The mock backends' own latency timers go through `systemMockClock`.
Dyno: zero findings after `MockBackend.dispose()` (disposed after every test by the setup), the counters store (it
armed a second poll timer and one after destroy) and `scroll-to-section.spec.ts` (left a `<main>` in the body); Dyno runs in
failing mode.

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
- **Command:** `HEAVY_LOCK_DIR=/Volumes/d8sst7/dev-targets/heavy-slots engineering-playbook/scripts/heavy.sh ./dev worktree add <name> --base origin/main -b <branch> --owner implementer:meas --root /Volumes/d8sst7/dev-targets/wt --agent <id> [--sparse crates,ui,scripts,docs,tools,apps/spotter]`, then `du -sk <folder>`
- **Runs:** 3 runs of each, interleaved; median and range of the wall time as `date` differences; disk is the same every run
- **Numbers:**

  | Measure | Sparse cone | Full checkout | Verdict |
  |---|---|---|---|
  | wall time, median (range) | 1.2 s (1.2 to 1.3) | 2.7 s (2.6 to 7.1) | full costs about 1.4 s more at median |
  | disk | 203 MiB | 544 MiB | full costs about 340 MiB more |

- **Notes:** the full checkout's slowest run (7.1 s) came while the load was high. An earlier run on a quieter machine gave 3.1 s and 1.9 s (two runs each, disk the same). Decision: a branch worktree is a full checkout (`--profile gates`), because conformance, `scripts/affected.mjs`, `./dev gates check` and `./dev plan` read most folders and a cone made them fail with ENOENT; agents added folders by hand at least four times on 2026-11-11. `--sparse` stays for `--detach` review copies. Not measured: build output, which is the same for both.

### 2026-11-15, pre-push heavy queue times (fix/pre-push-base-and-budget)

- **Commit:** 49e1a97cc (train/2026-11-14-11 when measured); the steps were run by the pre-push gates of that day's agents
- **Machine:** Mac mini, Apple M4 Pro, 14 cores, 64 GB, macOS 27.2; 1-minute load about 13 to 39 over the window
- **Source:** heavy's activity log (`pitcrew-lock-activity.log` in heavy's lock folder), 2026-11-14 13:02 to 19:09Z, every
  `pre-push done` line: queue time = `waited` (lock) + `held` - `ran` (the load gate, which runs while the lock is held)
- **Command:** `grep " done rc=" pitcrew-lock-activity.log | perl -ne 'print "$1 ",$4+$3-$2,"\n" if /Z (\S+) done rc=\S+, ran (\d+)s \(held (\d+)s, waited (\d+)s\)/'`, sorted
- **Numbers:** 145 steps, all `pre-push`: p50 21 s, p90 386 s, p95 703 s, p99 993 s, max 1,187 s
- **Notes:** (superseded 2026-11-21 by the stall limit below) the basis of `QUEUE_MAX` (30 min) as the whole push's queue allowance (tools/paddock-dev/src/pre_push/exec.rs, ADR
  0102 amendment 2026-11-15): one step's worst wait (about 20 min) fits; a push whose heavy steps wait 30 min in all is a busy
  machine's finding, reported "not measured" with the time queued and the load. Not measured: the queue time per push (the log
  does not say which push a step belonged to).

### 2026-11-21, pre-push heavy queue progress gaps (fix/push-gate-queue-stall)

- **Commit:** 3d776b605 (main when measured)
- **Machine:** Mac mini, Apple M4 Pro, 14 cores, 64 GB, macOS 27.2; the shared machine with other agents running
- **Source:** heavy's activity log (`pitcrew-lock-activity.log` in heavy's lock folder), 2026-11-14 13:02Z to 2026-11-21 10:12Z, 22,646 lines
- **Method:** the queue's progress events are the `took the lock` and `done rc=` lines; a gap is the time between two consecutive events with at least one `waiting` line inside it (somebody was waiting, nothing moved). A short Python script over the log (not kept; the log is in the machine's temp folder).
- **Numbers:** 1,386 gaps: p50 81 s, p90 314 s, p95 453 s, p99 813 s, max 1,446 s (the five longest: 1,112, 1,143, 1,182, 1,443 and 1,446 s)
- **Queue length:** the most steps waiting at once in the same log: 13 (`position N of 13`). Rows of the three refused pushes of 2026-11-21 (watch-rescan push4, push5, push6): 45 rows decided per push (32 passed + 13 not measured, 31 + 14, 30 + 15) and 20 skipped; the longest of those gate runs was 76 minutes (the log files' times, 15:32 to 16:48), and the push of this change itself, with the new gate, ran 95 minutes (5,705 s, 27 rows passed, none not measured: clippy crates queued 1,225 s and ran 312 s, clippy-windows crates queued 758 s and ran 339 s, doctest crates queued 593 s and ran 334 s).
- **Derived limits (ER-12):** `WAIT_BACKSTOP` = 2 x 13 x 1,122 s (the slowest healthy row, "Doctest row of the push gate") = 29,172 s (8 h 6 min); the member gate's outer `TIMEOUT` is 24 h, 15 times the longest push gate run seen (95 min). Both are backstops for a hung client or gate: the judgement is the row cap and the queue's progress.
- **Holders count while they live:** a holder whose record is inside the cap its own row lives under (the waiter's `--run-max`, `ROW_CAP` + grace, 3,675 s) is progress by itself, so a waiter behind a healthy 31 to 60 minute step is not stalled; past the cap it is not live, and the same picture ends the wait a stall limit later.
- **Notes:** the basis of `QUEUE_STALL` (30 min, 1,800 s) in `tools/paddock-dev/src/throttle.rs`: over the longest gap seen, so a queue that moves is never ended. A holder that runs longer is ended by its own row cap. The per-push totals it replaces (15 min of run, 30 min of queue) were exceeded by three pushes on 2026-11-21 with no row running too long: 13, 14 and 15 rows, and 5 rows, were not measured. Not measured: the gaps of a quiet machine.

### 2026-11-24, scope grace: from the stop request to the exit (feat/adr-0119-wp1-scope)

- **Commit:** 9f6b241dc plus the uncommitted ADR 0119 WP-1 change (the measured code is `paddock_proc::ask` and the guardian handshake as committed)
- **Machine:** Mac mini, Apple M4 Pro, 14 cores, 64 GB, macOS 27.2
- **Load:** 1-minute load 27 to 35 during the runs (other agents' builds and tests)
- **Build:** debug (the `it` test binary of paddock-proc); node v24.22.0
- **Corpus:** stand-in children, each started, waited for until it prints `ready`, then asked to stop (`paddock_proc::ask`: SIGTERM by identity); the exit is seen through the held `Child` (`try_wait` every 0.2 ms)
- **Command:** `cd crates && cargo nextest run -p paddock-proc --test it --run-ignored only -E 'test(measure_grace)' --no-capture -j 1` (`PADDOCK_SCOPE_GRACE_RUNS`, default 200; the trap kind and the first start run a quarter of it)
- **Runs:** single run per kind; median, 99th percentile and worst
- **Numbers:**

  | Measure | n | Median | p99 | Worst | Verdict |
  |---|---|---|---|---|---|
  | Rust program, default SIGTERM action | 200 | 1.0 ms | 20.9 ms | 45.1 ms | well inside the grace |
  | `sh` running `sleep`, default action | 200 | 0.8 ms | 4.4 ms | 6.7 ms | well inside |
  | node program, default action | 200 | 1.7 ms | 8.4 ms | 10.4 ms | well inside |
  | `sh` script that traps SIGTERM to clean up, one-second steps | 50 | 1.01 s | 1.02 s | 1.02 s | the worst kind: the trap runs when the current foreground command ends |
  | first start of a process with the orphan guardian (start, guardian check, `serving` answer, child start) | 50 | 6.0 ms | 28.0 ms | 28.0 ms | far inside the 10 s `SERVING_BOUND` |

- **Second run** after the WP-1 review (commit 460e7d60d plus the review round; the guardian now starts in `Scope::new`, so the
  first-start row measures `Scope::new` plus the first start), 1-minute load 9.5 to 16.5:

  | Measure | n | Median | p99 | Worst |
  |---|---|---|---|---|
  | Rust program, default SIGTERM action | 200 | 0.46 ms | 0.91 ms | 3.1 ms |
  | `sh` running `sleep`, default action | 200 | 0.45 ms | 1.7 ms | 2.7 ms |
  | node program, default action | 200 | 1.1 ms | 6.8 ms | 8.8 ms |
  | `sh` script that traps SIGTERM, one-second steps | 50 | 1.01 s | 1.01 s | 1.01 s |
  | `Scope::new` plus the first start, guardian included | 50 | 4.6 ms | 5.3 ms | 5.3 ms |

- **Notes:** the scope's `GRACE` is 2 s (`paddock_proc::FINISH_GRACE` itself since the WP-1 review): twice the worst kind measured (a script whose step is one second), equal to `paddock_proc::FINISH_GRACE`, and paid once per scope end, since every child is asked at once. A script with longer foreground steps is ended after the grace (reported as `ended`, not `asked`). The guardian's `SERVING_BOUND` (10 s) only meets a guardian stopped before its owner check (App Tamer, a debugger); a refused guardian answers at once with end of file. Windows is not measured: `ask` sends WM_CLOSE to a program's windows and a console program refuses it, so the end there is immediate for console children. Raw output: the test's `grace:` lines, not kept.

### 2026-11-24, the conformance-line test of the CI model: main, the graph train and the cut (fix/graph-train-conformance-line-test-cost)

- **Commits:** main d04af7b46, train/2026-11-23-graph fe331ca55, and the fix on top of it; debug test builds, `CARGO_BUILD_JOBS=2`
- **Machine:** Mac mini, Apple M4 Pro, 14 cores, 64 GB, macOS 27.2, shared with two train CI runs: 1-minute load 42 to 121 (main
  against train) and 24 to 32 (train against fix), so CPU time (user + sys, children included) is the measure and wall time an indication
- **Command:** the lib test binary run directly, `--exact ci_run::tests::adding_a_check_changes_no_golden_row_but_the_rest_of_the_conformance_line_does --test-threads 1`, under `/usr/bin/time -p`, 5 interleaved rounds per pair; a `sample` of one train run for where the time goes
- **Where the time goes (train):** 91% of the test thread in `gates::ci::build` (the walk of every step of every job, per matrix leg);
  the test built the whole model four times and reads only ci.yml's rows
- **Main against train:** CPU 10.1-11.8 s (median 10.5) against 11.4-12.4 s (median 11.7), +11%. The train's model code adds two leg keys
  and a `hostTests` read per app; the rest is real new work, the steps the train added to the workflows (text +6.3%, 494 to 525 KB of
  workflows and actions; ci.yml +160 lines, debrief.yml +86), multiplied by the matrix legs. No repeated parse, `cargo metadata` or git start
- **Train against fix:** CPU 8.3-9.4 s (median 8.7) against 4.7-5.1 s (median 4.9), -44%: `rows_of` builds the model from ci.yml alone
  (every action and contract kept) and the test keeps one full build as the control that the narrowing loses no row. So the other
  workflows were about 70% of each build. Alone on the CI Mac runner the test took 12.6 s (run 8838); beside two heavy jobs it was stopped
  at 60 s on both tries (jobs 47411, 47644)

### 2026-11-24, test time tiers (feat/test-hang-by-progress, ADR 0121 amendment)

- **Inputs:** the final run of "test cost classes" below (8 test threads, median 1-minute load 38.9 on the shared Mac): wall times are
  upper bounds of each test's own work. Contention: 5 to 15 times the local time for whole-tree tests in jobs 47411 (run 8830 macOS) and
  47644 (the coordinator's reading of both job logs); 15 is used. The train replay's rule tables: 157.6 to 185.9 s of one thread
  (2026-11-23, below)
- **Tiers (longest test x 15, rounded up to Bazel's tiers):** unclassed 16.3 s -> 245 s, moderate 300 s (ci profile); `tree` 41.4 s ->
  621 s, long 900 s; `graph` without the replay 26.9 s -> 404 s, long 900 s; the train replay 186 s -> 2790 s, eternal 3600 s
- **Check:** `./dev ci test-cost --wall-asserts --log <that run>` with the tiers: 0 tests whose time x 15 passes their stop; 19 asserts on
  elapsed wall time in 15 tests (ci_run, input_hash, load, local_sccache, pre_push::exec 8, heavy_tool, stall 2 asserts, worktree::ops),
  report-only, each a fix item: an injected clock or a CPU bound
- **Not measured:** CPU time per test (it comes with `./dev test-guard`); Linux and Windows

### 2026-11-24, test cost classes (feat/test-cost-classes, ADR 0121)

- **Commit:** train/2026-11-23-graph at fe331ca55 with this change; before = the train's `tools/paddock-dev/.config/nextest.toml`
  (per-name waivers and the `repo-graph` filter, `--config-file`), after = the class table. The same build and binaries for both
  (debug test profile, `CARGO_BUILD_JOBS=2`)
- **Machine:** Mac mini, Apple M4 Pro, 14 cores, 64 GB, macOS 27.2, shared with the graph train's CI jobs and other agents. Load sampled
  every 30 s during each run (1-minute load average): before min 30.5, median 41.9, max 57.3 (19 samples); after min 32.0, median 38.9,
  max 47.2 (11 samples). A first after run with an earlier inventory ran at median 59.5 (max 105.6) and is used only for the inventory
- **Command:** `NEXTEST_TEST_THREADS=8 cargo nextest run --profile ci --locked --no-fail-fast` (8 = the macOS runner's test threads,
  build-jobs) in tools/paddock-dev, with a `cargo` first on PATH that logged each `metadata` call with its test and `NEXTEST_TEST_GROUP`,
  and `PADDOCK_TEST_COST_REPORT` set (the charge report)
- **Runs:** 1 before, 1 after (final tree); n = 1 each, so the numbers are an indication of the shape, not a distribution
- **Numbers:**

| | before | after |
|---|---|---|
| suite wall time | 556 s (2588 tests; 2 FLAKY whole-tree tests that hit the 60 s stop on try 1, plus 3 failures of this change's own new tests and audit under the old config) | 306 s, 2588 passed, 0 FLAKY |
| graph class (44 tests): sum, max, median, over 30 s | 754 s, 146.9 s, 8.5 s, 8 | 408 s, 79.2 s, 7.3 s, 1 |
| tree class (29 tests): sum, max, median, over 30 s | 655 s, 59.1 s, 18.3 s, 7 | 269 s, 41.4 s, 7.8 s, 1 |
| unclassed (2501): sum, max | 1158 s, 23.3 s | 902 s, 16.3 s |
| `ci_run::tests::..._conformance_line_does` (runs 8830, 8843) | 59.1 s (1 s from its stop) | 11.6 s (12.6 s alone, per the coordinator) |
| `repo_model::code::tests::on_the_real_tree_...` | 32.5 s | 10.1 s |
| `check_engine::this_working_tree_...` | 54.0 s | 11.1 s |
| `repo_graph_net::train_replay_...` | 146.9 s | 79.2 s |
| `cargo metadata` calls outside `repo-graph` (shim) | 0 of 1028 | 0 of 1028 |
| charges recorded / outside their group | n/a | 1032 / 0 (4 more than the shim: two `pre_push::tests` run cargo in a child whose environment the test replaces) |

- **Inventory (the first after run, median load 59.5):** the class table without the `pre_push::` module in a group: the eight
  `tests/it` `pre_push::` tests that start `./dev check --pre-push` over the real tree took 22 to 52 s (6 to 30 s before, in the old
  group), so they joined the tree class; the charge named the two `src` `pre_push::tests` above. `./dev ci test-cost` on the final run:
  one unclassed test over a quarter of its stop, `golden::ci_plan_declares_a_scope_...` 16.3 s (a fixture plan, not the real tree; a
  fix item, not a class).
- **Not measured:** Linux and Windows (the landing run of the train with this change, read with `./dev ci test-cost --run <id>`, is the
  first; run 8838 Windows had `on_the_real_tree_` at 29.8 s alone); more than one run per side.

### 2026-11-24, which paddock-dev tests run `cargo metadata`, and the repo-graph group (fix/graph-train-r5-repo-graph-group)

- **Commit:** 47a27bf1f (train/2026-11-23-graph) before, the same with the new `tools/paddock-dev/.config/nextest.toml` after
- **Machine:** Mac mini, Apple M4 Pro, 14 cores, 64 GB, macOS 27.2; shared with other agents and CI. 1-minute load 8 to 13 during the
  run before, 29 to 36 during the run after, so the wall times are an indication, not an A/B
- **Command:** `cargo nextest run --profile ci --test-threads 6 --no-fail-fast` (paddock-dev, every binary, debug build,
  `CARGO_BUILD_JOBS=2`) with a `cargo` first on PATH that logged each `metadata` call (manifest, the test that started it, found as the
  ancestor process started with `--exact`, and the test's `NEXTEST_TEST_GROUP`), then ran the real cargo unchanged
- **Runs:** 1 before, 1 after
- **Numbers:** 2576 tests, all passed both times; 1028 `cargo metadata` calls both times, none from an unknown test. Before: 15 tests
  outside the group made calls: 7 over the real tree's 25 workspaces (`repo_model::code::tests::on_the_real_tree_` 9.7 s,
  `repo_model::tests::every_declared_smoke_set_` 1.3 s, `repo_graph_net::every_register_under_docs_is_owned` 9.1 s, its three
  `generators::` tests 8.1 to 11.1 s, `repo_graph_net::train_replay_` 98.4 s) and 8 over fixture trees (2 to 16 calls, 0.2 to 5.4 s).
  After: 0 calls outside the group. Those 15 tests took 417 s in all before and 499 s after (the load was about three times higher);
  `on_the_real_tree_` 9.7 s to 7.5 s. Suite wall time 297.8 s before, 389.9 s after (load, and the group's two slots)
- **Not measured:** Windows, where the test was FLAKY (job 47457: stopped at 60 s, then 23.4 s); the landing run on the new head is the
  check

### 2026-11-24, the repository model's ownership test: where the time goes, and the cut (fix/owner-test-windows-cost)

- **Commit:** 727810fe9 (train/2026-11-23-graph) against the fix, both built as the test profile (debug), `CARGO_BUILD_JOBS=2`
- **Machine:** Mac mini, Apple M4 Pro, 14 cores, 64 GB, macOS 27.2; the train's CI and other agents shared it: 1-minute load 69 to 147
  during the A/B (not a quiet run, so CPU time is the measure of work and wall time an indication)
- **Source:** the checkout's tracked files (14,291; 692 outside every project folder, which the test asks about)
- **Method:** phase timers and counters added for the measurement only (not committed): `Instant` around each phase, a counter of
  working-tree reads, `paddock_env::starts::count_starts()` for process starts. A/B: both test binaries run directly, interleaved, 8 rounds
  each, `/usr/bin/time -p` (user and sys include the child processes)
- **Work of one run:** 29 process starts (25 `cargo metadata`, one per Cargo workspace; 2 `git ls-files`; 1 `git grep`; 1 `git cat-file
  --batch-command`); 3,354 working-tree file reads (2,662 to build the world, 692 for the reach) plus 692 blobs at HEAD (36.8 MB through the
  one cat-file process); parses: 428 Rust sources with syn (2.5 MB), about 2,200 JS/TS files with oxc, 58 YAML files (27 workflows, 31
  actions); 4,628 model nodes
- **Phases before (quietest single run, load about 50):** `cargo metadata` x 25 0.60-0.70 s; CI adapter 0.71 s (of it 0.33 s matching 137
  dynamic script words against all 14,291 paths, twice each); code adapter 2.07 s (syn parse and visit 0.84 s; oxc parse 0.83 s in a debug
  build); reach 0.75 s (HEAD blobs 0.37 s; the owner lookups 0.30 s: every node visited per changed file, three times)
- **Cuts:** oxc parser crates built with `opt-level = 3` in the dev profile as syn already is (oxc parse 0.83 s to 0.11 s); each macro's
  tokens parsed once in the reads finder (`macro_pieces` parsed every macro a second time); syn parses in 4 threads (`PARSE_WORKERS`,
  results in input order); the model's owner lookups through a file index (0.30 s to 0.003 s); glob matching over the tracked set by its
  literal prefix and its longest literal run (0.33 s to 0.16 s)
- **A/B (8 rounds each):** CPU user + sys: before 6.75-7.87 s (mean 7.35 s), after 4.59-5.48 s (mean 5.10 s), -31%. Wall: before
  17.5-26.8 s (median 22.1 s), after 7.5-15.1 s (median 10.9 s). Process starts: 29 before and 29 after (pinned by the test now).
  Stress: 10 of 10 passed in the ci profile at a load of 115 to 125 (5.9-11.4 s)
- **Windows (estimated from the counts, not measured):** 29 starts at about 45 ms (54 with the rustup proxy in front of each `cargo`) are
  about 1.3-2.4 s and unchanged; the rest is the parse work and the 3,354 file reads through Defender, which the cut reduces by the same
  CPU share. Run 8694 try 2 took 18.2 s; at the Mac's ratio the fix would take about 9-12 s. Try 1's 60 s stop came under load beside the
  other tests that run `cargo metadata` over every workspace; the test now joins their `repo-graph` group in the ci profile.
- **Not cut, and why:** the 25 `cargo metadata` runs (one per workspace is the input the graph needs); the 36.8 MB of HEAD blobs (the
  reach's content detection reads both sides of every changed file; answering them through gix in process took 2.0 s in a debug build
  against 0.37 s through git, measured and reverted); pipelining cat-file requests (a pipelined shell run of the same reads took 0.33 s:
  the cost is the bytes, not the round trips)

### 2026-11-23, the repository model's ownership test without the OS placement (fix/owner-test-batched-git)

- **Commit:** 64e01dd84 (train/2026-11-23-graph) against the fix
- **Machine:** Mac mini, Apple M4 Pro, 14 cores, 64 GB, macOS 27.2; 1-minute load 26 over the window (an indication, not a quiet run). Debug build (the test profile), `CARGO_BUILD_JOBS=2`
- **Source:** the checkout's own tracked files (about 14,000, of which about 700 outside every project folder)
- **Command:** `cargo nextest run -E 'test(every_tracked_file_of_the_tree)'` (paddock-dev lib), run alone; git starts counted with a `git` shim on PATH that logs every call
- **Numbers:** git starts: 5 before and 5 after (two `ls-files`, one `grep`, two `cat-file --batch-command` readers), the same for any tree size. Wall time: before 15.4 s and 15.0 s, after 8.6, 8.7 and 8.5 s. A sample of the run before: 99% of the test thread was in `read_graph` deriving the OS placement (`os_placement::derive`, lexing every Rust source of three workspaces); the model's own work (`build_world`, `reach_in`) was the rest
- **Notes:** run 8674 (Windows) timed out at 60 s on try 1 and took 37.9 s on try 2; the cause was not git starts (the reader was already one process per revision) but this placement work, which the test never reads. `read_graph_unplaced` skips it for the repository model's own checks; `read_graph` (plans) still derives it. What remains is the model's parsing of every source file (syn, oxc, the lexers) in a debug build. Not measured: Windows.

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
| Window weight ([`conformance/src/window-weight`](../conformance/src/window-weight)) | JS each window loads before first paint (rule 35) | `ng build --stats-json`, then `paddock-conformance window-weight [kind] [--max kB] [--record]` | The build | `windowWeight` in `paddock.conformance.json`, **not configured anywhere** | none (its check C29 was retired on 2026-11-24, configured nowhere) | — |
| `./dev hook pre-tool-use` (RT-4, [ADR 0083](adr/0083-agents-cannot-write-outside-their-own-worktree.md) §2) | Per-call time of the installed guard binary on a PreToolUse call, the shim around it, and the binary's size growth | 300 runs of `<git-common-dir>/paddock/hooks/paddock-dev hook pre-tool-use` with a JSON call on stdin, as described in the section below (the script is not kept) | A short, a 6 KB and a malformed call | none yet (the plan expects about 4 to 6 ms) | No | [results](#hook-per-call-cost-and-binary-size-rt-4-2026-11-10) |
| `./dev gates check` (C100, [ADR 0060](adr/0060-every-check-runs-somewhere.md)) | Run time of the meta-check: 328 checks, the CI model, 3,192 Rust files scanned, the docs | `./dev gates check` (release build of `tools/paddock-dev`) | This repository | none (ADR 0060: quote it only from here; a few seconds at most) | `ci.yml` conformance job, `nightly.yml` c100 job | [results](#2026-11-08-dev-gates-check) |
| `./dev deps ui` (shared npm and ui/dist cache) | `npm ci` plus the ui build in a fresh worktree, before against cold and warm cache | `./dev deps ui` | This repository's root workspace | none | No | [results](#2026-11-26-shared-npm-and-uidist-cache-featnpm-ui-cache) |
| Angular build budgets in `ui/angular.json` | Initial chunk size | `npm run build -w ui` (and each demo's build) | — | gallery 1.5 MB / 3 MB; shell-demo 500 kB / 1 MB; workbench-demo 580 kB / 1 MB (warn / error) | `ci.yml` ui jobs, through `ng build` | — |
| `paddock_proc::scope::GRACE` ([`crates/paddock-proc/tests/it/scope.rs`](../crates/paddock-proc/tests/it/scope.rs), `measure_grace`, [ADR 0119](adr/0119-every-process-is-held-by-its-handle-and-ends-with-its-scope.md) WP-1) | Time from a stop request (SIGTERM by identity) to the exit, per kind of child; the first start with the orphan guardian | `cd crates && cargo nextest run -p paddock-proc --test it --run-ignored only -E 'test(measure_grace)' --no-capture -j 1` (Unix) | Stand-in children | none (it sets the grace constant, ER-12) | No | [2026-11-24](#2026-11-24-scope-grace-from-the-stop-request-to-the-exit-featadr-0119-wp1-scope) |
| `./dev ci timings` (`tools/paddock-dev/src/ci_runs/timings.rs`, removed on 2026-11-24 with the command; the script `scripts/ci-timings.mjs` that made its recorded golden was deleted on 2026-11-18) | CI wall time, queueing, runners, per-step and compile/test phases, critical path (Gitea API, read-only) | `GITEA_TOKEN=… ./dev ci timings [--repos paddock] [--limit 30] [--cache DIR] [--out docs/research/2026-10-ci-timings.md] [--json-file F]` (default repo: `paddock`; the script defaulted to the old per-app repos) | Gitea run history | none | No (manual) | [research/2026-10-ci-timings.md](research/2026-10-ci-timings.md) |

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
| Debrief start-up (macOS) | fails over tree CPU 600 ms (host idle median 30% or more) or 875 ms (busy), or a mark over 5 times its baseline (firstInteractive 3100 ms); baselines windowCreated 208 … firstInteractive 620 ms (2026-11-17) | Rule 16, Debrief ADR 0058 | `startup-bench.mjs` (nightly, manual and every `debrief-v` tag) |
| First-paint JS | Debrief main 1355 kB, About 480 kB; other apps not set | Rule 35 | `window-weight.mjs` (Debrief) |
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
- **Superseded 2026-11-24:** the 8 test threads were removed (3 jobs × 8 threads on 12 usable cores broke ER-48, and this block measured one project's
  lib tests alone, single runs, never three lanes at once, so it does not support an exception). macOS now takes the core share, 4. The cost the
  block shows for it, 47.8 s against 15.7 s for the dyno lib tests, is accepted by the user's decision of 2026-11-24 ("cut the Mac now").
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
   - Window weight outside Debrief.
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
   - CI build parallelism on Linux (updated 2026-11-20): the 2026-10-30 note here (build-jobs left `CARGO_BUILD_JOBS` unset, cargo
     using every core in each of up to four containers, 16 cores and 127,941 MB on d-gtr9, run 7211, job 23231) is superseded.
     d-gtr9 now has 30 CPUs and 64 GiB with runner capacity 6, and the class layout is in the repo (6 jobs per host: 30 / 6 = 5 build
     jobs and 5 test threads), because with no share each of the 6 jobs used all 30 cores and ran 1.2 to 1.7 times slower than
     at 3 lanes (runs 8335 against 8346); see "CI job cost, step by step" (on docs/ci-job-cost-research until it lands).
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

## Pre-commit Clippy through the heavy slot, measured with `HEAVY_SLOTS=1` (2026-11-13, fix/precommit-heavy-slot)

The pre-commit hook started one `cargo clippy --workspace --all-targets` per app with staged Rust files, all at once; they now go through
`./dev heavy -- cargo clippy ...`, which finds the slot tool with the push gate's resolver (`heavy_command_for`: `PADDOCK_HEAVY`, `heavy.sh` or `heavy` on PATH, else the `heavy` built from tools/heavy) and refuses to run when there is none (ER-24). The numbers below were measured with `heavy.sh` called directly, before `./dev heavy` existed; the slot logic is the same script. Method: two apps (Dyno, Pitcrew) cold, started in parallel as
lefthook does, `CARGO_BUILD_JOBS=2` each, own target folder on d7, the 1-minute load sampled every 5 s. Plain commands, then the same
commands through `heavy.sh` with `HEAVY_SLOTS=1` (the default is 5 slots machine-wide, so with the default setting two builds still run
together unless other jobs hold slots). One run each on the shared Mac mini while other agents ran, so the load at the start differed.

| Run | Start load (1, 5, 15 min) | Wall | Peak 1-min load |
| --- | --- | ---: | ---: |
| plain, parallel | 30.5, 29.0, 20.6 | 363 s | 66.6 |
| through `heavy.sh`, `HEAVY_SLOTS=1` | 9.0, 13.4, 16.3 | 532 s | 30.8 |

Result: taking turns costs wall time (about 1.5 times here, the two builds no longer overlap) and the peak stayed under half. Not measured:
a quiet machine, the default 5 slots, and a real `lefthook run pre-commit` (lefthook is not installed in this worktree); the difference in
start load makes the peak comparison indicative only.

## Orphan guardian step, Linux crates job (2026-11-13, feat/ci-in-rust-wp5)

- **Command:** `./dev ci timings --json --limit 15 --no-logs` (Gitea API, no token printed), the `ci.yml (push)` section, job `crates (fmt, workspace hack, unused deps, coverage, deny)`, which runs `./dev ci guardian-as-user` (the shell script before)
- **Corpus:** 7 successful push runs of ci.yml, all on 2026-11-12, runner d-gtr9 (Linux)
- **Numbers:** the whole job: 2 m 13 s median, 3 m 13 s p90 (193 s). The step "Orphan guardian as a user (Linux)": 5 s median (it is in the job's biggest steps; its build is a cache hit after the workspace tests, its two test runs take about a second each). The coverage step, the largest, is 86 s.
- **Decision:** the two bounds are new (the shell had none, ER-32), not raised. Build: `PADDOCK_GUARDIAN_BUILD_TIMEOUT` default 600 s, about three times the p90 of the whole job, so a cold compile cache still fits and a hang ends. Test run: `PADDOCK_GUARDIAN_TEST_TIMEOUT` default 300 s, 60 times the step's median. Both only end a hang.
- **Not measured:** a cold-cache run of the build (the job's runs all had a warm sccache); the step's maximum (the summary gives medians and p90 of jobs only).

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

### Pre-commit after Clippy moved to the push gate (2026-11-13, fix/precommit-heavy-slot)

Clippy left the pre-commit hook (the user's decision): it runs rustfmt, ESLint on touched files, the Pitwall privacy check and commit-msg
only, and Clippy runs in `./dev check --pre-push` through the heavy slot. One real `lefthook run pre-commit` with a Rust file staged in
each of `apps/dyno` and `apps/pitcrew`, same machine, load about 15: both rustfmt commands took 0.06 and 0.07 s, 1 s wall in all. Before,
the same change ran two Clippy builds in parallel: 363 s wall at a peak 1-minute load of 66.6 (section above, cold builds). One run;
not measured: a change touching all five apps, which previously started five Clippy builds.

## Trial merges and the train-gates job (2026-11-13, ADR 0098)

Mac mini (14 cores), 1-minute load 9 to 12, warm git objects. One run each; quoted by ADR 0098.

| What | Time |
| --- | ---: |
| `git merge-tree --write-tree <merge>^1 <merge>^2` for train 8's merges `047a6d05f`, `10b6b88f0` (in memory, no checkout) | 0.01 s each |
| the same for `ad6bc2e9b` (`feat/generator-registry`) | 0.03 s |
| the conformance line (`node conformance/bin/paddock-conformance.mjs $PADDOCK_CONFORMANCE_ARGS`, origin/main `f7bbe2b86`) | 20.8 s |
| CI: the `train gates` job of run 8053 (ci.yml dispatch on `train/2026-11-13-8` at `3a342cb42`, Linux runner, green), from the jobs API `started_at` to `completed_at` | 288 s |
| CI: run 8053 as a whole, `started_at` to `completed_at` | 13 min 20 s |

The per-gate times of `./dev check --pre-push` on a shared-crate change (about 100 s of wall time with warm build folders) are in
"`./dev check --pre-push` run times" above.

### 2026-11-14, VM lock and kept-clone tests under load (fix/vm-tooling-and-kept-clone-tests)

- **Commit:** base 8c9c01dde (train 11) → this branch
- **Machine:** the shared Mac mini (14 cores, macOS 27.2), with the CI runners and other agents running
- **Load:** extra load: 6 `yes` burners (scratch `load.sh 6`); measured 1-minute load during the runs 35.5–120.5 (kept-clone, the first
  runs still in the decay of an earlier 42-burner run) and 22–29 (VM tests); no runs at the coordinator's request beyond 6 burners
- **Build:** debug test binary (`cargo nextest run --profile ci`, `NEXTEST_TEST_THREADS=4`, `CARGO_BUILD_JOBS=2`); node 24 for the VM tests
- **Command:** `cargo nextest run --profile ci --test it -E 'test(/host_steps::kept/)'` (20 times);
  `node --test scripts/vm-lock.test.mjs scripts/vm-win.test.mjs` (3 times, the 5-minute share of the load budget)
- **Numbers:**

  | Measure | Base | Change | Budget | Verdict |
  |---|---|---|---|---|
  | `./dev ci slot status` through the wrapper, idle / load 36 / load 200 (CPU) | 0.17 / 0.4 / 0.4 (max 1.0) s | binary: 0.01 s | none | the VM tests now run the binary |
  | kept-clone "foreign, stale, locked, unmarked, too big" (one test before, six after), quiet | 25.6 s | 2.1–2.7 s each | nextest 60 s | pass |
  | kept-clone "each phase bounded" (one test before, five after), quiet | 33.1 s | 2.8–4.5 s each | nextest 60 s | pass |
  | the same two, load 95 (42 burners, base only) | 41.1 / 35.3 s | not run at that load | nextest 60 s | base fails on CI (runs 8054, 8099) |
  | slowest kept-clone test, 20 runs, 6 burners | — | max 5.2 s (clean phase), 0 failures | nextest 60 s | pass |
  | VM lock + VM win tests, whole run, quiet | 133 s | 99 s | none | — |
  | the same, 6 burners, 3 runs | — | 138–142 s, 71/71 pass, 0 `slot hold` left after each run | none | pass |
  | `slot hold` processes left after a run, quiet | 1 (seen by `ps` after node exited) | 0 | 0 (ER-14) | pass |

- **Notes:** the kept-clone speed-up is parity runs of the shell and Rust implementations at the same time (two fixtures, nothing
  shared), one test per case, and bounded.sh's first second of naps at 0.05 s instead of 0.2 s (a kept clone's six phases each waited
  at least one nap). No limit was raised. The CI stall of single `./dev` calls past 60 s in run 8099 was not reproduced here (CPU load to
  200 and a 4 × 6 GB write load gave at most 1.0 s per wrapper call): not measured. Raw output in the agent's scratch folder, not kept.

## gen check: cold build of a generator's test binary (2026-11-14, fix/gen-check-build-first)

Question: how long does the build phase of the slowest `./dev gen check` entry take cold, so its own bound (`build_limit()` in
`tools/paddock-dev/src/generators/run.rs`) rests on a measurement (ER-12)? Before this branch the build ran inside the check's
1800 s hang guard; on train 10 and on the vault run2 (2026-11-14) `ts-debrief-wire` hit that guard cold and passed once the
folder was warm, so cold build plus run was more than 1800 s there.

- **Machine:** Mac mini (arm64), freshly rebooted (up 19 minutes), 1-minute load 29 at the start (5-minute 73), 42 at the end,
  peaks near 190 from other agents during the run
- **Command:** `./dev heavy -- cargo test --no-run -p debrief-app --lib` in `apps/debrief` (the entry's `build`), commit
  0a78cae1f, `CARGO_BUILD_JOBS=2`, empty `CARGO_TARGET_DIR`, no `RUSTC_WRAPPER` (no sccache), libduckdb from `cargo xtask libduckdb`
- **Runs:** 1 (a cold build is one event per folder)
- **Numbers:**

  | What | Time |
  |---|---|
  | cargo inside the heavy slot (607 crates compiled) | 866 s (cargo: 14 m 25 s); slot wait 0 s |
  | wall, including building this branch's `./dev` and `heavy` | 1021 s (user 1290 s, sys 47 s) |

- **Memory measure:** not measured
- **Bound chosen:** 3600 s for the build phase, about 4x the measured 866 s: the train 10 run under heavier load needed more than
  1800 s for build plus run, and the bound is a hang guard, not a budget. The check's own guard stays 1800 s and now covers only
  the run. Not measured: a cold build at load 150 or more, and the other entries' builds (all smaller workspaces or crates).

## C100 in the push gate (2026-11-17, feat/pre-push-unwired-check)

Question: what does the `gates-register` row (`./dev gates check`, C100) add to `./dev check --pre-push`? It runs on every push
(ER-41; a hand list of C100's inputs missed `apps/*/paddock.ci.json`, so none is kept).

- **Machine:** the shared Mac mini (arm64), other agents running; 1-minute load given per run
- **Tree:** origin/main 97176067c plus this branch, warm `./dev`
- **Runs:** 4 (the first rebuilt `./dev` after the edit, so its wall time is not the command's)

  | What | Load (1 min) | Seconds (wall) | C100's own time |
  |---|---:|---:|---:|
  | `./dev gates check` (includes a `./dev` rebuild) | 4.9 | 40.7 | 4.9 s |
  | `./dev gates check` | 9.8 | 5.4 | 4.9 s |
  | `./dev gates check` | 9.1 | 5.5 | 4.9 s |
  | `./dev gates check` (earlier, first look) | 8.7 | 7.7 | 6.9 s |

- **Added time per push:** the row takes 5 to 8 s and runs in parallel with the 70 s conformance tests (rank 0, three rows at a time
  on a quiet machine), so it adds about no push wall time.
- **Not measured:** a run under heavy load (above 20), and Linux or Windows.

## Conformance tests in the push gate (2026-11-14, ci/conformance-every-change)

Question: what does running the conformance row and the conformance tests on every push (ER-41) add to `./dev check --pre-push`?
The conformance row already ran on every push (the plan marks conformance on every change since b013e1142); the new row is
`conformance-tests` (`./dev ci run conformance tests`, that is `cd conformance && npm test`, 1000 tests).

- **Machine:** the shared Mac mini (arm64), other agents running; 1-minute load given per run
- **Tree:** this branch on train/2026-11-14-11 (1ecc53eee), warm `node_modules`, `./dev` already built
- **Runs:** 3 of the conformance tests, 2 of the conformance row

  | What | Load (1 min) | Seconds |
  |---|---:|---:|
  | `./dev ci run conformance tests`, alone | 57 | 75.6 |
  | `./dev ci run conformance tests`, alone | 40 to 27 | 71.2 |
  | `conformance-tests` row inside `./dev check --pre-push --refs-from-stdin --remote origin` | 22.7 | 71.1 |
  | `./dev ci run conformance paddock` (the row), alone | 57 | 24.6 |
  | `conformance` row inside the same pre-push run | 22.7 | 31.8 |

- **Added time per push:** about 71 to 76 s of run time for `conformance-tests`. At a load of 20 or more the gate runs one row at a
  time, so that is added wall time (the measured push gate took 4 min 3 s in all, 94 s of it queued for the heavy slot); on a
  quieter machine (three rows at a time) it overlaps the Clippy rows. It fits the 15 minute budget.
- **Not measured:** a run at a load under 10 (three rows at a time), and the conformance tests on Linux or Windows.

### Contracts step added to `conformance tests` (2026-11-19, refactor/retire-js-planner)

Question: what does the `cargo nextest run` line added to the phase (the whole-tree repo contracts and the audit fixture tests, moved
from the retired JS planner's test) add to the push gate's `conformance-tests` row?

- **Machine and tree:** the shared Mac mini (arm64) under other agents' load; a fresh worktree (`./dev worktree add`) of this branch
  at 4b94bbf55, `./dev` built, a new empty build folder (`PADDOCK_GATE_TARGET_DIR`, passed as `--config build.target-dir=`, never a
  `CARGO_*` variable), the local sccache on (6,480 hits; 29 Rust misses, the workspace's own crates)
- **Run:** `./dev ci run conformance tests` with `PADDOCK_TEST_CONCURRENCY=4`, one run

  | Line | Seconds |
  |---|---:|
  | `npm test` | 105.2 |
  | the `cargo nextest run` line (compile in a new folder, then the contracts and audit-fixture tests) | 31.0 |
  | the phase | 136.2 |

- **Added time per push:** about 31 s in a fresh worktree with a cold build folder and a warm sccache; the folder is kept between
  pushes, so a later push compiles only what changed. (`npm test` took 105 s here against 71 s on 2026-11-14: the load differs.)
- **CI (conformance job, Linux container):** cold 71 s, warm 1 s in the local CI image, recorded in docs/lessons.md (2026-11-19).
- **Not measured:** a warm second push in the same worktree, an sccache-cold machine, a Windows or amd64 run.

### With the cap of 4 test processes (2026-11-15, same branch, after review)

The gate now sets `PADDOCK_TEST_CONCURRENCY=4` (ER-24; `node --test` otherwise starts one process per core, about 13 here).
One run of `PADDOCK_TEST_CONCURRENCY=4 ./dev ci run conformance tests` (1004 tests) at a 1-minute load of 27 rising to 37: 75.1 s,
against 70.0 s uncapped a few minutes earlier at a load of about 24 to 29. So the cap costs at most a few seconds at this load
and leaves about 9 cores to the Clippy rows beside it. Not measured: the capped run on a quiet machine, where the gap could be
larger.

## Test timing redesign (2026-11-14, feat/test-timing-redesign, ADR 0103)

### 2026-11-14, test timing redesign

Question: what do the migrated tests cost before and after ADR 0103's helpers, and what do the helpers themselves cost on the
loaded machine (the reaper's look at the process table, the crash model with and without a test group)?

- **Commit:** base 1ecc53eee (train 11) → this branch
- **Machine:** the shared Mac mini (14 cores, macOS 27.2), with the CI runners and other agents running (about 890 processes)
- **Load:** other agents' 1-minute load 8-78 during the session; the crash-model A/B with 6 extra `yes` burners (scratch
  `model-load.sh`, `timeout 600` on each, ended on exit) at load 27-32, interleaved before/after, two rounds each
- **Build:** debug test binaries, `CARGO_BUILD_JOBS=2`; nextest 0.9.152; node 24
- **Command:** `cargo nextest run -p debrief-core --lib -E 'test(a_scan_stops_at_the_entry_limit_and_says_so)'` (before: train 11's
  file, after: this branch); `NEXTEST_TEST_THREADS=8 cargo nextest run --config-file <before|after>.toml --profile ci -p paddock-service
  --test it -E 'test(/^model::the_crash_model_/)'`; `cargo nextest run --test it -E 'test(/^host_steps::/)'` (tools/paddock-dev);
  `node --test scripts/vm-lock.test.mjs scripts/vm-win.test.mjs`; an ignored probe test of `Reaper` (50 looks with one live child,
  10 marker scans, removed after the measurement)
- **Numbers:**

  | Measure | Base | Change | Budget | Verdict |
  |---|---|---|---|---|
  | debrief scan-limit test, quiet | 4.845 s (50 001 files) | 0.013 s (21 files) | nextest 60 s (CI: 90.4 s TMT, run 8061) | pass |
  | crash model, 9 tests, slowest test, 6 burners, load 27-32 | 6.8 / 8.5 s (no group) | 6.2 / 6.8 s (group of 2) | nextest 60 s (CI: 59-60 s, run 8066) | group not kept: wall 7-9 s → 20-22 s |
  | paddock-dev `host_steps::*`, 37 tests, whole run | 15.3 s (load ~30-60) | 13.0 / 16.7 s (load ~30 / 78) | none | no change beyond the shared machine's noise; 0 leaks found |
  | VM lock + VM win, whole run | 98 s, 71/71 (load 25) | 99 s before the poll migration, 98 s after it (committed tree, load 11-13), 71/71, 0 processes reaped | none | pass |
  | `Reaper` look at the process table, one live child, load 60 | — | 18.2 ms | — | sampler capped at one look per 0.5 s |
  | `processes_mentioning` (marker scan), load 60 | — | 24-29 ms | — | once per reap |
  | Node kit tests (`scripts/test-kit.test.mjs`), 3 runs | — | 2.87-2.90 s, 6/6 | none | pass |

- **Notes:** the crash model's slowdown in CI (9x) comes from other jobs' load: two at a time lowered the slowest test only a little
  and tripled the wall time, so the tests got a size-class override (120 s, with this measurement in the waiver) instead. The
  host_steps comparison is dominated by the other agents' load (30 → 78 between runs); it shows no cost of the reaper above that
  noise. Two VM runs in between took 138-146 s: tools/paddock-dev had uncommitted changes, so the tests ran the `./dev` wrapper
  instead of the cached binary (82d5ee496's cost, 0.17-0.4 s per call); on the committed tree it was 98 s again. Raw output in the
  agent's scratch folder, not kept.

### 2026-11-15, test timing redesign, review round 1

Question: what do the tests migrated in ADR 0103's second round cost before and after, and does the debrief-app session test's
CI timeout reproduce under local load?

- **Commit:** base c48ffe017 (this branch after merging train 11 at 31cdde673) → this branch
- **Machine:** the shared Mac mini (14 cores, macOS 27.2), with the CI runners and other agents running; Linux runs in the
  `paddock-ci-local:v3-arm64` and `node:24-bookworm` images (`--cpus 2-4`, `--memory 2-8g`)
- **Load:** other agents' 1-minute load 18-29; the session-test load run with 6 extra `yes` burners (scratch `session-load.sh`,
  `timeout 600` each, ended on exit) at load 19-27
- **Build:** debug test binaries, `CARGO_BUILD_JOBS=2`; nextest 0.9.152; node 24
- **Command:** `cargo nextest run --test it -E 'test(/^golden::live_/)'` (tools/paddock-dev, before) and `-E 'test(/^golden::live_(range|workflow)/)'`
  (after); `cargo nextest run -p debrief-app --lib -E 'test(/workspace_commands::tests::a_session_/)'` (3 runs before, 4 after);
  `NEXTEST_TEST_THREADS=8 cargo nextest run --profile ci -p debrief-app --lib -E 'test(/workspace_commands::tests::/)'` with 6
  burners (5 runs)
- **Numbers:**

  | Measure | Base | Change | Budget | Verdict |
  |---|---|---|---|---|
  | golden `live_range_plans_...` (5 first-parent ranges) | 85.8 s, load 27 | 33.2 s, load 29 | ci override 480 s | pass |
  | golden `live_workflow_action_and_script_edits_...` (25 edits) | 21.0 s, load 27 | 12.3 s, load 29 | ci override 480 s (CI: SLOW 80-121 s) | pass |
  | debrief-app session test, quiet | 1.4-2.1 s (one test, three steps) | 1.0 s and 1.3 s (two tests, two steps each) | nextest 90 s (CI: 92.5 s TMT, run 8061) | pass |
  | the same, 6 burners, load 19-27, 5 runs | 1.8-2.0 s | not run | nextest 90 s | the CI timeout did not reproduce at this load |
  | debrief-app session test in CI run 8061 (from its wait log) | import 29.6 s, reopen 12.4 s, import again 20.0 s | at most one import and one step per test | nextest 90 s | an estimate from the log, not measured |
  | Node `processTable()` (one `ps`), 863 processes, load 16 | — | 28.75 ms | — | sampler backs off to 0.5 s |
  | Rust reaper tests, red (candidates() empty) / green | 4 of 10 failed, no process ended by the test | 17/17 three times (macOS), 112/112 twice with paddock-testkit (Linux) | none | pass |
  | Node kit tests, red (choose empty) / green | 3 of 9 failed in 1 s, no process ended by the test | 9/9 three times (macOS), three times (node:24-bookworm) | none | pass |

- **Notes:** the session test's CI wait log shows each step 10-20 times slower than here; the local load (at most 6 burners, the
  agents' cap) does not reach that, so the split rests on the log's per-step times, not on a local reproduction. macOS 27 shows
  no other process's environment (`ps -E` and sysinfo's `environ()` both return none for a same-user `sleep` given a variable),
  so the reaper's token is read on Linux only. Raw output in the agent's scratch folder, not kept.

## Queue scenario tests: git children per test (2026-11-15, fix/queue-tests-fast)

Question: why do the merge-queue scenario tests (`tools/paddock-dev/src/queue/tests.rs`) take 20 to 55 s each on the Windows
runner, and four of them cross nextest's 60 s limit when the job runs 42% slower (train 11 run 8115: job 577 s against 405 s in
run 8110)?

- **Machine:** Mac mini (arm64), 1-minute load 13 to 23 (train CI and other agents on it); Windows times from the CI logs of the
  `paddock-dev (windows-latest)` job of runs 8054 (main), 8110 and 8115 (train 11)
- **Command:** `cargo nextest run --locked --test-threads=1 queue::tests` in `tools/paddock-dev` (base 6fda267d3, then this branch);
  git children counted by a `git` wrapper first on `PATH` that writes `$NEXTEST_TEST_NAME` per call (same run, default threads)
- **Runs:** 1 counted run and 1 timed run each side; the module after the change 20 more times (all green, 6.3 to 6.4 s each)
- **Numbers:**

  | Test (after: the halves of a split test) | git children before | after | macOS before | after | Windows 8110 before |
  |---|---|---|---|---|---|
  | two_pull_requests_one_breaks... | 220 | 98 | 1.88 s | 1.11 s | 31.4 s |
  | several_suspects... / only_failures_of_mains_own... | 309 | 82 / 46 | 2.86 s | 0.97 / 0.58 s | 43.1 s |
  | the_backup_window_holds... / an_unread_hour_never... | 208 | 75 / 35 | 2.06 s | 0.85 / 0.48 s | 31.4 s |
  | a_pull_request_whose_plan_runs_everything... / a_failure_only_the_pull_request... | 309 | 68 / 60 | 2.87 s | 0.81 / 0.75 s | 38.5 s |
  | a_gate_job_failure... (split in two) | 313 | 70 / 62 | 3.02 s | 0.85 / 0.74 s | 34.9 s |
  | a_blocked_train... (split in two) | 249 | 67 / 57 | 2.52 s | 0.94 / 0.66 s | 26.4 s |
  | the whole module, 78 tests before, 90 after | 6953 | 3576 | 73.2 s | 48.9 s | |

- **Where the time went:** in git children. The Windows time of a test divided by its git children is 0.11 to 0.15 s for every
  slow test in run 8110 (0.19 s or more in 8115, where they hit 60 s), and on macOS the children were about two thirds of the
  wall time. The fixture's two clones are 2 of about 220 children. Per tick the queue's adapter ran one fetch per branch, two
  children per `contains` (`cat-file -e`, then `merge-base --is-ancestor`) and three per commit in `verify_train`; the real merge
  check ran about 25 per merge.
- **Change:** one fetch for all branches that must exist (the train branch stays its own fetch), `contains` as one
  `merge-base --is-ancestor`, `verify_train` reads each commit once (`cat-file commit`: tree, parents and message), the scenario
  tests use a clean merge check unless they are about it (the real one still runs in the one-green, conflict and `GitTrain::open`
  scenarios), and the five tests that built two worlds are split in two.
- **Windows estimate (not measured):** at 0.11 to 0.15 s per child the slowest test after (105 children) takes 12 to 16 s, the
  four that timed out 4 to 15 s; at run 8115's 0.19 to 0.27 s about 28 s at most, half the 60 s limit.
- **Windows measured after (train 11 run 8118, ci.yml on cac11aa00, job 38802 `paddock-dev (windows-latest)`):** the four tests
  that had timed out in run 8115 took 15.0 s (a_pull_request_whose_plan_runs_everything...), 11.7 s (the_backup_window_holds...),
  14.1 s (several_suspects...) and 16.6 s (two_pull_requests_one_breaks...): within the estimate above and a quarter of the 60 s
  limit. The job ran 1354 tests, all passed, in 459 s (7 slow). One run; the estimate line above stays as the reasoning.
- **Memory measure:** not measured (no memory question)

## How long a run holds the Mac and Windows runners (2026-11-15, fix/small-foundation-2026-11-15)

The limit of the `runner-wait` action (`.gitea/actions/runner-wait/action.yml`, `max`) must cover the time an older run keeps
the single-machine runners busy.

- **Machine:** the Gitea Actions API of decryp7/paddock (read-only), from the Mac mini
- **Load:** not relevant (API reads)
- **Command:** a scratch script (curl over `/actions/runs` and `/actions/runs/<id>/jobs`): per completed, not cancelled run,
  the span from the first start to the last end of its jobs whose labels name macOS or Windows
- **Window:** the newest 200 runs, 2026-11-10T02:46 to 2026-11-15T12:25 (146 runs had such jobs)
- **Numbers:**

  | Measure | Value |
  |---|---|
  | median span | 19.7 min |
  | p90 | 84.3 min |
  | p95 | 130.1 min |
  | longest since 2026-11-13 | 94.6 min (debrief.yml, run 8047) |
  | longest in the window | 327.9 min (debrief.yml, run 7925, 2026-11-11, runs overlapping on the Mac before `ci dispatch` waited) |
  | runs over 150 min | 6 of 146, all from 2026-11-11 |
  | longest single job | 151.8 min (VM tooling tests on macOS, run 7932, 2026-11-11) |

- **Verdict:** 2h30m (150 min) covers every run since 2026-11-13 and 140 of 146 in the window; the six longer ones were overlap
  runs of 2026-11-11, the shape the one-at-a-time rule removes. It stays under act_runner's default 3 h job timeout (the
  runners' own configured timeout was not read).
- **Notes:** the span counts jobs queued behind the runner too (start is when a runner took the job). Raw output in the agent's
  scratch folder, not kept.

## Member audits in the member gate (2026-11-17, feat/member-gate-audits)

Question: what does the `member-audits` row (the repository-wide audits of `tools/paddock-dev/tests/it`, selected by the nextest
profile `member-audits`) add to the member gate of `./dev merge check --gates`? The user had been told about a minute. The row runs
only when the push gate is the member gate (`check --pre-push --member-gate`); a developer's push skips it with a reason.

- **Machine:** the shared Mac mini (arm64), other agents running; `CARGO_BUILD_JOBS=2`, `RUSTUP_AUTO_INSTALL=0`
- **Command:** `cargo nextest run --profile member-audits --locked --no-tests=fail` in `tools/paddock-dev`, run 1 time per row
  below (one run each: a second run was not made)

### Final: the positive list without the history replay (merged tree, train 13 plus this branch)

The profile lists the audits (dev_bin, cargo_target_lint, parity_corpus, tree, golden, check_allow_list, safe_remove_lint, the live
and working-tree checks of the check modules, the inputs-against-cargo-metadata tests, the ipc-endpoints, contracts and generators
registry tests) and leaves out `golden::live_range_plans_match_affected_mjs_on_this_repositorys_first_parent_history` (over 120 s, it
replays the first-parent history and audits no fresh code) and the fixture goldens (`build_jobs_golden`, `host_steps_golden`: they
read no tracked tree).

  | What | Load (1 min) | CPU idle | Tests | Seconds |
  |---|---:|---:|---:|---:|
  | warm build folder, nextest's own run | 6.6 | 89 % | 80 | 34.5 |
  | warm build folder, whole command (a small recompile included) | 6.6 | 89 % | 80 | 41.8 |
  | cold build folder (new empty target dir; the compile cache may have helped), whole command | 6 to 9 | not read | 80 | 113.3 (nextest 31.8) |

### Option considered: the whole `it` binary minus the replay (a negative filter only)

Review asked for it as the common-practice option (no list to drift), if warm took at most about 2 minutes. Same tree, one run, load 6.4
to 9.4 (CPU idle 66 %): 572 tests, nextest 129.9 s, whole command 160.0 s (the build of the merged tree included, which is not a cold
build). Not taken: warm is over the 2 minute line, and one test (`cli::a_trial_with_bad_arguments_is_refused_before_anything_is_cloned_or_pushed`)
ran into its 120 s stop under load. That run also had 10 failures, which were not the option's: they came from the build folder
exported in the shell that ran it (`CARGO_TARGET_DIR`; fixed in 05f72d31a, and the same tests pass with `RUSTUP_AUTO_INSTALL=0`
exported, so the earlier note that the gate's own variable broke the wrapper tests was wrong and is withdrawn).
The positive list is checked clause by clause against the binary's test names (`tests/it/member_audits.rs`).

### Earlier runs (superseded by the table above)

On feat/member-gate-other-os (72cf747e9), load 18, warm folder, 68 tests, the list then in the profile: nextest 122.5 s, whole command
127.8 s, almost all of it the history replay (over 120 s). Without the replay (67 tests, load 10.6): nextest 24.9 s, whole command
25.2 s. Those runs predate train 13's audits (dev_bin, cargo_target_lint, parity_corpus), which add tests to the list.

- **Not measured:** a busier machine and a second run of any row.

## Prune size, one pass (2026-11-15, fix/c125-and-arm64-tests)

Question: how long does `tools/prune-build-folders.sh` (the shell kit's `prune-folder`, the release path) take to size a large build
folder, now that the size is the regular files' own length and blocks from `kit.sh portable` instead of `du` (CI run 8138)? Before
this change `size()` ran two kit passes (file-allocated, file-bytes) and the script called it twice under the cap (up to four times
over it); now one `portable file-sizes` pass gives both numbers and the script measures once (and again only after the incremental
caches went).

- **Machine:** Mac mini (arm64), macOS 27.2.1, 14 cores, 1-minute load 6.5 to 9.6 (5-minute 10 to 15), no CI run active
- **Folder:** a paddock-dev `CARGO_TARGET_DIR` (15,306 files, 3.1 GiB), warm in the file cache (one stat pass first)
- **Command:** `bash <script> <folder> 100000` (a cap in GB far above the folder, so nothing is removed), wall time by
  `Time::HiRes`, three runs interleaved
- **Numbers:**

  | What | Runs (s) |
  |---|---|
  | `du -sk <folder>` alone | 0.106, 0.113, 0.105 |
  | one `kit.sh portable file-sizes` pass (find + BSD `stat -f`, sorted) | 7.16, 7.14, 7.31 |
  | script at f51766792 (`du -sk` plus its own find + stat, two `size()` calls) | 16.0, 14.5, 14.7 |
  | script at ffe91b1ac (two kit passes per `size()`, two calls) | 28.4, 28.4, 27.8 |
  | script now (one pass) | 6.9, 9.0, 7.3 |

- **Memory measure:** not measured
- **Reading:** one pass costs about 70 times `du` on macOS, so the script now runs it once (four times faster than ffe91b1ac).
  Not measured: GNU `find -printf` on the Linux runners (no `stat` per file), Windows (Git Bash) and a cold file cache.

## axe scans per screen (2026-11-15, fix/main-webkit-e2e): superseded

Superseded on 2026-11-16 before it landed: `axeViolations` no longer detects motion and scans again (the `MAX_SCANS` this
measured is gone). It turns transitions and animations off for a single scan instead (the user's decision after the fourth
instance of the class; lessons.md, "axe read a table row half-way into the dark theme"). The measurement (at most 3 scans per
screen over ui, Dyno and Pitwall, 409 tests) is in this branch's history, commit b661502f0.

## Pre-push Linux Clippy by default, cold and warm (2026-11-16, feat/pre-push-linux-clippy)

Question: what does the Linux Clippy row cost now that `./dev check --pre-push` runs it by default for a touched workspace with Linux-only code (a `cfg` on `target_os = "linux"`)?

- **Machine:** Mac mini (arm64), OrbStack, CI image `paddock-ci:v3` as linux/amd64 (emulated), `CARGO_BUILD_JOBS=2`, 1-minute load 12 to 17, CPU idle 35 to 50% at the start.
- **Command:** `paddock-dev check --pre-push --only clippy-linux` (one container run through `heavy`), workspace `tools/paddock-dev`, one run each.
- **Numbers:**

  | Run | Seconds |
  |---|---:|
  | cold (no `/target/tools-paddock-dev` in the container's kept volume yet) | 198.8 |
  | warm, nothing changed | 3.2 |
  | warm, one planted dead-code lint (red, exit 5) | 26.6 |

- **More numbers (same machine, one run each; load 12 to 17 at the start):**

  | Workspace and run | Seconds |
  |---|---:|
  | `crates` cold (`--all-features`) | 521.9 |
  | `crates` warm | 3.2 |
  | `apps/dyno` cold, Tauri crate included | over 899: cut short by the 15 minute budget, "not measured" (the compiled part stays in the volume) |
  | `apps/dyno` with a planted Linux-only lint in dyno-agent (red, after the Tauri crate was left out, so a rebuild) | 292.6 |
  | `apps/dyno` clean again | 86.5 |

- **Apps:** the CI image has the Tauri system libraries (gdk, cairo, pango, atk compiled), but the `dyno-app` build script writes the sidecar placeholders into the read-only repository mount and fails (`Read-only file system`), so the row leaves the `src-tauri` crates out of an app workspace and prints a `::warning::`; CI lints them on Linux.
- **`./dev check --warm-linux` (2026-11-16, same machine, idle 51 to 69% at the start, one run):** `crates tools/paddock-dev apps/debrief apps/dyno apps/pitcrew apps/pitwall apps/spotter` in one command, one container run each, no budget.

  | Workspace | Seconds | Note |
  |---|---:|---|
  | `crates` | 4.3 | already warm from the runs above |
  | `tools/paddock-dev` | 18.0 | already warm; stopped on two real lints in the new code (a disallowed `Command::new` and a missing doc), fixed |
  | `apps/debrief` | 658.0 | cold, `debrief-app` left out |
  | `apps/dyno` | 3.0 | already warm from the runs above, `dyno-app` left out |
  | `apps/pitcrew` | 256.0 | cold, `pitcrew-app` left out |
  | `apps/pitwall` | 326.0 | cold, `pitwall-app` left out |
  | `apps/spotter` | 414.8 | cold, `spotter-app` left out |

  Each app row printed the `::warning::` naming its Tauri crate, so the `cargo metadata` match found `debrief-app`, `pitcrew-app`, `pitwall-app` and `spotter-app` in the container.
- **Not measured:** a push touching several workspaces, a quiet machine. The Linux code scan (text match over the touched workspaces' `.rs` files, no cache) is part of the 5.2 s wall time of the warm run beside the container start; it was not timed apart.
- **Reading:** the cold cost is paid once per workspace, in the container's kept target volume; warm runs fit the 15 minute budget with room.

### Pre-push Linux Clippy, native (2026-11-16, feat/pre-push-linux-clippy)

The row now runs on the native arm64 CI image (the helper's `--ci-image`, built locally from ci-image/), not the x64 image under emulation (user decision: CI's x64 Linux legs and the member gate catch the rare arch-specific lint). The emulated numbers above are superseded.

- **Machine:** Mac mini (arm64), OrbStack, `CARGO_BUILD_JOBS=2`, CPU idle 48 to 53% at the start, cargo registry volume already filled, target folders cold (a new `v3/<workspace>` folder each).
- **Method:** `./dev check --warm-linux crates tools/paddock-dev apps/debrief apps/dyno apps/pitcrew apps/pitwall apps/spotter`, one cold run per workspace, the Tauri app crates left out.

  | Workspace | Cold seconds |
  |---|---:|
  | `crates` | 100.3 |
  | `tools/paddock-dev` | 39.7 |
  | `apps/debrief` | 158.9 |
  | `apps/dyno` | 173.7 |
  | `apps/pitcrew` | 74.2 |
  | `apps/pitwall` | 104.8 |
  | `apps/spotter` | 116.1 |
  | all seven | 767.6 |

- **Warm, native:** `apps/dyno` with a planted dead-code lint in dyno-agent: red in 5.0 s; the clean tree: 4.7 s (row text `clippy-linux apps/dyno (left out: dyno-app)`).
- **Limits set from this:** `--warm-linux` ends one workspace's run at 360 s (about twice the largest cold run, 173.7 s) and waits at most 15 minutes for the heavy slot (another agent's full warm-linux holds it for about 768 s). Recorded in `tools/paddock-dev/src/pre_push/warm.rs`.
- **Size:** after these runs the kept volume `paddock-linux-target-arm64` is 10.2 GB and `paddock-cargo-registry` 644 MB (`docker system df -v`); each CI image version adds its own `v<version>/` folders. `bash tools/linux-container.sh --id paddock --clean` removes both volumes (and the local images); the next `--warm-linux` refills them. The older amd64 volume `paddock-linux-target` (12.6 GB, from the emulated runs) is no longer used.
- **Local CI image build, cold (2026-11-17):** `linux-container.sh --id paddockimgprobe --ci-image -- true` on a copy of ci-image/ with one changed line (a fresh tag, so the existing image stayed) and `docker build --no-cache`: garage stage 167 s, ci-image/Dockerfile 212 s, 379 s in all; the two images are 4.32 GB and 6.64 GB (about 11 GB). The probe's images and volumes were removed with its own `--clean`. `./dev check --warm-linux` builds the image first as its own step (bound 760 s, about twice this) when `linux-container.sh --image-ready` says it is not built; the push row never builds it: it reports "local CI image missing" and names `./dev check --warm-linux`.
- **Not measured:** a quiet machine, a push touching several workspaces.

## sccache 0.19.0 across worktrees, local disk cache (2026-11-16, docs/sccache-basedirs)

- **Machine:** Mac mini (arm64), macOS 27.2, 14 cores, CPU idle 46 to 69% at each start (`top -l 2 -n 0 -s 2`), load
  average 11 to 12 (other agents running), no CI run of mine
- **Tool:** sccache 0.19.0 release binary (aarch64-apple-darwin, sha256 a35a0b7d...286af, checked against the release's
  `.sha256`), one server on a private port, `SCCACHE_DIR` a scratch folder on the vault volume, `SCCACHE_BASEDIRS` set to the
  three worktree roots (0.19.0 does not use it for Rust, see the note)
- **Workspace:** `tools/paddock-dev` at 3585bcab1 (260 lockfile packages; 210 compile requests, 152 cacheable), Rust 1.100.0,
  `cargo build --locked` (dev profile), `CARGO_BUILD_JOBS=2`, one shared scratch `CARGO_HOME`, target folder removed before
  each run; three detached worktrees A, B, C of the same commit at different paths; single runs, run in the order listed
- **Numbers:**

  | Run | Target folder | Wall (s) | Rust hits |
  |---|---|---|---|
  | A, no sccache | `<A>/tools/paddock-dev/target` | 66 | n/a |
  | B, no sccache | `<B>/...` | 64 | n/a |
  | A, empty cache | `<A>/...` | 51 | 0 / 146 |
  | A again, same path | `<A>/...` | 23 | 146 / 146 |
  | B, other worktree | `<B>/...` | 28 | 120 / 146 (82%) |
  | C, other worktree, `--config build.target-dir=<scratch>` | outside the worktree | 32 | 120 / 146 (82%) |
  | C, `CARGO_TARGET_DIR=<scratch>` in the environment | outside the worktree | 58 | 0 / 146 |
  | C, default target | `<C>/...` | 28 | 120 / 146 (82%) |

- **Memory measure:** not measured; cache on disk 369 MiB after all runs, one target folder 1.4 GiB
- **Reading:** across worktrees 0.19.0 hits 82% of the third-party Rust compiles (wall time 28 s against 64 s cold)
  as long as the target folder is not given through `CARGO_TARGET_DIR` (every `CARGO_*` variable is part of the key, so
  that gives 0%). The 26 misses are crates whose build scripts set `OUT_DIR` (proc-macro2, serde, serde_json, thiserror,
  libsqlite3-sys, ...) and crates built on them. The workspace's own crates are never cached in a dev build
  (`CannotCache(incremental)`), nor are build scripts, binaries and proc-macros. The empty-cache run being faster than the
  no-sccache run is noise of single runs on a loaded machine, not a sccache effect. Not measured: an app workspace,
  Linux, Windows, release profile.

## Local sccache: local disk against RustFS, and the CI-like environment (2026-11-16, feat/local-sccache)

- **Command:** `cargo build --locked --config build.target-dir='<scratch>'` with `RUSTC_WRAPPER=<sccache 0.19.0>`; `tools/paddock-dev` (dev
  profile) and `cargo build -p dyno-cli` in `apps/dyno`; `CARGO_BUILD_JOBS=2`, `RUSTUP_AUTO_INSTALL=0`, the default `CARGO_HOME`; the
  target folder removed before each run; `--zero-stats` before each; two sparse detached worktrees A and B of 3585bcab1 at different
  paths; single runs. The CPU idle was 30% or more on two readings a minute apart before each series (38 and 41; 60 and 58; 60 and 59).
  The scripts were in the scratch folder, not committed.
- **Hardware / OS:** Mac mini, macOS 27.2 (Darwin 27.2.0), arm64; RustFS at 172.16.1.220:15000 on the LAN, plain HTTP, path-style.
- **Numbers (wall seconds, Rust hits / Rust compiles):**

  | Workspace | Run | Local disk | RustFS (S3) |
  |---|---|---|---|
  | paddock-dev | no sccache, cold | 70 | n/a |
  | paddock-dev | empty cache, worktree A | 55, 0 / 152 | 58, 0 / 152 |
  | paddock-dev | warm cache, worktree B | 32, 126 / 152 (83%) | 37, 126 / 152 (83%) |
  | dyno-cli | no sccache, cold | 141 | n/a |
  | dyno-cli | empty cache, worktree A | 92, 2 / 138 | 90, 2 / 138 |
  | dyno-cli | warm cache, worktree B | 46, 111 / 138 (80%) | 48, 111 / 138 (80%) |

- **CI-like environment, local disk, cache filled by the agent runs above (paddock-dev, worktree A's path, another build folder):**

  | Setting of the CI-like run | Wall (s) | Hits |
  |---|---|---|
  | `CARGO_INCREMENTAL=0` as a variable (what `setup-rust` writes to `$GITHUB_ENV`) | 63 | 6 / 153 (4%) |
  | the same setting as `--config build.incremental=false` | 38 | 126 / 159 (79%) |
  | dyno-cli, `--config build.incremental=false` | 45 | 111 / 153 (73%) |

- **Not measured:** the `CARGO_TARGET_DIR` control in this series (the script failed on bash 3's empty array; the earlier note measured it:
  0 / 146); Linux, Windows, a release profile; a second machine; the cache's size on RustFS (the local cache was 625 MiB after all runs).
- **First RustFS attempt:** the first series (prefix `bench-20261116233304`) had a write error on every compile (138 of 138 on Dyno) and so
  0 hits; the cause is a read-only server: sccache 0.19.0 latches a failed startup write check for the server's lifetime (docs/lessons.md; found by
  the later investigation, not measured here; a debug run a few minutes later, with the same settings and a small build, had none). The series above
  is a second run from scratch with a new prefix and no write errors. Treat RustFS as unproven under load until it has run for days.
- **Reading:** passing the folder as configuration gives the 80% across worktrees on both workspaces; the S3 backend on the LAN costs
  about 5 s more on a warm build than the local disk (37 against 32 s, 48 against 46 s) and needs the network and the key. The CI-like
  run hits the agents' entries only when `CARGO_INCREMENTAL=0` is cargo configuration: the variable is part of sccache's key. CI keeps
  its own per-slot caches (user decision 2026-11-17), so this is a measurement, not a setting.

## Merge check: reverted-base warning (2026-11-17, feat/merge-check-reverted-base)

Question: what does the report-only reverted-base step of `./dev merge check` (a member built on a merge the target reverted)
cost on the real train history?

- **Machine:** Mac mini (arm64), macOS 27.2.1, 14 cores, CPU idle at or above 30% on two readings before the runs, no CI run
  measured against
- **History:** `origin/train/2026-11-15-13` (8,492 commits, 18 whose subject starts with "Revert"), each member as
  `origin/<branch>`, debug build of `paddock-dev`
- **Command:** `paddock-dev merge check --branch origin/<b> --target origin/train/2026-11-15-13 --no-generators`, wall time by
  `date`, one run each
- **Numbers (after the review fixes: revert subjects, member forked from the reverted branch, per-branch lines):**

  | What | Time (s) |
  |---|---|
  | whole `merge check`, feat/dev-vm-broker (no reverted base) | 0.48 |
  | feat/dev-launch-rust (one reverted merge) | 3.8 before dropping a per-merge `git show` loop, then not re-measured |
  | fix/testkit-ambient-hardening (6 reverted merges of feat/test-env-builder, one line) | 4.7 before that fix, 1.32 after |
  | feat/pre-push-linux-clippy (trailer-less `Revert "..."` 3bbbfc7c7, found by subject) | 1.70 |

- **Result:** the members that hold feat/test-env-builder (fix/testkit-ambient-hardening, diag/testkit-git-cost, feat/dev-launch-rust)
  warn; feat/pre-push-linux-clippy warns through the revert that has no trailer; feat/dev-vm-broker and feat/debrief-duckdb-2 are
  silent. The cost is about 0.2 s per live revert (rev-list, subject, containment: 3 to 5 git calls), one run each, debug build.
- **Memory measure:** not measured
- **Not measured:** a member that merges cleanly (the full detector run on top), Windows, a cold file cache, release build.

## TestCommand construction: per command against once per process (2026-11-16, feat/test-env-builder, ADR 0100)

Question: why did the paddock-dev tests that start many children through `TestCommand` run about 6 times slower on the
Windows CI runner (ci.yml run 8152, job 39497: merge_check median 5.8 to 44 s, golden 24.5 to 160.5 s, the other 302 tests
1.06x), and which part of `TestCommand::new` costs it: the toolchain links, the `PATH` resolution or `git --exec-path`?

- **Machine:** the Windows 11 Arm test VM (x64 binary under emulation), through `scripts/vm-win.sh run --ci-like` (the runner's
  temp folder, Git's bin first on PATH) under the test machines lock; the Mac mini for the Mac row. 1-minute Mac load 16.9.
- **Command:** `crates/paddock-testkit/examples/measure_test_command.rs` built with `cargo xwin` (debug), run as
  `measure_test_command --runs 20` (no toolchain on PATH), `--fake-toolchain <dir>` (a 15 MB `rustup` with six proxies as hard
  links of it and a separate `cargo`, first on PATH and in `CARGO`) and `--fake-toolchain <dir> --saturate` (the `rustup` file
  given 1017 more hard links first: NTFS refuses the next one with os error 1142, as when many commands hold links to it).
  "before" is 88d0d9fb3 (links made per command in the command's own folder, PATH worked out per command); "after" is this
  change (one link folder per toolchain, made once; PATH worked out once per machine PATH and `CARGO`). One run each, same VM
  session.
- **Numbers (milliseconds per `TestCommand::new`; first call, then the median and max of the next 19):**

  | Case | before: first / median / max | after: first / median / max |
  |---|---|---|
  | no toolchain on PATH | 67.2 / 13.4 / 16.3 | 79.5 / 1.1 / 2.6 |
  | toolchain, hard links work | 60.3 / 6.8 / 11.0 | 83.1 / 1.0 / 17.5 |
  | toolchain, hard links refused (copies) | 522.4 / 72.6 / 3481.6 | 162.5 / 0.9 / 12.8 |
  | Mac, no toolchain on PATH (before) | 1.6 / 0.8 / 1.0 | |

  Parts, measured separately in the same runs: seven hard links 0.6 to 15 ms; seven 15 MB copies 33 to 43 ms (plus the
  scan of each new program file; a max of 3.5 s per command before); `git --exec-path` 40 to 71 ms, already asked once per
  process before this change; the seven-name `path_report` resolve 4 to 6 ms.
- **Cause:** work done per command. Where hard links work it is mainly the `PATH` resolution (13.4 ms median with nothing
  found early on PATH); where they are refused (another volume, or the 1024-link limit of one rustup file that every command
  linked again) it is the copy, 72.6 ms median and up to 3.5 s. Which of the two the CI runner hit is not recorded in its log
  (the runner's volumes are not printed; the copy fallback now says so on stderr); both are now paid once per test process,
  and the copy once per toolchain and target folder (in CI: once per job, whose target folder is new). `git --exec-path` was
  not the cause. Since the review of df9af3cda the folder is in the build's target folder, not the temp folder; the numbers
  above were measured with it in the temp folder, which costs the same.
- **Rename retry (2026-11-16):** in six runs of `measure_test_command --saturate` on the VM (copies, so a scanner may hold the
  new files), run from a target-folder layout, no rename of a staging folder was refused, so the retry never ran. Its 500 ms
  bound was ADR 0078's, not a measured hold, so the retry was dropped: a refused rename leaves the process on its complete
  staging folder (logged), which is correct. Per command after the first: median 1.2 to 2.5 ms.
- **Not measured:** the CI runner itself (the integrator's rerun of the failed jobs is the check), a release build, a cold
  Defender cache.

## `rustc --print sysroot` through rustup's proxy (2026-11-16, fix/testkit-ambient-hardening, ADR 0100)

Question: how long the test kit's one-per-process `rustc --print sysroot` (finding the toolchain's cargo when `CARGO` is unset)
takes, so its hang guard `SYSROOT_LIMIT` rests on a measurement (ER-12).

- **Machine:** the Mac mini (arm64), 1-minute load 23.8 to 26.5 with other agents' builds running; rustup 1.29.3, rustc
  1.100.0 (the repository's pinned toolchain, installed).
- **Command:** `env RUSTUP_AUTO_INSTALL=0 rustc --print sysroot` from `crates/` (the proxy reads `rust-toolchain.toml`), timed
  with `/usr/bin/time -p`, 10 runs.
- **Numbers:** 0.04 to 0.06 s wall (median 0.05 s).
- **Bound chosen:** `SYSROOT_LIMIT` is 30 s, a hang guard about 500 times the measured time: it ends only a proxy that hangs
  (a toolchain install that `RUSTUP_AUTO_INSTALL=0` should prevent), never a slow answer on a loaded machine. Not measured:
  Windows, a cold disk cache.

## A child's `git` on Windows: Git's launcher against the real program (2026-11-16, fix/testkit-ambient-hardening, ADR 0100)

Question: ci.yml run 8161 (job 39640, windows-latest) ran the merge_check integration tests about 5.7 times slower than run 8138
(median 37.6 s against 5.8 s, 9 timeouts at 60 s) while the other tests were back to normal; merge_check's program under test
starts `git` many times through its own `PATH`, which `TestCommand` curates. Which `git` does a child find, and what does it cost?

- **Machine:** the Windows 11 Arm test VM (x64 program under emulation, Git's native `clangarm64` git), through
  `scripts/vm-win.sh run --ci-like` (the runner's temp folder, Git's `bin` first on PATH) under the test machines lock.
- **Command:** `crates/paddock-testkit/examples/measure_test_command.rs --git-calls 100` (which git the child's curated PATH and
  the test process's PATH find, 100 runs of `--version` each) and `--git-ab 100` (Git's `bin\git.exe` launcher and the real git
  called in turn, A B A B, so a busy moment costs both alike), alone and with 8 copies at once.
- **Numbers:**

  | Measure | Before (991365671) | After (this change) |
  |---|---|---|
  | `git` on a child's curated PATH | `C:\Program Files\Git\bin\git.exe` (the launcher) | `C:\Program Files\Git\clangarm64\bin\git.exe` |
  | launcher against real, in turn, alone (ms per call) | 62.3 against 44.3 | |
  | the same, 8 at once (ms per call, 8 runs) | 163.2 to 168.7 against 98.8 to 102.6 | |

- **Cause:** the curated PATH put Git for Windows' `usr/bin` and `bin` before git's own folder, so a child's `git` was
  `bin\git.exe`, a launcher that starts the real git: one more process per call, 1.4 times the cost alone and 1.65 times with 8
  at once. Run 8138 predates that order (its PATH had git's own folder and no `bin`). Fix: git's own folder first, then
  `usr/bin`; `bin` (launchers only) is left out.
- **The full suite on the test VM (2026-11-16):** `scripts/member-gate-windows.sh <tree> "nextest tools/paddock-dev paddock-dev"`
  (feat/member-gate-other-os at ec9d7a662 merged locally with this branch; x86_64 programs on the Arm VM, the toolchain without
  rustup), one run each, about 1040 to 1060 s of guest time:

  | Tree | merge_check tests | median | total | max |
  |---|---:|---:|---:|---:|
  | CI run 8138 (job 39335, x64 runner), the reference | 67 | 4.54 s | 345 s | |
  | CI run 8161 (job 39640), the slow one | 58 | 26.9 s | 1399 s | |
  | VM, the launcher first on the PATH (the order before 756046bdc) | 79 | 3.73 s | 263 s | 20.5 s |
  | VM, git's own folder first (756046bdc) | 79 | 3.38 s | 236 s | 17.4 s |
  | VM, the same plus the toolchain-bin fix (85c21b7d6) | 79 | 3.19 s | 218 s | 16.3 s |

  On the VM the launcher costs merge_check about 1.17 times (total 263 against 218 s), and no tree on the VM is near run 8161's
  5.7 times: **the CI slowdown is not reproduced on the VM, so its main cause is still not found**; the launcher fix is real but
  small. The VM run also found 13 failures from TestCommand (a hard-linked `rustc.exe` without its DLLs, fixed in 85c21b7d6) and 9
  that are not this branch's (`places::` and `brief_check::` with `/srv` paths, `build_jobs_golden` memory probe, the
  `safe_remove` guard's verbatim-path parity), the same 9 in every run.
- **Not measured:** the CI runner itself with these trees (the train's next Windows run).

## paddock-dev on the Windows test VM, TestCommand re-landed on train 13 (2026-11-17, feat/test-env-builder-v2)

- **Machine:** the test VM (Windows 11 Arm, 4 vCPU, x86_64 programs under emulation, the toolchain without rustup) on the Mac mini; the host's
  load was 20 to 33 and its CPU idle 0 to 30% during the runs (other agents and a CI run), so the absolute times are noisy. Not the CI
  runner (ER-9: a different machine counts as a comparison with the base on the same machine, not as a result for CI).
- **Method:** `scripts/member-gate-windows.sh <tree> "nextest tools/paddock-dev paddock-dev"`, one run per tree, the same VM image.
- **Numbers (merge_check tests are the ones run 8161 slowed, 26.9 s median against 4.54 s in run 8138):**

  | Tree | tests | failed (distinct) | suite wall | merge_check median | merge_check total | max |
  |---|---:|---:|---:|---:|---:|---:|
  | train 13 head 3f6428b89 (base) | 1511 | 6 | 987 s | 3.36 s | 230 s | 16.7 s |
  | v2 before the hardening and stand-in picks (888907c4f) | 1523 | 20 | 1138 s | 3.92 s | 274 s | 19.0 s |
  | v2 final (da4ef6eda) | 1546 | 4 | 843 s | 3.20 s | 238 s | 17.1 s |

- **Result:** the re-landed TestCommand costs merge_check no measurable time on the VM (3.20 s against 3.36 s median), and run 8161's
  slowdown is **still not reproduced and its cause is still not found**. The failures of v2 final (`build_jobs_golden` probe,
  `local_sccache` twice, `safe_remove` parity) are a subset of the base's 6 on the same VM (Unix paths in expectations, the memory
  probe). The first v2 run's 20 failures were the toolchain lookups that `85c21b7d6` fixes.
- **Not measured:** the CI Windows runner with this tree (the train's next Windows run); a repeat run for variance (one run per tree).


## TestCommand on the Windows CI runner: what one start costs (2026-11-18, exp/testcommand-runner-ab, ADR 0107)

Question: why ci.yml run 8245 (job 41212) ran merge_check 19 times slower on windows-latest once its fixture started git through
`TestCommand`, when the test VM never showed it (the two entries above).

- **Machine:** the Windows CI runner itself (windows-latest), ci run 8246, job 41213 (`testcommand-ab.yml` on
  `trial/exp/testcommand-runner-ab` = `2ccf8dc00`, train 14 `5484f36eb` plus `a80479dd7`), set up as the crate job (job isolation,
  kept clone, the kept build folder `target-paddock-crate-s1`), the program copied into the profile's `deps` folder; no other run
  on the runner. Mac rows: the Mac mini, 1-minute load 21 to 34.
- **Command:** `measure_test_command --ab 20`: the merge_check fixture's four git starts (`init`, `add`, `commit`, `rev-parse`) in
  a new repository per sequence, 20 rounds, each round every variant once in an order rotated per round; `--first` ten times.
- **Numbers (ms per sequence of four starts, runner):** plain 190.7 median / 212.1 p90; plain resolved 185.9 / 199.7; plain with
  `CREATE_NO_WINDOW` 240.6 / 284.1; plain with TestCommand's cleared environment 180.3 / 201.4; that without the ceiling 185.6 /
  208.8, with the machine's PATH 188.8 / 204.0, with the machine's folders 181.8 / 207.4; plain with the ceiling alone 191.3 /
  220.1; `TestCommand::new` per start 485.3 / 571.8; `new_in` one home 443.1 / 470.2; `TestCommand` without the flag 431.3 /
  452.3; `TestCommand` with the inherited environment 482.6 / 524.5; four builds and drops, nothing started, 219.7 / 234.1. First
  `TestCommand::new` of a process: 169 to 217 ms (10 processes).
- **Finding:** the child's PATH began with a link folder in the command's own private folder (`...\paddock-test-cmd-*\toolchain\...`),
  and no "cannot make the toolchain link folder" line: `ambient::link_root()` was `None`, so the kept target folder (made by
  `./dev ci isolation path target` before cargo ran) has no `CACHEDIR.TAG`, and every command links the toolchain itself.
- **Mac, both ways (10 rounds):** from a copied `debug/deps` folder without `CACHEDIR.TAG`: private link folder, four builds 14.8
  ms; with the tag: the shared `paddock-test-toolchain`, 3.9 ms. Mac, 20 rounds, tagged target: plain 80.6, cleared environment
  109.3, `TestCommand::new` 103.2, `TestCommand` with the inherited environment 71.7, four builds 5.6 (noisy at load 21).
- **Not measured:** the runner with the tag present (ADR 0107 WP-2's rerun), and the runner under the real 8-test load (the
  link-limit copy fallback).

## TestCommand on the Windows CI runner with CACHEDIR.TAG present (2026-11-18, fix/ci-target-cachedir-tag, ADR 0107 WP-2)

Question: does option A of ADR 0107 (`./dev ci isolation path target` writes cargo's `CACHEDIR.TAG`) remove the builder's
per-command cost measured in run 8246 (the block above)?

- **Machine and command:** as in run 8246: `testcommand-ab.yml`, ci run 8247, job 41214, on `trial/exp/testcommand-runner-ab` =
  `c114b9adc` (`exp/testcommand-runner-ab` `a80479dd7` merged onto `origin/fix/ci-target-cachedir-tag` = `f09a919a9`); `./dev` built
  from the merged tree in the job ("paddock-dev 07c485db7... is not published ...; building it", "built ... in 59 s (not
  downloaded)"); no other run on the runner; `measure_test_command --ab 20` and `--first` ten times.
- **The fault is gone:** the child's PATH now starts with the shared
  `C:\Users\***\.paddock-ci\target-paddock-crate-s1\paddock-test-toolchain\0741e9b7bbf772ab` (run 8246: a folder inside each
  command's own private folder).
- **Numbers (ms per sequence of four git starts; run 8247 against run 8246):**

  | Variant | 8247 median / p90 | 8247 / plain | 8246 median / p90 | 8246 / plain |
  |---|---:|---:|---:|---:|
  | plain | 243.4 / 270.5 | 1.00 | 190.7 / 212.1 | 1.00 |
  | plain, resolved path | 215.5 / 255.0 | 0.89 | 185.9 / 199.7 | 0.98 |
  | plain + `CREATE_NO_WINDOW` | 274.5 / 340.4 | 1.13 | 240.6 / 284.1 | 1.26 |
  | plain + cleared environment | 231.1 / 271.6 | 0.95 | 180.3 / 201.4 | 0.95 |
  | cleared, no ceiling / machine PATH / machine folders | 245.8 / 244.4 / 240.1 | 0.99 to 1.01 | 185.6 / 188.8 / 181.8 | 0.95 to 0.99 |
  | plain + ceiling alone | 217.5 / 264.9 | 0.89 | 191.3 / 220.1 | 1.00 |
  | `TestCommand::new` per start | 312.0 / 402.4 | **1.28** | 485.3 / 571.8 | 2.55 |
  | `new_in`, one home per sequence | 309.5 / 341.8 | 1.27 | 443.1 / 470.2 | 2.32 |
  | TestCommand, flag off | 270.7 / 297.1 | 1.11 | 431.3 / 452.3 | 2.26 |
  | TestCommand, inherited environment | 357.6 / 377.5 | 1.47 | 482.6 / 524.5 | 2.53 |
  | four builds and drops, nothing started | **23.7** / 25.3 | 0.10 | 219.7 / 234.1 | 1.15 |

  First `TestCommand::new` of a process: 150 to 167 ms (run 8246: 169 to 217 ms); drop 1.2 to 1.5 ms (5 to 10 ms).
- **Result against ADR 0107's bar:** `TestCommand::new` per start 1.28 times plain (bar: within 1.5): met. Four builds 23.7 ms
  (bar: under 20 ms; the bar was an estimate, not a measurement): missed by 3.7 ms; the builder fell from 55 to 5.9 ms per
  command (9.3 times less), the rest being the per-command private folder, its `.gitconfig` and its removal. The runner was
  slower overall in this run (plain 243 against 191 ms). One run of 20 rounds after the fix, one before (ER-28: fault present in
  8246, removed in 8247, both on the same runner and kept folder).
- **Not measured:** the paddock-dev suite under the real 8-test load with the fix (the train's next Windows run).

## Train 14's Windows slowdown: host or code, and the merge check's repeated git reads (2026-11-18, fix/windows-testcommand-slow, perf/git-heavy-tests)

Question: why did paddock-dev's git-heavy tests run 5 to 10 times slower on windows-latest in train 14's runs 8245 and 8248 than in
main's run 8241 (`pre_push::tests::known_commit::a_delete_only_push_and_an_explicit_base_never_use_evidence` 5.3 s, then 56 s, then a
timeout on both tries), and is it the host or the code?

- **Machine:** the Windows CI runner itself (windows-latest), one job per trial of `windows-slow-ab.yml` (branch
  `exp/windows-slow-ab`, never merged) set up as ci.yml's paddock-dev job (job isolation, kept clone, the paddock-dev job's kept build
  folder), nothing else on the runner (`./dev ci dispatch` waited for it). In the same job: host probes (Dyno's `dyno-bench cpu` and
  `dyno-bench disk --disk-mib 256` in the job's work folder, 200 `git --version`, 50 new repositories with `init`, `add`, `commit`)
  right before and right after paddock-dev's tests. Debug test build, as CI's. Mac: the Mac mini, 1-minute load 4 to 11. Linux: the
  local CI-image container (`tools/linux-container.sh --ci-image`, arm64), an indication only (ER-9).
- **Trees:** main `f5f08f71e`, train 14 `842ef2d1a`, the fix `48fa2449a` (on the train), the fix plus the memo `54759b464`.
- **Cause, seen directly:** the train's new push-gate row `gates-register` (6120c49cf, C100 on every push) starts `Facts::dev_exe`,
  which was `current_exe()`. In a unit test that is the test harness, which reads `gates check` as two test-name filters and runs every
  test named `gates` or `check` again, at full parallelism, inside the test. On the Mac two such children (`<test binary> gates check`)
  run during the test; it took 0.40 s on main and 14.2 to 17.5 s on the train.

| Windows runner, 8 test threads, `pre_push` and `merge_check` | main (8251) | train (8252) | fix (8253) |
|---|---:|---:|---:|
| `known_commit::a_delete_only_push...` | 3.1 s | **48.2 s** | 3.8 s |
| `merge_check::a_clean_merge_is_clean` | 4.1 s | 7.1 s | 7.2 s |
| `merge_check::a_hunk_with_most_of_its_lines_kept...` | 10.7 s | 25.2 s | 17.4 s |
| merge_check tests both trees have (45), total | 165.8 s | 361.6 s | 310.8 s |
| tests run / wall | 169 / 38 s | 239 / 115 s | 240 / 94 s |
| host before / after: 200 `git --version` (ms each) | 42.2 / 47.1 | 37.3 / 47.6 | 38.4 / 38.8 |
| host: 50 x init+add+commit (ms each) | 345.5 / 220.6 | 289.5 / 227.0 | 285.8 / 300.2 |
| host: dyno-bench cpu single / multi | 343-368 / 6429-6467 | 490-491 / 6700-6744 | 373-494 / 6495-6698 |
| host: dyno-bench disk 1 MiB read / write MB/s | 781-849 / 353-1075 | 730-772 / 999-1022 | 688-785 / 1066-1102 |

| Windows runner, merge_check (it) only, 1 test thread | main (8254) | fix (8255) | fix + memo (8257) |
|---|---:|---:|---:|
| `merge_check::a_clean_merge_is_clean` | 3.8 s | 4.1 s | 4.7 s |
| `merge_check::a_hunk_with_most_of_its_lines_kept...` | 8.0 s | 13.7 s | 10.5 s |
| the 45 tests both trees have, total | 137.9 s | 178.5 s (1.32 times, median per test) | 176.8 s (1.31 times) |
| all merge_check tests of the tree | 45: 137.9 s | 84: 340.2 s | 84: 340.7 s |
| host: 200 `git --version` (ms each) | 43.2 / 47.6 | 40.5 / 45.3 | 47.7 / 47.4 |
| host: 50 x init+add+commit (ms each) | 291.5 / 295.2 | 316.2 / 295.1 | 265.3 / 284.8 |

- **Host or code:** code. The host probes are the same on every tree within the run-to-run spread (git start 37 to 48 ms, a small
  repository 220 to 345 ms, CPU and disk the same or better on the slow tree), while the train's test is 15 times main's in the
  same minutes and back to main's with the fix. The earlier slow runs did not meet the VM's nightly Proxmox backup either (it started
  about 16:52 UTC on 2026-11-10): paddock-dev on Windows ran 22:47-23:02 UTC (8152), 02:37-02:52 (8161), 01:47-01:55 (8241, green),
  03:17-03:52 (8245), 06:21-06:29 (8248); the trials ran 10:01-11:15 UTC. In run 8245 the paddock-dev unit tests outside `pre_push`
  that took over 0.5 s on main kept a median of 1.1 times main's, while the integration tests that start many children took 3 times.
- **The rest of the gap (serial, fix against main, 1.32 times):** each test of the train's tree does more: its fixture starts git
  through `TestCommand` (ADR 0100; `CREATE_NO_WINDOW` and the builder, about 17 ms of a 60 ms start, ADR 0107's run 8247), and
  `./dev merge check` starts git more often (the train's stale-base, reverted-base and re-land detectors: 80 against 64 git starts for
  `a_clean_merge_is_clean`, 211 against 179 for `a_hunk_with_most...`, counted on the Mac with a logging `git` first on `PATH`). Of a
  merge check's 33 starts for a two-commit merge, 7 were the same read of the same object ids asked again by another detector.
- **The memo (perf/git-heavy-tests, a06109c92):** the merge check's git reader answers a read whose revisions are all full object ids
  (ls-tree, diff, merge-base, merge-tree, log, cat-file) or `rev-parse --is-shallow-repository` from memory after its first run.
  Mac, merge_check and pre_push tests at 8 threads, runs 2 and 3 of 3 (run 1 of each was a cold cache): merge_check total 89.6 and
  90.5 s before, 80.5 and 81.3 s after (median per test 0.71 to 0.63 s). Linux container, merge_check, 2 runs each: total 18.9 and
  18.0 s before, 17.6 and 38.3 s after (the second under another agent's build; an indication only). Windows, serial, one run
  each: 340.2 s before (8255), 340.7 s after (8257), with the host's git start at 40.5 to 45.3 ms in the first and 47.4 to 47.7 ms
  in the second: **no gain measurable on the Windows runner** with one run per tree; the memo stays because it is correct and
  cheap, not for a measured Windows gain. (Trials 8254, 8255 and 8257 report failure only because the trial's summary step
  stopped on a test name the filter left out; all their tests passed.)
- **Not measured:** a Defender exclusion for the runner's work folder (host configuration, the user's decision); git starts are 38 to
  48 ms there against 5.6 ms on the Mac (`git --version`), and a small repository 220 to 345 ms against 45 ms.

## Doctest row of the push gate (2026-11-18, feat/member-gate-doctests)

Question: what does the `doctest <workspace>` row (`cargo test --doc`; nextest does not run doctests) cost? In `crates` it runs on
every push for the touched crates and the crates in `crates/` that depend on them (`-p`, from `cargo metadata`); in an app workspace it runs `--workspace`, only in the member gate (`--member-gate`).

- **Machine:** the shared Mac mini (arm64), other agents running: 1-minute load 29 to 35 during the runs (not an idle machine);
  `CARGO_BUILD_JOBS=2`, `RUSTUP_AUTO_INSTALL=0`, one run per row, build folders in a scratch folder (not the worktree's)
- **Commands:** `cargo test --doc --locked -p paddock-diag` in `crates` (one crate, one doctest); `cargo test --doc --locked --workspace`
  in `apps/dyno`. The script and logs were in a scratch folder and are not kept.

  | What | Where | Seconds |
  |---|---|---:|
  | typical push: one crate with `-p`, first run (builds that crate's test-profile dependencies) | crates | 51 |
  | typical push: one crate with `-p`, warm build folder | crates | 3 |
  | member gate: one app workspace, cold build folder | apps/dyno | 1122 |
  | member gate: one app workspace, warm build folder | apps/dyno | 13 |

The cold app figure is a new empty build folder under load 30 or more, and is why the app row is member-gate only: the gate's kept
build folder per workspace (`kept_targets`) makes the warm figure the usual one. Not measured: the app row on the CI runner.
The crates row already caught the bug it was made for: on the train base, `-p paddock-testkit` fails with E0308 until
fix/testkit-doctest lands.

### Reverse dependencies and `--all-features` (2026-11-18, review of 1be287647)

The row now passes `--all-features` in `crates` (CI's own doc-test step does) and adds the dependents of the touched crates, so a public API
change that breaks a dependent's doctest is caught. Measured in `crates` of the fix branch on 1be287647, same machine and settings as
above (load 13 to 28), one build folder, `cargo test --doc --all-features --locked --no-fail-fast`, 57 members:

| Selection | Test programs | Seconds |
|---|---:|---:|
| `--workspace`, cold build folder | 63 | 289 |
| `--workspace`, warm, twice in a row | 63 | 33, 30 (and 31, 30 on later repeats) |
| one leaf crate (`-p paddock-diag`), warm | 1 | 1 to 2 |
| four dependents of `paddock-log` (`-p`), warm | 4 | 3 |
| `paddock-proc` and its 31 dependents (the largest realistic set below the root crates), warm | 32 | 12 |
| the same, first run after a different selection (feature sets differ, cargo rebuilds) | 32 | 61 |

A `-p` selection rebuilds once when the previous run used another selection (features unify per selection), then runs in seconds; a warm
`--workspace` takes about 30 s every time. The dependents row therefore costs at most about as much as `--workspace` and usually far less,
so it is kept (the root crates `paddock-env`, `paddock-limits` and the workspace hack reach nearly all 57 and cost what `--workspace` does).
Not measured: the CI runner; load was 13 to 28, so absolute values are noisy.

## Source audits in the member gate (2026-11-18, fix/push-gate-queue-time)

Question: what do the source audits of `tools/paddock-dev` (`tool_cache::tests::no_source_file_reads_its_own_path_except_through_self_exe`,
`git_env::tests::no_source_file_starts_git_without_the_builder`) add to the `member-audits` row?

- **Machine:** the shared Mac mini (arm64), other agents running (15-minute load 16 at the time); `CARGO_BUILD_JOBS=2`
- **Command:** `cargo nextest run -E 'test(/no_source_file_/)'` in `tools/paddock-dev`, warm build folder, 1 run
- **Result:** 2 tests, 0.17 s of nextest time; the row's compile is the one the `it` audits already need (same test binaries).

## Load-aware heavy slots (2026-11-18, feat/heavy-load-aware-slots)

Question: with the slot count following the load, how long does a queue of 4 clippy-sized jobs take, against the single slot?

- **Job:** a stand-in for a clippy run: two CPU-bound processes for 15 s (`perl` busy loops), 4 jobs started together through
  `heavy run --no-gate` in a private `HEAVY_DIR` (the real queue was not touched), the debug build of `tools/heavy`, 1 run per row.
- **Machine:** the shared 14-core Mac mini, other agents running. The load was NOT low (a build storm had run: 1-minute load 137
  twenty minutes earlier), so the 15-minute load was 44 to 48 during every row: the rule must stay at 1 here, and it did.

  | Row | Setting | 15-min load | Wall time of 4 jobs |
  |---|---|---:|---:|
  | before | `HEAVY_SLOTS=1` (the old default) | 48 | 61 s |
  | before, again | `HEAVY_SLOTS=1` | 41 | 60 s |
  | after, real state | `HEAVY_SLOTS=auto` | 44 | 60 s (held at 1: the 15-minute load fills the cores) |
  | after, a machine with room | `HEAVY_SLOTS=auto HEAVY_CPUS=64` (the cores are an override, load and idle are real) | 44 | 17 s (4 together; the idle share was above 30 %) |

- **Not measured:** the rule at a really low 15-minute load on this machine (it was not low at any point of this run); the row with
  `HEAVY_CPUS=64` stands in for it and shows the mechanism, not the machine. The deterministic cases (two jobs together at low load,
  serialised at high load, busy CPU, unknown load, container) are tests in `tools/heavy/tests/it/slots.rs`.
- **Rule after review:** the rule also closes on a 1-minute load at the core count (the fast brake) and primes the CPU reading when heavy starts;
  neither changes these rows (the 15-minute load decided them).
- **Cost to note:** the 15-minute load lags, so after a spike the count stays at 1 for up to a quarter of an hour; the 1-minute
  load had fallen to 7 while the 15-minute was still 44 (second `after` row's neighbours).

## Batched git reads: per-call git against one cat-file process against gix, and the Defender exclusions (2026-11-18, perf/git-heavy-tests-2, ADR 0108 G1 and G2)

Question: how much does one long-lived `git cat-file` process save over one git start per read, and does reading in process with gix
save more, on each OS? And did the user's Defender process exclusions for git change the Windows runner's numbers?

- **Machines:** the Windows CI runner (windows-latest, `windows-host-probe.yml` on `exp/windows-host-probe`, never merged, nothing else
  on the runner); the Mac mini (load 8.5 to 10.7, an indication); the local Linux CI-image container (arm64, an indication, ER-9).
- **Spike:** `tools/git-read-spike` on that branch (release build): a synthetic repository made by one `git fast-import` stream (2,000
  or 12,000 files in 40 folders, 20 commits), then the same reads three ways, each 3 times in rotated order with the results compared:
  every commit's tree listed recursively (`git ls-tree -r` per commit) plus 300 blobs (`git cat-file blob` per blob); the same through
  one `git cat-file --batch` process with trees parsed in process (paddock-git's `Reader` does the same over `--batch-command`); the
  same with gix 0.90 in process.

| ms, median of 3 | Windows runner (8267) | Mac | Linux container |
|---|---:|---:|---:|
| 2,000 files: per call | 11,145 | 2,418 (1,806 min) | 159 |
| 2,000 files: one cat-file | 428 | 508 (83 min) | 34 |
| 2,000 files: gix | 9.4 | 14.2 | 8.0 |
| 12,000 files: per call | 11,950 | 1,776 | 233 |
| 12,000 files: one cat-file | 685 | 231 | 145 |
| 12,000 files: gix | 39.4 | 28.8 | 29.8 |

- **Host probe (`./dev ci host-probe`, medians, ms):** on the Windows runner an empty program (`cmd /d /c exit 0`) takes 28 to 45 ms to
  start, `git --version` 32 to 42 ms, a one-commit repository (4 git starts) 229 to 325 ms, a 4 KiB file 1.0 to 1.6 ms. On the Mac:
  1.7, 5.2, 37.0 and 0.2 ms. So on Windows the cost is the OS's process start itself, not git: git adds 3 to 8 ms to an empty start.
- **Defender process exclusions for git (applied by the user on 2026-11-18, before run 8265; the "before" figures are runs 8251 to
  8257 of the same day's morning and run 8263, whose timing relative to the change is not known):** git start 32 to 41 ms after
  (8265) against 37 to 48 ms (8251 to 8257) and 33 to 42 ms (8263); a repository 238 to 306 ms against 220 to 346 and 239 to 325 ms;
  empty start 28 to 39 ms against 30 to 45 ms (8263). merge_check serial, 84 tests on the same tree: 289.5 s (8265) against 353.5 s
  (8263); 340.2 s on the fix tree (8255, morning). **No change in any host-probe phase beyond the run-to-run spread**; the one serial
  merge_check run was 18% faster, which one run per side cannot separate from the spread. The fixtures' repositories and the test
  programs were already in excluded folders (`C:\gitea-runner\work\<job>\tmp`, `C:\Users\<user>\.paddock-ci`).
- **Not measured:** gix against a partial clone (`--filter=blob:none`, as the CI kept clones are): gix does not fetch a missing blob
  from the promisor remote, git does.

## The shared reader moved in: git starts per merge check, gix's build cost, and the windows-sys edges (2026-11-18, perf/git-heavy-tests-2, ADR 0108 G2 and TD-1, TD-3 to TD-6, TD-8, TD-10)

- **Git starts of `./dev merge check`** (counted on the Mac with a logging `git` first on `PATH`, the starts whose parent is
  paddock-dev): `merge_check::a_clean_merge_is_clean` 67 before the memo (a06109c92), 43 now; `a_hunk_with_most_of_its_lines_kept...`
  151, 97 now. The rest are `diff -U0`, `log`, `merge-base`, `merge-tree` and `rev-parse` of ref names, which the reader does not
  answer.
- **`pr plan-status` (TD-10):** 61 references (20 on main, 10 off it, 30 short ids, one missing) cost at most 2 git starts (the
  base's `rev-list`; short ids through the reader's git), against up to 3 per reference before (`repo_git_tests`).
- **`GitHistory::file_at` (TD-1):** 121 reads of 60 files at two revisions start the reader 0 times (gix in process) plus one
  `rev-parse` per revision (`file_at_for_many_files_starts_no_git_per_file`); before, one `git show` per read.
- **Cold debug build, paddock-proc alone against paddock-git with gix, `CARGO_BUILD_JOBS=2`, no sccache:** Mac 44 s against 37 s at a
  load of 32 to 80 (other agents' builds): **not measured** (the order and the load decided it). Windows runner: not measured yet.
- **Windows runner, the whole paddock-dev suite at 8 test threads** (`windows-host-probe.yml`, nothing else on the runner; host probes
  the same in all three: empty start 29 to 41 ms, git start 33 to 44 ms, a small repository 238 to 323 ms):

  | | 8277: train 14 + memo | 8280: + the gix reader (f4b085304) | 8281: + one test per case (381137e33) |
  |---|---:|---:|---:|
  | `merge_check::the_remerge_subject_...` | 63.6 s timeout, then 45.2 s (FLAKY) | 60.0 s timeout, then 44.4 s (FLAKY) | 4 tests, 15.1 to 19.2 s |
  | `merge_check::generated_files_are_skipped_...` | 45.1 s | 49.3 s | 5 tests, 12.0 to 14.9 s |
  | `merge_check::a_clean_merge_is_clean` | 6.7 s | 3.9 s | 4.0 s |
  | merge_check tests: n, median, max | 86, 8.8 s, 45.2 s | 86, 7.3 s, 49.3 s | 92, 8.8 s, 27.3 s |

  The slow tests were fixture-bound (116 of the remerge test's 188 git starts were its own fixture): the reader cut the merge check's
  part, the split cut each test to one case (docs/lessons.md, "One test per fixture case").

## Push gate run time: what the old 15-minute budget cut, and the per-row cap (WP-1) (2026-11-19, feat/gate-budget-per-workspace)

- **Question:** the push gate and the member gate had one fixed 15-minute run budget ([`pre_push::exec`]). A change that reaches every
  Cargo workspace (a lockfile or workspace-hack change) cannot fit it, even one row at a time: a cold doctest build costs minutes per
  workspace. Train 15's re-merge of train 14 left clippy for `tools/wiki` and the doctests of 10 workspaces "not measured". The brief counts
  four such cases; only this one is recorded with its run, the earlier ones are not found in the logs or lessons (the research note, section 0).
- **Source:** the push gate's own logs under `<git common dir>/paddock-pre-push/` of the main checkout, read on 2026-11-19:
  `queue-times.log` (one line per push run, 2,042 runs: `heavy_steps`, `queued_s`, `ran_s`) and `logs/<run>/<row>.log` (the last
  `heavy: <step> timing queued Q s, ran R s` line of each retained row, 56 rows). Machine: the shared Mac mini, other agents running.
- **Result, runs:** the run time of a push (`ran_s`, summed over its heavy steps): 8 of 2,042 ended at the 900 s limit, 52 ran 600 s or
  more, the mean is 38.5 s. The limit is the maximum seen (900.8 s), so the real time of the longest runs was never measured.
- **Result, rows (run seconds, warm and cold mixed, retained logs only):**

  | Row kind | Runs | Typical (warm) | Cold runs seen |
  |---|---|---|---|
  | `clippy <ws>` | 24 | 0.2 to 93.1 s (median about 4 s) | `apps/spotter` 258.4 s, `apps/pitwall` 256.0 s |
  | `clippy-windows <ws>` | 6 | 4.9 to 78.1 s | `apps/dyno` 410.8 s, `apps/pitwall` 354.8 s |
  | `doctest <ws>` | 5 | 1.1 to 32.3 s | `apps/debrief` 672.1 s, `apps/dyno` 227.1 s |
  | `clippy-linux <ws>` | 17 | 2.6 to 125.3 s | (kept container volume; `--warm-linux`) |
  | `conformance-tests` | 4 | 78.5 to 104.3 s | |
  | `member-audits` | 3 | 57.0 to 157.8 s | |

- **Reading:** a cold build costs 10 to 100 times a warm one and which one a run meets is not known in advance, so any total over the
  rows, and any per-row figure scaled by workspace count, is a second fixed limit with the same failure. heavy's activity log (the
  research note's section 0) shows the budget cut 87 steps in five days at a median of 33 s of run: they got what earlier rows had
  left of the total.
- **Decision (WP-1, option A of docs/research/2026-11-19-gate-hang-detection.md):** the 15-minute total (`BUDGET`) only stops new rows
  from starting; a started row runs until it ends or reaches `ROW_CAP`, 60 min (GitLab's default job timeout; the slowest healthy row
  known is a cold app doctest, 1122 s, so the cap is over three times it). The cap is enforced by the executor's monitor, which writes
  the process list and a stack sample of the tree (`sample` on macOS, `eu-stack` on Linux when installed, the process list on
  Windows) to `<row>.stacks.txt` beside the row's log before it ends the tree through `paddock_proc` (pid plus start time). The stall
  judgement (WP-2) and history limits plug into the per-step watchdog (`pre_push/watchdog.rs`).

## Push gate stall gap (WP-0) (2026-11-19, feat/gate-budget-per-workspace)

- **Question:** the progress watchdog (docs/research/2026-11-19-gate-hang-detection.md, WP-2) calls a step stuck after a gap with no
  finished unit. How long is the longest gap between two finished units that a healthy cold step shows, per workspace?
- **Method:** the gate's own cargo commands (`cargo clippy --workspace --all-targets --locked`, `cargo test --doc --workspace --locked`, and
  `cargo xwin clippy --target x86_64-pc-windows-msvc` for crates, apps/dyno and apps/pitwall; `--all-features` in `crates`), each with
  `--message-format=json-render-diagnostics`, one step at a time through `heavy` with `CARGO_BUILD_JOBS=2`, in a fresh build folder
  (`--config build.target-dir`, on the dev volume, removed afterwards), no sccache (`RUSTC_WRAPPER` unset: the worst case, every
  compile is real). A wrapper stamped every output line with a monotonic clock and counted what the watchdog counts: `compiler-artifact`,
  `build-script-executed`, and for doctests the `Doc-tests` and `test ... ok/FAILED` lines; the gap is the time between two counted
  events, from the start to the first and from the last to the end included. `crates` was also run warm right after the cold run.
- **Machine:** the shared Mac mini (arm64, 14 cores), other agents' pushes and trains running (15-minute load 8 to 10 at the end of the
  run, 25 cold steps over about five hours), at the heavy slot count the load allowed. Not measured: a heavier load than this, a
  second cold run of the same workspace (so run-to-run spread within a workspace is unknown), Windows and Linux hosts, the Linux
  container row (output lines only; the container's CPU is not visible).
- **Raw figures** (all runs, with the commands and the fields, in `docs/benchmarks/2026-11-19-gate-stall-gap-wp0.json`; the stamping script is
  a 30-line wrapper that timestamps each output line):

| Workspace | Step | Run | Seconds | Progress events | Longest gap, s (at s) | p99 gap, s |
|---|---|---|---:|---:|---:|---:|
| `apps/debrief` | clippy | cold | 375 | 1085 | 11.2 (331) | 3.5 |
| `apps/debrief` | doctest | cold | 684 | 939 | 35.1 (533) | 8.1 |
| `apps/dyno` | clippy | cold | 288 | 833 | 12.4 (5) | 3.9 |
| `apps/dyno` | doctest | cold | 505 | 680 | 22.2 (480) | 8.9 |
| `apps/dyno` | xwin | cold | 335 | 865 | 21.8 (118) | 4.5 |
| `apps/pitcrew` | clippy | cold | 223 | 631 | 10.5 (133) | 3.3 |
| `apps/pitcrew` | doctest | cold | 314 | 537 | 9.8 (146) | 5.9 |
| `apps/pitwall` | clippy | cold | 250 | 691 | 10.5 (133) | 4.3 |
| `apps/pitwall` | doctest | cold | 861 | 597 | 43.1 (261) | 15.8 |
| `apps/pitwall` | xwin | cold | 278 | 699 | 14.2 (3) | 4.2 |
| `apps/shell-demo/src-tauri` | clippy | cold | 93 | 604 | 5.1 (28) | 0.7 |
| `apps/shell-demo/src-tauri` | doctest | cold | 124 | 512 | 8.9 (115) | 1.6 |
| `apps/spotter` | clippy | cold | 260 | 669 | 13.0 (189) | 5.2 |
| `apps/spotter` | doctest | cold | 363 | 584 | 12.2 (64) | 6.9 |
| `apps/workbench-demo/src-tauri` | clippy | cold | 81 | 573 | 4.8 (30) | 0.6 |
| `apps/workbench-demo/src-tauri` | doctest | cold | 141 | 484 | 8.7 (132) | 2.2 |
| `crates` | clippy | cold | 112 | 857 | 1.1 (4) | 0.7 |
| `crates` | clippy | warm | 1 | 857 | 1.0 (0) | 0.0 |
| `crates` | doctest | cold | 199 | 746 | 7.9 (157) | 1.7 |
| `crates` | doctest | warm | 28 | 746 | 4.2 (0) | 0.8 |
| `crates` | xwin | cold | 122 | 871 | 1.7 (26) | 0.8 |
| `crates` | xwin | warm | 2 | 871 | 1.5 (0) | 0.0 |
| `tools/heavy` | clippy | cold | 27 | 193 | 1.1 (2) | 0.9 |
| `tools/heavy` | doctest | cold | 40 | 178 | 3.1 (24) | 2.2 |
| `tools/paddock-dev` | clippy | cold | 52 | 250 | 11.6 (3) | 1.2 |
| `tools/paddock-dev` | doctest | cold | 79 | 232 | 11.9 (3) | 4.8 |
| `tools/wiki` | clippy | cold | 27 | 197 | 1.2 (2) | 0.7 |
| `tools/wiki` | doctest | cold | 36 | 182 | 4.1 (21) | 1.7 |

- **Result:** the longest gap in any cold run is **43.1 s** (`cargo test --doc` of apps/pitwall, at 261 s of 861 s), then 35.1 s
  (doctests of apps/debrief) and 22.2 s (doctests of apps/dyno); clippy's longest gap is 13.0 s (apps/spotter); the median of the
  25 cold runs' longest gaps is 10.5 s, so the longest is 4.1 times the median. Warm runs replay every unit at once: 1.0 to 4.2 s.
  Every step passed (exit 0).
- **Set from it** (constants in `pre_push/stall.rs` and `pre_push/history.rs`, figures next to them): the fallback gap `GAP` is 5 min
  (7 times the longest gap seen, replacing the 15 minutes that was only a guess); `K` stays 4 (the max-to-median ratio of 4.1);
  `FLOOR` stays 120 s (2.8 times the longest gap). One cold run per workspace is a small sample: the watchdog stays report-only
  (`PADDOCK_GATE_WATCHDOG=enforce` to kill) until a week of `step-history.jsonl` records of real pushes agrees (ER-12, ER-41).

## The sccache shim named `sccache`: native C stays cached (2026-11-19, fix/gate-tooling-env-select)

- **Question:** a wrapper with another file name (`rustc-wrapper`) kept cc-rs from wrapping `clang-cl`, but also from wrapping every native C compile. The shim named `sccache` (`local_sccache::shim_text`) keeps native C cached and sends only the Windows C compiles past sccache. What does that cost or save on a cold build?
- **Method:** `cargo check --locked` of `apps/spotter/app/src-tauri` in a fresh build folder each time, `CARGO_BUILD_JOBS=2`, sccache 0.19.0 with the shared local cache (`--zero-stats` before each run), through `./dev heavy`, one run per row, macOS arm64, 1-minute load 9 to 12 from other agents. The first row filled the cache; the other two ran after it, so rustc hits are the same and only C differs. Script and logs: `<dev-targets>/gtf-bench/` (scratch; not kept).

  | Row | `RUSTC_WRAPPER` | Wall time | C/C++ cache hits | Rust hits / misses |
  | --- | --- | --- | --- | --- |
  | warm-up (cache fill) | the shim | 116 s | 270 | 241 / 234 |
  | after | the shim | 95 s | 270 | 311 / 164 |
  | before (the first version) | `rustc-wrapper`, no `sccache` name | 112 s | 0 (no C compile went through sccache) | 311 / 164 |

- **Result:** the shim gives 270 C/C++ hits (libsqlite3-sys, zstd-sys, ring and the like) where the first version gave none, and the run took 95 s against 112 s. **Not measured:** a repeat (one run per row, so the 17 s difference is inside this machine's noise at that load; the hit counts are not), and a Linux or Windows host. A `cargo xwin` build is as uncached for C as before; ring's C is a few seconds.

## CI's sccache on RustFS: the `s3` backend on the three runner classes (2026-11-19, perf/sccache-hit-rate, docs/research/2026-11-19-sccache.md WP-8)

- **Question:** does each CI runner class reach the user's RustFS S3 store, write to it and read from it through `./dev ci setup-rust --sccache-backend s3` (sccache's chain `disk,s3`, the local disk first), and what does it save?
- **Method:** the workflow `sccache-s3-trial.yml` of the branch `diag/sccache-s3-trial` (not for landing), merged onto `origin/train/2026-11-19-16` (7af38a878) by `./dev ci dispatch --trial`, on the real runners: the Mac mini runners (macOS arm64), the Linux job in the `paddock-ci:v3` container and the Windows runner (x86_64). Each job: `cargo clippy --all-targets --locked` and `cargo nextest run --profile ci --locked --no-run` in `tools/paddock-dev`, sccache 0.19.0, per-level numbers from `sccache --show-stats`. From run 8312 on, the local disk was emptied before the server started and the build folder was new (its name has the run id), so every compile went through sccache and every hit came from S3 (L1). One run per row; the three runner classes ran at the same time, each on its own machine or slot. The S3 key was never printed.
- **Reach:** the unauthenticated health check `GET /minio/health/live` answered 200 on every runner: Mac 4 to 7 ms connect, 10 to 119 ms total; Linux 0.3 to 0.5 ms connect, 0.8 to 1.2 ms total; Windows 3 to 23 ms connect, 5 to 24 ms total.

  | Run | What | Mac | Linux | Windows |
  | --- | --- | --- | --- | --- |
  | 8305 | first run, S3 empty | 4 of 319 hits (1.3%); L1 writes 312, 0 failures; clippy 37 s + test build 33 s | 163 of 337 hits (48.8%, all from the container's `/sccache` mount, L1 hits 0); L1 writes 171, 0 failures; 22 s + 24 s | 2 of 325 hits (0.6%); L1 writes 320, 0 failures; 73 s + 76 s |
  | 8306 | second run, nothing emptied | 0 compile requests (the kept build folder; cargo ran no rustc) | 334 of 337 hits, all L0; L1 hits 0; 14 s + 17 s | 0 compile requests (kept build folder) |
  | 8312 | local disk emptied, new build folder | not run: the job was refused for disk (39.7 GiB free, below the runner's 50 GiB refusal level) | 171 of 337 hits (50.7%), all L1; 18 s + 19 s | 243 of 325 hits (74.8%), all L1; 38 s + 39 s |
  | 8323 | the same as 8312, after the Mac volume was freed | 237 of 319 hits (74.3%), all L1; 0 L1 write failures; 25 s + 25 s | not read | not read |

- **Result:** all three runner classes reach RustFS, write to it (S3 writes equal local writes, 0 failures) and read from it with a cold local disk. With S3 warm and the local disk empty, clippy plus test build took 77 s on Windows against 149 s cold (48% less) and 50 s on the Mac against 70 s cold (29% less); on Linux 37 s against 46 s with 49% local hits already (there is no fully cold Linux baseline in these runs). The L1 hit rate of 50 to 75% is lower than the 99 to 100% a warm local disk gives (run 8306, Linux): the misses are probably cache keys that contain the per-run build folder path (`paddock-trial-<run id>`); that is a guess, not investigated.
- **Not measured:** a store that hangs in the middle of a job (closed by the timeout guard, measured in the next section); a large workspace (Spotter, Debrief) and a whole train's CI; a repeat of each row (one run each, so a 10% difference in seconds is inside the noise of a shared machine); the Linux and Windows rows of run 8323.

## The S3 timeout guard against a store that stops answering (2026-11-20, perf/sccache-hit-rate)

- **Question:** does the stall guard (`paddock_net::stall_guard`, `./dev ci s3-guard`) keep a compile on the local disk when the S3 store hangs, where sccache 0.19.0 alone waits 60 s per read and has no timeout for a write?
- **Measured, guard against a broken server:** the guard's tests in `crates/paddock-net` (`stall_guard/tests.rs`) run it against `paddock_testkit::net::broken_server` with a 300 ms stall and a 600 ms cooldown: a store that accepts and never answers is cut after one stall (at least 250 ms, under 5 s), the next request is refused at once and the store is not asked again during the cooldown, the store is tried again after it; a healthy store is forwarded unchanged and a slow store that keeps sending is never cut (each test has the healthy server as its positive control).
- **Measured, sccache against a store that never answers from the start (macOS arm64, sccache 0.19.0, chain `disk,s3`, dummy key, scratch folders, port 4238):** `sccache --start-server` fails after 10 s (exit 2) both straight at the black hole and through the guard (the guard cuts the startup request at its 10 s stall, the server's startup limit is also 10 s), and the compile then runs without the cache (rc 0, 1 s and 5 s). In CI that case does not arise: `ci setup-rust` checks the store with an HTTP health check first and uses the local disk alone when it fails.
- **Not measured:** the case the guard is for, a store that answers at the start and then hangs in the middle of a compile, against the real sccache. The local attempt (a fake S3 that answers PUT 200 and GET 404 and then goes silent) did not exercise the S3 level: the server reported 0 compile requests, so no number comes from it. The first measurement against the real store on the three runner classes is the trial run of this branch (the guard's log line `did not answer within` shows a cut); a real hang there is not expected, so the trial shows the guard in the path and its cost, not a cut.

## ci.yml with the S3 level and the stall guard on every Rust job: the first full run on the real runners (2026-11-20, run 8363, perf/sccache-hit-rate at 5a776327e)

- **Question:** with `sccache_backend=s3` for one run of ci.yml (the dispatch input; the repository variable stays unset), do all three runner classes use the S3 level through the stall guard, and what does a first run show?
- **Method:** `./dev ci dispatch ci.yml --ref perf/sccache-hit-rate --input sccache_backend=s3`, dispatched after train 17's round (run 8361) ended; numbers are the `sccache --show-adv-stats` of each job's stats step (jobs with 0 compile requests left out). One run; the S3 prefix of each class held what the earlier trial runs (8305 to 8323, `tools/paddock-dev` only) had written, so this is almost a cold fill.

  | Class | Jobs with compiles | Compile requests | Hits (L0 disk / L1 S3) | L1 misses | L1 writes (failures) | Cache timeouts |
  | --- | --- | --- | --- | --- | --- | --- |
  | Linux (container) | 7 | 3,720 | 560 (393 / 167) | 3,160 | 3,138 (0) | 0 |
  | macOS | 4 | 676 | 208 (0 / 208) | 468 | 465 (0) | 0 |
  | Windows | 4 | 1,655 | 10 (0 / 10) | 1,645 | 1,637 (0) | 0 |

- **Result:** the dispatch input switched the S3 level on in every job (`sccache: local disk .../s3, then S3 bucket ... (through the timeout guard on 127.0.0.1:<port>)` on all 15 jobs that compile or start the server); all three classes reached RustFS, wrote to it (5,240 writes, 0 failures) and read from it (385 L1 hits, 208 of them on macOS in the `paddock-dev` job, whose crates the earlier trials had stored). The guard cut **no** request: its logs held 185 lines `was quiet for 10 s and is closed` (idle keep-alive connections, routine) and no `did not answer`. One guard ended by its 15-minute idle limit in a job whose compiles were more than 15 minutes apart; from then on that job used its local disk alone (a connect to the guard is refused, which sccache's `l0` policy ignores). That is the measured cause for the change of the idle limit to 6 hours (`Limits::default`); the job's own end stops the guard. The hit rate of a first run is low by construction (macOS 31%, Linux 15%, Windows 0.6%); the second run of the same head is the measurement of the shared cache, and was not made (one run per head).
- **Not measured:** a second run's hit rate, the time saved on a full ci.yml run, and a store that hangs on the real runners (none did).

## sccache on RustFS as the default everywhere it is allowed (2026-11-21, feat/sccache-s3-default)

- **Question:** with `s3` as the default of `setup-rust` and of `./dev sccache env` (user decision 2026-11-21), what are the hit rates (L0 disk, L1 S3) per runner class and on the developer's Mac?
- **Not measured yet:** the first-run hit rates of ci.yml on the three runner classes and of a local `./dev sccache env` session will be added here, with their run ids, after the first full CI run on this change (docs/research/2026-11-19-sccache.md, section 8, "Not measured"). Until then the only numbers are the 2026-11-20 trial above (run 8363, a dispatch with `sccache_backend=s3`).
- **dev-tool cache (same branch):** `conformance/ci/dev-tool-cache.sh` read the wrong lines of `./dev --wrapper-hash`, so in run 8509 every Linux job logged "gave no hash" and built `./dev` cold (about 1 to 2 minutes per job, about 40 jobs a run). The minutes saved by the fix are not measured yet: the second job of the first run on this change should log `state=hit`; its time and the first job's build time will be recorded here.

## CI job cost, step by step (2026-11-20, docs/ci-job-cost-research)

Quoted by [docs/research/2026-11-20-ci-job-cost.md](research/2026-11-20-ci-job-cost.md).

- **Method.** Gitea API job and step times and the job logs of runs 8335 (`ci.yml`) and 8338 (`debrief.yml`) on e0fd0f511 at
  3 Linux lanes, WebKit on, and 8346 (`ci.yml`, WebKit off, 6 failed jobs) and 8347 (`debrief.yml`, Chromium only, 2 failed
  jobs) on 0c44bc3a3 at 6 lanes.
  Each job's log was cut at its `::group::` markers and each segment classified by step name; cargo compile is the sum of
  cargo's `Finished ... in` lines, nextest its `Summary` lines, Playwright its `N passed (T)` line and, per test, the list
  reporter's lines (3,817 tests). sccache: each job's last `--show-stats`. Limits: 8335 and 8346 are different commits;
  segment classes follow step names; per-job CPU was not sampled (**not measured**).
- **One round, 3 lanes (8335 + 8338), job-min:**

  | | Linux (46 jobs) | macOS (11) | Windows (8) |
  |---|---:|---:|---:|
  | Total | 250.5 | 56.2 | 81.7 |
  | Plan job's runner wait | 14.3 | 0 | 0 |
  | `./dev` source builds | 4.3 | 1.2 | 3.0 |
  | Container start | 1.7 | 0 | 0 |
  | Checkout | 9.8 | 3.3 | 4.5 |
  | OS setup | 2.9 | 2.5 | 8.6 |
  | npm ci, node caches | 6.3 | 1.9 | 2.7 |
  | `npm run build -w ui` | 4.9 | 0 | 0 |
  | App builds | 12.4 | 8.6 | 21.6 |
  | cargo steps | 25.6 | 21.8 | 38.1 |
  | Playwright | 140.3 | 12.4 | 1.1 |
  | Teardown | 3.7 | 0.2 | 1.1 |
  | Other | 13.8 | 4.2 | 0.9 |
  | cargo compile (`Finished` lines) | 28.1 | 16.1 | 36.2 |
  | nextest execution | 5.3 | 12.6 | 19.0 |

- **Repeats per round:** `./dev` built 7 times in 8338 (510 s) and 11 times in 8346 (1,219 s, 743 s of it on Windows; 28 to
  327 s each); 0 in 8335, whose commit is `main`'s (downloaded). The Windows `app dyno` job of 8346 built it twice (75 s, then
  169 s in the `hostRustChecks` step, in the kept clone's `tools/paddock-dev/target`). Checkouts 53 (17.6 job-min, all OS).
  `npm run build -w ui` 18 times, 285 s (13 to 24 s each; 10 times, 221 s, in 8346). `npm ci` 33 times, 5 to 9 s each.
  Angular e2e app builds 12, 124 s.
- **3 against 6 lanes, the same jobs (8335 against 8346), seconds:** `app dyno (e2e-1)` 726 → 1,023; `(e2e-2)` 669 → 807;
  `reference apps (2/2)` 391 → 580; `app dyno` 360 → 523 (compile 128 → 265); `app pitwall` 218 → 343 (compile 107 → 180);
  `crate (ubuntu)` 102 → 163 (compile 46 → 104); `app shell-demo` 97 → 140; `app pitwall (e2e-2)` 93 → 156. Linux compile
  in `ci.yml`: 9.4 → 17.6 job-min. 8346 ran up to 5 Linux jobs at once; 8347's Linux jobs waited for it, and its plan job
  held a Linux lane for 1,861 s in `runner-wait` (8335's: 794 s; 8338's: 64 s). 8347 was a branch-mode plan (Chromium only,
  no bundles or smoke tests): `debrief.yml`'s Linux Rust jobs 0.89 to 1.23 times 8338's; `./dev` built 5 times, 453 s. Every Linux job
  logged `CARGO_BUILD_JOBS stays unset: cargo uses all 30 cores`.
- **sccache:** Linux 94 to 100% hits, 1,000 to 2,000 requests per Rust job (empty target folder). macOS and Windows (per-slot
  local caches, kept target folders): 0 to 37% hits where anything compiled: Windows `app dyno` 204 misses, 447 s compile;
  macOS `app dyno` 59 misses, 145 s; `bundle` and `real-app smoke` 41 misses on each.
- **Playwright per suite (8335 + 8338), tests / test-s / median:** Debrief WebKit 692 / 5,086 / 6.4 s; Debrief Chromium 704
  / 2,470 / 3.1 s; Dyno WebKit 442 / 1,606 / 1.7 s; Dyno Chromium 442 / 1,229 / 0.92 s; reference apps Chromium 527 / 852;
  reference apps WebKit (macOS) 458 / 703; Pitwall WebKit 245 / 422 / 1.3 s, Chromium 245 / 249 / 0.82 s; Spotter 62 / 75.
  10th percentile per test (the app-start floor): 0.55 s (Dyno Chromium) to 0.99 s (Debrief WebKit).
- **Slowest specs, test-s per round:** Dyno `page-layout.spec.ts` 671 (WebKit) + 529 (194 tests each); Debrief
  `live-tail-follow.e2e.ts` 513 (WebKit, 34 tests) + 170; Dyno `layout.spec.ts` 254 + 197; Debrief `dock-vs` 171, `live-tail`
  155, `popout` 128 (WebKit); Pitwall `layout.e2e.ts` 148 + 114; Dyno `row-actions.spec.ts` 146 + 142; Debrief
  `main-layout` 110, `analysis-views` 106; Dyno `axe.spec.ts` 94 + 76.
- **Slowest 30 tests (s, suite, engine, test):**

  | s | Suite | Engine | Test |
  |---:|---|---|---|
  | 72.0 | Debrief | webkit | `e2e/dock-panels.e2e.ts:33` |
  | 66.0 | reference apps (macOS) | shell-demo-webkit | `e2e/shell-demo.spec.ts:669` |
  | 66.0 | Dyno | webkit | `e2e/axe.spec.ts:59` |
  | 53.6 | Dyno | chromium | `e2e/axe.spec.ts:59` |
  | 44.8 | Dyno | webkit | `e2e/dialogs.spec.ts:15` |
  | 43.9 | reference apps | shell-demo | `e2e/shell-demo.spec.ts:669` |
  | 41.2 | reference apps (macOS) | shell-demo-webkit | `e2e/shell-demo-size.spec.ts:21` |
  | 39.7 | Dyno | chromium | `e2e/row-actions.spec.ts:87` |
  | 39.7 | Dyno | webkit | `e2e/row-actions.spec.ts:100` |
  | 39.6 | Dyno | webkit | `e2e/row-actions.spec.ts:87` |
  | 38.4 | Dyno | chromium | `e2e/row-actions.spec.ts:100` |
  | 37.6 | Debrief | webkit | `e2e/activity-errors.e2e.ts:31` |
  | 32.8 | Debrief | webkit | `e2e/jobs.e2e.ts:110` |
  | 32.6 | Debrief | webkit | `e2e/activity.e2e.ts:54` |
  | 31.8 | Dyno | chromium | `e2e/dialogs.spec.ts:15` |
  | 31.6 | reference apps (macOS) | shell-demo-webkit | `e2e/shell-demo-a11y.spec.ts:13` |
  | 31.1 | reference apps | shell-demo | `e2e/shell-demo-size.spec.ts:21` |
  | 29.6 | Debrief | webkit | `e2e/live-tail.e2e.ts:139` |
  | 29.1 | Dyno | webkit | `e2e/layout.spec.ts:211` |
  | 28.9 | Dyno | webkit | `e2e/layout.spec.ts:167` |
  | 28.5 | Debrief | webkit | `e2e/markers-highlight.e2e.ts:26` |
  | 28.2 | Dyno | webkit | `e2e/axe.spec.ts:62` |
  | 28.2 | Debrief | chromium | `e2e/dock-panels.e2e.ts:33` |
  | 27.9 | Dyno | webkit | `e2e/layout.spec.ts:140` |
  | 27.5 | Dyno | webkit | `e2e/layout.spec.ts:140` |
  | 27.5 | Dyno | webkit | `e2e/layout.spec.ts:167` |
  | 27.5 | Dyno | webkit | `e2e/layout.spec.ts:211` |
  | 27.0 | Dyno | webkit | `e2e/layout.spec.ts:167` |
  | 27.0 | Dyno | webkit | `e2e/layout.spec.ts:211` |
  | 26.8 | Dyno | webkit | `e2e/dialogs.spec.ts:22` |

- **Rust:** `crate (windows)` doc tests 198 s (8335) and 125 s (8346) for about 30 doctests in 58 crates; the 37 crates
  without one took 48 s and 32 s of it. Linux: 21 s. paddock-dev suite, test-seconds (deduplicated): Windows 4,334 (8335)
  and 3,878 (8346), macOS 2,545 and 1,680, Linux 814 (8346); `merge_check` 1,089 / 303 / 53, `golden` 643 / 189, `queue`
  637 / 388 / 58; queue tests 7.3 s median on Windows.
- **Fixed waits:** `waitForTimeout` 10 in Debrief's specs, 3 in Dyno's, 5 in `ui/e2e`, 0 in Pitwall's.

## OS-scoped tests: minutes saved on the Windows and macOS legs (2026-11-22, feat/os-scoped-tests, WP-5)

Rule: a test runs on Windows or macOS only when its code behaves differently there (`hostTests` in the CI contracts, conformance C155;
docs/research/2026-11-22-os-scoped-tests.md). The note's estimates, from the job logs of run 8554 (train 18, full, green), before anything
moved: Windows about 16 of 55 job-minutes (paddock-dev about 10, crate about 2.5, the apps about 3.5), macOS about 4.5 of 39 plus 4 to 8
for the deleted `tools-macos` job and 3 to 5 per debrief.yml run; per train (two to three ci.yml runs) 35 to 50 Windows and 10 to 35 macOS
minutes. These are estimates, not results.

**Measured at landing: not yet.** Read the first CI run after this branch lands on main (a full run, not a member run: member runs also
narrow by package) and record here, per job, the minutes of `crate (windows|macos)`, `paddock-dev (windows|macos)`, `app <app>
(windows|macos)` and debrief.yml's `rust tests (macOS)` against run 8554 (and debrief 8547 for the macOS tests), and the Windows and macOS
span of the run: the run id to read is `TBD` (the first ci.yml run on main after the landing; the integrator fills it in from
`./dev ci verdict` in the same step that records C155's first green run in docs/gates.toml). Method: Gitea API job metadata, started to
completed, as in the table of the note (`jobs.py` over `/actions/runs/<id>/jobs`), nextest wall and test count from each job log's
summary line.

| Job | run 8554 (before) | first run after landing | tests before | tests after |
|---|---|---|---|---|
| paddock-dev (windows) | 14.9 min | TBD | 1,871 | about 1,000 of 1,871 expected (1,264 of 2,319 listed on macOS) |
| paddock-dev (macos) | 7.5 min | TBD | 2,285 | TBD |
| crate (windows) | 11.4 min | TBD | 4,813 | TBD |
| crate (macos) | 12.1 min | TBD | | TBD |
| app dyno (windows) | 16.6 min | TBD | | TBD |
| app dyno (macos) | 7.7 min | TBD | | TBD |
| debrief `rust tests (macOS)` | 5.6 and 11.5 min (8496, 8547) | TBD | | TBD |
| tools-macos (VM tooling tests) | 4 to 8 min | deleted | | |

The test counts a host leg runs, listed from the tree before landing (what `cargo nextest list` selects with each leg's filter) are in the
CHANGELOG entry of this change.

## Worker budget: Vitest and Playwright workers in the ER-48 lane budget (2026-11-22, feat/fixed-wait-checks)

Cause: ER-48 counted cargo jobs and nextest threads per lane, not the other worker pools of a job. Vitest's default in a non-watch run is
`availableParallelism() - 1` workers (`getDefaultMaxWorkers` in vitest 5.0.5, `dist/chunks/index.1TqnEahn.js`), so every `ng test` job
on a shared host used the whole host, as cargo had before 2026-11-20 (docs/benchmarks.md, "CI job cost, step by step"). Playwright's default
is half the cores, and the configs set 1 (`paddockConfig`, Dyno, Spotter, Pitcrew, Pitwall, ui) or, in Debrief, 2 on CI. Change:
`./dev ci build-jobs` (and `conformance/ci/build-jobs.sh`) also write `PADDOCK_E2E_WORKERS` and `VITEST_MAX_WORKERS`, the job's core share
((cores - kept) / jobs per host / split; no class exception, the macOS 8 threads are nextest's and were measured for nextest only);
Vitest reads `VITEST_MAX_WORKERS` itself, and `e2eWorkers` (`@decryp7/paddock-e2e`) takes the smaller of the variable and a config's own count
and warns naming both. A runner can set `PADDOCK_CI_TEST_WORKERS`; the step then warns naming both numbers when it times the jobs per host
over the cores.

Measured on this tree (no CI run, a quiet-enough 14-core Mac, `uptime` load 13 at the time, so the numbers are counts, not times):

| Check | Before | After |
|---|---|---|
| Debrief's real vitest config (`apps/debrief/app/vitest-base.config.mts`) resolved through `vitest/node` `resolveConfig`, `VITEST_MAX_WORKERS` unset / set to 4 | `maxWorkers` 13 | 4 |
| `build-jobs` goldens (`tests/it/build_jobs_golden.rs`, `build_jobs.rs`) for the three classes | no worker variable | Linux 5, Windows 5, macOS 4 |

Lanes times workers per host, from the class layout (lanes x workers against the usable cores):

| Class | Lanes | Cores usable | Vitest before | Vitest after | Playwright (configs, CI) after |
|---|---|---|---|---|---|
| Linux d-gtr9 | 6 | 30 | 6 x 29 = 174 | 6 x 5 = 30 | at most 1 (Debrief 2), capped at 5 |
| Windows | 3 | 16 | 3 x 15 = 45 | 3 x 5 = 15 | at most 1 (Debrief 2), capped at 5 |
| macOS | 3 | 12 | 3 x 13 = 39 | 3 x 4 = 12 | at most 1 (Debrief 2), capped at 4 |

Not measured: the run time of the Vitest jobs at the lower worker count (the first full CI run after this lands says; ER-12: a count that
turns out too low is raised only with that measurement recorded here). Playwright was already within the budget (workers 1, Debrief 2), so
its row changes only where a host's share falls below a config's own count.

### 2026-11-24: a job outside the budget (debrief.yml frontend, run 8753; fix/debrief-vitest-timeouts-8753)

debrief.yml's `frontend` and `e2e` jobs and nightly.yml's `dialog-commands` ran no build-jobs step, so `VITEST_MAX_WORKERS` was never set:
Vitest started 29 workers on d-gtr9 beside the run's five other lanes (rust tests, four e2e shards). Same tree in both runs (no change under
`ui/` or `apps/debrief` between 727810fe9 and 27a7dd840), CI job logs:

| Run (frontend job) | vitest Duration | per-test p90 / p99 / max | tests >= 15 s | the two failing tests |
|---|---|---|---|---|
| 8695 (45748), green | 134.2 s | 4.4 / 9.5 / 14.2 s | 0 | 7.8 s, 7.8 s |
| 8753 (46355), red | 167.1 s | 6.2 / 13.8 / 21.9 s | 8 (3 over 20 s) | 20.8 s, 21.9 s (timeout 20 s) |

Per-test ratio red/green over the 940 tests of 0.5 s or more: median 1.15, p90 2.29, p99 4.39. A third test ("evaluates every tile kind of
the demo dashboard") ended at 20.75 s.

Local, the dev Mac (14 cores, load 13 to 29 from the train's CI, so times not absolutes), the full Debrief suite with `--coverage`, the
same tree, `VITEST_MAX_WORKERS` set:

| Workers | Runs | outliers-view test, median (max) | traces test, median (max) | per-run slowest test, median (max) | suite wall |
|---|---|---|---|---|---|
| 5 (d-gtr9's share after this fix) | 20 | 0.86 s (1.53 s) | 1.28 s (2.71 s) | 2.07 s (3.50 s) | 82 to 151 s |
| 26 (about twice the cores, as the 29 on d-gtr9 with its neighbours) | 5 | 4.96 s (5.66 s) | 5.49 s (7.27 s) | 7.54 s (9.37 s) | 78 to 108 s |

More workers barely shorten the suite (the host has no free cores to give) but make each test 4 to 6 times longer, which is what a fixed
per-test timeout sees. Scaled to CI's headroom (20 s / 7.8 s = 2.6 times the green time), all 20 runs at 5 workers stay inside the bound
(the slowest of the two tests 2.71 s against 3.3 s) and none of the 5 at 26 do (3.4 to 7.3 s, every one of the ten over its bound). Not measured: the frontend job's time on d-gtr9 at 5 workers (the next debrief.yml run says; it is not the run's
critical path, the e2e shards end 6 to 9 minutes after it in runs 8695 and 8753).

## Planner run time before the repository graph model (2026-11-23, docs/adr-repo-graph-model, base 803aed12d)

The baseline ADR 0117 §9 holds the graph model to. Dev Mac (14 cores, load about 15 at the time, so the numbers are a ceiling), `./dev`
already built, one run each, `/usr/bin/time -p`:

| Command | real | user | sys |
|---|---|---|---|
| `./dev plan --all` | 2.90 s | 0.38 s | 0.35 s |
| `./dev plan origin/main~20 --explain` | 12.79 s | 3.92 s | 3.55 s |

Not measured: the spawn count (WP-1 adds the budget test), and the same commands on the Linux runner.

## Planner with the repository model beside it (2026-11-23, feat/repo-graph-model WP-1 to WP-3, 05455f446)

ADR 0117 §9 and §10 budget the model at the planner's own time or less. Dev Mac (14 cores, 1-minute load 13 to 20), the tool binary
run directly (`./dev --print-binary`), two warm runs each, `/usr/bin/time -p`:

| Command | real | user | sys |
|---|---|---|---|
| `plan origin/main~20 --explain` (the rule tables alone) | 0.65 s, 0.75 s | 0.30 s, 0.31 s | 0.24 s, 0.25 s |
| `plan origin/main~20 --compare` (the rules, then the model: CI, Cargo and code adapters, reach, selection) | 1.06 s, 1.03 s | 0.34 s, 0.35 s | 0.30 s, 0.31 s |

The first, cold `--compare` run took 7.24 s real (the batched reader's object reads and the `git grep` for read candidates cold).
The model's processes: at most one `git cat-file --batch-command` per reader (two readers, pinned by
`repo_model::tests::the_bulk_reader_starts_at_most_one_git_for_many_files_and_the_control_counts_one_per_reader`), one `git grep -l`,
and no `cargo metadata` of its own (it replays what `read_graph` recorded). Not measured: the Linux runner, and a cold run per adapter.

## Planner with oxc imports and syn reads (2026-11-23, feat/repo-graph-wp3, fa390b7ea)

ADR 0117 §10 budgets the whole extraction at 3 s on the dev Mac. Dev Mac (14 cores) under other agents' load (1-minute load 25 to
31), the tool binary run directly, two warm runs each, the shell's `time`:

| Command | real | user | sys |
|---|---|---|---|
| `plan --files README.md --explain` (the rule tables alone) | 1.27 s, 0.87 s | 0.39 s, 0.34 s | 0.35 s, 0.28 s |
| `plan --files README.md --compare` (the rules, then the model with the oxc, syn, CI, Cargo and code adapters) | 3.00 s, 4.58 s | 2.70 s, 2.81 s | 0.60 s, 0.63 s |
| `plan 59e5f7ec3^ 59e5f7ec3 --explain` | 0.90 s, 1.29 s | 0.34 s, 0.43 s | 0.28 s, 0.40 s |
| `plan 59e5f7ec3^ 59e5f7ec3 --compare` | 2.64 s, 5.09 s | 2.71 s, 3.04 s | 0.87 s, 1.04 s |

The code adapter alone over the real tree (release build, `repo_model::code::tests::on_the_real_tree_every_read_candidate_parses_with_syn`,
`--no-capture`): 1.13 s for one `git grep -l`, 420 Rust sources through `syn` (0 fell back to the text scan) and 1,810 JS/TS files
through `oxc_parser` and `oxc_resolver`; the text scan of the same 421 read candidates took 0.20 s (472 reads). The model now costs
about 2.4 s of user time over the rule tables, single-threaded: above the 3 s budget once the machine is loaded. Not done yet: the
`parallel` feature (rayon over the parsers, §9), a cold run, and the Linux runner.

## Import and cfg scan of the whole repository (2026-11-23, docs/adr-repo-graph-model)

The lower bound ADR 0117 §10 compares a parser with. Dev Mac under load (about 15), one run each, `/usr/bin/time -p`, over the tracked files:

| Files | Count | Bytes | Scan | real |
|---|---|---|---|---|
| `*.ts`, `*.mjs` | 3,780 + 633 | 24.1 MB + 4.8 MB | `grep -oE` for relative `from`/`import`/`require(` specifiers (15,743 hits) | 0.81 s to 0.86 s |
| `*.rs` | 3,732 | 46.3 MB | `grep -oE` for `use crate::`, `mod x;`, OS `cfg` (8,115 hits) | 0.91 s |

Not measured: `oxc_parser` and `syn` on the same files (WP-3, WP-6), and the Linux runner.

## The base search reads the run list in pages (2026-11-23, ADR 0117 WP-4)

`ci_runs::green::find_base` reads the Actions run list (all branches and events, newest first, 50 a page) once and matches runs to
main's first-parent commits in memory, so a base costs the pages plus two calls (the job list and the plan log) for each run that is
evidence, never a call per commit. `MAX_PAGES` (6) bounds it; the measurement behind it (read-only API, no load on the runner):

| Measure | Value |
|---|---|
| Runs listed on 2026-11-23 over 12 pages | 600, from 2026-11-07 11:43 to 2026-11-23 07:00 (16 days, 37 a day on average) |
| Runs a day, newest 6 days (all workflows, all branches) | 29, 25, 21, 38, 33, 44 (ci.yml 7, 11, 7, 13, 4, 12) |
| Position of the landing run 8610 (train 19, 11-22) in the list | index 38, page 1 |
| `MAX_PAGES` 6 | 300 runs, 6.8 days at the busiest day measured (44 a day); about 8 days at the average |

So a base up to about a week old is found; an older one plans everything (the safe side), which is what a base older than the pages
means. Narrowing the request to ci.yml is not possible (the API's list takes `event`, `branch`, `status` and `head_sha`, not a
workflow, and a train's run is a `workflow_dispatch` on a `train/*` branch, which a `branch=main` filter would drop). Raise `MAX_PAGES`
only with a new measurement recorded here (ER-12).

## OS placement derived from the code: what `std::process::Command` decides, and the cost of deriving (2026-11-23, feat/repo-graph-wp6, ADR 0117 WP-6)

`./dev` derives which packages and modules run on the Windows and macOS legs from the code (`tools/paddock-dev/src/os_placement.rs`
over the repository graph's `os` edges, `crates/paddock-repo-graph/src/model/os.rs`); the `hostTests` of each contract hold only
exceptions with a reason. ADR 0117 §7 asked to measure, before it is used, how much narrowing the marks by `std::process::Command`
moves tests on or off a host OS.

Method: `os_placement::derive` and `derive_without` over `git ls-files` of this branch (base `59e5f7ec3` merged with
`origin/feat/os-scoped-tests` `9959bcb3f`), each workspace of a contract that declares `hostTests`, with every signal on, with
the `layer` signal (portable-layer entry points: `std::process::Command`, `paddock_proc::command`, file locks) left out, and with the
`dependency` signal left out. Counts are packages (module prefixes for paddock-dev) that a host leg tests. Dev Mac, debug build,
load average about 21 (not a quiet machine: counts are exact, times are indications).

| Workspace | Known | All signals | Without `layer` | Without `dependency` |
|---|---|---|---|---|
| `crates` (packages) | 58 | 49 | 47 (`paddock-git`, `paddock-siblings`; 31 `#[test]`s) | 49 |
| `apps/debrief` | 47 | 27 | 27 | 27 |
| `apps/dyno` | 12 | 10 | 10 | 10 |
| `apps/pitcrew`, `pitwall`, `spotter` | 4, 6, 5 | 3, 6, 4 | 3, 6, 4 | 3, 6, 4 |
| `tools/paddock-dev` (modules) | 298 | 125 | 116 (9 modules: `ci_runs::reservation`, `gates::shell`, `golden_cmd`, `load`, `pre_push::warm`, `release_isolation::ledger`, `repo_inputs`, `repo_model`, `repo_model::code`) | 125 |

Result. Dropping `std::process::Command` narrows 2 of 49 crates (31 tests) and 9 of 125 modules; every app is unchanged. It would
also take Windows and macOS tests away from code that starts `git`, `cargo` or a shell, where the per-process cost differs most per
OS (about 45 ms a git start on the Windows runner against 6 ms on the Mac, ER-20, Paddock train 14). So the signal is kept: the saving
is small and it is the unsafe direction. The `dependency` signal (`sysinfo`, `notify`, `tauri`, the OS-API crates) adds nothing to the
placement on this tree (every package it marks has a code signal already); it stays as the net for a crate that gains such a
dependency before it gains a call. `os:layer` is a mark of its own (`select_os(.., ignoring)` leaves it out), so the measurement can be
repeated on any tree: `derive_without(root, files, read, &[Signal::Layer])`.

Against the hand lists of `feat/os-scoped-tests`: every package of the old lists is derived (46 of 46 in `crates`, 27, 10, 3, 4, 4 in
the apps); the derivation adds `paddock-bench` and `paddock-log` (`env::consts::OS`), `paddock-workspace-hack` (a `[target.'cfg(..)']` table;
it has no tests), and `pitwall-core` (`env::consts::OS`) and, by the code, `pitwall-app`, which `lastPackages` keeps off the host
legs. For paddock-dev 101 listed prefixes become 125 derived modules (the import edge now also carries through `ci_run`, `hook::install`,
`merge_check::git` and others), 17 listed prefixes with no OS signal stay as `add` exceptions with a reason (host-runner production
code of the CI kit and the callees of host code inside the dev-Mac folders), and the 24 `only.macos-latest` entries stay as `only`
exceptions (user decision 2026-11-22).

Cost of deriving, release build, load average about 21, all workspaces one after the other: 1.24 s for `crates` 257 ms,
`apps/debrief` 492 ms, `apps/dyno` 210 ms, `tools/paddock-dev` 87 ms, the rest 230 ms; one bulk read of each workspace's `.rs` files and a
substring prefilter that skips the lexer for a file with none of the signal words. The same derivation in a debug build without the
prefilter took 5.6 to 6.5 s (the build and the prefilter were changed together, so the prefilter's share is not separated). The planner
runs it once per plan, only for the workspaces whose contract declares `hostTests`.

## Train replay: rule tables against the model (2026-11-23, feat/repo-graph-wp57, ADR 0117 WP-5)

`./dev plan --replay 9cbd77a63..803aed12d`: 392 non-merge commits (the member commits of trains 15 to 20), each planned against its
parent by the rule tables and by the model, on a debug build of the tool, Dev Mac (14 cores, 15-minute load 27 to 35), one run each.

| Run | rule tables | model | wall |
|---|---|---|---|
| one thread | 157.6 s, 185.9 s | 15.5 s, 18.0 s | 2 m 18 s and over 3 min |
| four threads (`repo_model::replay::WORKERS`) | in the same total | in the same total | 77.9 s and 91.5 s (the test `repo_graph_net::train_replay_has_no_unlisted_narrowing`) |

The rule tables are 90% of the time (git reads per commit); the model reads each tree once and each commit's texts through its batched
reader. The replay test's nextest budget (`tools/paddock-dev/.config/nextest.toml`) cites this block. Not measured: the Linux runner and
a release build. Node hashes (`./dev plan --hash crate:crates/paddock-proc,crate:crates/paddock-slots`): 2.7 s wall, one `git ls-tree`,
one `git status`, one `rustc -vV`, same load.

## paddock-dev tests under load (2026-11-23, fix/bounded-test-runs, base 64e01dd84)

Dev Mac (14 cores), debug build, `cargo nextest run` default profile with 4 workers unless stated. The load figures are the 1/5/15-minute
load averages, which include the other agents and the graph train's CI.

| What | Before | After | Load |
|---|---|---|---|
| full suite, nextest, 4 workers | 2574 passed in 328.5 s (2 slow) | 6 runs with 4 to 6 CPU burners: 5 all passed (327.6, 326.3, 542.7, 277.4, 294.4 s), 1 had one failure (`worktree::concurrent::two_adds...`, an open fix item in docs/lessons.md) | before: 15 to 33 / 18 to 22 / 18 to 20; after: started at a 15-minute load under 30 (21 to 30), peaks 57 / 48 while the train CI ran |
| full `it` suite, plain libtest (the binary run directly, 14 threads) | stalled at 81 and at 142 finished tests; git children in disk wait 20 s to over 2 min each | 723 passed in 161 s | 19 to 25 / 21 to 22 / 24 |
| `worktree::list_clean::clean_dry_run` after `check_c129::a_control_holds...`, plain, 1 thread | stopped at 300 s | 1.0 s (alone: 1.1 s) | 11 to 20 / 21 / 25 |
| `os_scoped::a_new_skip_or_only_entry...` | 31.5 s | 9.4 s | about 21 to 27 / 22 to 25 |
| `os_scoped::an_approval_the_merge_base_already_covers...` | 10.5 s | 3.4 s | same |
| `ci_run::tests::adding_a_check_changes_no_golden_row...` | 8.7 s | 5.0 s | same |
| one `gates::ci::build` of the real tree (debug) | 2.15 to 2.2 s; 6 to 13 s while the plain run held the disk | (unchanged) | 18 / 22 / 24 |
| `slots::a_holder_lets_go_when_its_lifeline_process_ends` | red: 60.8 s, "slot s1 never became free" | 0.59 s | about 28 / 37 / 28 |

The cause of the plain-harness stall: a deleted working folder makes macOS's getcwd read the user temp folder (about 992,000 entries)
for every git start, with disk0 at about 32,000 operations a second (`iostat`, 4 KB transfers). See docs/lessons.md, 2026-11-23,
"The long paddock-dev test hangs had one cause".

## Member gate: CI tests of the touched workspaces (2026-11-25, feat/member-check-touched-tests)

Dev Mac (14 cores, 64 GiB), the CI image native (arm64, `tools/linux-container.sh --ci-image`, 4 CPUs, warm build volume from the
Linux Clippy). The member is this branch itself (it touches `tools/paddock-dev` only); `./dev check --pre-push --member-gate --base
origin/main`. Up to three rows run at once (`workers`: one at machine load 20 or more), but a push runs as many heavy steps at once as it has workers (1 until 2026-11-26, then as many as heavy's slots and the load allow, ADR 0102 amendment 2026-11-26): the rows beyond heavy's slots queue, and the cheap rows run beside them. "queued" is the wait for the machine-wide heavy slot, beside other agents' gates. (Corrected 2026-11-26: this said "three at a time through the heavy slot", which was wrong. Since feat/gate-row-timings a run prints and records a per-row table of queued s, run s and concurrency: `row-timings.tsv` in the run's log folder. The `tests-linux` rows below were removed on 2026-11-26: no container runs on this Mac.)

| Row | Selected | Run | Queued | Load (1/5/15) |
|---|---|---|---|---|
| `tests tools/paddock-dev` (macOS, `ci` profile, 4 threads) | 2428 tests (112 skipped by the profile) | 376.1 s | 135.3 s | started at 5-minute 23.9 |
| same, second run (clone at the branch head) | 2428 | 556.7 s (nextest 425.1 s) | 125.9 s | 42 / 19 / 21 |
| `tests-linux tools/paddock-dev` (container, OS-scoped by module) | 1519 of 2540 (1021 not OS-conditional) | 117.2 s (nextest 85.2 s) | 374.2 s | 9 / 18 / 21 |
| whole member gate, first run (all rows) | | 1905 s wall | | 24 / 24 / 18 at the start |

Added per member that touches paddock-dev: about 380 to 560 s of macOS test run and about 120 s of container run, which run beside
the other rows; the slot queue, not the tests, set the wall time. The OS scope (the `hostTests` placement derived from the code)
leaves 40% of paddock-dev's tests off the container run; a pure workspace (no marked package or module) has no container run at all.
Not measured: a `crates` member (the container would also compile the whole shared-crate workspace) and the guardian-as-user step.

## Concurrent push-gate test rows on the shared Mac (2026-11-25, feat/heavy-two-slots-when-idle, base 4284e23d5)

Two interleaved rounds: round 1 in the order 1, 2, 3 gates at once, round 2 in the order 3, 2, 1. r2n2 overlapped this branch's own push
gate (mean 1-minute load 18.5) and still matched r1n2.

- **Machine:** Mac mini, Apple M4 Pro, 14 cores, 64 GB, macOS 27.2, other agents running (the loads below include them).
- **Workload, one "gate":** the push gate's `tests tools/paddock-dev` row in its own detached worktree and build folder:
  `touch src/lib.rs`, then `CARGO_BUILD_JOBS=2 NEXTEST_TEST_THREADS=<14/n> cargo nextest run --locked --no-fail-fast` (debug, no sccache,
  dependencies warm; rebuilds paddock-dev and its test binaries, then 2557 tests). n gates at once, each with 14/n test threads.
- **Each batch** ran as one `./dev heavy` step and started only while the 5-minute load was under 12. Wall and CPU (user + sys of the
  whole tree) from `/usr/bin/time -l`; load and `memory_pressure` free share sampled every 5 s.
- **Scripts:** a batch script and a driver in the agent's scratch folder (not kept); the commands are the ones above.

| Batch | Gates at once | Test threads each | Wall per gate (s) | CPU per gate (s) | Batch wall (s) | Gates per hour | Load at start (1/5/15) | Mean 1-min load | Min memory free |
|---|---|---|---|---|---|---|---|---|---|
| r1n1 | 1 | 14 | 149.6 | 854.9 | 150 | 24.0 | 4.6 / 8.4 / 14.8 | 11.8 | 69 % |
| r1n2 | 2 | 7 | 244.8, 244.9 | 876.6, 880.1 | 245 | 29.4 | 7.9 / 11.2 / 13.9 | 13.9 | 69 % |
| r1n3 | 3 | 4 | 421.1, 421.1, 417.3 | 906.0, 908.1, 899.2 | 422 | 25.6 | 18.9 / 14.5 / 14.4 | 19.7 | 68 % |
| r2n3 | 3 | 4 | 477.5, 476.3, 477.3 | 902.1, 898.4, 903.6 | 478 | 22.6 | 4.6 / 8.4 / 12.3 | 24.3 | 68 % |
| r2n2 | 2 | 7 | 246.7, 247.1 | 888.8, 890.4 | 247 | 29.1 | 4.5 / 11.7 / 14.6 | 18.5 | 69 % |
| r2n1 | 1 | 14 | 146.3 | 843.7 | 146 | 24.7 | 6.4 / 11.9 / 14.3 | 13.0 | 69 % |

Reading: two gates at once finished 18 to 22 % more gates per hour than one (29.1 to 29.4 against 24.0 to 24.7) for 3 to 5 % more CPU
per gate; three at once finished fewer than two (25.6 and 22.6), with 6 to 7 % more CPU per gate than one (contention: the 1-minute load rose to about 20 on 14 cores). Memory
was never short (68 % free at the lowest; a gate's largest process is about 1.4 GiB). So heavy caps at two slots (`MAX_AUTO_SLOTS = 2`)
and opens the second under a 5-minute load of 0.85 x cores (11.9 here): every batch started under 12, and there two beat one; above it
nothing measured says a second step helps. The round-1 3-gate batch started during another agent's burst (1-minute load 18.9); round 2
repeated it from a quiet start (4.6) and was slower still (22.6 gates per hour, mean 1-minute load 24.3), so three is not a gain.

## Member gate: parallel heavy rows (2026-11-26, feat/gate-two-heavy-rows)

ER-20, one real gate, two runs on the same tree (this branch, 38 changed files in `tools/paddock-dev`, `docs/`, `conformance/`):
`./dev check --pre-push --member-gate --no-reuse --base origin/main`, a Mac with 14 cores, other agents' gates running throughout.
Serial is `PADDOCK_HEAVY_AT_ONCE=1` (the rule since 2026-11-15); parallel is the default, one heavy step per worker (3 workers, heavy's
slots and load gate deciding). The study of train i's member gate (1 h 45 min, 64 % queue, 36 % run) is the serial baseline of the
production gate.

| Run | Wall | Load 1/5/15 at start | at end | Heavy steps at once | Failed rows |
|---|---|---|---|---|---|
| serial (limit 1) | 1547 s | 22.1 / 16.3 / 15.8 | 5.1 / 11.4 / 14.4 | 1 | `conformance-tests` (a load-dependent conformance test, see below) |
| parallel (default) | 721 s | 5.1 / 11.4 / 14.4 | 44.5 / 26.0 / 18.6 | 3 | `conformance-tests` (same), `tests tools/paddock-dev` (a timing assumption in the new concurrency test, fixed: the fake now holds a batch together) |

| Row | serial queued / run (s) | parallel queued / run (s) |
|---|---|---|
| clippy crates | 183.4 / 1.0 | 83.3 / 0.8 |
| clippy-windows crates | 0.0 / 0.9 | 67.9 / 0.8 |
| clippy tools/paddock-dev | 161.9 / 4.1 | 5.2 / 0.4 |
| conformance-tests | 109.8 / 81.5 | 15.5 / 73.7 |
| member-audits | 0.0 / 105.2 | 15.7 / 91.1 |
| tests crates | 135.8 / 320.7 | 167.7 / 78.6 |
| tests tools/paddock-dev | 26.2 / 325.3 | 162.4 / 450.7 |

The wall time fell to 47 % (1547 s to 721 s) with three heavy steps at once; the per-row table shows where: the serial run's rows waited
for each other (concurrent 1), the parallel run's started beside two others (concurrent 3), and the queued seconds stayed a separate
column, so waiting for the machine is still told from running. The two runs are not a controlled A/B: the load differed (the serial run
started at 22, the parallel one at 5 and ended at 44 because the gate and other agents were all running), and the clippy rows reuse a
warm build folder, so their run times are about one second. `tests tools/paddock-dev` ran 450.7 s in parallel against 325.3 s serial
because its threads were heavy's share of the cores (`step_threads`: cores divided by the steps in flight) beside two other test rows.
One pair is a measurement, not a distribution (ER-28): repeat it on a quiet machine before quoting the factor. A failed
`conformance-tests` row in both runs is `verify reads the real dev self hash --json shape`, which took 30 to 37 s under load; it is not
touched by this change.

## Git fixture repositories of the ci_runs tests (2026-11-26, perf/slow-git-tests, base 8333e185f)

Train n's Windows paddock-dev job (CI run 9118) spent 710 s of its test time in 75 tests over 5 s; the slowest were `ci_runs::trial::tests::*`
(13 to 24 s each) and the `ci_runs::green::tests::*` that read `train_g_shape` (9 to 14 s). Cause, confirmed by reading the fixtures: each test
built its own repository with 14 to 17 `git` starts (trial's `world()`: 17; green's `train_g_shape()`: 14) before its own work, and a git start
costs about 45 ms on the Windows runner against about 6 ms here (rule 85's measure of 2026-11-18). The test-side fix: each fixture is built
once per machine and recipe in `<temp>/paddock-git-fixture-templates/<name>-<hash of the recipe source>` (`paddock_testkit::git_fixture::template`, the shared module since rule 102, 712 KB
in all) and each test copies it with plain file copies (`copy_tree`): no git start, no shared mutable state. nextest runs each test in its own
process, so a `OnceLock` per binary would have shared nothing. Budget tests: `a_world_is_a_private_copy_of_the_template_and_costs_no_git_start`
(a counter of git starts on the test's thread: a world costs 0) and `a_shape_is_a_private_copy_of_its_template_with_the_same_commits`.

- **Machine:** Mac mini, Apple M4 Pro, 14 cores, macOS 27.2, shared. Debug build, `cargo nextest run` of the 56 tests of the filter
  (`ci_runs::trial::tests`, `ci_runs::green::tests`, the clean-revert test, the syn real-tree test), warm build folder.
- **Load:** 1/5/15-minute load at the start 20.7 / 29.3 / 26.2 (before), 15.8 / 22.7 / 24.3 (after, warm templates). Not a quiet machine
  (rule 2 above): single runs, so the times are indications; the difference in git starts is exact.
- **Not measured on Windows:** the estimate below is the Mac ratio scaled by the start cost, not a run.

| Family | Tests | Git starts of the fixture per test, before / after | Sum of test times on the Mac, before | after (templates warm) |
|---|---|---|---|---|
| `trial::tests` with a `world()` | 8 | 17 / 0 | 11.7 s (1.1 to 2.2 s each) | 4.7 s (0.3 to 1.2 s each) |
| `green::tests` reading `train_g_shape` | 8 | 14 / 0 | 4.7 s (0.47 to 0.9 s each) | 1.5 s (0.07 to 0.33 s each) |
| `dispatch_guards` clean-revert test | 1 | one repository, one test: unchanged | 0.8 s | 0.5 s (noise) |
| `graph_on_the_real_tree_every_read_candidate_parses_with_syn` | 1 | unchanged | 10.1 s | 7.7 s (noise); the syn extract is 0.85 s and the text scan 0.38 s of it, the rest is `read_graph` (`cargo metadata`, the production path under test) |

Windows estimate: a trial test of 13 to 24 s there is about 40 git starts at 45 ms plus file work; 17 of them go, so about 5 to 10 s each (the
`prepare` and `prune` calls under test still start git); a shape test of 9 to 14 s goes to about 1 to 2 s (all its starts were the build). About
150 s of the trial and green families' time becomes about 50 s. The first test process on a machine builds each template (the same cost as before,
once); a cold CI job pays it once per recipe change, not per test.

Left as it was: the syn test (its time is `cargo metadata`, which a shared parse would not remove, and a per-binary share does not exist under
nextest) and `prune`'s one `merge-base --is-ancestor` per live trial ref (no bulk form answers it; commented `// per-item:`).

## Local sccache: a second worktree hits the first one's entries (2026-11-26, fix/sccache-local-reuse, base 202d0a181)

- **Machine:** Mac mini, Apple M4 Pro, macOS 27.2, other agents running; 1-minute load 41 to 75 at the time (an indication, not a timing).
- **Setup:** `./dev sccache env` of this branch (debug build) on a private port, a private tool home and a fresh local disk, with the shared
  S3 store behind it under this machine's prefix `paddock-ci/macOS-aarch64`. The base list was the 206 worktrees of the repository (the
  list this branch computes); the main checkout and both measured worktrees were in it. `SCCACHE_BASEDIRS_VERBATIM_ENV_DEPS` as in the tool.
- **Workload:** `cargo build -p paddock-repo-files --lib` (dev profile, `-j 4`) in `crates/`, a fresh target folder each time (outside every
  base), `--zero-stats` before each build. 112 compile requests executed per build (the crate and its dependencies).

| Build | Worktree | Server state | L0 (disk) hits | L1 (S3) hits | L1 writes |
|---|---|---|---|---|---|
| 1 | A | empty disk | 0 | 0 | 109 |
| 2 | B (another worktree, clean target) | same server | 101 | 0 | 119 |
| 3 | B, `CARGO_INCREMENTAL=0` | same server | 101 | 0 | not recorded |
| 4 | B, a second server with an empty disk | empty disk, same prefix | 0 | 101 | 0 |

- **Reading:** a second worktree hits 101 of 112 requests, the other 11 being the crate itself, its build scripts and crates that read
  `CARGO_MANIFEST_DIR` (kept per checkout on purpose). With an empty disk the same 101 come from the S3 store, so a worktree on
  another machine or after a disk wipe reuses them too. `CARGO_INCREMENTAL=0` does not change a key.
- **Local against CI:** the 112 requests of build 1 found no entry that CI had stored under the same prefix (L1 hits 0 of 112, while the
  prefix held 1000 or more CI objects from 2026-11-19 on). So the keys of a local build and of CI's jobs do not match today, third-party
  crates included. The difference is not `CARGO_INCREMENTAL` and not the checkout folder (bases strip it); the cause is not yet
  identified (candidates: the command mode and profile of CI's steps, rustflags, the toolchain path). The common prefix is in place so
  that the entries share once the keys do.

## Local keys against CI's: the two CARGO variables (2026-11-26, fix/sccache-key-env, base 202d0a181)

Why a local build found none of CI's compile-cache entries (the earlier block "Local sccache: a second worktree hits...": 0 of 112).

- **Method:** `SCCACHE_LOG=debug` prints each compile's hash key; the key is the object's name in the S3 store
  (`<prefix>/<k0>/<k1>/<k2>/<key>`), so a key's presence and `last-modified` say whether CI stored it. The same units were compiled with
  `cargo nextest run -p paddock-diag -p paddock-updater --no-run` (308 keys) in four environments and each key looked up with a signed HEAD
  (a throwaway script, not kept). A stored CI entry (`stable_deref_trait`, its `-C extra-filename` hash and the fingerprint json equal to
  a local build's) gave a leaf crate with the same cargo unit and a different sccache key.
- **Cause:** sccache 0.20.1 (`src/compiler/rust.rs`, `generate_hash_key`) hashes every `CARGO_*` variable of the compile except a few
  (`CARGO_MAKEFLAGS`, `CARGO_TARGET_DIR`, `CARGO_BUILD_*`, ...). CI's jobs have `CARGO_INCREMENTAL=0` (setup-rust) and
  `CARGO_TERM_COLOR=always` (the workflows' `env`); a local shell has neither. Not the checkout folder (bases strip it), not the registry
  path (same HOME), not the toolchain, not the profile.

| Environment of the compile | Keys among the 308 that CI had stored before 2026-11-23 |
|---|---|
| neither variable | 0 |
| `CARGO_INCREMENTAL=0` only | 0 |
| `CARGO_TERM_COLOR=always` only | 0 |
| both | 166 |

- **Fix:** the RUSTC_WRAPPER shim that `./dev sccache env` writes exports the two variables for a rustc compile (`CI_KEY_ENV`), so sccache
  sees them whatever the caller's shell has; cargo's own environment is unchanged (no colour codes in piped output, incremental builds
  of workspace crates stay).
- **Cross hit, measured:** a private server (fresh disk, S3 behind it under `paddock-ci/macOS-aarch64`), this branch's shim, a worktree that
  had never built, no CARGO variable in the shell, the same command: 294 compile requests executed, **174 L1 (S3) hits**, 0 L0, 117 misses
  (the workspace crates, which keep their checkout path, and what no CI job built with that command), 117 writes.
- **CI side (same branch):** setup-rust now exports both variables to `$GITHUB_ENV` from the same definition (`CI_KEY_ENV`), so every job
  that sets up the cache has the same key class. Before it, only jobs whose workflow or job `env` had `CARGO_TERM_COLOR=always` did. Jobs
  that call setup-rust and had it: `app-fuzz.yml` fuzz; `ci.yml` app, crate, heavy, paddock-dev; `debrief.yml` bundle, plan, plugin-sdk,
  rust-lint, rust-test, rust-test-macos, tauri-smoke; `nightly.yml` dialog-window-probe. Jobs that called setup-rust and lacked it (a key
  class of their own, so no sharing with the rest): `ci.yml` audit, conformance, crates, e2e, e2e-webkit, tools, ui; `nightly.yml` audit,
  c100, escapes, generators, ipc-endpoints; `dev-tool.yml`, `heavy-release.yml` and `app-release.yml` jobs (release and tag builds never read the
  store). The per-job `CARGO_TERM_COLOR` lines stay: they colour the log of jobs that have no cache.
- **Limits:** Windows has no shim (the wrapper is the sccache binary), so a local Windows build does not share. The shell copy of
  setup-rust (release path) exports the two variables too, as its twin test requires.

## Pre-push reuse after the member gate, and the train-audits guard (2026-11-26, feat/reuse-member-audits)

ER-20, a Mac with 14 cores. **(1) Member gate, then the push hook, on one clean tree** (`check --pre-push --member-gate --base origin/main`, then
the hook's own form, `--refs-from-stdin` in a throw-away worktree): every row the gate ran was reused by the hook (lockfile, fmt, Clippy,
`conformance`, `conformance-tests`, `gates-register`); only `commit-lint` (0.1 s, never reused: it reads commits) ran again, and
`member-audits` and `tests` are member-gate-only by design. **(2) The train-audits guard's command**
(`check --pre-push --member-gate --only member-audits,conformance --base origin/main`) on one commit: 221 s (computed), 112 s (computed
again, because `origin/main` had moved between the two runs: a train landed), then 6 s and 6 s (both rows `reused`, `origin/main` still).
With `member-audits` in the whole-tree class a repeat costs 6 s, not about 2 to 4 minutes. **Why rows re-ran in train o:** a whole-tree row's
key holds the resolved tip of the base (`origin/main`), so any landing or fetch between the member gate and the push makes conformance and
the audits run again; and a member's gate plans only its own delta (the merge's first parent as base), so rows of earlier members'
workspaces were not in the plan of the gate that ran last. `merge check --gates --gates-base origin/main` on the last member plans the whole
train's rows. Not changed, for a decision: keying the whole-tree rows on `merge-base(HEAD, base)` instead of the base's tip would survive
landings, but conformance's checks that read the base's files (shrink-only lists) could then judge an older base; that trade is ER-33's.


## Merge check: heavy generators deferred to the final gen check (2026-11-26, perf/merge-check-generators, base 43c28c456)

ER-20, ER-36, a Mac with 14 cores. Train s's integrator saw `./dev merge check` take 10 to 23 minutes per merge (host load 12 to 29). **Cause:** the
step runs every generator of `docs/generators.toml` the merge changed against its first parent, and a heavy one builds a workspace
(`ci-run-golden`: `cargo test --no-run --lib` of tools/paddock-dev), then `./dev gen check --base origin/main` ran them all again on the train.
**Measured** (load 12 to 17, sccache on, warm build folder): merge `8fa93ff0f` (due: `notices-dyno`, `ci-run-golden`): 236 s before (notices
41 s, build 139 s, test 54 s), 1.3 s after. Merges that make no generator due (`15cc05b38`, `bc248b630`): 2.3 s and 1.7 s, unchanged. **Change:** a
heavy generator is named `deferred` in the merge check (never a pass, ER-33) and runs once in the final `gen check --base origin/main`, whose due set is
the union of every merge's; `--heavy-generators` runs them per merge. Fast generators still run per merge. Also seen: the per-merge
`ci-run-golden` was STALE on a mid-train merge that a later member fixed, so the per-merge run named a member that was not at fault.
Not measured: the final gen check on a whole train (it runs what the per-merge steps ran before, once).


## Push gate hang limits of the long rows (2026-11-27, fix/gate-row-stall)

ER-12, ER-36, a Mac with 14 cores. When the local sccache server hung on 2026-11-27 a gate row sat idle until the 60-minute row cap: the rows had
`--queue-stall` and `--run-max` and no `throttle --stall`. **Measured** (`step-history.jsonl` of this checkout, passed steps only; `max_gap_secs` is
the longest time between two progress events, so an upper bound of the time with no output and no CPU that `--stall` counts): longest gap
clippy 230.1 s (844 steps), clippy-windows 291.0 s (352), conformance-tests 294.8 s (592), member-audits 113.1 s (201), tests 291.5 s (123).
**Change:** each of these rows runs under `throttle --stall S`, S at least three times the longest gap, in 5-minute steps: 900 s, 900 s, 900 s, 600 s,
900 s (`HANG_LIMITS` in `pre_push/budget.rs`, which holds the measured gaps beside them). A hung row ends with exit 81 and reads "not measured: hung (no
output, no CPU for S s)" with the next step, never a pass; a hung row now costs 10 to 15 minutes, not 60. Not measured: a real sccache hang through the
gate (the stand-in test exercises the reading of exit 81; `throttle`'s own tests end a silent idle command and leave a silent busy one alone).

## paddock-dev host legs: OS-independent areas leave Windows and macOS (expected saving, 2026-11-27, ci/paddock-dev-os-scope)

Expected, not measured: nothing was run on a host. **Source:** the per-area test seconds of run 9212 (jobs 52148 macOS, 52162 Windows) in the
block "paddock-dev test time by area" of docs/benchmarks.md on branch docs/paddock-dev-test-split (ADR [0125](adr/0125-paddock-devs-tests-always-run-and-the-crate-is-layered-before-any-split-by-area.md)). Decision 114
(user, 2026-11-27): the host legs run only OS-specific tests, so `tools/paddock-dev/paddock.ci.json` skips `merge_check::` (except `merge_check::git::`),
`ci_runs::` (except `dispatch_guards::`; `reservation::` stays macOS only), `ci_run::`, the planner's model and readers and `repo_model::hash::` and
`replay::` from both hosts. **Windows:** merge_check 545 s and ci_runs plus ci_run 228 s of 1,191 s summed per-test seconds (65%) leave; the kept
`merge_check::git::` and `ci_runs::dispatch_guards::` tests are not separated in the source, so the saving is at most 773 s (about 180 s of the
282 s test step in 9212 if the share holds), plus part of repo_model 53 s and the other skipped areas (not separated in the source).
**macOS:** merge_check 88 s and ci_runs plus ci_run 86 s of 817 s (21%) at most (the kept git and reservation tests are inside these figures); about
75 s of the 360 s test step. **Linux** runs everything, unchanged. Limits: per-test seconds sum the threads; the wall share is the sum share. The
real figure is read from the first run after the change.

## paddock-dev test time by area and churn per train (2026-11-27, docs/paddock-dev-test-split, base e60b56d27)

Read-only, from CI logs (no test was run). **Source:** nextest's per-test `PASS [ Ns]` lines of the paddock-dev jobs of runs 9212 (jobs
52161 Linux, 52148 macOS, 52162 Windows) and 9180 (51687, 51674, 51688), fetched through the Gitea API; history from `git log
--first-parent --diff-merges=first-parent origin/main`. **Test step against job:** 9212 Linux 302 s of 11.8 min (the `./dev` tool cache
missed: 4.7 min), macOS 360 s of 8.1 min, Windows 282 s of 10.5 min; 9180: 295 s of 8.4, 329 s of 5.8, 458 s of 8.9 min. **Sum of per-test
seconds** (9212): Linux 861 s over 2,702 tests, macOS 817 s over 2,379, Windows 1,191 s over 1,322. **Whole-repository classes** (ADR 0121
`graph_`, `tree_`, `live_`): Linux 480 s (56%; 9180: 58%), macOS 143 s (17%), Windows 146 s (12%); 450 of Linux's 480 s are in `tests/it`.
**Largest areas:** Linux it/tree 159 s, it/repo_graph_net 85 s, pre_push (unit and it) 129 s, it/host_steps 68 s, it/wrapper 60 s;
macOS pre_push 154 s, merge_check 88 s, ci_runs and ci_run 86 s, it/wrapper 82 s; Windows merge_check 545 s (46%), ci_runs and ci_run
228 s. **Module graph:** 45 of 66 top-level `src/` modules (125,963 of 140,334 lines) form one strongly connected component of `crate::`
references; about 65 items cross a five-layer sketch upwards. **Churn:** 20 of 20 trains in two weeks changed tools/paddock-dev; the 15
trains of 2026-11-26/27 touched 16 areas on average, and 11 of 15 changed a layer every area uses. **Replay** (share of today's test seconds
each option would have run; whole-repository classes keyed on the whole tree): trains 93 to 98% for every option, members 55 to 78% against
82% today. Limits: the layering is a sketch; per-test seconds sum the threads. Quoted by ADR [0125](adr/0125-paddock-devs-tests-always-run-and-the-crate-is-layered-before-any-split-by-area.md).
