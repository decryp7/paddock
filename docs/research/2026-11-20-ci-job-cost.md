# CI job cost: where each job's minutes go, step by step, and a ranked plan to cut them (2026-11-20)

Research note. Question (user, 2026-11-20): make Paddock's CI jobs "extremely cheap". Since the Linux runner `d-gtr9` went
from 3 to 6 lanes the same day and the host peaked at about 96% CPU, **CPU, not lanes, is the limit**: the lever is the
work each job does, not more parallelism. Gates, tests and independent reviews stay (ER-36); nothing here buys speed with
sleeps, raised limits or skipped tests (ER-12).

Builds on, and does not repeat, [2026-11-20-ci-speed.md](https://dev.decryptology.net/decryp7/paddock/src/branch/docs/ci-speed-research/docs/research/2026-11-20-ci-speed.md)
on `docs/ci-speed-research` (where the round's wall time goes; Linux 63% of job-minutes, Playwright about 60% of Linux).
That note's approved items are being built elsewhere and are only accounted for here (section 4).

**Heads read** (ADR 0091), fetched 2026-11-20 from 01:20 UTC: `origin/main` e0fd0f511991 (this branch's base),
`origin/train/2026-11-19-16` 0c44bc3a38dd, `origin/docs/ci-speed-research` 93bbd80637b3, `origin/perf/sccache-hit-rate`
534918839350, `origin/feat/ci-host-sampler` 36afb26894df, `origin/perf/merge-check-batched-reads` bac2d6e3a9f4 (in main).
`ci/staged-fast-then-long` and `feat/train-fix-ladder` and the D2/D3/D4 branches were **not on origin**: what this note
says about them comes from the brief. Re-read before the commit: see the end.

Every number is in the results block [benchmarks.md, "CI job cost, step by step"](../benchmarks.md#ci-job-cost-step-by-step-2026-11-20-docsci-job-cost-research)
(rule 48), with method and limits.

## 0. Method

Runs: full train rounds **8335** (`ci.yml`) and **8338** (`debrief.yml`) on e0fd0f511 at 3 Linux lanes, WebKit on; train
16's **8346** (`ci.yml`, WebKit off, 6 failed jobs) and **8347** (`debrief.yml`, a branch-mode plan: Chromium only, no
bundles or smoke tests, 2 failed jobs) on 0c44bc3a3 at 6 lanes. Data: the Gitea
API (`runs/{id}/jobs` step times, `jobs/{id}/logs`). Each log line has a timestamp; a job's time was cut at every
`::group::` marker and each segment classified (checkout, container, OS setup, `./dev` build, npm, ui build, app build,
cargo, Playwright, teardown). Cargo compile time is the sum of cargo's own `Finished ... in` lines; nextest time its
`Summary [...]` lines; Playwright time its `N passed (T)` line, and per test the list reporter's `✓ ... (T)` lines (3,817
tests parsed). sccache numbers are the last `--show-stats` block of each job. Logs were read into
`/Volumes/vault-dev/dev-targets/scratch/ci-cost-research/` (not committed). No run was dispatched or cancelled.

Limits: 8335/8338 and 8346 are different commits, so the 3-against-6-lane comparison mixes code change with load (it
compares the same jobs only); 8346 ran without WebKit. Segment classes are by step name, so a step that does two things
is counted under one (noted where it matters). 8347's e2e shards ran Chromium only, so its shard times are not
comparable with 8338's (Chromium per test was about the same: 1.6 to 2.0 s of wall per test at 2 workers in both). Per-job CPU-seconds are **not measured**: the host sampler
(`feat/ci-host-sampler`, not landed) is what gives them.

## 1. Where one job's minutes go

### 1.1 Per OS, one full round (8335 + 8338, 3 lanes)

| Job-min | Linux (46 jobs) | macOS (11) | Windows (8) |
|---|---:|---:|---:|
| Total | **250.5** | 56.2 | 81.7 |
| Runner wait inside the plan job | 14.3 | 0 | 0 |
| `./dev` built from source | 4.3 | 1.2 | 3.0 |
| Container start | 1.7 | 0 | 0 |
| Checkout (fetch, kept clone) | 9.8 | 3.3 | 4.5 |
| OS setup (setup-rust, job isolation, build-jobs, stable workspace) | 2.9 | 2.5 | 8.6 |
| npm ci and node caches | 6.3 | 1.9 | 2.7 |
| `npm run build -w ui` | 4.9 | 0 | 0 |
| App builds (`ng build`, production e2e build, debug app, installers) | 12.4 | 8.6 | 21.6 |
| cargo steps (clippy, nextest, doc tests, coverage) | 25.6 | 21.8 | 38.1 |
| Playwright | **140.3** | 12.4 | 1.1 |
| Teardown (sccache stats, uploads, prune) | 3.7 | 0.2 | 1.1 |
| Other (conformance, script tests, unit specs, notices, SVG drift) | 13.8 | 4.2 | 0.9 |
| of which cargo compile (`Finished` lines, all steps) | 28.1 | 16.1 | 36.2 |
| of which nextest execution | 5.3 | 12.6 | 19.0 |

**Setup is cheap; tests are not.** Per Linux job, container plus checkout plus OS setup plus npm is 15 to 30 s. The
Linux minutes are Playwright (56%), then cargo (10%, mostly compiling from sccache) and app builds (5%). On Windows the
minutes are cargo: compile 36 job-min, test execution 19.

### 1.2 What repeats in one round

| Repeated work | Times per round | Job-min per round (3 lanes) | Measured at 6 lanes (8346, `ci.yml` only) |
|---|---:|---:|---|
| `./dev` built from the same inputs hash (train heads; on `main` it is downloaded) | 7 in 8338, 0 in 8335 (`main`'s commit) | 8.5 | **11 builds, 20.3 job-min** in 8346 (12.4 on Windows), 28 to 327 s each; 5 more, 7.6 job-min, in 8347 |
| ... of which twice in one job | Windows `app dyno`: once in job isolation, again in the `hostRustChecks` step, which calls the `./dev` wrapper from the kept clone (`tools/paddock-dev/target`, a second target folder) | | 75 s + **169 s** on the Windows Dyno critical path |
| Checkout of the same commit | 53 | 17.6 (all OS) | 31 fetches |
| `npm run build -w ui` (same sources) | 18 | 4.7 | 10 jobs, 221 s |
| `npm ci` (same lockfile, warm npm cache) | 33 | about 3.5 (5 to 9 s each) | 157 s |
| Angular app builds, e2e (Debrief production build 4 shards, Pitwall per shard) | 12 | 2.1 | |
| Rust dependency graph compiled in an empty Linux target folder (every container job) | 12 Linux Rust jobs, 1,000 to 1,800 sccache hits each | 28.1 compile | 17.6 for `ci.yml` alone (9.4 at 3 lanes) |

The repeats that are pure waste (same inputs, same output: `./dev` and `ui/dist`) add up to about **13 to 25 job-min a
round**, depending on how many jobs build `./dev`: 3 to 6% of the round. The compiles of the Linux Rust jobs are not the same work (each is a
different workspace or profile; the previous note's 1.3 found the same), but each starts from an empty target folder.

### 1.3 3 lanes against 6 lanes, the same jobs

From 8335 (3 lanes) to 8346 (up to 5 Linux jobs at once, `debrief.yml` 8347 queued behind it until 8346 ended):

| Job (Linux unless named) | 3 lanes | 6 lanes | x | Compile 3 → 6 | Tests 3 → 6 |
|---|---:|---:|---:|---|---|
| `app dyno (e2e-1)` | 726 s | 1,023 s | 1.41 | | 678 → 972 s |
| `app dyno (e2e-2)` | 669 | 807 | 1.21 | | 570 → 756 |
| `reference apps (2/2)` | 391 | 580 | 1.48 | | 368 → 472 |
| `app dyno` (Rust and unit) | 360 | 523 | 1.45 | 128 → 265 s | |
| `app pitwall` | 218 | 343 | 1.57 | 107 → 180 | |
| `crate (ubuntu-latest)` | 102 | 163 | 1.60 | 46 → 104 | |
| `app shell-demo` | 97 | 140 | 1.44 | 34 → 83 | |
| `app pitwall (e2e-2)` | 93 | 156 | 1.68 | | 56 → 96 |

**At 6 lanes each Linux job took 1.2 to 1.7 times as long** (the Mac and Windows jobs, on other machines, mostly did not).
The cause in the logs: `build-jobs` leaves `CARGO_BUILD_JOBS` and `NEXTEST_TEST_THREADS` unset on Linux, so every job's
cargo, rustc and nextest use all 30 cores it sees ("CARGO_BUILD_JOBS stays unset: cargo uses all 30 cores, as before (this
runner class's share is not measured yet)", every Linux job of 8346; `conformance/ci/build-jobs.sh` lines 170 to 180, the
Linux row of its table). Five jobs at once then run up to 150 compile or test threads on 30 vCPUs, plus the WebKit and
Chromium processes, which Playwright's `workers` does not count. The table's own comment still describes `d-gtr9` as
"capacity 4, 16 cores". What part of the slowdown is oversubscription and what part is just a busier host is **not
measured** (the sampler gives per-job CPU).

## 2. Caches: cold and warm

| Cache | Measured | Verdict |
|---|---|---|
| sccache, Linux (local disk `/sccache`, shared by the container jobs) | 94 to 100% hit rate, 1,000 to 2,000 requests per Rust job | Warm. The cost left is non-cacheable work: linking, build scripts, proc macros in an empty target |
| sccache, macOS and Windows (one local cache per slot: `sccache-s1`, `-s2`, `-s3`) | Few requests (kept target folders), but **0% to 37% hits** where something compiles: Windows `app dyno` 204 misses and 447 s compile, macOS `app dyno` 59 misses, `bundle` and `real-app smoke` 41 misses each on both | Cold per slot: a crate compiled in slot 1 misses in slot 3. This is what `perf/sccache-hit-rate` (one RustFS S3 cache) fixes |
| npm cache (`/sccache/npm`) | `npm ci` 5 to 9 s per Linux job | Warm |
| Angular cache (`/sccache/angular/<name>`) | ui library build 13 to 24 s; Debrief production build 14 to 17 s; Dyno dev build 6 s | Warm |
| Playwright browsers | In the `paddock-ci` image on Linux (nothing downloaded); macOS `WebKit` step 16 s | Warm; Playwright's own docs advise against caching browsers and for an image (section 5) |
| Kept clones (macOS, Windows) | Kept-clone step 5 to 17 s; checkout 6 to 60 s | Warm; Linux containers fetch at depth 1, 6 to 25 s each |
| `./dev` (paddock-dev) | Downloaded on `main` commits; built from source on train heads, per job, 28 to 327 s | Cold on every train round: D3 |

## 3. Playwright

### 3.1 Where the test-seconds go (8335 + 8338, 3,817 tests, 12,693 test-seconds)

| Suite | Engine | Tests | Test-s | Median | p90 | Workers |
|---|---|---:|---:|---:|---:|---|
| Debrief | WebKit | 692 | **5,086** | 6.4 s | 13.2 s | 2, shards 3 and 4 |
| Debrief | Chromium | 704 | 2,470 | 3.1 | 6.0 | 2, shards 1 and 2 |
| Dyno | WebKit | 442 | 1,606 | 1.7 | 6.4 | 1, one job |
| Dyno | Chromium | 442 | 1,229 | 0.9 | 5.6 | 1, 2 shards |
| Reference apps (gallery, shell, workbench) | Chromium | 527 | 852 | 0.7 to 2.1 | | 1, 2 shards |
| Reference apps | WebKit (macOS runner) | 458 | 703 | 0.8 to 1.6 | | 1 |
| Pitwall | WebKit / Chromium | 245 / 245 | 422 / 249 | 1.3 / 0.8 | | 1 |
| Spotter | both | 62 | 75 | | | 1 |

**Debrief WebKit is 40% of all Playwright test-seconds.** A Debrief test costs twice as much in WebKit as in Chromium;
Dyno's 1.3 to 1.8 times.

The slowest specs (file x engine, test-seconds per round): Dyno `page-layout.spec.ts` 671 (WebKit) + 529 (Chromium), 194
tests each; Debrief `live-tail-follow.e2e.ts` 513 (WebKit, 34 tests at 15 s) + 170 (Chromium); Dyno `layout.spec.ts` 254 +
197; Debrief `dock-vs` 171, `live-tail` 155, `popout` 128; Pitwall `layout.e2e.ts` 148 + 114; Dyno `row-actions.spec.ts`
146 + 142 (8 tests at 18 s); Debrief `main-layout` 110, `analysis-views` 106; Dyno `axe.spec.ts` 94 + 76 (2 tests). The
slowest tests: Debrief `dock-panels.e2e.ts:33` 72 s in WebKit, the shell demo's "every dialog fits at 1, 1.25 and 1.5" 66 s
(macOS WebKit) and 44 s, Dyno's axe "every screen" 66 s / 54 s, Dyno's `dialogs.spec.ts:15` 45 / 32 s, Dyno's row-action
tooltips 38 to 40 s each. The full top 30 is in the results block.

### 3.2 Fixed waits, app start, servers

- **Fixed waits are not the cost.** `waitForTimeout` appears 10 times in Debrief's specs (150 to 2,500 ms, about 6 s per
  engine in all), 3 in Dyno, 5 in `ui/e2e`, 0 in Pitwall. Time-driven specs already use Playwright's clock (`page.clock`: 57
  uses in Debrief, 52 in Dyno); `live-tail-follow` steps a pinned clock in 20 ms steps, so its 15 s are rendering work, not
  waiting.
- **App start per test** (estimated from the fastest tests, which do little more than load the page): the 10th percentile is
  0.55 s (Dyno Chromium) to 0.99 s (Debrief WebKit). Times the number of tests that is 13% (Debrief WebKit) to 48% (the
  gallery) of a suite's test-seconds; the rest is the tests' own work. Dyno's `page-layout.spec.ts` loads the app afresh for
  each of its 194 cases (24 pages x 4 sizes x 2 themes, `page.goto` per case).
- **Three suites test a dev server.** Dyno (`npx ng serve --port $PORT --poll 1500`), Spotter (`ng serve`) and the reference
  apps (`npm run build:css && npx ng serve <app>`) run against Angular's dev server with file watching; Dyno's polls the tree
  every 1.5 s for the whole 27-minute WebKit job. Debrief and Pitwall serve one production build through the shared
  `ui/projects/paddock-e2e/src/serve-prod.ts`, Pitwall for a stated ER-12 reason ("a dev server compiles and serves every
  module on request, and one stalled module request (@angular_material_radio.js, CI run 7854) left a page blank",
  `apps/pitwall/app/playwright.config.ts`). The CPU a watching dev server costs on the busy host is **not measured**.
- **Workers against CPUs.** Every suite but Debrief runs `workers: 1` (`apps/dyno/app/playwright.config.ts`,
  `apps/pitwall/...`, `apps/spotter/...`, `ui/playwright.config.ts`); Debrief 2 ("three made WebKit tests time out",
  `apps/debrief/app/playwright.config.ts`). With CPU the limit, more workers shorten a job but do not cut its CPU; they help
  only on an idle host.
- **Both engines.** Branch and fix runs are Chromium only; train, main, tag and nightly runs add WebKit, as rule 3 requires
  ("The full suite, Chromium and WebKit, runs once on the combined result", `docs/rules.md` rule 3; `debrief.yml` lines 618
  to 662). That is the required minimum; this note does not propose dropping it. The skip of the main push's re-run (D4)
  removes the second WebKit pass a landing pays today.

## 4. Rust

| Job | Compile | Tests | Doc tests | Other |
|---|---:|---:|---:|---|
| `crate (windows-latest)` (8335) | 235 s | 338 s nextest (4,738 tests) | **198 s** | clippy 50 s |
| `paddock-dev (windows-latest)` | 85 s | **604 s** (1,715 tests, 4,334 test-s) | | |
| `app dyno (windows-latest)` | 447 s (0% sccache hits) | 85 s | 28 s | |
| `debrief bundle (windows-latest)` | 1,127 s | | | the previous note's 1.4 |
| `crate (macos-latest)` | 110 s | 84 s | 41 s | |
| `paddock-dev (macos-latest)` | 44 s | 319 s (2,073 tests, 2,545 test-s) | | |
| Linux Rust jobs (12) | 28.1 job-min | 5.3 job-min | 21 s (crates) | coverage 87 s |

- **Windows test execution is spawn cost.** The same paddock-dev suite costs 4,334 test-s on Windows, 2,545 on the Mac and
  814 on Linux (8346). The groups: `merge_check` 1,089 s on Windows against 303 (Mac) and 53 (Linux), `golden` 643 against
  189, `queue` 637 against 388 against 58; the queue tests take 7.3 s median on Windows, each building git fixtures. This is
  the class of ADR 0107/0108 (git and process starts on Windows cost about 45 ms against 6 ms on the Mac); the batched
  reader (`perf/merge-check-batched-reads`) is already in main, so this is what is left after it. The `golden` tests run
  `scripts/affected.mjs` against `./dev plan` (Node plus git per case): 239 s for one test on Windows.
- **Doc tests on Windows cost 125 to 199 s for 30 doctests** (58 crates; per crate 0.5 to 13 s of compiling and linking).
  37 crates have no doctest and still take 32 to 48 s of the step. Linux runs the same doctests in 21 s.
- **Profiles are already cheap for tests**: `debug = "line-tables-only"`, dependencies at opt-level 2 in the apps, a few hot
  crates at opt-level 3 (`crates/Cargo.toml`, `apps/*/Cargo.toml`); `lto` and `codegen-units = 1` only in `[profile.dist]`
  (installers). Nothing to cut there for test jobs.
- **Build once, test many** (`cargo nextest archive`): each Rust workspace is tested in one job per OS, and the jobs that
  share a workspace use different profiles or instrumentation (coverage against plain tests). An archive would be built and
  used once; its value is splitting a long test run across machines (partitioning), which only Windows `paddock-dev` (604 s)
  might need. `cargo-hakari` is already in use (the "workspace hack" step).
- **Linux Rust jobs start from an empty target folder** in the container (`.gitea/actions/build-folder/action.yml`: "the
  workspace's target/ in a Linux container"), while host runners keep one per app and slot. Whether a kept per-slot Linux
  target would beat 95 to 100% sccache hits is **not measured**: the macOS jobs with kept folders do not compile faster per
  job (110 s for `crate` against Linux's 46 s, on other hardware).

## 5. Work the change did not need

- **The main push re-runs a green train round**: about 250 Linux job-min per landing (8335 + 8338's Linux). D4 removes it.
- **The plan job waits for the runner reservation inside a Linux job**: 794 s in 8335's plan, 64 s in 8338's, and **1,861 s
  in 8347's** (it waited for 8346, the same train's `ci.yml`), so a Linux lane sat idle for 13 and 31 minutes. A waiter
  holding a lane is ER-43's class. D2 (both workflows of a train at once) removes the wait between a train's two runs; what
  is left is a lane held while another train's run finishes. With CPU the limit it costs a lane, not CPU.
- **`./dev` built twice in one Windows job** (1.2): 169 s, a defect, not a design choice.
- **Pre-push against CI**: the pre-push gates are the static ones (fmt, clippy, lockfiles, commit-lint, member audits:
  `tools/paddock-dev/src/pre_push/gates.rs`); CI runs them again on the combined tree, as ER-8 and ER-9 require. Their CI cost
  is under 3 job-min a round. Not a cut.
- **Affected plan on trains**: a train run is full by design (ER-8: the full suite once on the combined result). No job ran
  that a full plan would not.
- **Empty doctest crates on Windows**: 32 to 48 s a round (4).

## 6. Common practice (ER-37)

| Practice | Source | Here |
|---|---|---|
| A fixed CPU share per job | GitHub-hosted Linux runners give each job 4 vCPU and 16 GB (public repos) or 2 vCPU and 8 GB (private): <https://docs.github.com/en/actions/reference/runners/github-hosted-runners> | Linux jobs take all 30 cores each (1.3) |
| Build once, fan out | nextest archives: build test binaries once, run elsewhere with `--archive-file`, the same source checked out: <https://nexte.st/docs/ci-features/archiving/>; Bazel's remote cache stores action outputs by hash so CI downloads them instead of re-running: <https://bazel.build/remote/caching> | The real repeats are small (1.2); sccache already plays the remote-cache role for rustc |
| Cache keyed on inputs | `actions/cache` keys from `hashFiles(...)`; an exact hit restores, a miss saves a new key: <https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching> | npm and Angular caches do this; `ui/dist` and `./dev` do not |
| Local object store for clones | `git clone --reference` borrows objects from a local repository "to reduce network and local storage costs": <https://git-scm.com/docs/git-clone> | Kept clones on hosts; Linux containers fetch each time |
| Browsers in the image, not cached | Playwright: a prebuilt Docker image; caching browsers "is not recommended" since restoring takes about as long as downloading: <https://playwright.dev/docs/ci> | Done (the `paddock-ci` image) |
| Workers on CI | Playwright advises `workers: 1` on CI for stability, more on powerful self-hosted machines (<https://playwright.dev/docs/ci>) | 1, Debrief 2 |
| Balanced shards | `fullyParallel: true` splits by test, otherwise by file; blob reports merged with `merge-reports`: <https://playwright.dev/docs/test-sharding> | In flight (Dyno WebKit shards) |
| Test impact analysis | `nx affected` runs only tasks whose declared inputs changed: <https://nx.dev/docs/features/ci-features/affected> | The affected plan on branches; trains stay full (ER-8) |
| Dev server or a build for e2e | Playwright's `webServer` page shows a dev server (`npm run start`) for local work and does not prescribe one for CI: <https://playwright.dev/docs/test-webserver> | Debrief and Pitwall chose the production build for an ER-12 reason; three suites have not |

## 7. Ranked cuts

"Per round" is one full train round (`ci.yml` and `debrief.yml`, WebKit on). "Measured" savings are the measured cost of
the removed work; "est." states its basis. All costs are agent-days.

| # | Cut | Saving per round | Cost | Risk to quality | Branch |
|---|---|---|---|---|---|
| 1 | **A CPU share per Linux job**: `PADDOCK_CI_JOBS_PER_HOST=6` for `d-gtr9` (its `runner.envs`), so `build-jobs.sh`'s formula gives `CARGO_BUILD_JOBS` and `NEXTEST_TEST_THREADS` of 5 each; fix the stale Linux row of its table; A/B with the sampler over two rounds | Not measured. Bound: at 6 lanes Linux jobs took 1.2 to 1.7 times as long (1.3); if oversubscription is half of it, about 15 to 25% of Linux job-min, **40 to 60 job-min** (est.) | 0.25 + the user's runner setting + 0.5 for the A/B | None to the gates. Fewer threads per job lowers starvation of timing-sensitive tests (ER-12 class); a lone job on an idle host compiles slower | New; measured with `feat/ci-host-sampler` |
| 2 | **`./dev` once per inputs hash, and never twice in one job** (the `hostRustChecks` step must use the job's tool, `PADDOCK_DEV`, not the wrapper's second target) | Measured: **20.3 job-min** in 8346, 8.5 in 8338; 169 s off the Windows Dyno critical path | 0.25 on top of D3 (a failing test first: one `dev: building` line per job at most) | None | D3 (in flight); the second build is a new finding for its scope |
| 3 | **Skip the main push's re-run of a green round** | Measured: about **250 Linux job-min per landing** | (D4) | None if landing keeps requiring the full green run on the exact commit | D4 (in flight) |
| 4 | **One production build served for Dyno, Spotter and the reference apps' e2e**, through the shared `serve-prod.ts` Debrief and Pitwall use (ER-22, one mechanism) | Not measured (dev server and 1.5 s polling for 25+ job-min of Dyno e2e). Est.: the build costs 10 to 20 s per job once; the gain is the dev server's CPU, to be read from the sampler | 1 | Positive: tests the shipped bundle under the CSP, and removes the stalled-module class Pitwall hit (run 7854). Needs 20 runs per engine before the switch (ER-28) | New |
| 5 | **Build `ui/dist` once per content hash** and restore it, like the Angular cache (`/sccache`); `appBuildGuard`'s content stamps still refuse a stale one | Measured: **4.7 job-min** (18 builds at 13 to 24 s; 221 s in 8346's `ci.yml` alone) | 0.5 to 1 | None: the build guard checks the stamps (`ui/projects/paddock-e2e/src/build-guard.ts`) | New |
| 6 | **Linux checkouts borrow a host mirror** (`--reference` to a repository on `/sccache`, the fetch still at the exact commit) | Measured cost 9.8 Linux job-min; est. saving 5 to 7 (fetch 6 to 25 s to about 2 s) | 0.5 to 1 | None: the same commit is checked out; a missing mirror falls back to a normal fetch | New |
| 7 | **Dyno `page-layout.spec.ts`: one load per page and theme, sizes changed in place**, keeping one fresh load per page | Est. 3 to 6 job-min (146 of 194 loads per engine avoided at 0.55 to 0.88 s each, plus re-render) | 0.5 | Stated: a resize is a different path from a first layout at that size; keeping a fresh load at one size per page and the 24-page coverage limits it. A decision | New |
| 8 | **Windows tests: spawn cost** (fixture git starts in `merge_check`, `queue`, `golden`) through the ADR 0108 bulk seams | Est. 3 to 5 min off the Windows critical path if Windows reaches the Mac's test-seconds (4,334 against 2,545) | 2 to 3 | None | ADR 0108 work |
| 9 | **Doc tests only where a crate has one** (a generated list, ER-40; every doctest still runs on every OS) | Measured: 32 to 48 s on Windows, under 15 s elsewhere | 0.25 | None if the list is generated and checked | New |
| 10 | **Debrief WebKit per test**: profile the 10 slowest WebKit specs with traces (`live-tail-follow`, `dock-vs`, `popout`, `dock-panels`) before any change | Debrief WebKit is 5,086 test-s (85 test-min) a round; what can be cut is **not measured** | 1 (research) | None (measurement) | New |
| 11 | **The runner wait outside a Linux lane** (ER-43: a waiter takes no capacity): wait at dispatch only, or on a runner class that does no work | Measured: 13 and 31 minutes of one Linux lane (8335, 8347); no CPU. D2 removes most of it | 0.5 | None | New; touches `runner-wait`; after D2 |
| 12 | Kept per-slot target folders for Linux Rust jobs | Not measured; the Mac evidence does not show a gain | 1 (A/B) | None | Not recommended before 1 |
| 13 | nextest archive and partitions | Small (4) | 1 | None | Only if Windows `paddock-dev` stays the critical path after 8 |
| 14 | Bazel or Buck2 remote execution | Everything cached | 20+ | Large migration | Not recommended |

What the in-flight work already takes, per the brief: staged CI stops a red round after its fast stage; the fix-round
ladder reruns only failures; RustFS sccache fixes the per-slot cold caches (2); Dyno WebKit shards and `fullyParallel`
shorten the critical path but not the CPU; D2 runs both workflows at once; D3 builds `./dev` once; D4 skips the main
re-run.

## 8. Recommendation: the first five

1. **CPU share per Linux job (cut 1)**: cheapest and broadest; it changes every Linux job, and with CPU the limit,
   oversubscription is the one cost that grows with lanes. Measure it, do not guess: the sampler over a 6-lane round
   before and after. 0.75 days plus the user's runner setting.
2. **`./dev` once, and the double build (cut 2)**: 20 job-min a train round measured, plus 3 minutes on the Windows critical
   path. Add the double-build test to D3's scope. 0.25 days.
3. **Production-build e2e for Dyno, Spotter and the reference apps (cut 4)**: removes the largest piece of avoidable CPU
   left in the Playwright jobs that are not the tests themselves, and it improves quality. 1 day.
4. **`ui/dist` once per content hash (cut 5)**: 4.7 job-min measured, every round, every train. 0.5 to 1 day.
5. **Linux checkout mirror (cut 6)**: 5 to 7 job-min est. of 9.8 measured. 0.5 to 1 day.

Together, measured and estimated: about **70 to 90 Linux job-min a round** (of 250), plus cut 4's unmeasured part, plus D4's 250 per landing, for
about 3 to 3.5 agent-days. Cuts 7 to 11 are the next tier (about 4 to 5 days); 10 first, because Debrief WebKit is the
largest single block and its cause is not measured yet.

**The cheaper common-practice option**, for each item beyond it (ER-37):

- Cut 1: the common practice is a fixed share per job (GitHub's 2 to 4 vCPU per job); the cheaper form is the env setting
  alone, without the A/B. Recommended with the A/B, because a smaller share can lengthen a lone job.
- Cut 4: the cheaper option is to drop only `--poll 1500` from Dyno's `ng serve` (0.1 day); it keeps the dev server and its
  stalled-module class.
- Cut 6: the cheaper option is none: depth-1 fetches are already common practice; skip the cut if 1 to 5 suffice.
- Cut 7: the cheaper option is to leave the spec as it is.

### Risks

- A smaller CPU share can show tests that only passed with 30 threads (a hidden dependency on speed). That is a finding to
  fix (ER-12), not a reason to keep oversubscribing.
- The production build for the three suites can change what a spec sees (the CSP, no dev-mode checks): the switch runs the
  suites 20 times per engine first (ER-28), as Pitwall's move did.
- Every saving above stays an estimate until the sampler measures per-job CPU; the results block records the measured
  part only.

### Work packages

| WP | What | Days | Depends on |
|---|---|---|---|
| WP-1 | `build-jobs.sh`: the Linux row gets the formula; the user sets `PADDOCK_CI_JOBS_PER_HOST=6` on `d-gtr9`; sampler A/B over two 6-lane rounds, recorded in benchmarks.md | 0.75 | `feat/ci-host-sampler` |
| WP-2 | D3 scope: a failing test that a job prints at most one `dev: building paddock-dev`; `app-gates` steps call `$PADDOCK_DEV` | 0.25 | D3 |
| WP-3 | Dyno, Spotter, `ui` Playwright configs on `serve-prod.ts` with a prebuilt build per job; 20 runs per engine | 1 | none |
| WP-4 | `ui/dist` restored by a content hash of its inputs under `/sccache`, host runners under their CI home; build guard unchanged | 0.5 to 1 | none |
| WP-5 | Linux container checkout with a `--reference` mirror on `/sccache`, refreshed by the plan job, falling back to a plain fetch | 0.5 to 1 | none |
| WP-6 | Debrief WebKit trace profile of the 10 slowest specs (one local run per spec, Linux container); a note with causes | 1 | none |
| WP-7 | Dyno `page-layout.spec.ts` restructure (after the user's D-3) | 0.5 | D-3 |
| WP-8 | Doctest crate list generated and checked; `cargo test --doc -p ...` | 0.25 | none |
| WP-9 | `runner-wait` without a lane | 0.5 | D2 |

## 9. Decisions for the user

- **D-1. Linux CPU share**: set `PADDOCK_CI_JOBS_PER_HOST=6` on `d-gtr9` and measure (recommended), or keep cargo on every
  core per job. It is your runner's setting.
- **D-2. e2e server**: production build for Dyno, Spotter and the reference apps (recommended), or only drop Dyno's
  `--poll 1500` (cheaper, keeps the dev server).
- **D-3. Dyno page-layout**: one load per page and theme with in-place resizes plus one fresh load per page, or keep a fresh
  load per case (the cheaper option, and no change in what it tests).

## Not measured

- Per-job CPU-seconds and the share of the 6-lane slowdown that is oversubscription (needs the sampler).
- The CPU of an Angular dev server against a static server during a Playwright job.
- What a kept Linux target folder would save against sccache hits.
- Why a Debrief WebKit test costs twice its Chromium run (needs traces).
- The WebKit legs and Debrief's bundles and smoke tests at 6 lanes: 8346 and 8347 ran without them (branch-mode plans).
  The first full 6-lane round gives them.

## Heads re-read before the commit

2026-11-20 01:40 UTC, unchanged: `origin/main` e0fd0f511991, `origin/train/2026-11-19-16` 0c44bc3a38dd,
`origin/docs/ci-speed-research` 93bbd80637b3, `origin/perf/sccache-hit-rate` 534918839350, `origin/feat/ci-host-sampler`
36afb26894df, `origin/perf/merge-check-batched-reads` bac2d6e3a9f4 (in main); `ci/staged-fast-then-long` and
`feat/train-fix-ladder` still not on origin.
