# CI timings measured for the Paddock family

Date: 2026-10-24. This is option 0 of [2026-10-ci-speed.md](2026-10-ci-speed.md): real numbers from the Gitea Actions
API for Debrief, Paddock, Pitcrew, Pitwall, Spotter and Dyno, replacing the estimates in that note. The tables are
produced by [scripts/ci-timings.mjs](../../scripts/ci-timings.mjs) (node, no dependencies, read-only API); re-run it with
`node scripts/ci-timings.mjs --out docs/research/2026-10-ci-timings.md` and only the region between the two marker
comments is replaced, so this text survives.

## Method and limits

- Sample: the last 30 successful or failed runs per workflow (fewer exist for the young apps), 2026-10-19 to 2026-10-24.
  Debrief's `build.yml` window (2026-10-21 to 24) has 88 runs of which 58 were cancelled by `cancel-in-progress` and 22
  failed, so its **wall times include failing runs** and the sample is a busy development stretch, not a quiet one.
- Per job: wall time from API `started_at`/`completed_at`, step durations from the API step timestamps (so no log
  parsing is needed for step times), runner name. Only successful jobs are timed.
- Compile versus test run comes from the job logs: cargo's `Finished ... in Xm Ys` lines, nextest's `Summary [ N s]`,
  Playwright's `N passed (Xm)`, Vitest's `Duration`, Angular's bundle time. Logs are cached per job id outside the repo.
- Runs are split into **with-bundles** (the run executed `bundle`/`release` jobs: tags and release candidates),
  **pull_request** and **push**.
- Queue time: the API gives one `created_at` per run, and the job `needs` graph is not exposed. "Delay" is the time from
  the previous job finishing (or the run being created) to the job starting, so it mixes `needs` waiting with queueing.
  "Queue, first wave" counts only jobs that started before any other job in the run had finished; that is pure queueing.
- The critical path is rebuilt from timestamps (the last job to finish, then repeatedly the job that finished last
  before it started). Where runs fail or are cancelled the chain varies from run to run, so read the shares as a guide.

## Headline numbers (Debrief `build.yml`, push runs, medians)

| item | measured |
| --- | --- |
| Run wall time | 24m (p90 39m); pull_request 40m (p90 58m); with bundles 39m (p90 44m) |
| Longest job | `end-to-end (shard 2/2)` 30m (p90 37m); shard 1 21m. It starts at once and **is the critical path**; nothing else comes within 15 min |
| `rust tests` | 12 to 13m: test-profile compile 6m50s, nextest run 2m30s, release compile 1m30s, memory budget 1m30 to 2m, doc tests 41s |
| `frontend build, lint + unit tests` | 4m10s: Vitest 2m25s, Angular builds 53s, lint 23s, `npm ci` 15s |
| `rust format + clippy` | 2m20s to 3m40s; cache restore 7s and save 6s |
| Per-job setup (checkout, toolchain, apt, setup-node) | about 20s in Debrief; 1 to 2 min in Pitwall and Pitcrew |
| Queue, first-wave jobs | median 4 to 5s on every runner; p90 4m10s (d-gtr9, d7-nas), 6m55s (Mac mini), 7m14s (Windows VM) |
| Slower runner | `d7-nas` is 1.5x slower than `d-gtr9` for the same job (median over 13 job names, up to 2.4x); an e2e shard 2 there took 37m versus 24m |

## Checking the estimates in 2026-10-ci-speed.md

| Estimate in the plan | Measured | Verdict |
| --- | --- | --- |
| Cold Linux compile "8 to 15 min" (option 1 assumption) | Debrief test-profile compile 6m50s plus release compile 1m30s: about 8m20s of a 12 to 13m job. Other apps: Pitcrew 2m28s, Pitwall 3m23s, Spotter 1m39s, Paddock 2m16s | **Confirmed at the low end for Debrief; far smaller elsewhere.** |
| "4 to 9 min off the Rust critical path" from sccache (option 1) | `rust tests` is 13m but the critical path is e2e at about 30m. Saving compile time cuts runner minutes and the Rust job, not the run's wall time until e2e drops below about 13m | **Corrected: runner-minute saving, not wall-clock, for now.** |
| Lint cache restore/save "72 to 163 s" | Restore 7s, save 6s median (rare outliers: one save of 11m). The big-cache problem was the test job, which has no target cache | **Corrected: obsolete for lint; the 72 to 163 s figure only applies to a test-job target cache.** |
| `apt-get`, toolchain, install-action, setup-node "20 to 90 s per Linux job" (option 2) | Debrief: apt 7s, rust-toolchain 8s, install-action 1 to 2s, setup-node 6s, checkout 4s: about 25s a job. Pitwall: apt 69s (max 336s), Playwright browsers 119s (max 1294s). Pitcrew: apt 48s (max 1260s) | **Confirmed for Pitwall and Pitcrew, overstated for Debrief.** The main value is removing the long-tail stalls (several minutes when apt or a download hangs), not the median. |
| e2e "about 10 min per shard" | Debrief shard 1 21m, shard 2 30m (p90 37m), 617 tests with 2 workers; Pitwall 2 to 5m, Spotter 1m50s, Dyno 1m50s, Paddock UI 3m | **Corrected: Debrief e2e is 2 to 3x the estimate, shard 2 about 1.4x shard 1, and it is the long pole of the whole workflow.** Re-balancing the shards, more shards and trimming WebKit now outrank every Rust option. |
| Build the UI once, "1 to 3 min per shard" (option 3) | One Angular build is about 25s (the whole `ng build` family is 53 to 78s in the frontend job); the shard `Install` step is 23s. The shard's build time is inside the 20 to 30 min test step | **Corrected: about 0.5 to 1 min per shard, not 1 to 3.** Worth doing only as a side effect. |
| Coverage adds "20 to 40 %" to the unit-test step (option 7) | Vitest is 2m25s in a 4m10s job that finishes 26 minutes before e2e; the coverage cost itself cannot be separated from these logs | **Not measurable here; no wall-clock effect** while e2e is the pole (runner-minutes only). |
| nextest archive pays off "only if the run phase exceeds about 6 min" (option 6) | nextest run is 2m30s (3670 tests, 221 binaries) | **Confirmed: skip it.** |
| Queue time "invisible" (option 5) | Median queue is 4 to 5s; the tail is the issue: p90 4 to 7 min, worst on the Mac mini and the Windows VM (single host runners). `d7-nas` takes about 19% of Linux jobs and is 1.5x slower, so unlucky placement alone adds about 13 min to an e2e shard | **Corrected: capacity is not a median problem; the slow runner and the host runners cause the tail.** |
| "Many pushes skip half the jobs" (option 4) | 58 of 88 Debrief build runs in the window were cancelled and 22 failed; only 8 completed green | **Strengthened: most CI minutes are spent on runs that are cancelled or failing, which supports path filtering and the "fix locally first" rules more than any compile speed-up.** |
| sccache is absent | Confirmed: no `sccache` or `RUSTC_WRAPPER` in the step names or logs | **Confirmed.** |

Reordering suggested by the data, for the owner to decide: (1) Debrief e2e (re-shard by measured file time, 3 to 4
shards, WebKit and prod-CSP only on main/nightly, `workers` from CPU count) because it alone sets the push wall time;
(2) move jobs off `d7-nas` or make it equal to `d-gtr9`; (3) the bind-mounted sccache and image bake (options 1 and 2)
for runner-minute savings and for Pitwall/Pitcrew setup stalls; (4) drop options 3, 6 and 7 from the wall-clock plan.
Re-run the script after each step and compare the headline table.

## Measured tables

<!-- ci-timings:begin -->
Generated by `scripts/ci-timings.mjs` on 2026-10-24; last 30 successful or failed runs per workflow (cancelled runs in the same span are only counted). 
Times: median / p90. "Delay" is the time from the previous job finishing (or the run being created) to the job starting, so it includes waiting for `needs` as well as queueing. Only successful jobs are timed.

### Overview

| repo | workflow (trigger) | sample | runs timed | success/failure/cancelled in window | run wall median | p90 | first job starts after (median) | critical path jobs-only | critical path waits |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| debrief | corpus.yml (push) | 2026-10-22 to 2026-10-24 | 3 | 3/1/0 | 3m05s | 4m06s | 2s | 3m02s | 2s |
| debrief | bench.yml (push) | 2026-10-21 to 2026-10-24 | 29 | 27/3/1 | 2m56s | 6m55s | 2s | 2m47s | 2s |
| debrief | build.yml (with-bundles) | 2026-10-21 to 2026-10-24 | 5 | 8/22/58 | 38m49s | 43m59s | 2s | 38m46s | 18s |
| debrief | build.yml (pull_request) | 2026-10-21 to 2026-10-24 | 7 | 8/22/58 | 39m33s | 57m44s | 22s | 38m39s | 72s |
| debrief | build.yml (push) | 2026-10-21 to 2026-10-24 | 18 | 8/22/58 | 24m20s | 38m35s | 17s | 23m39s | 37s |
| debrief | fuzz.yml (push) | 2026-10-19 to 2026-10-24 | 3 | 0/3/3 | 42m40s | 63m57s | 1s | 42m38s | 2s |
| debrief | mutants.yml (pull_request) | 2026-10-19 to 2026-10-23 | 6 | 4/2/26 | 15m23s | 20m27s | 14m23s | 51s | 14m23s |
| debrief | runner-check.yml (push) | 2026-10-22 to 2026-10-22 | 1 | 0/1/0 | 52s | 52s | 3s | 12s | 40s |
| paddock | ci.yml (with-bundles) | 2026-10-22 to 2026-10-24 | 3 | 12/6/11 | 2m49s | 3m05s | 22s | 1m35s | 28s |
| paddock | ci.yml (push) | 2026-10-22 to 2026-10-24 | 15 | 12/6/11 | 3m05s | 6m43s | 3s | 3m01s | 4s |
| paddock | publish.yml (push) | 2026-10-24 to 2026-10-24 | 1 | 1/0/0 | 6m10s | 6m10s | 1m31s | 4m38s | 1m32s |
| pitcrew | build.yml (with-bundles) | 2026-10-22 to 2026-10-24 | 14 | 17/9/8 | 32m16s | 54m34s | 74s | 16m29s | 14m29s |
| pitcrew | build.yml (push) | 2026-10-22 to 2026-10-24 | 12 | 17/9/8 | 15m37s | 29m52s | 4s | 8m55s | 5m56s |
| pitwall | build.yml (with-bundles) | 2026-10-22 to 2026-10-24 | 17 | 8/9/2 | 20m38s | 30m30s | 29s | 14m10s | 5m42s |
| pitwall | release.yml (with-bundles) | 2026-10-23 to 2026-10-23 | 4 | 4/0/0 | 16m10s | 33m16s | 2s | 9m04s | 7m07s |
| spotter | build.yml (with-bundles) | 2026-10-22 to 2026-10-24 | 2 | 7/12/7 | 36m27s | 53m26s | 1m50s | 24m51s | 11m36s |
| spotter | build.yml (push) | 2026-10-22 to 2026-10-24 | 17 | 7/12/7 | 16m25s | 23m48s | 4s | 10m51s | 3m25s |
| dyno | build.yml (with-bundles) | 2026-10-23 to 2026-10-24 | 3 | 0/6/2 | 36m55s | 47m23s | 2m15s | 16m24s | 20m15s |
| dyno | build.yml (push) | 2026-10-23 to 2026-10-24 | 3 | 0/6/2 | 16m11s | 65m00s | 4m18s | 11m55s | 4m25s |

### Runners (all repos, all timed runs)

| runner | successful jobs | busy time | first-wave jobs | queue median | queue p90 |
| --- | --- | --- | --- | --- | --- |
| Gitea Action Runner (d-gtr9) | 513 | 2511m28s | 249 | 5s | 4m15s |
| decryp7ology-dev | 120 | 850m13s | 37 | 4s | 7m14s |
| d7-mac-mini | 122 | 805m32s | 50 | 4s | 6m55s |
| Gitea Action Runner (d7-nas) | 120 | 688m00s | 64 | 5s | 4m10s |

### debrief / corpus.yml (push)

Timed on 3 successful runs.

Critical path (rebuilt from timestamps; share = fraction of runs where the job is on it):

| job | share | wall (median) | delay before it (median) |
| --- | --- | --- | --- |
| real-world EVTX corpus | 100% | 3m02s | 2s |

Jobs:

| job | n | wall median | p90 | delay median | queue, first-wave runs (median; n) | runners (count; median wall) | log phases (median) | biggest steps (median) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| real-world EVTX corpus | 3 | 3m02s | 4m04s | 2s | 2s; 3 | (d7-nas) 1; 4m19s, (d-gtr9) 2; 2m39s | 252 crates compiled; cargo build (test) 1m32s | EVTX-ATTACK-SAMPLES through the EVTX importer 2m41s; dtolnay/rust-toolchain@stable 8s; actions/checkout@v4 3s; actions/cache@v4 0s |

Step categories per job (median seconds summed per job run; categories by step name):

| job | categories |
| --- | --- |
| real-world EVTX corpus | other 2m41s; toolchain / tools 8s; checkout 3s |

### debrief / bench.yml (push)

Timed on 27 successful runs.

Critical path (rebuilt from timestamps; share = fraction of runs where the job is on it):

| job | share | wall (median) | delay before it (median) |
| --- | --- | --- | --- |
| engine benchmarks | 96% | 2m46s | 2s |
| plan | 7% | 5s | 2s |
| store benchmarks (10M events) | 4% | 8m48s | 33s |

Jobs:

| job | n | wall median | p90 | delay median | queue, first-wave runs (median; n) | runners (count; median wall) | log phases (median) | biggest steps (median) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| store benchmarks (10M events) | 1 | 8m48s | 8m48s | 33s | 33s; 1 | (d-gtr9) 1; 8m48s | 283 crates compiled; cargo build (release) 3m07s | Import, index and reads at 10M events (budgets asserted) 6m36s; The worker's memory budgets (20 copies of the samples) 46s; dtolnay/rust-toolchain@stable 8s; actions/checkout@v4 5s; libduckdb (prebuilt, pinned SHA-256) 1s |
| engine benchmarks | 29 | 2m47s | 4m43s | 2s | 2s; 27 | (d7-nas) 12; 3m31s, (d-gtr9) 17; 2m18s | 5 crates compiled; cargo build (bench) 53s | Benchmarks (quick) 1m35s; dtolnay/rust-toolchain@stable 11s; Upload criterion report 7s; actions/cache/save@v4 6s; actions/cache/restore@v4 3s |
| plan | 2 | 5s | 7s | 2s | 2s; 2 | (d7-nas) 2; 5s | - | actions/cache/restore@v4 0s; Decide 0s |

Step categories per job (median seconds summed per job run; categories by step name):

| job | categories |
| --- | --- |
| store benchmarks (10M events) | other 6m36s; rust tests (compile + run) 46s; toolchain / tools 9s; checkout 5s |
| engine benchmarks | other 1m42s; toolchain / tools 11s; cache save 6s; cache restore 4s |
| plan |  |

### debrief / build.yml (with-bundles)

Timed on 5 all finished (fewer than 3 successful) runs.

Critical path (rebuilt from timestamps; share = fraction of runs where the job is on it):

| job | share | wall (median) | delay before it (median) |
| --- | --- | --- | --- |
| end-to-end (shard 2/2) | 40% | 34m33s | 1s |
| bundle (windows-latest) | 40% | 22m35s | 0s |
| real-app smoke (windows-latest) | 40% | 7m57s | 8s |
| licences, notices + commits | 40% | 3m02s | 36s |
| rust format + clippy | 40% | 2m11s | 2s |
| release | 40% | 40s | 1s |
| plan | 40% | 2s | 3s |
| end-to-end (shard 1/2) | 20% | 20m46s | 0s |

Jobs:

| job | n | wall median | p90 | delay median | queue, first-wave runs (median; n) | runners (count; median wall) | log phases (median) | biggest steps (median) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| end-to-end (shard 2/2) | 4 | 30m28s | 35m23s | 1s | - | (d-gtr9) 3; 28m28s, (d7-nas) 1; 36m39s | playwright run 29m15s | Frontend end-to-end tests 29m16s; Install 23s; actions/setup-node@v4 6s; actions/checkout@v4 5s; actions/cache@v4 1s |
| bundle (windows-latest) | 5 | 22m46s | 24m55s | 0s | - | decryp7ology-dev 5; 22m46s | ng build bundle 27s; 52 crates compiled; cargo build (dist) 18m17s | Build installers 21m40s; actions/checkout@v4 22s; actions/upload-artifact@v3 6s; dtolnay/rust-toolchain@stable 3s; libduckdb (prebuilt, pinned SHA-256) 1s |
| end-to-end (shard 1/2) | 4 | 21m44s | 24m20s | 0s | - | (d-gtr9) 4; 21m44s | playwright run 20m27s | Frontend end-to-end tests 20m30s; Install 24s; actions/checkout@v4 5s; actions/setup-node@v4 5s; actions/cache@v4 3s |
| rust tests | 2 | 21m29s | 28m04s | 5s | 5s; 2 | (d-gtr9) 1; 13m15s, (d7-nas) 1; 29m43s | 991 crates compiled; cargo build (test) 11m39s; nextest run 4m13s; cargo build (release) 3m11s | Tests 16m13s; Memory budget 3m31s; Doc tests 45s; Linux dependencies for Tauri 8s; actions/checkout@v4 7s |
| bundle (ubuntu-latest) | 5 | 13m14s | 14m18s | 1s | - | (d-gtr9) 5; 13m14s | ng build bundle 24s; 52 crates compiled; cargo build (dist) 11m28s | Build installers 12m17s; actions/setup-node@v4 8s; actions/upload-artifact@v3 8s; Linux dependencies for Tauri 7s; actions/cache@v4 5s |
| bundle (macos-latest) | 5 | 12m47s | 22m23s | 3s | - | d7-mac-mini 5; 12m47s | ng build bundle 11s; 52 crates compiled; cargo build (dist) 10m21s | Build installers 10m56s; actions/setup-node@v4 25s; actions/cache@v4 14s; actions/checkout@v4 7s; actions/upload-artifact@v3 7s |
| real-app smoke (windows-latest) | 3 | 7m00s | 7m42s | 2s | - | decryp7ology-dev 3; 7m00s | 52 crates compiled; cargo build (dev) 3m35s; ng build bundle 36s | Debug app binary 4m26s; Smoke tests (Windows) 2m01s; actions/checkout@v4 19s; dtolnay/rust-toolchain@stable 3s; Edge WebDriver matching WebView2 1s |
| real-app smoke (macos-latest) | 2 | 6m40s | 9m18s | 1s | - | d7-mac-mini 2; 6m40s | ng build bundle 10s; 416 crates compiled; cargo build (dev) 4m50s | Debug .app (macOS) 5m10s; actions/setup-node@v4 23s; Smoke test, sidecars and quit within 10 s (macOS) 19s; actions/checkout@v4 8s; Git bash on PATH and a persistent build folder (Windows) 0s |
| real-app smoke (ubuntu-latest) | 3 | 4m35s | 16m07s | 2m07s | - | (d7-nas) 2; 11m48s, (d-gtr9) 1; 3m26s | 52 crates compiled; cargo build (dev) 1m51s; ng build bundle 23s | Debug app binary 2m31s; Smoke tests (Linux) 51s; Linux dependencies for Tauri and WebDriver 9s; actions/setup-node@v4 7s; actions/checkout@v4 6s |
| frontend build, lint + unit tests | 4 | 4m18s | 5m01s | 3s | 35s; 2 | (d-gtr9) 4; 4m18s | ng build bundle 79s; vitest 1m51s | Frontend unit tests 2m24s; Translations are extracted and the pseudo-locale builds 29s; Frontend build 26s; Lint 22s; Install 14s |
| licences, notices + commits | 3 | 2m19s | 2m43s | 4s | 4s; 3 | (d-gtr9) 3; 2m19s | - | Third-party notices are up to date 64s; actions/checkout@v4 14s; Licences, advisories and sources 8s; actions/setup-node@v4 4s; actions/cache/restore@v4 3s |
| rust format + clippy | 5 | 2m14s | 4m33s | 1s | 2s; 3 | (d-gtr9) 4; 3m02s, (d7-nas) 1; 2m07s | 3 crates compiled; cargo build (dev) 73s | Clippy 74s; actions/checkout@v4 11s; Linux dependencies for Tauri 6s; Format 5s; actions/cache/restore@v4 4s |
| plugin SDK templates | 5 | 88s | 1m56s | 2s | 4s; 3 | (d-gtr9) 5; 88s | 202 crates compiled; cargo build (debug) 21s; cargo build (dev) 25s; cargo build (test) 0s; cargo build (release) 19s | A new importer passes cargo debrief-plugin test, packages and signs 30s; Install cargo debrief-plugin 21s; New projects of every kind build 20s; actions/checkout@v4 3s; dtolnay/rust-toolchain@stable 1s |
| release | 2 | 40s | 41s | 1s | - | (d-gtr9) 2; 40s | - | SBOMs (CycloneDX) 18s; actions/checkout@v4 5s; actions/setup-node@v4 4s; Publish the release 3s; actions/download-artifact@v3 2s |
| plan | 2 | 2s | 2s | 3s | 3s; 2 | (d-gtr9) 2; 2s | - | actions/checkout@v4 0s; actions/cache/restore@v4 0s; Decide 0s |

Step categories per job (median seconds summed per job run; categories by step name):

| job | categories |
| --- | --- |
| end-to-end (shard 2/2) | playwright run (e2e) 29m16s; npm ci 23s; toolchain / tools 6s; checkout 5s |
| bundle (windows-latest) | bundle / app build 21m42s; checkout 22s; other 6s; toolchain / tools 4s |
| end-to-end (shard 1/2) | playwright run (e2e) 20m30s; npm ci 24s; checkout 5s; toolchain / tools 5s; cache restore 3s |
| rust tests | rust tests (compile + run) 20m29s; apt / system libs 8s; checkout 7s; toolchain / tools 6s; cache restore 4s |
| bundle (ubuntu-latest) | bundle / app build 12m17s; cache restore 16s; toolchain / tools 10s; other 8s; apt / system libs 7s; checkout 4s |
| bundle (macos-latest) | bundle / app build 11m02s; cache restore 28s; toolchain / tools 25s; checkout 7s; other 7s |
| real-app smoke (windows-latest) | bundle / app build 6m23s; checkout 19s; toolchain / tools 5s |
| real-app smoke (macos-latest) | other 5m10s; toolchain / tools 23s; bundle / app build 19s; cache restore 15s; checkout 8s |
| real-app smoke (ubuntu-latest) | bundle / app build 3m22s; cache restore 18s; toolchain / tools 9s; apt / system libs 9s; checkout 6s |
| frontend build, lint + unit tests | vitest / unit tests 2m24s; ng build 54s; lint / format / audit 22s; npm ci 14s; toolchain / tools 5s; checkout 3s |
| licences, notices + commits | other 64s; checkout 14s; lint / format / audit 8s; toolchain / tools 6s; cache restore 3s; cache save 3s |
| rust format + clippy | lint / format / audit 83s; checkout 11s; apt / system libs 6s; cache restore 5s |
| plugin SDK templates | other 74s; checkout 3s |
| release | other 24s; checkout 5s; toolchain / tools 5s |
| plan |  |

### debrief / build.yml (pull_request)

Timed on 7 all finished (fewer than 3 successful) runs.

Critical path (rebuilt from timestamps; share = fraction of runs where the job is on it):

| job | share | wall (median) | delay before it (median) |
| --- | --- | --- | --- |
| rust format + clippy | 86% | 4m38s | 22s |
| end-to-end (shard 2/2) | 71% | 32m48s | 1s |
| end-to-end (shard 1/2) | 29% | 34m39s | 6s |
| licences, notices + commits | 29% | 2m23s | 2m08s |

Jobs:

| job | n | wall median | p90 | delay median | queue, first-wave runs (median; n) | runners (count; median wall) | log phases (median) | biggest steps (median) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| end-to-end (shard 2/2) | 3 | 30m45s | 34m27s | 1s | - | (d-gtr9) 3; 30m45s | playwright run 29m12s | Frontend end-to-end tests 29m12s; Install 24s; actions/setup-node@v4 6s; actions/checkout@v4 4s; actions/cache@v4 4s |
| end-to-end (shard 1/2) | 5 | 24m19s | 32m36s | 1s | - | (d-gtr9) 4; 23m07s, (d7-nas) 1; 37m38s | playwright run 22m06s | Frontend end-to-end tests 22m10s; Install 1m37s; actions/setup-node@v4 4s; actions/cache@v4 4s; actions/checkout@v4 2s |
| rust tests | 5 | 13m25s | 14m17s | 66s | 66s; 5 | (d-gtr9) 5; 13m25s | 991 crates compiled; cargo build (test) 6m43s; nextest run 2m42s; cargo build (release) 1m44s | Tests 9m08s; Memory budget 1m57s; Linux dependencies for Tauri 59s; Doc tests 41s; dtolnay/rust-toolchain@stable 9s |
| real-app smoke (windows-latest) | 6 | 6m39s | 7m21s | 24s | - | decryp7ology-dev 6; 6m39s | 52 crates compiled; cargo build (dev) 3m10s; ng build bundle 30s | Debug app binary 3m56s; Smoke tests (Windows) 2m05s; actions/checkout@v4 18s; dtolnay/rust-toolchain@stable 3s; libduckdb (prebuilt, pinned SHA-256) 1s |
| real-app smoke (ubuntu-latest) | 5 | 4m41s | 7m02s | 28s | - | (d-gtr9) 5; 4m41s | 52 crates compiled; cargo build (dev) 1m32s; ng build bundle 16s | Debug app binary 2m15s; Linux dependencies for Tauri and WebDriver 48s; Smoke tests (Linux) 43s; dtolnay/rust-toolchain@stable 8s; actions/setup-node@v4 8s |
| frontend build, lint + unit tests | 6 | 4m09s | 4m28s | 1s | - | (d-gtr9) 5; 4m03s, (d7-nas) 1; 4m16s | ng build bundle 78s; vitest 1m48s | Frontend unit tests 2m24s; Translations are extracted and the pseudo-locale builds 26s; Frontend build 24s; Lint 23s; Install 15s |
| rust format + clippy | 7 | 3m43s | 6m05s | 22s | 22s; 7 | (d-gtr9) 4; 4m09s, (d7-nas) 3; 3m43s | 3 crates compiled; cargo build (dev) 64s | Linux dependencies for Tauri 87s; Clippy 66s; actions/cache/restore@v4 9s; dtolnay/rust-toolchain@stable 8s; Format 6s |
| licences, notices + commits | 7 | 2m30s | 5m44s | 1s | 3m43s; 3 | (d-gtr9) 6; 2m28s, (d7-nas) 1; 10m13s | - | Third-party notices are up to date 1m50s; dtolnay/rust-toolchain@stable 8s; actions/setup-node@v4 7s; actions/cache/restore@v4 4s; actions/cache/save@v4 4s |
| plugin SDK templates | 7 | 1m40s | 2m20s | 2m36s | 2m36s; 7 | (d-gtr9) 7; 1m40s | 202 crates compiled; cargo build (debug) 24s; cargo build (dev) 19s; cargo build (test) 0s; cargo build (release) 16s | A new importer passes cargo debrief-plugin test, packages and signs 32s; Install cargo debrief-plugin 24s; A new importer passes cargo imt-plugin test, packages and signs 21s; Install cargo imt-plugin 20s; New projects of every kind build 13s |

Step categories per job (median seconds summed per job run; categories by step name):

| job | categories |
| --- | --- |
| end-to-end (shard 2/2) | playwright run (e2e) 29m12s; npm ci 24s; toolchain / tools 6s; checkout 4s; cache restore 4s |
| end-to-end (shard 1/2) | playwright run (e2e) 22m10s; npm ci 1m37s; toolchain / tools 4s; cache restore 4s |
| rust tests | rust tests (compile + run) 12m00s; apt / system libs 59s; toolchain / tools 10s; cache restore 3s |
| real-app smoke (windows-latest) | bundle / app build 5m59s; checkout 18s; toolchain / tools 5s |
| real-app smoke (ubuntu-latest) | bundle / app build 2m57s; apt / system libs 48s; cache restore 25s; toolchain / tools 16s; checkout 4s |
| frontend build, lint + unit tests | vitest / unit tests 2m24s; ng build 51s; lint / format / audit 23s; npm ci 15s; toolchain / tools 6s |
| rust format + clippy | apt / system libs 87s; lint / format / audit 76s; cache restore 10s; toolchain / tools 8s |
| licences, notices + commits | other 1m51s; toolchain / tools 16s; cache restore 4s; cache save 4s; lint / format / audit 3s |
| plugin SDK templates | other 58s; toolchain / tools 9s |

### debrief / build.yml (push)

Timed on 6 successful runs.

Critical path (rebuilt from timestamps; share = fraction of runs where the job is on it):

| job | share | wall (median) | delay before it (median) |
| --- | --- | --- | --- |
| end-to-end (shard 2/2) | 67% | 26m10s | 1s |
| licences, notices + commits | 67% | 1m53s | 50s |
| plan | 33% | 37s | 3s |

Jobs:

| job | n | wall median | p90 | delay median | queue, first-wave runs (median; n) | runners (count; median wall) | log phases (median) | biggest steps (median) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| end-to-end (shard 2/2) | 8 | 23m16s | 32m28s | 1s | - | (d-gtr9) 7; 22m47s, (d7-nas) 1; 39m17s | playwright run 18m51s | Frontend end-to-end tests 18m53s; Install 1m46s; actions/setup-node@v4 8s; actions/cache@v4 3s; actions/checkout@v4 2s |
| end-to-end (shard 1/2) | 11 | 19m23s | 27m42s | 1s | 41s; 2 | (d-gtr9) 11; 19m23s | playwright run 16m12s | Frontend end-to-end tests 16m15s; Install 1m57s; actions/setup-node@v4 5s; actions/cache@v4 3s; actions/checkout@v4 2s |
| rust tests | 4 | 9m44s | 12m03s | 50s | 50s; 4 | (d-gtr9) 4; 9m44s | 798 crates compiled; cargo build (test) 5m24s; nextest run 1m30s; cargo build (release) 78s (2 runs) | Tests 6m45s; Memory budget 1m31s; Linux dependencies for Tauri 45s; Doc tests 36s; dtolnay/rust-toolchain@stable 5s |
| real-app smoke (windows-latest) | 13 | 9m15s | 24m07s | 4s | - | decryp7ology-dev 13; 9m15s | 49 crates compiled; cargo build (dev) 5m43s; ng build bundle 32s | Debug app binary 6m25s; Smoke tests (Windows) 2m03s; actions/checkout@v4 23s; dtolnay/rust-toolchain@stable 4s; Edge WebDriver matching WebView2 2s |
| real-app smoke (ubuntu-latest) | 13 | 5m44s | 8m23s | 1s | - | (d7-nas) 6; 5m41s, (d-gtr9) 7; 6m43s | 49 crates compiled; cargo build (dev) 1m34s; ng build bundle 19s; cargo build (release) 14s (1 runs) | Debug app binary 2m18s; Linux dependencies for Tauri and WebDriver 1m30s; Smoke tests (Linux) 47s; dtolnay/rust-toolchain@stable 12s; actions/setup-node@v4 8s |
| rust format + clippy | 13 | 3m55s | 6m07s | 23s | 23s; 13 | (d-gtr9) 5; 2m27s, (d7-nas) 8; 4m03s | 3 crates compiled; cargo build (dev) 63s | Linux dependencies for Tauri 1m37s; Clippy 67s; actions/cache/restore@v4 20s; dtolnay/rust-toolchain@stable 13s; actions/checkout@v4 3s |
| frontend build, lint + unit tests | 8 | 3m38s | 6m01s | 1s | 15s; 4 | (d-gtr9) 8; 3m38s | ng build bundle 74s; vitest 85s | Frontend unit tests 1m57s; Translations are extracted and the pseudo-locale builds 27s; Frontend build 25s; Lint 22s; Install 15s |
| licences, notices + commits | 14 | 1m55s | 3m00s | 28s | 31s; 13 | (d-gtr9) 14; 1m55s | - | Third-party notices are up to date 72s; dtolnay/rust-toolchain@stable 9s; actions/setup-node@v4 6s; actions/cache/save@v4 4s; actions/cache/restore@v4 3s |
| plugin SDK templates | 14 | 84s | 1m55s | 65s | 65s; 14 | (d-gtr9) 14; 84s | 202 crates compiled; cargo build (debug) 19s; cargo build (dev) 22s; cargo build (test) 0s; cargo build (release) 18s | A new importer passes cargo debrief-plugin test, packages and signs 34s; A new importer passes cargo imt-plugin test, packages and signs 27s; Install cargo debrief-plugin 21s; Install cargo imt-plugin 19s; New projects of every kind build 18s |
| plan | 2 | 37s | 59s | 3s | 3s; 2 | (d-gtr9) 2; 37s | - | actions/checkout@v4 2s; actions/cache/restore@v4 0s; Decide 0s |

Step categories per job (median seconds summed per job run; categories by step name):

| job | categories |
| --- | --- |
| end-to-end (shard 2/2) | playwright run (e2e) 18m53s; npm ci 1m46s; toolchain / tools 8s; cache restore 3s |
| end-to-end (shard 1/2) | playwright run (e2e) 16m15s; npm ci 1m57s; toolchain / tools 5s; cache restore 3s |
| rust tests | rust tests (compile + run) 8m05s; apt / system libs 45s; toolchain / tools 7s; checkout 3s; cache restore 3s |
| real-app smoke (windows-latest) | bundle / app build 8m25s; checkout 23s; toolchain / tools 6s |
| real-app smoke (ubuntu-latest) | bundle / app build 3m00s; apt / system libs 1m30s; cache restore 32s; toolchain / tools 22s |
| rust format + clippy | apt / system libs 1m37s; lint / format / audit 70s; cache restore 21s; toolchain / tools 13s; checkout 3s |
| frontend build, lint + unit tests | vitest / unit tests 1m57s; ng build 51s; lint / format / audit 22s; npm ci 15s; toolchain / tools 6s |
| licences, notices + commits | other 72s; toolchain / tools 19s; cache save 4s; cache restore 3s |
| plugin SDK templates | other 67s; toolchain / tools 10s |
| plan |  |

### debrief / fuzz.yml (push)

Timed on 3 all finished (fewer than 3 successful) runs.

Critical path (rebuilt from timestamps; share = fraction of runs where the job is on it):

| job | share | wall (median) | delay before it (median) |
| --- | --- | --- | --- |
| fuzz (shard 6/6) | 33% | 35m32s | 0s |
| fuzz (shard 5/6) | 33% | 33m42s | 2s |
| fuzz_text | 33% | 8m54s | 0s |
| fuzz_csv | 33% | 8m50s | 2s |
| fuzz_profile_toml | 33% | 8m48s | 0s |
| fuzz_prof_nettrace | 33% | 7m54s | 0s |
| fuzz_w3c_access_har | 33% | 7m48s | 1s |
| fuzz_json | 33% | 7m46s | 1s |

Jobs:

| job | n | wall median | p90 | delay median | queue, first-wave runs (median; n) | runners (count; median wall) | log phases (median) | biggest steps (median) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| fuzz (shard 4/6) | 1 | 53m00s | 53m00s | 1s | 1s; 1 | (d-gtr9) 1; 53m00s | 275 crates compiled; cargo build (release) 6m04s | Fuzz ${{ join(matrix.targets, ', ') }} 45m14s; Build every fuzz target 6m03s; dtolnay/rust-toolchain@master 33s; actions/checkout@v4 3s; actions/cache@v4 0s |
| fuzz (shard 3/6) | 1 | 48m02s | 48m02s | 1s | 1s; 1 | (d-gtr9) 1; 48m02s | 275 crates compiled; cargo build (release) 6m03s | Fuzz ${{ join(matrix.targets, ', ') }} 40m12s; Build every fuzz target 6m02s; dtolnay/rust-toolchain@master 33s; actions/checkout@v4 3s; actions/cache@v4 0s |
| fuzz (shard 1/6) | 1 | 47m09s | 47m09s | 1s | 1s; 1 | (d-gtr9) 1; 47m09s | 275 crates compiled; cargo build (release) 6m04s | Fuzz ${{ join(matrix.targets, ', ') }} 40m12s; Build every fuzz target 6m03s; dtolnay/rust-toolchain@master 33s; actions/checkout@v4 3s; actions/cache@v4 0s |
| fuzz_archive | 2 | 9m26s | 9m43s | 1s | 1s; 2 | (d-gtr9) 1; 9m47s, (d7-nas) 1; 9m05s | 215 crates compiled; cargo build (release) 3m33s | Fuzz ${{ matrix.target }} 8m19s; Install cargo-fuzz 17s; dtolnay/rust-toolchain@nightly 12s; actions/checkout@v4 2s; actions/cache@v4 1s |
| fuzz_eventlog | 2 | 9m03s | 9m09s | 3s | 3s; 2 | (d7-nas) 1; 9m10s, (d-gtr9) 1; 8m55s | 215 crates compiled; cargo build (release) 3m39s | Fuzz ${{ matrix.target }} 8m23s; Install cargo-fuzz 19s; dtolnay/rust-toolchain@nightly 11s; actions/checkout@v4 2s; actions/cache@v4 1s |
| fuzz_csv | 2 | 8m46s | 8m49s | 2s | 2s; 2 | (d-gtr9) 2; 8m46s | 215 crates compiled; cargo build (release) 3m27s | Fuzz ${{ matrix.target }} 8m14s; Install cargo-fuzz 16s; dtolnay/rust-toolchain@nightly 10s; actions/checkout@v4 1s; actions/cache@v4 0s |
| fuzz_text | 2 | 8m25s | 8m48s | 0s | - | (d-gtr9) 2; 8m25s | 10 crates compiled; cargo build (release) 1m36s | Fuzz ${{ matrix.target }} 6m38s; dtolnay/rust-toolchain@nightly 11s; actions/cache@v4 2s; actions/checkout@v4 1s; Install cargo-fuzz 0s |
| fuzz_syslog | 2 | 7m54s | 7m59s | 0s | - | (d-gtr9) 2; 7m54s | 10 crates compiled; cargo build (release) 1m33s | Fuzz ${{ matrix.target }} 6m34s; dtolnay/rust-toolchain@nightly 10s; actions/cache@v4 2s; actions/checkout@v4 1s; Install cargo-fuzz 1s |
| fuzz_profile_toml | 2 | 7m52s | 8m37s | 0s | - | (d-gtr9) 2; 7m52s | 10 crates compiled; cargo build (release) 1m40s | Fuzz ${{ matrix.target }} 6m42s; dtolnay/rust-toolchain@nightly 8s; actions/cache@v4 2s; actions/checkout@v4 1s; Install cargo-fuzz 0s |
| fuzz_prof_folded | 2 | 7m52s | 7m52s | -1s | - | (d-gtr9) 2; 7m52s | 10 crates compiled; cargo build (release) 1m36s | Fuzz ${{ matrix.target }} 6m39s; dtolnay/rust-toolchain@nightly 9s; actions/checkout@v4 1s; actions/cache@v4 1s; Install cargo-fuzz 0s |
| fuzz_w3c_access_har | 2 | 7m52s | 7m54s | 1s | - | (d-gtr9) 2; 7m52s | 10 crates compiled; cargo build (release) 1m36s | Fuzz ${{ matrix.target }} 7m09s; dtolnay/rust-toolchain@nightly 8s; actions/checkout@v4 1s; actions/cache@v4 1s; Install cargo-fuzz 0s |
| fuzz_otel | 2 | 7m48s | 7m52s | 1s | - | (d-gtr9) 1; 7m53s, (d7-nas) 1; 7m43s | 10 crates compiled; cargo build (release) 1m56s | Fuzz ${{ matrix.target }} 6m58s; dtolnay/rust-toolchain@nightly 11s; actions/cache@v4 4s; actions/checkout@v4 2s; Install cargo-fuzz 0s |
| fuzz_minidump | 2 | 7m48s | 7m54s | 1s | - | (d7-nas) 1; 7m39s, (d-gtr9) 1; 7m56s | 10 crates compiled; cargo build (release) 1m54s | Fuzz ${{ matrix.target }} 6m56s; dtolnay/rust-toolchain@nightly 13s; actions/cache@v4 4s; actions/checkout@v4 2s; Install cargo-fuzz 1s |
| fuzz_registry | 2 | 7m47s | 8m25s | 20s | - | (d-gtr9) 2; 7m47s | 10 crates compiled; cargo build (release) 1m39s | Fuzz ${{ matrix.target }} 6m41s; dtolnay/rust-toolchain@nightly 8s; actions/checkout@v4 1s; actions/cache@v4 1s; Install cargo-fuzz 0s |
| fuzz_ql | 2 | 7m47s | 8m25s | 20s | - | (d-gtr9) 2; 7m47s | 10 crates compiled; cargo build (release) 1m38s | Fuzz ${{ matrix.target }} 6m41s; dtolnay/rust-toolchain@nightly 9s; actions/cache@v4 2s; actions/checkout@v4 1s; Install cargo-fuzz 0s |
| fuzz_json | 2 | 7m46s | 7m46s | 1s | - | (d-gtr9) 2; 7m46s | 215 crates compiled; cargo build (release) 2m25s | Fuzz ${{ matrix.target }} 7m16s; Install cargo-fuzz 11s; dtolnay/rust-toolchain@nightly 9s; actions/checkout@v4 1s; actions/cache@v4 0s |
| fuzz_prof_pprof | 2 | 7m43s | 7m44s | 0s | - | (d7-nas) 2; 7m43s | 10 crates compiled; cargo build (release) 2m15s | Fuzz ${{ matrix.target }} 7m17s; dtolnay/rust-toolchain@nightly 13s; actions/cache@v4 7s; actions/checkout@v4 2s; Install cargo-fuzz 1s |
| fuzz_prof_cpuprofile | 2 | 7m42s | 7m43s | 1s | - | (d7-nas) 2; 7m42s | 10 crates compiled; cargo build (release) 2m16s | Fuzz ${{ matrix.target }} 7m18s; dtolnay/rust-toolchain@nightly 12s; actions/cache@v4 7s; actions/checkout@v4 1s; Install cargo-fuzz 0s |
| fuzz_prof_heapsnapshot | 2 | 7m34s | 7m47s | 0s | - | (d-gtr9) 2; 7m34s | 10 crates compiled; cargo build (release) 1m36s | Fuzz ${{ matrix.target }} 7m08s; dtolnay/rust-toolchain@nightly 8s; actions/cache@v4 2s; actions/checkout@v4 0s; Install cargo-fuzz 0s |
| fuzz_prof_nettrace | 2 | 7m26s | 7m48s | 0s | - | (d-gtr9) 2; 7m26s | 10 crates compiled; cargo build (release) 1m34s | Fuzz ${{ matrix.target }} 6m37s; dtolnay/rust-toolchain@nightly 11s; actions/cache@v4 2s; actions/checkout@v4 1s; Install cargo-fuzz 0s |
| fuzz_prof_speedscope | 2 | 7m26s | 7m44s | 0s | - | (d-gtr9) 2; 7m26s | 10 crates compiled; cargo build (release) 1m36s | Fuzz ${{ matrix.target }} 6m38s; dtolnay/rust-toolchain@nightly 11s; actions/checkout@v4 1s; actions/cache@v4 1s; Install cargo-fuzz 0s |
| fuzz_procmon_csv | 2 | 6m59s | 7m01s | 1s | - | (d-gtr9) 2; 6m59s | 10 crates compiled; cargo build (release) 1m36s | Fuzz ${{ matrix.target }} 6m38s; dtolnay/rust-toolchain@nightly 10s; actions/cache@v4 2s; actions/checkout@v4 1s; Install cargo-fuzz 0s |
| fuzz_perfmon_csv | 2 | 6m58s | 7m00s | 1s | - | (d-gtr9) 2; 6m58s | 10 crates compiled; cargo build (release) 1m38s | Fuzz ${{ matrix.target }} 6m42s; dtolnay/rust-toolchain@nightly 8s; actions/cache@v4 2s; actions/checkout@v4 1s; Install cargo-fuzz 0s |
| fuzz_prof_perf_script | 2 | 6m56s | 6m58s | 1s | - | (d-gtr9) 2; 6m56s | 10 crates compiled; cargo build (release) 1m34s | Fuzz ${{ matrix.target }} 6m41s; dtolnay/rust-toolchain@nightly 10s; actions/checkout@v4 1s; actions/cache@v4 1s; Install cargo-fuzz 0s |
| fuzz_prof_chrome_trace | 2 | 6m55s | 6m57s | 1s | - | (d-gtr9) 2; 6m55s | 10 crates compiled; cargo build (release) 1m34s | Fuzz ${{ matrix.target }} 6m36s; dtolnay/rust-toolchain@nightly 10s; actions/cache@v4 2s; actions/checkout@v4 1s; Install cargo-fuzz 0s |
| fuzz_timestamp | 1 | 6m50s | 6m50s | -1s | - | (d-gtr9) 1; 6m50s | 10 crates compiled; cargo build (release) 1m34s | Fuzz ${{ matrix.target }} 6m37s; dtolnay/rust-toolchain@nightly 9s; actions/checkout@v4 1s; actions/cache@v4 1s; Install cargo-fuzz 0s |

Step categories per job (median seconds summed per job run; categories by step name):

| job | categories |
| --- | --- |
| fuzz (shard 4/6) | other 51m17s; toolchain / tools 33s; checkout 3s |
| fuzz (shard 3/6) | other 46m14s; toolchain / tools 33s; checkout 3s |
| fuzz (shard 1/6) | other 46m15s; toolchain / tools 33s; checkout 3s |
| fuzz_archive | other 8m36s; toolchain / tools 12s |
| fuzz_eventlog | other 8m42s; toolchain / tools 11s |
| fuzz_csv | other 8m29s; toolchain / tools 10s |
| fuzz_text | other 6m38s; toolchain / tools 11s |
| fuzz_syslog | other 6m35s; toolchain / tools 10s |
| fuzz_profile_toml | other 6m42s; toolchain / tools 8s |
| fuzz_prof_folded | other 6m39s; toolchain / tools 9s |
| fuzz_w3c_access_har | other 7m09s; toolchain / tools 8s |
| fuzz_otel | other 6m58s; toolchain / tools 11s; cache restore 4s |
| fuzz_minidump | other 6m57s; toolchain / tools 13s; cache restore 4s |
| fuzz_registry | other 6m41s; toolchain / tools 8s |
| fuzz_ql | other 6m41s; toolchain / tools 9s |
| fuzz_json | other 7m27s; toolchain / tools 9s |
| fuzz_prof_pprof | other 7m18s; toolchain / tools 13s; cache restore 7s |
| fuzz_prof_cpuprofile | other 7m18s; toolchain / tools 12s; cache restore 7s |
| fuzz_prof_heapsnapshot | other 7m08s; toolchain / tools 8s |
| fuzz_prof_nettrace | other 6m37s; toolchain / tools 11s |
| fuzz_prof_speedscope | other 6m38s; toolchain / tools 11s |
| fuzz_procmon_csv | other 6m38s; toolchain / tools 10s |
| fuzz_perfmon_csv | other 6m42s; toolchain / tools 8s |
| fuzz_prof_perf_script | other 6m41s; toolchain / tools 10s |
| fuzz_prof_chrome_trace | other 6m36s; toolchain / tools 10s |
| fuzz_timestamp | other 6m37s; toolchain / tools 9s |

### debrief / mutants.yml (pull_request)

Timed on 4 successful runs.

Critical path (rebuilt from timestamps; share = fraction of runs where the job is on it):

| job | share | wall (median) | delay before it (median) |
| --- | --- | --- | --- |
| mutants | 100% | 51s | 14m23s |

Jobs:

| job | n | wall median | p90 | delay median | queue, first-wave runs (median; n) | runners (count; median wall) | log phases (median) | biggest steps (median) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| mutants | 4 | 51s | 68s | 14m23s | 14m23s; 4 | (d-gtr9) 3; 47s, (d7-nas) 1; 74s | 123 crates compiled; cargo build (release) 28s (3 runs) | Install cargo-mutants 31s; dtolnay/rust-toolchain@stable 12s; actions/checkout@v4 2s; actions/cache@v4 1s; Mutants in the changed lines 1s |

Step categories per job (median seconds summed per job run; categories by step name):

| job | categories |
| --- | --- |
| mutants | other 31s; toolchain / tools 12s |

### paddock / ci.yml (with-bundles)

Timed on 3 successful runs.

Critical path (rebuilt from timestamps; share = fraction of runs where the job is on it):

| job | share | wall (median) | delay before it (median) |
| --- | --- | --- | --- |
| ui (tokens, contrast, components, gallery) | 100% | 77s | 22s |
| release tarball | 67% | 16s | 24s |
| release (npm registry) | 33% | 12s | 6s |

Jobs:

| job | n | wall median | p90 | delay median | queue, first-wave runs (median; n) | runners (count; median wall) | log phases (median) | biggest steps (median) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| ui (tokens, contrast, components, gallery) | 3 | 77s | 2m15s | 22s | 22s; 3 | (d-gtr9) 3; 77s | vitest 1s; ng build bundle 1s; playwright run 10s | Gallery checks (axe, keyboard, CSP) 60s; actions/setup-node@v4 4s; Install 3s; Component specs 2s; Build the library 2s |
| release tarball | 2 | 16s | 18s | 24s | - | (d-gtr9) 2; 16s | - | actions/setup-node@v4 6s; npm ci 4s; Pack and attach paddock-ui-<version>.tgz to the release 2s; Pack and attach suite-ui-<version>.tgz to the release 2s; actions/checkout@v4 1s |
| release (npm registry) | 1 | 12s | 12s | 6s | - | (d-gtr9) 1; 12s | - | actions/setup-node@v4 3s; npm ci 3s; Publish to the npm registry and create the release 3s; actions/checkout@v4 1s |

Step categories per job (median seconds summed per job run; categories by step name):

| job | categories |
| --- | --- |
| ui (tokens, contrast, components, gallery) | other 65s; toolchain / tools 4s; npm ci 3s |
| release tarball | toolchain / tools 6s; npm ci 4s |
| release (npm registry) | toolchain / tools 3s; npm ci 3s; other 3s |

### paddock / ci.yml (push)

Timed on 9 successful runs.

Critical path (rebuilt from timestamps; share = fraction of runs where the job is on it):

| job | share | wall (median) | delay before it (median) |
| --- | --- | --- | --- |
| ui (tokens, contrast, components, gallery) | 44% | 85s | 5s |
| crate (ubuntu-latest) | 22% | 5m38s | 3s |
| crate (macos-latest) | 22% | 4m51s | 55s |
| crates (fmt, clippy, test, coverage, deny) | 22% | 1m42s | 3s |
| ui (tokens, contrast, components, entry points, reference apps, publish dry run) | 11% | 2m46s | 4s |

Jobs:

| job | n | wall median | p90 | delay median | queue, first-wave runs (median; n) | runners (count; median wall) | log phases (median) | biggest steps (median) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| crate (ubuntu-latest) | 10 | 4m56s | 5m24s | 4s | 4s; 9 | (d-gtr9) 10; 4m56s | 680 crates compiled; cargo build (dev) 1m46s; cargo build (test) 2m16s | paddock-commands 76s; paddock-shell 58s; paddock-dialogs 47s; paddock-window 20s; paddock-download 18s |
| crate (macos-latest) | 10 | 4m33s | 5m02s | 4s | 4s; 9 | d7-mac-mini 10; 4m33s | 650 crates compiled; cargo build (dev) 1m32s; cargo build (test) 2m20s | paddock-commands 60s; paddock-shell 50s; paddock-updater 31s; paddock-window 27s; paddock-download 26s |
| crate (windows-latest) | 9 | 3m40s | 3m51s | 3s | 2s; 7 | decryp7ology-dev 9; 3m40s | cargo build (dev) 34s; 47 crates compiled; cargo build (test) 1m32s | paddock-jobs 32s; paddock-shell 31s; paddock-download 28s; paddock-updater 23s; paddock-window 14s |
| ui (tokens, contrast, components, entry points, reference apps, publish dry run) | 9 | 2m55s | 3m21s | 2s | 2s; 9 | (d7-nas) 6; 2m54s, (d-gtr9) 3; 3m16s | vitest 5s; ng build bundle 10s; playwright run 1m42s | Reference app checks (gallery and shell demo; axe, keyboard, CSP, 320 px) 1m52s; Component specs 12s; Build the library (all entry points) 7s; actions/setup-node@v4 6s; Install 6s |
| crates (fmt, clippy, test, coverage, deny) | 2 | 1m42s | 2m03s | 3s | 5s; 1 | (d-gtr9) 2; 1m42s | 101 crates compiled; cargo build (dev) 57s; cargo build (test) 8s | cargo clippy --workspace --all-targets -- -D warnings 58s; Coverage (>= 99 % lines) 6s; cargo test --workspace 5s; actions/checkout@v4 1s; cargo deny check 1s |
| ui (tokens, contrast, components, gallery) | 5 | 82s | 2m05s | 4s | 4s; 5 | (d-gtr9) 4; 85s, (d7-nas) 1; 47s | vitest 2s; ng build bundle 2s; playwright run 10s | Gallery checks (axe, keyboard, CSP) 64s; Install 6s; actions/setup-node@v4 5s; Component specs 5s; Build the library 2s |
| crates (fmt, coverage, deny) | 10 | 78s | 2m05s | 4s | 4s; 10 | (d-gtr9) 10; 78s | 334 crates compiled; cargo build (test) 54s | Coverage (>= 99 % lines) 61s; actions/checkout@v4 2s; cargo deny check 2s; cargo fmt --all --check 0s |

Step categories per job (median seconds summed per job run; categories by step name):

| job | categories |
| --- | --- |
| crate (ubuntu-latest) | other 4m11s |
| crate (macos-latest) | other 4m28s; checkout 3s |
| crate (windows-latest) | other 3m19s; checkout 11s; toolchain / tools 4s |
| ui (tokens, contrast, components, entry points, reference apps, publish dry run) | other 2m27s; toolchain / tools 6s; npm ci 6s |
| crates (fmt, clippy, test, coverage, deny) | lint / format / audit 59s; other 6s; rust tests (compile + run) 5s |
| ui (tokens, contrast, components, gallery) | other 73s; npm ci 6s; toolchain / tools 5s |
| crates (fmt, coverage, deny) | other 61s |

### pitcrew / build.yml (with-bundles)

Timed on 8 successful runs.

Critical path (rebuilt from timestamps; share = fraction of runs where the job is on it):

| job | share | wall (median) | delay before it (median) |
| --- | --- | --- | --- |
| bundle (ubuntu-latest) | 63% | 7m20s | 4m25s |
| rust format + clippy | 63% | 1m45s | 5m38s |
| macOS clippy + tests | 50% | 6m36s | 2m31s |
| rust tests (windows-latest) | 50% | 2m03s | 2m42s |
| frontend build, lint + unit tests | 50% | 47s | 10s |
| bundle (windows-latest) | 38% | 6m53s | 3s |
| rust tests (ubuntu-latest) | 38% | 4m28s | 39s |
| release | 38% | 81s | 8m14s |

Jobs:

| job | n | wall median | p90 | delay median | queue, first-wave runs (median; n) | runners (count; median wall) | log phases (median) | biggest steps (median) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| bundle (windows-latest) | 14 | 8m28s | 9m24s | 3s | - | decryp7ology-dev 14; 8m28s | 5 crates compiled; cargo build (dist) 5m43s; ng build bundle 9s | Build installers 7m41s; actions/checkout@v4 9s; dtolnay/rust-toolchain@stable 3s; actions/upload-artifact@v3 2s; Collect installers 1s |
| bundle (ubuntu-latest) | 14 | 7m03s | 11m26s | 3s | - | (d-gtr9) 12; 6m43s, (d7-nas) 2; 10m59s | 486 crates compiled; cargo build (dist) 3m47s; ng build bundle 4s | Build installers 5m02s; Linux dependencies for Tauri and the bundles 63s; dtolnay/rust-toolchain@stable 10s; actions/upload-artifact@v3 8s; actions/setup-node@v4 6s |
| macOS clippy + tests | 14 | 6m19s | 13m25s | 78s | 21s; 10 | d7-mac-mini 14; 6m19s | 485 crates compiled; cargo build (dev) 2m29s; cargo build (test) 2m44s | Tests (macOS) 3m44s; Clippy (macOS) 2m29s; actions/checkout@v4 2s; Toolchain present 0s; Clean up test processes 0s |
| bundle (macos-latest) | 14 | 4m07s | 8m06s | 2s | - | d7-mac-mini 14; 4m07s | 408 crates compiled; cargo build (dist) 3m38s; ng build bundle 3s | Build installers 3m54s; actions/checkout@v4 3s; actions/upload-artifact@v3 1s; Linux dependencies for Tauri and the bundles 0s; Restore the sccache compile cache 0s |
| rust tests (ubuntu-latest) | 13 | 3m52s | 5m02s | 4s | 16s; 6 | (d-gtr9) 12; 3m50s, (d7-nas) 1; 28m08s | 427 crates compiled; cargo build (test) 1m54s | Tests 2m26s; Linux dependencies for Tauri 59s; dtolnay/rust-toolchain@stable 9s; Restore the sccache compile cache 2s; actions/checkout@v4 1s |
| rust format + clippy | 14 | 1m46s | 4m26s | 65s | 16s; 9 | (d-gtr9) 11; 1m42s, (d7-nas) 3; 4m03s | 1 crates compiled; cargo build (dev) 6s (12 runs) | Linux dependencies for Tauri 75s; dtolnay/rust-toolchain@stable 12s; Clippy 6s; actions/cache/restore@v4 4s; actions/checkout@v4 1s |
| rust tests (windows-latest) | 10 | 1m45s | 2m10s | 10s | 10s; 8 | decryp7ology-dev 10; 1m45s | 14 crates compiled; cargo build (test) 37s | Tests 83s; actions/checkout@v4 9s; dtolnay/rust-toolchain@stable 3s; Git bash on PATH and a persistent build folder (Windows) 0s; Linux dependencies for Tauri 0s |
| release | 3 | 81s | 2m53s | 8m14s | - | (d-gtr9) 2; 54s, (d7-nas) 1; 3m16s | 318 crates compiled; cargo build (release) 1m47s (2 runs) | Release notes (git-cliff) 60s; actions/setup-node@v4 9s; dtolnay/rust-toolchain@stable 8s; actions/checkout@v4 2s; actions/download-artifact@v3 2s |
| frontend build, lint + unit tests | 14 | 46s | 90s | 0s | 74s; 2 | (d-gtr9) 12; 43s, (d7-nas) 2; 53s | ng build bundle 10s (13 runs); vitest 12s (13 runs) | Frontend unit tests 17s; actions/setup-node@v4 5s; Install 5s; Frontend build 5s; Lint 3s |
| licences, scripts + commits | 14 | 34s | 54s | 7s | 15s; 6 | (d-gtr9) 9; 26s, (d7-nas) 5; 50s | - | dtolnay/rust-toolchain@stable 9s; actions/setup-node@v4 6s; actions/cache/restore@v4 3s; actions/cache/save@v4 3s; actions/checkout@v4 2s |

Step categories per job (median seconds summed per job run; categories by step name):

| job | categories |
| --- | --- |
| bundle (windows-latest) | bundle / app build 7m41s; checkout 9s; toolchain / tools 3s; other 3s |
| bundle (ubuntu-latest) | bundle / app build 5m02s; apt / system libs 63s; toolchain / tools 15s; other 8s; cache save 3s |
| macOS clippy + tests | other 3m44s; lint / format / audit 2m29s |
| bundle (macos-latest) | bundle / app build 3m54s |
| rust tests (ubuntu-latest) | rust tests (compile + run) 2m26s; apt / system libs 59s; toolchain / tools 9s |
| rust format + clippy | apt / system libs 75s; toolchain / tools 12s; lint / format / audit 7s; cache restore 4s |
| rust tests (windows-latest) | rust tests (compile + run) 83s; checkout 9s; toolchain / tools 3s |
| release | other 65s; toolchain / tools 14s |
| frontend build, lint + unit tests | vitest / unit tests 17s; toolchain / tools 5s; npm ci 5s; ng build 5s; lint / format / audit 3s |
| licences, scripts + commits | toolchain / tools 16s; cache restore 3s; cache save 3s |

### pitcrew / build.yml (push)

Timed on 9 successful runs.

Critical path (rebuilt from timestamps; share = fraction of runs where the job is on it):

| job | share | wall (median) | delay before it (median) |
| --- | --- | --- | --- |
| rust format + clippy | 44% | 5m09s | 2m26s |
| rust tests (ubuntu-latest) | 44% | 4m35s | 5m02s |
| rust tests (windows-latest) | 44% | 2m06s | 6m39s |
| macOS clippy + tests | 33% | 6m44s | 12s |
| frontend build, lint + unit tests | 33% | 49s | 1s |
| licences, scripts + commits | 22% | 29s | 27s |

Jobs:

| job | n | wall median | p90 | delay median | queue, first-wave runs (median; n) | runners (count; median wall) | log phases (median) | biggest steps (median) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| macOS clippy + tests | 10 | 5m51s | 7m00s | 9s | 5s; 8 | d7-mac-mini 10; 5m51s | 485 crates compiled; cargo build (dev) 2m25s; cargo build (test) 2m26s | Tests (macOS) 3m23s; Clippy (macOS) 2m25s; actions/checkout@v4 2s; Toolchain present 0s; Clean up test processes 0s |
| rust tests (ubuntu-latest) | 10 | 4m35s | 6m50s | 5s | 5s; 5 | (d-gtr9) 9; 4m16s, (d7-nas) 1; 9m29s | 427 crates compiled; cargo build (test) 2m28s | Tests 3m00s; Linux dependencies for Tauri 57s; dtolnay/rust-toolchain@stable 9s; actions/cache/save@0057852bfaa89a56745cba8c7296529d2fc39830 2s; actions/checkout@v4 1s |
| rust format + clippy | 11 | 2m39s | 7m38s | 4m09s | 4m09s; 9 | (d-gtr9) 9; 2m38s, (d7-nas) 2; 7m49s | 1 crates compiled; cargo build (dev) 9s | Linux dependencies for Tauri 71s; Clippy for macOS (core + service) 30s; dtolnay/rust-toolchain@stable 10s; Clippy 9s; actions/cache/restore@v4 4s |
| rust tests (windows-latest) | 10 | 2m05s | 3m03s | 2m15s | 1m56s; 8 | decryp7ology-dev 10; 2m05s | 14 crates compiled; cargo build (test) 48s | Tests 1m43s; actions/checkout@v4 10s; dtolnay/rust-toolchain@stable 3s; Git bash on PATH and a persistent build folder (Windows) 0s; Linux dependencies for Tauri 0s |
| frontend build, lint + unit tests | 11 | 55s | 1m40s | 0s | 8s; 2 | (d-gtr9) 8; 53s, (d7-nas) 3; 61s | ng build bundle 10s; vitest 13s | Frontend unit tests 19s; Install 6s; Frontend build 6s; actions/setup-node@v4 5s; Lint 3s |
| licences, scripts + commits | 12 | 33s | 1m33s | 3s | 5s; 4 | (d-gtr9) 9; 31s, (d7-nas) 3; 52s | - | dtolnay/rust-toolchain@stable 9s; actions/setup-node@v4 5s; actions/cache/save@v4 4s; actions/cache/restore@v4 3s; Licences, advisories and sources 3s |

Step categories per job (median seconds summed per job run; categories by step name):

| job | categories |
| --- | --- |
| macOS clippy + tests | other 3m23s; lint / format / audit 2m25s |
| rust tests (ubuntu-latest) | rust tests (compile + run) 3m00s; apt / system libs 57s; toolchain / tools 9s |
| rust format + clippy | apt / system libs 71s; toolchain / tools 10s; lint / format / audit 9s; cache restore 4s |
| rust tests (windows-latest) | rust tests (compile + run) 1m43s; checkout 10s; toolchain / tools 3s |
| frontend build, lint + unit tests | vitest / unit tests 19s; npm ci 6s; ng build 6s; toolchain / tools 5s; lint / format / audit 3s |
| licences, scripts + commits | toolchain / tools 15s; cache save 4s |

### pitwall / build.yml (with-bundles)

Timed on 8 successful runs.

Critical path (rebuilt from timestamps; share = fraction of runs where the job is on it):

| job | share | wall (median) | delay before it (median) |
| --- | --- | --- | --- |
| bundle (windows-latest) | 75% | 8m30s | 0s |
| windows tests + clippy | 75% | 1m49s | 45s |
| rust format + clippy | 63% | 1m42s | 33s |
| licences, privacy, scripts + commits | 38% | 2m15s | 46s |
| bundle (ubuntu-latest) | 25% | 6m44s | 37s |
| rust tests + core coverage (linux) | 25% | 6m24s | 3m17s |
| bundle (macos-latest) | 25% | 4m06s | 44s |
| macOS clippy + tests | 25% | 2m15s | 9m03s |

Jobs:

| job | n | wall median | p90 | delay median | queue, first-wave runs (median; n) | runners (count; median wall) | log phases (median) | biggest steps (median) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| bundle (windows-latest) | 17 | 7m57s | 11m30s | 2s | - | decryp7ology-dev 17; 7m57s | 6 crates compiled; cargo build (dist) 5m11s; ng build bundle 9s | Build installers 7m16s; actions/checkout@11d5960a326750d5838078e36cf38b85af677262 9s; dtolnay/rust-toolchain@b725eda525362e7cf34af9a7243cac1e3f01a7ec 3s; actions/upload-artifact@ff15f0306b3f739f7b6fd43fb5d26cd321bd4de5 3s; Linux dependencies for Tauri and the bundles 0s |
| bundle (ubuntu-latest) | 17 | 6m54s | 12m35s | 4s | - | (d-gtr9) 13; 6m31s, (d7-nas) 4; 12m48s | 532 crates compiled; cargo build (dist) 3m48s; ng build bundle 4s | Build installers 5m08s; Linux dependencies for Tauri and the bundles 68s; dtolnay/rust-toolchain@b725eda525362e7cf34af9a7243cac1e3f01a7ec 10s; actions/upload-artifact@ff15f0306b3f739f7b6fd43fb5d26cd321bd4de5 9s; actions/setup-node@49933ea5288caeca8642d1e84afbd3f7d6820020 6s |
| rust tests + core coverage (linux) | 10 | 5m52s | 6m44s | 5s | 5s; 7 | (d-gtr9) 10; 5m52s | 657 crates compiled; cargo build (test) 3m23s | Tauri shell tests 1m55s; Linux dependencies for Tauri 79s; Tests 66s; Tests in another time zone 29s; Core coverage guard (>= 99% lines) 13s |
| playwright e2e (mock backend) | 17 | 5m01s | 7m24s | 1s | 5s; 4 | (d-gtr9) 13; 3m57s, (d7-nas) 4; 6m19s | playwright run 1m48s | End-to-end tests 2m04s; Playwright browsers 1m59s; actions/setup-node@49933ea5288caeca8642d1e84afbd3f7d6820020 6s; npm ci 4s; actions/cache@0057852bfaa89a56745cba8c7296529d2fc39830 2s |
| bundle (macos-latest) | 17 | 4m08s | 5m13s | 1s | - | d7-mac-mini 17; 4m08s | 448 crates compiled; cargo build (dist) 3m45s; ng build bundle 3s | Build installers 4m03s; actions/checkout@11d5960a326750d5838078e36cf38b85af677262 3s; actions/upload-artifact@ff15f0306b3f739f7b6fd43fb5d26cd321bd4de5 1s; Clean up (macOS) 1s; Linux dependencies for Tauri and the bundles 0s |
| rust format + clippy | 17 | 1m52s | 2m46s | 6s | 5s; 15 | (d7-nas) 5; 2m38s, (d-gtr9) 12; 1m39s | 1 crates compiled; cargo build (dev) 7s | Linux dependencies for Tauri 66s; dtolnay/rust-toolchain@b725eda525362e7cf34af9a7243cac1e3f01a7ec 10s; Clippy 8s; actions/cache/restore@0057852bfaa89a56745cba8c7296529d2fc39830 4s; actions/checkout@11d5960a326750d5838078e36cf38b85af677262 2s |
| macOS clippy + tests | 16 | 1m51s | 2m14s | 59s | 5s; 9 | d7-mac-mini 16; 1m51s | 199 crates compiled; cargo build (dev) 45s; cargo build (test) 45s | Tests (macOS) 62s; Clippy (macOS) 45s; actions/checkout@11d5960a326750d5838078e36cf38b85af677262 3s; Toolchain present 0s; Clean up test processes and temp dirs 0s |
| windows tests + clippy | 14 | 1m37s | 2m11s | 41s | 5s; 6 | decryp7ology-dev 14; 1m37s | cargo build (dev) 15s; 4 crates compiled; cargo build (test) 29s | Tests (core, loopback, server) 61s; Clippy (core, loopback, server incl. Windows service code) 15s; actions/checkout@11d5960a326750d5838078e36cf38b85af677262 10s; dtolnay/rust-toolchain@b725eda525362e7cf34af9a7243cac1e3f01a7ec 3s; Git bash on PATH and a persistent build folder 0s |
| licences, privacy, scripts + commits | 17 | 73s | 2m34s | 5s | 5s; 11 | (d-gtr9) 14; 63s, (d7-nas) 3; 2m34s | 153 crates compiled; cargo build (test) 42s | Privacy (outbound HTTP only through egress.rs) 42s; dtolnay/rust-toolchain@b725eda525362e7cf34af9a7243cac1e3f01a7ec 9s; actions/setup-node@49933ea5288caeca8642d1e84afbd3f7d6820020 3s; actions/cache/restore@0057852bfaa89a56745cba8c7296529d2fc39830 2s; Licences, advisories and sources 2s |
| frontend build, lint + unit tests | 17 | 41s | 62s | 2s | 5s; 9 | (d-gtr9) 16; 41s, (d7-nas) 1; 67s | ng build bundle 11s (16 runs); vitest 10s (16 runs) | Unit tests 16s; Build 6s; actions/setup-node@49933ea5288caeca8642d1e84afbd3f7d6820020 5s; Install and audit 4s; Lint 3s |

Step categories per job (median seconds summed per job run; categories by step name):

| job | categories |
| --- | --- |
| bundle (windows-latest) | bundle / app build 7m16s; checkout 9s; toolchain / tools 3s; other 3s |
| bundle (ubuntu-latest) | bundle / app build 5m08s; apt / system libs 68s; toolchain / tools 15s; other 9s |
| rust tests + core coverage (linux) | other 2m36s; apt / system libs 79s; rust tests (compile + run) 66s; toolchain / tools 12s |
| playwright e2e (mock backend) | playwright run (e2e) 4m02s; toolchain / tools 6s; npm ci 4s |
| bundle (macos-latest) | bundle / app build 4m03s; checkout 3s |
| rust format + clippy | apt / system libs 66s; toolchain / tools 10s; lint / format / audit 8s; cache restore 4s |
| macOS clippy + tests | other 62s; lint / format / audit 45s |
| windows tests + clippy | other 61s; lint / format / audit 15s; checkout 10s; toolchain / tools 3s |
| licences, privacy, scripts + commits | other 43s; toolchain / tools 14s |
| frontend build, lint + unit tests | vitest / unit tests 16s; lint / format / audit 7s; other 6s; toolchain / tools 5s |

### pitwall / release.yml (with-bundles)

Timed on 4 successful runs.

Critical path (rebuilt from timestamps; share = fraction of runs where the job is on it):

| job | share | wall (median) | delay before it (median) |
| --- | --- | --- | --- |
| release | 100% | 1m39s | 23s |
| bundle (windows-latest) | 75% | 7m47s | 7m11s |
| bundle (macos-latest) | 25% | 10m10s | 4m15s |
| bundle (ubuntu-latest) | 25% | 7m08s | 3s |

Jobs:

| job | n | wall median | p90 | delay median | queue, first-wave runs (median; n) | runners (count; median wall) | log phases (median) | biggest steps (median) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| bundle (ubuntu-latest) | 4 | 7m49s | 9m57s | 2s | 2s; 4 | (d7-nas) 1; 10m35s, (d-gtr9) 3; 7m08s | 532 crates compiled; cargo build (dist) 4m24s; ng build bundle 5s | Build installers 5m33s; Linux dependencies for Tauri and the bundles 81s; dtolnay/rust-toolchain@b725eda525362e7cf34af9a7243cac1e3f01a7ec 11s; actions/upload-artifact@ff15f0306b3f739f7b6fd43fb5d26cd321bd4de5 7s; actions/setup-node@49933ea5288caeca8642d1e84afbd3f7d6820020 5s |
| bundle (windows-latest) | 4 | 7m23s | 8m28s | 3m38s | 3m38s; 4 | decryp7ology-dev 4; 7m23s | 6 crates compiled; cargo build (dist) 5m04s; ng build bundle 10s | Build installers 6m41s; actions/checkout@11d5960a326750d5838078e36cf38b85af677262 10s; dtolnay/rust-toolchain@b725eda525362e7cf34af9a7243cac1e3f01a7ec 3s; actions/upload-artifact@ff15f0306b3f739f7b6fd43fb5d26cd321bd4de5 2s; Linux dependencies for Tauri and the bundles 0s |
| bundle (macos-latest) | 4 | 4m10s | 8m22s | 2m10s | 4s; 3 | d7-mac-mini 4; 4m10s | 448 crates compiled; cargo build (dist) 3m45s; ng build bundle 3s | Build installers 4m03s; actions/checkout@11d5960a326750d5838078e36cf38b85af677262 3s; actions/upload-artifact@ff15f0306b3f739f7b6fd43fb5d26cd321bd4de5 2s; Clean up (macOS) 1s; Linux dependencies for Tauri and the bundles 0s |
| release | 4 | 1m39s | 2m27s | 23s | - | (d-gtr9) 3; 83s, (d7-nas) 1; 2m41s | 318 crates compiled; cargo build (release) 48s | Release notes (git-cliff) 59s; dtolnay/rust-toolchain@b725eda525362e7cf34af9a7243cac1e3f01a7ec 11s; actions/setup-node@49933ea5288caeca8642d1e84afbd3f7d6820020 4s; actions/checkout@11d5960a326750d5838078e36cf38b85af677262 3s; Publish the release 2s |

Step categories per job (median seconds summed per job run; categories by step name):

| job | categories |
| --- | --- |
| bundle (ubuntu-latest) | bundle / app build 5m33s; apt / system libs 81s; toolchain / tools 16s; other 7s |
| bundle (windows-latest) | bundle / app build 6m41s; checkout 10s; toolchain / tools 3s |
| bundle (macos-latest) | bundle / app build 4m03s; checkout 3s |
| release | other 61s; toolchain / tools 15s |

### spotter / build.yml (push)

Timed on 6 successful runs.

Critical path (rebuilt from timestamps; share = fraction of runs where the job is on it):

| job | share | wall (median) | delay before it (median) |
| --- | --- | --- | --- |
| macOS clippy + tests | 67% | 8m21s | 4s |
| rust tests (windows-latest) | 33% | 5m35s | 6m08s |
| rust tests (ubuntu-latest) | 33% | 2m23s | 2m49s |
| e2e (playwright, mock backend) | 33% | 2m04s | 14s |
| rust format + clippy | 33% | 65s | 3m25s |
| licences, scripts + commits | 33% | 32s | 1s |
| frontend build, lint + unit tests | 17% | 50s | 0s |

Jobs:

| job | n | wall median | p90 | delay median | queue, first-wave runs (median; n) | runners (count; median wall) | log phases (median) | biggest steps (median) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| macOS clippy + tests | 17 | 7m44s | 13m05s | 55s | 4s; 6 | d7-mac-mini 17; 7m44s | 511 crates compiled; cargo build (dev) 2m56s; cargo build (test) 2m57s | Tests (macOS) 4m32s; Clippy (macOS) 2m57s; Install the test browser (macOS) 15s; actions/setup-node@v4 5s; actions/checkout@v4 2s |
| rust tests (windows-latest) | 10 | 5m12s | 6m00s | 1m47s | 4s; 4 | decryp7ology-dev 10; 5m12s | 5 crates compiled; cargo build (test) 33s | Tests 4m10s; actions/setup-node@v4 25s; Install the test browser 17s; actions/checkout@v4 8s; dtolnay/rust-toolchain@stable 3s |
| rust tests (ubuntu-latest) | 12 | 3m32s | 4m39s | 4s | 7s; 8 | (d-gtr9) 12; 3m32s | 456 crates compiled; cargo build (test) 1m39s | Tests 2m24s; actions/setup-node@v4 5s; dtolnay/rust-toolchain@stable 2s; Restore the test browser 2s; actions/checkout@v4 1s |
| e2e (playwright, mock backend) | 16 | 1m50s | 3m07s | 1s | 7s; 2 | (d-gtr9) 13; 1m53s, (d7-nas) 3; 1m46s | playwright run 46s | End-to-end tests 46s; Playwright browsers (Chromium + WebKit) 8s; Install 5s; actions/setup-node@v4 4s; actions/checkout@v4 1s |
| rust format + clippy | 14 | 63s | 2m46s | 66s | 66s; 14 | (d7-nas) 3; 28s, (d-gtr9) 11; 71s | 1 crates compiled; cargo build (dev) 20s (11 runs) | Clippy 13s; actions/cache/restore@v4 3s; actions/checkout@v4 2s; dtolnay/rust-toolchain@stable 2s; Format 1s |
| frontend build, lint + unit tests | 16 | 42s | 51s | 0s | 5s; 5 | (d7-nas) 4; 48s, (d-gtr9) 12; 41s | ng build bundle 11s (13 runs); vitest 12s (13 runs) | Frontend unit tests 17s; Frontend build 6s; Install 6s; actions/setup-node@v4 5s; Lint 3s |
| licences, scripts + commits | 16 | 31s | 63s | 4s | 5s; 6 | (d-gtr9) 14; 29s, (d7-nas) 2; 57s | - | Licences, advisories and sources 6s; actions/setup-node@v4 4s; dtolnay/rust-toolchain@stable 3s; actions/checkout@v4 2s; actions/cache/restore@v4 2s |

Step categories per job (median seconds summed per job run; categories by step name):

| job | categories |
| --- | --- |
| macOS clippy + tests | other 4m48s; lint / format / audit 2m57s |
| rust tests (windows-latest) | rust tests (compile + run) 4m10s; checkout 8s; toolchain / tools 3s |
| rust tests (ubuntu-latest) | rust tests (compile + run) 2m24s; toolchain / tools 6s |
| e2e (playwright, mock backend) | playwright run (e2e) 88s; npm ci 5s; toolchain / tools 4s |
| rust format + clippy | lint / format / audit 14s; cache restore 3s |
| frontend build, lint + unit tests | vitest / unit tests 17s; ng build 6s; npm ci 6s; toolchain / tools 5s; lint / format / audit 3s |
| licences, scripts + commits | toolchain / tools 11s; lint / format / audit 6s |

### dyno / build.yml (with-bundles)

Timed on 3 all finished (fewer than 3 successful) runs.

Critical path (rebuilt from timestamps; share = fraction of runs where the job is on it):

| job | share | wall (median) | delay before it (median) |
| --- | --- | --- | --- |
| bundle (windows-latest) | 67% | 8m48s | 4m24s |
| rust tests (ubuntu-latest) | 67% | 4m23s | 3m56s |
| playwright (webkit) | 67% | 2m06s | 1s |
| rust format + clippy | 67% | 65s | 7m24s |
| frontend build, lint + unit tests | 67% | 34s | 1s |
| macOS clippy + tests | 33% | 12m38s | 7m52s |
| bundle (ubuntu-latest) | 33% | 7m27s | 6m43s |
| bundle (macos-latest) | 33% | 5m35s | 5m43s |

Jobs:

| job | n | wall median | p90 | delay median | queue, first-wave runs (median; n) | runners (count; median wall) | log phases (median) | biggest steps (median) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| bundle (windows-latest) | 2 | 8m28s | 8m57s | 5m49s | - | decryp7ology-dev 2; 8m28s | 23 crates compiled; cargo build (dist) 6m03s; ng build bundle 9s | Build installers 7m38s; actions/checkout@v4 12s; Prune the build folder (host runners) 7s; dtolnay/rust-toolchain@stable 3s; actions/upload-artifact@v3 3s |
| bundle (ubuntu-latest) | 2 | 7m22s | 7m26s | 3m57s | - | (d-gtr9) 2; 7m22s | 505 crates compiled; cargo build (dist) 5m02s; ng build bundle 7s | Build installers 6m19s; Linux dependencies for Tauri and the bundles 33s; actions/upload-artifact@v3 8s; dtolnay/rust-toolchain@stable 5s; actions/checkout@v4 4s |
| macOS clippy + tests | 3 | 6m49s | 11m28s | 7m52s | 7m09s; 2 | d7-mac-mini 3; 6m49s | 513 crates compiled; cargo build (dev) 2m45s; cargo build (test) 2m40s | Tests (macOS) 3m12s; Clippy (macOS) 2m45s; Agent measures once (macOS) 43s; actions/checkout@v4 5s; Toolchain present 1s |
| bundle (macos-latest) | 2 | 5m05s | 5m29s | 5m52s | - | d7-mac-mini 2; 5m05s | 436 crates compiled; cargo build (dist) 4m36s; ng build bundle 4s | Build installers 4m54s; actions/checkout@v4 5s; actions/upload-artifact@v3 2s; Host Node matches app/.nvmrc 1s; Linux dependencies for Tauri and the bundles 0s |
| playwright (webkit) | 2 | 2m06s | 2m16s | 1s | - | (d-gtr9) 2; 2m06s | playwright run 70s | Browser flows 72s; Install 42s; actions/checkout@v4 4s; actions/setup-node@v4 4s; actions/cache@v4 2s |
| rust format + clippy | 3 | 1m41s | 4m17s | 7m23s | 7m23s; 3 | (d-gtr9) 2; 65s, (d7-nas) 1; 4m56s | 5 crates compiled; cargo build (dev) 16s (2 runs) | Linux dependencies for Tauri 63s; Clippy 15s; actions/cache/restore@v4 11s; dtolnay/rust-toolchain@stable 9s; actions/checkout@v4 4s |
| playwright (chromium) | 2 | 85s | 85s | 1s | - | (d-gtr9) 2; 85s | playwright run 44s | Browser flows 45s; Install 27s; actions/setup-node@v4 5s; actions/checkout@v4 4s; actions/cache@v4 2s |
| frontend build, lint + unit tests | 1 | 66s | 66s | 1s | - | (d-gtr9) 1; 66s | ng build bundle 22s; vitest 15s | Tests are timezone-independent 16s; Frontend unit tests (with coverage) 14s; Frontend build 10s; Install 8s; Lint 6s |
| licences, scripts + commits | 3 | 37s | 56s | 1s | - | (d-gtr9) 2; 32s, (d7-nas) 1; 61s | - | dtolnay/rust-toolchain@stable 10s; actions/checkout@v4 6s; actions/setup-node@v4 5s; Licences, advisories and sources 4s; actions/cache/restore@v4 3s |

Step categories per job (median seconds summed per job run; categories by step name):

| job | categories |
| --- | --- |
| bundle (windows-latest) | bundle / app build 7m39s; checkout 12s; other 11s |
| bundle (ubuntu-latest) | bundle / app build 6m19s; apt / system libs 33s; toolchain / tools 9s; other 9s; checkout 4s |
| macOS clippy + tests | other 3m55s; lint / format / audit 2m45s; checkout 5s |
| bundle (macos-latest) | bundle / app build 4m54s; checkout 5s |
| playwright (webkit) | other 72s; npm ci 42s; checkout 4s; toolchain / tools 4s |
| rust format + clippy | apt / system libs 63s; lint / format / audit 15s; cache restore 11s; toolchain / tools 9s; checkout 4s |
| playwright (chromium) | other 45s; npm ci 27s; toolchain / tools 5s; checkout 4s |
| frontend build, lint + unit tests | other 16s; vitest / unit tests 14s; ng build 10s; npm ci 8s; lint / format / audit 6s; toolchain / tools 5s; checkout 4s |
| licences, scripts + commits | toolchain / tools 16s; checkout 6s; lint / format / audit 4s; cache restore 3s; cache save 3s |

### dyno / build.yml (push)

Timed on 3 all finished (fewer than 3 successful) runs.

Critical path (rebuilt from timestamps; share = fraction of runs where the job is on it):

| job | share | wall (median) | delay before it (median) |
| --- | --- | --- | --- |
| macOS clippy + tests | 100% | 11m46s | 4m25s |
| rust tests (ubuntu-latest) | 33% | 4m30s | 1s |
| rust tests (windows-latest) | 33% | 2m15s | 25m37s |
| playwright (webkit) | 33% | 1m52s | 1s |
| frontend build, lint + unit tests | 33% | 51s | 0s |
| licences, scripts + commits | 33% | 38s | 0s |
| rust format + clippy | 33% | 32s | 4m18s |

Jobs:

| job | n | wall median | p90 | delay median | queue, first-wave runs (median; n) | runners (count; median wall) | log phases (median) | biggest steps (median) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| macOS clippy + tests | 3 | 11m46s | 31m05s | 4m25s | 4m25s; 1 | d7-mac-mini 3; 11m46s | 505 crates compiled; cargo build (dev) 4m40s; cargo build (test) 4m38s | Tests (macOS) 5m45s; Clippy (macOS) 4m40s; Agent measures once (macOS) 45s; actions/checkout@v4 5s; Toolchain present 0s |
| playwright (webkit) | 3 | 1m52s | 2m01s | 1s | - | (d-gtr9) 2; 1m41s, (d7-nas) 1; 2m03s | playwright run 84s | Browser flows 83s; Install 14s; actions/setup-node@v4 6s; actions/checkout@v4 3s; actions/cache@v4 2s |
| playwright (chromium) | 3 | 1m50s | 2m28s | 1s | 22s; 1 | (d7-nas) 2; 1m40s, (d-gtr9) 1; 2m37s | playwright run 60s | Browser flows 62s; Install 15s; actions/setup-node@v4 6s; actions/cache@v4 4s; actions/checkout@v4 2s |
| frontend build, lint + unit tests | 1 | 1m38s | 1m38s | 8s | 8s; 1 | (d-gtr9) 1; 1m38s | ng build bundle 30s; vitest 28s | Frontend unit tests (with coverage) 27s; Tests are timezone-independent 25s; actions/setup-node@v4 12s; Frontend build 11s; Install 10s |
| licences, scripts + commits | 3 | 46s | 70s | 3s | 3s; 1 | (d-gtr9) 1; 38s, (d7-nas) 2; 61s | - | Licences, advisories and sources 10s; actions/cache/restore@v4 8s; actions/setup-node@v4 7s; actions/checkout@v4 6s; actions/cache/save@v4 6s |
| rust format + clippy | 3 | 33s | 2m16s | 4m18s | 4m18s; 3 | (d-gtr9) 3; 33s | 4 crates compiled; cargo build (dev) 18s | Clippy 19s; actions/checkout@v4 2s; actions/cache/restore@v4 2s; actions/cache/save@v4 2s; dtolnay/rust-toolchain@stable 1s |

Step categories per job (median seconds summed per job run; categories by step name):

| job | categories |
| --- | --- |
| macOS clippy + tests | other 7m01s; lint / format / audit 4m40s; checkout 5s |
| playwright (webkit) | other 83s; npm ci 14s; toolchain / tools 6s; checkout 3s |
| playwright (chromium) | other 62s; npm ci 15s; toolchain / tools 6s; cache restore 4s |
| frontend build, lint + unit tests | vitest / unit tests 27s; other 25s; toolchain / tools 12s; ng build 11s; npm ci 10s; lint / format / audit 7s |
| licences, scripts + commits | toolchain / tools 10s; lint / format / audit 10s; cache restore 8s; checkout 6s; cache save 6s |
| rust format + clippy | lint / format / audit 20s |

<!-- ci-timings:end -->
