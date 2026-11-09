# 80. CI at the speed of its critical path: measured lanes, failed-only fix rounds, no GitHub at run time

- Status: Proposed (2026-11-09). Drafted at the user's request of 2026-11-09 ("i want the CI to be really optimized and
  intelligent and at the speed of light"). Needs the user's decisions D1 to D10 below before any build; an independent
  review comes before the user sees it.
- Date: 2026-11-09
- Related: ER-36 (speed and quality together; engineering-rules `checklists/speed-and-quality.md`),
  ER-7/ER-8 and rule 3 (affected tests, full suite once), ER-9/ER-10 and rule 50 (h) and (i) (local checks overlap CI;
  fix rounds rerun only what failed, then one full run), ER-12 (no retries that turn red green), ER-20 / rule 51
  (parallelism and cost), ER-24 / rule 41 (machine load), ER-31 (per-app release judgement), rule 48 (benchmarks
  recorded), rule 54 (installers before landing); ADR 0014 §6 (affected CI), ADR 0022 (`./dev`, the Rust planner and its
  prebuilt binary, amendment of 2026-11-09), ADR 0025 (CI job isolation, §8 one job per Linux host), ADR 0038 (CI parity
  before push, `./dev check --pre-push`), ADR 0060 (every check runs somewhere), ADR 0074 (declared hosts and machine
  leases; on `train/2026-11-09-2`); the research notes [2026-10-ci-speed.md](../research/2026-10-ci-speed.md) and
  [2026-10-ci-timings.md](../research/2026-10-ci-timings.md).
- In flight, not re-decided here: the Rust planner (`feat/dev-plan-port`), the prebuilt `./dev` (`ci/dev-prebuilt`), the
  kept-clone sparse checkout (on `train/2026-11-09-2`: Windows checkout 102 s to 34 s), the private build-inputs store
  (`ci/build-inputs-wp1`), `./dev plan --since-run` and `.gitea/` precision in the Rust planner (planned).

## Summary

CI today is not slow because jobs are slow; it is slow because jobs wait. Over 5.5 days, the median `ci.yml` run took
45 min from creation to its last job, while its critical path without any queueing (plan, then the longest job) was
20 min. The last job of a run waited a median 34 min for a runner. One train pair (runs 7821 and 7822) put 159 Linux
job-minutes on a host that runs 4 jobs at once: 40 min of Linux floor before anything else counts. Playwright is 47 % of
the Linux job time, `nextest` run phases are 3 %. Meanwhile 59 % of all runner minutes went to runs that failed and 20 %
to runs that were cancelled; inside a failed run only 24 % of the minutes were in the jobs that failed.

So the plan, in order of measured effect: (1) a fix round reruns only the failed jobs and tests on the new head
(`--since-run`), (2) the Linux lane gets lighter (Playwright workers, merged tiny jobs) and wider (a shared compile cache
that lets the host run 6 jobs instead of 4), (3) nothing is downloaded from GitHub during a run, (4) cheap static checks run
first and, in fix rounds and branch runs, gate the heavy Linux jobs, (5) the Debrief macOS test job, the longest job left,
is fixed at its slow tests and then partitioned. Every gate and every independent review stays; a fix round is followed by
one full run, as rule 50 (i) already requires.

Targets: a typical fix round **under 10 min** (today a fix round is a new full run: median 45 min); a full `ci.yml` run
**about 22 min** after phases 0 and 1 (from 45), and a full-run critical path **about 15 min** after phase 2.

## Measured baseline

All numbers come from the Gitea Actions API (job and step timestamps, runner names) and the job logs, read-only, for
runs 7480 to 7825 (2026-11-04 05:00 to 2026-11-09 18:38 +08; 216 runs, 3,624 jobs), plus logs of runs 7696, 7697,
7796, 7797, 7803, 7810, 7812, 7813, 7819, 7821 and 7822. Runs 7824 and 7825 were still running at 18:38 and were read,
not waited for. The results block, with method and limits, is in
[benchmarks.md, "2026-11-09, CI baseline"](../benchmarks.md#2026-11-09-ci-baseline-queues-critical-path-setup-cache-and-failures-docsadr-ci-speed).
"Queue" is the time from a job becoming ready (the end of `plan`, or of `rust format + clippy` for Debrief's smoke and
bundle jobs, which is what their `needs` say) to its start.

| Measure | Value |
| --- | --- |
| `ci.yml` run wall (48 completed runs, not cancelled) | median 45.1 min, p90 127 min |
| `ci.yml` critical path without queueing (plan end + longest job) | median 19.6 min, p90 41 min |
| `ci.yml` queue of the last job to finish | median 33.8 min, p90 113 min |
| `debrief.yml` run wall / no-queue critical path (13 runs) | median 62 / 23 min; the last job is `rust tests (macOS)` in each of the last 5 workflow-dispatch runs |
| Train pair 7821 (`ci.yml`) / 7822 (`debrief.yml`), 2026-11-09 15:25 | 47.4 min / 21.8 min. 7821: 26 Linux jobs, 104 job-min; its last Linux job started 31.5 min after it was ready. 7822: 11 Linux jobs, 55 job-min |
| Train pair 7824 / 7825 at 16.7 min | 19 `ci.yml` jobs still queued for Linux; Windows and macOS legs mostly done |
| Linux job time by phase (6 runs of logs) | Playwright 47 %, cargo compile 15 %, nextest run 3 %, the rest npm, Angular, checks and setup |
| macOS / Windows job time by phase | compile 50 % / 54 %, nextest run 18 % / 13 % |
| Setup share (pre-step, checkout, npm, tools, upload, cleanup) | Linux 20 % (checkout median 10 s; 54 % of Linux jobs last under 2 min); Windows 43 % (checkout median 105 s before the kept clone); macOS 31 % (checkout median 57 s, p90 268 s) |
| Runner concurrency observed | `d-gtr9` runs 4 jobs at once (47 % of the last 24 h at exactly 4); Windows ran 1 job at a time until the afternoon of 2026-11-09, 3 since; macOS 3 runners since 2026-11-09 01:03 |
| Queue wait, last 72 h | Linux p50 7 min, p90 35 min; Windows p50 21 min, p90 80 min; macOS p50 20 min, p90 73 min |
| Average busy lanes, last 72 h | Linux 1.3 of 4, Windows 0.5, macOS 0.7: the queues are bursts inside runs, not a full machine |
| sccache (local disk per host) | Linux hit rate 91 % (34,027 hits, 3,217 misses); macOS 24 %, Windows 27 %. On the host runners the kept build folders already skip unchanged crates, so the low rate is mostly genuine changes: `crate` missed 78 on Linux and 79 on macOS in run 7821. Cold slots are the exception: run 7810 `app dyno (windows)` on slot `s3` missed 1,020 |
| Repeated compiles | 7821's 10 Linux Rust jobs made 12,729 compile requests (one per crate built), each job building its own dependency graph from cache |
| Downloads from GitHub during a run | `taiki-e/install-action` for `cargo-about` took 277 s, 279 s and 276 s in 7821/7822 (eight HTTP 504 retries each; 1 to 19 s when GitHub answers). Action downloads (`Prepare all required actions`) took up to 88 s |
| Runner minutes by run outcome (5.5 days) | Linux 9,547: failed runs 59 %, cancelled 20 %, green 20 %. Windows 3,839: 64 / 15 / 21 %. macOS 4,211: 59 / 24 / 16 % |
| Failed runs: minutes in the failed jobs | 24 % (45 failed `ci.yml`/`debrief.yml` runs); median share of jobs failing 9 % |
| Slowest tests | `debrief-store-duck` suites on macOS: 60 to 126 s each, three timeouts at 180 s in 7822; `paddock-dev` golden 196 s on Windows; tests over 10 s are 37 % of all test time in the sample |
| Found only by CI (12 failed runs, 49 failed jobs) | 7 failed on static checks that run in seconds locally (C13 `clippy.toml` in three jobs, `check-wording`, C99, conformance, the `ui` job's check); 2 on the job-isolation holder (infrastructure); most of the rest on tests that pass alone and fail under the runner's load or on one OS: `paddock-chromium` real browser (3 runs, macOS), `dyno-bench` disk (4 runs, macOS), `dyno-platform` locks (2, macOS), `paddock-mcp` listener (Windows), `debrief-store-duck` writer (2, macOS) |

Three of these facts reorder the old plan in [2026-10-ci-speed.md](../research/2026-10-ci-speed.md):

1. **The critical path is the Linux queue, not a compile.** Without queueing a full run would take about 20 min; the
   Linux host's 4 lanes turn 159 job-minutes into at least 40 min. Anything that cuts Linux job-minutes or adds Linux lanes
   shortens the run; anything that speeds up a macOS or Windows job only helps once that is done.
2. **Most minutes are spent on runs that fail.** Fix rounds that rerun only what failed cut the biggest pile of minutes,
   with no gate removed, because the full run still follows.
3. **Test-level selection has little to win.** The nextest run phase is 3 % of Linux job time and 13 to 18 % on the host
   runners. Compile, Playwright and queueing are where the time is.

## Options evaluated

Savings are estimates derived from the measurements above unless marked measured. "Quality" says what happens to the
gates; no option removes one.

| Option | Expected saving | Cost | Risk | Quality |
| --- | --- | --- | --- | --- |
| **A. Fix rounds rerun only the failed jobs and tests on the new head** (`./dev plan --since-run <run>`: the failed jobs plus whatever the fix's diff reaches; nextest gets the failed tests as a filter) | about 76 % of a fix round's runner minutes (measured share outside the failed jobs); wall from a full run (median 45 min) to the failed job's own time plus a short queue: about 8 to 12 min | 2 agent-days (on top of the planned `--since-run`) | a fix that breaks a job that passed before is caught only by the full run that follows (rule 50 (i) keeps it) | unchanged: the full plan still runs once on the final tree |
| B. Gitea's `rerun-failed-jobs` on the same run | none for a fix: it reruns the old commit. Useful only for infrastructure failures (runner lost, GitHub outage) | 0.25 day (a `./dev ci rerun --failed` wrapper) | rerunning a red test on the same commit is a retry (ER-12) | only for jobs that failed before their tests ran |
| **C. Playwright workers per job from the host declaration** (3 on Linux), `fullyParallel` where specs are isolated, shards rebalanced by measured file time | Linux Playwright is 59 of 159 job-min per train pair; 3 workers save an estimated 30 to 40 job-min, so 8 to 10 min of Linux floor | 3 agent-days (spec isolation in `ui`, Dyno, Spotter, Pitcrew; ports per worker through `e2ePort`) | specs that share mock-server state fail in parallel; found by running each suite 20 times under load before the switch | also fixes ER-12: Debrief runs `retries: 2` in CI and Dyno, Spotter and Pitcrew `retries: 1` without `failOnFlakyTests` |
| **D. A shared compile cache on the Linux host** (sccache's WebDAV backend against a `bazel-remote` on `d-gtr9`'s local disk), then Linux from 4 to 6 jobs at once | removes the single-server race of ADR 0074 I7; 6 lanes turn 159 job-min into a 27 min floor instead of 40 (with C: about 21) | 2 agent-days plus a host action by the user | a new service on the host; a network hop per cache read (sub-millisecond on the same host) | unchanged; sccache keys stay content hashes |
| D'. One remote cache for all hosts (the act_runner cache server 172.16.1.188, or Redis/Valkey) | adds cross-host hits only where targets match (they do not across OSes); the host runners' misses are mostly genuine | 2 to 3 days | network latency on every hit; a shared failure point; Redis 7.4+ is not under an allowed licence (Valkey is BSD; licence facts read earlier, not re-verified here) | unchanged |
| **E. No GitHub at run time**: actions mirrored into the Gitea instance, every tool through the build-inputs store (`ci/build-inputs-wp1`) | removes the 4.6 min 504 tails (three jobs in 7821/7822, one of them the last job of 7821) and up to 88 s of action download per job | 1 agent-day after `ci/build-inputs-wp1` lands | a mirror that falls behind; Renovate keeps the pins | stronger: pinned, hashed inputs |
| **F. One preflight job for the cheap static checks** (conformance, C13, C99, wording, commit lint, `./dev gates check`, the endpoint register, icons, ESLint preset, tools) replacing 7 tiny Linux jobs; in fix rounds and branch runs the heavy Linux jobs `need` it | 6 fewer container starts and checkouts per run (about 4 job-min and 6 lane slots); a red static check stops about 90 Linux job-min in a gated run (the heavy Linux jobs of 7821) | 1.5 agent-days | 2 to 3 min added before the Linux jobs in a gated run, hidden today by a 30 min queue; on main, tags and nightly nothing is gated, so every failure is still reported | unchanged; failures arrive sooner |
| G. Build once, test many (a nextest archive per OS) | small: on each OS the Rust jobs build different workspaces or profiles (coverage, clippy, release); only Debrief's macOS tests have a run phase over 6 min (the 2026-10 threshold) | 2 days for Debrief macOS | archive and checkout must match (nextest: "The project source must be checked out to the same revision") | unchanged |
| H. Partition the Debrief macOS tests across two Mac slots (`--partition slice:m/2` over the archive), after fixing its slowest tests | 19 to 44 min today; slow tests fixed first (60 to 180 s each), then two partitions: about 11 to 12 min | 2 days (tests, time-boxed) + 2 days (partition) | nextest has no timing-based partitioning (only `slice`, `hash`, deprecated `count`), so heavy suites need their own test group | the slow tests are fixed, not skipped |
| I. Test-level selection from a per-test coverage map | at most the run phase: 3 % of Linux, 13 to 18 % of host job time | 6 to 10 days, plus upkeep of the map | silent misses: tests read files, goldens and environment that a coverage map does not see | weaker; a nightly full run would be the only net |
| **J. Package-level selection inside a workspace** (`-E 'rdeps(<changed crates>)'` from the planner) on branch runs and fix rounds, full on main, train, tags and nightly | the test build and run of crates the change cannot reach; mainly the shared `crates/` workspace on macOS and Windows (8 to 11 min jobs) | 2 agent-days | a test that reads a file of an unrelated crate (goldens): the planner's declared doc inputs mark those already | the train and main still run everything |
| K. Merge tiny jobs, split long ones by timing | merge: in F. Split: in H | — | — | — |
| L. Critical path first | Gitea's order of dispatch for queued jobs is not documented (not verified); the plan can still start long jobs first by listing them first and giving the Linux preflight-gated jobs a lower place | 0.5 day experiment | none | unchanged |
| M. Runner capacity and placement under ADR 0074 | Linux: D. Windows (16 vCPU, 32 GB, 3 jobs since 2026-11-09) and macOS (3 runners on one Mac shared with agents) are not the bottleneck once Linux is fixed; their average busy lanes are 0.5 and 0.7 | declared in ADR 0074's inventory | macOS lanes compete with agents (rule 41) | — |
| N. Skip docs-only and comment-only changes | rule 50 (d) and the planner already skip docs; the plan job itself takes a median 34 s | 0 | — | — |
| O. A fuller CI image (node_modules, more tools) | small: `paddock-ci:v2` already carries the toolchain, nextest, libduckdb, Chrome for Testing and Playwright's browsers; Linux `npm ci` is a median 5 s from the kept cache | 1 day | an image to rebuild on every lockfile change | — |
| P. Angular build once per run, Playwright browsers cached | browsers are already in the image (Linux) and kept on host runners; one Angular build is 13 to 42 s per job | 1 day | artifacts between jobs cost upload and download | — |
| Q. Cheaper local pre-push (ADR 0038 WP-2, WP-3) | the 7 of 49 failed jobs that a 1 to 2 min local static run catches, and the Linux- and Windows-only failures a container and `xwin` run catch; each cost a full run | already planned in ADR 0038 | — | moves checks earlier (ER-36) |

Not chosen: I (cost and silent misses for at most a few percent), D' (no cross-OS hits to gain, a remote failure point),
O and P beyond what exists (measured as small).

## Decision

1. **A fix round is a failed-only run on the new head.** `./dev plan --since-run <run>` (planned) gives the jobs that
   failed in `<run>` plus everything the fix's diff reaches; within a Rust job nextest receives the failed tests as a
   filterset, within a Playwright job the failed spec files. The run summary names the source run. After the last fix
   round, the full affected plan runs once on the final tree (rule 50 (i)). Gitea's `rerun-failed-jobs` (present in this
   instance's API, version 28.3.0) is used only for infrastructure failures on the same commit, never for a red test.
2. **The Linux lane gets lighter and wider.** Playwright workers come from the host declaration of ADR 0074 (3 per job on
   `d-gtr9`), specs that cannot run in parallel are fixed, and the shards are rebalanced from recorded file times. The
   Linux sccache moves to one cache service on the host, so that several jobs never share a local cache directory, and
   the host then runs 6 jobs at once (one declaration change in ADR 0074's inventory, measured in pairs before and after).
3. **Nothing comes from GitHub during a run.** Actions are mirrored into this Gitea; tools come from the build-inputs
   store. A run that would need the internet fails at the step that asks, with an `::error::` that names the input
   (ER-27).
4. **Cheap checks first.** One preflight job runs every static check in one container. On fix rounds and branch runs, the
   heavy Linux jobs need it; macOS and Windows jobs start at once (they are not the bottleneck). On main, the train, tags
   and the nightly nothing is gated, so every failure is reported in one run.
5. **The longest job is fixed, then split.** The Debrief macOS test job's slow suites are made faster (not skipped); if
   its run phase is still over 6 min, it is built once as a nextest archive and run in two partitions on two Mac slots.
6. **Selection stays at the package level.** Branch runs and fix rounds use `rdeps()` of the changed crates inside each
   workspace; per-test coverage maps are not built.
7. **Measured, always.** `./dev ci timings` (a port of the analysis used for this ADR) prints the baseline table for any
   run range; every work package records a before and after block in `docs/benchmarks.md` (rule 48), in pairs on a quiet
   machine where possible.

## Parallelism and cost (rule 51)

- **More parallelism:** Linux lanes 4 to 6 (D2), Playwright workers 1 to 3 per job (D6), Debrief macOS tests in two
  partitions (D7). The bound is the host's declared cores and the agents' share (ADR 0074): `d-gtr9` has 16 cores and
  128 GB and runs containers only; 6 jobs with 3 Playwright workers each stay inside it, Rust jobs keep
  `CARGO_BUILD_JOBS` from the declaration.
- **Cheaper:** failed-only fix rounds (D1), no downloads (D8), merged static jobs (D3), package-level selection (D5),
  faster slow tests (D7).

## Work packages and estimates (agent-days)

| WP | Content | Phase | Days | Depends on |
| --- | --- | --- | --- | --- |
| WP-0 | `./dev ci timings`: the baseline table for a run range (queue, phases, critical path, sccache, failures), results block output | 0 | 1.5 | prebuilt `./dev` |
| WP-1 | Actions mirror and the remaining tools through the build-inputs store; a check that no workflow names a GitHub action or URL | 0 | 1 | `ci/build-inputs-wp1` |
| WP-2 | Fix-round mode: `--since-run` failed-only plan, nextest and Playwright filters, run summary, `./dev ci rerun --failed` for infrastructure failures only | 0 | 2 | `--since-run`, Rust planner |
| WP-3 | Preflight job (7 tiny Linux jobs merged), `needs` on fix rounds and branch runs only, C100 entries updated | 1 | 1.5 | ADR 0060 register |
| WP-4 | Playwright parallelism: workers from the declaration, spec isolation, shard balance; ER-12 retry settings in Debrief, Dyno, Spotter, Pitcrew | 1 | 3 | ADR 0074 inventory |
| WP-5 | Linux compile cache service (`bazel-remote` on `d-gtr9` local disk, sccache WebDAV), then 6 Linux lanes; pairs measured | 1 | 2 | user host action (D4) |
| WP-6 | Debrief macOS: the slow `debrief-store-duck` and `debrief-core` property suites made faster (time-boxed), then archive and partition if still needed | 2 | 2 + 2 | — |
| WP-7 | Package-level selection (`rdeps`) per workspace on branch runs and fix rounds; planner output and tests | 2 | 2 | Rust planner |
| WP-8 | Critical path first: job order and placement experiment, kept only if a measured pair shows it | 2 | 0.5 | WP-0 |
| WP-9 | Local pre-push parity (ADR 0038 WP-2, WP-3) | 2 | as ADR 0038 | ADR 0038 |

Total new work: about 17.5 agent-days, of which phase 0 is 4.5, phase 1 is 6.5 and phase 2 is 6.5 (WP-9 counted in
ADR 0038).

## What each phase saves (estimates, to be measured)

| Phase | Fix round wall | Full `ci.yml` wall (median) | Runner minutes |
| --- | --- | --- | --- |
| Today (measured) | a new full run: median 45 min | 45 min (no-queue path 20 min) | 59 % in failed runs |
| 0: WP-0, 1, 2 | about 8 to 12 min | 40 to 43 min (the 504 tails off the critical path) | fix rounds about 76 % cheaper |
| 1: WP-3, 4, 5 | under 10 min | about 22 min (Linux floor about 21 min, no-queue path 20 to 21 min) | a red static check stops the heavy Linux jobs in gated runs |
| 2: WP-6, 7, 8, 9 | about 6 to 8 min | about 15 min critical path (Debrief macOS tests about 11 to 12 min plus plan) | fewer test builds on branch runs; fewer CI-only failures |

## Decisions for the user

- **D1. Fix rounds:** a new failed-only run on the new head (`--since-run`), with Gitea's `rerun-failed-jobs` only for
  infrastructure failures. *Recommended.* Alternative: rerun on the same commit (rejected: it is a retry, ER-12).
- **D2. Linux lanes 4 to 6** after the shared cache, measured in pairs. *Recommended.* Alternative: stay at 4 and rely on
  lighter jobs only (Linux floor about 30 min instead of 21).
- **D3. Preflight gating** on fix rounds and branch runs only; main, train, tags and nightly ungated. *Recommended.*
  Alternative: gate every run (adds 2 to 3 min to green full runs) or none (keeps runs like 7821 spending about 90 Linux job-min after a static check is already red).
- **D4. Compile cache backend:** `bazel-remote` (Apache-2.0) on `d-gtr9`'s local disk, Linux only, through sccache's
  WebDAV backend. *Recommended.* Alternatives: the act_runner cache server for all hosts (network on every hit, no
  cross-OS hits), Valkey (BSD); not Redis 7.4 or later (its licences are outside the allow-list; not re-verified here).
- **D5. Test selection:** package level (`rdeps`), no per-test coverage map. *Recommended.*
- **D6. Playwright:** 3 workers per Linux job and the ER-12 retry fix in the same work package. *Recommended.*
- **D7. Debrief macOS tests:** fix the slow suites first, partition only if the run phase stays over 6 min.
  *Recommended.* Alternative: partition now (uses a second Mac slot at once, keeps slow tests).
- **D8. No GitHub at run time:** actions mirrored into Gitea. *Recommended.* It needs a mirror owner (the coordinator)
  and Renovate on the mirror pins.
- **D9. CI image:** no further baking (node_modules, Angular caches). *Recommended* (measured small).
- **D10. Order:** phase 0 now, alongside the in-flight planner and prebuilt `./dev`; phase 1 after the train lands;
  phase 2 after Dyno's next tag. *Recommended.* Each phase ends with a measured results block before the next starts.

## Risks

- Parallel Playwright exposes shared state in specs; the switch waits for 20 runs per suite under load (checklist §1).
- A failed-only fix round misses a regression the fix caused elsewhere; the full run before landing catches it (rule 50 (i)),
  so it costs time, not quality.
- More Linux lanes raise memory and I/O pressure on `d-gtr9`; the ADR 0074 live check and the paired measurement decide,
  and the change is one number to roll back.
- The cache service is a new component on the host; if it is down, sccache compiles locally (slower, not red). This
  fallback is to be verified in WP-5 with a positive control (ER-33).
- Estimates here are derived from one busy week; the 2026-10 note showed that load changes timings by up to 60 %. Every
  phase is judged on its own measured pair, not on these numbers.

## Consequences

- Rule 50 (i) gets its mechanism (`--since-run`); ER-12 violations in four Playwright configs are fixed on the way.
- ADR 0025 §8 ("one job per Linux host") is superseded for Linux by the cache service and the ADR 0074 declaration.
- `docs/benchmarks.md` carries the CI baseline and each phase's results; `./dev ci timings` replaces ad hoc scripts.

## Alternatives considered

- **Per-test coverage selection** (option I): little to win, silent misses.
- **One cross-host remote cache** (D'): no cross-OS hits, a network hop on every hit.
- **More Windows and macOS runners first**: their average busy lanes are 0.5 and 0.7; Linux is the bottleneck.
- **Fail-fast on every run**: on main and the train every failure must be reported in one run; fail-fast belongs to fix
  rounds and branch runs.

## Sources

- nextest archiving: https://nexte.st/docs/ci-features/archiving/ (read 2026-11-09: archives omit sources; same revision
  required; `--workspace-remap`).
- nextest partitioning: https://nexte.st/docs/ci-features/partitioning/ (read 2026-11-09: `slice`, `hash`, deprecated
  `count`; no timing-based mode described).
- nextest filtersets: https://nexte.st/docs/filtersets/reference/ (read 2026-11-09: `deps()`, `rdeps()`).
- sccache local storage: https://github.com/mozilla/sccache/blob/main/docs/Local.md (read 2026-11-09: "The local storage
  only supports a single sccache server at a time").
- sccache WebDAV: https://github.com/mozilla/sccache/blob/main/docs/Webdav.md (read 2026-11-09: lists Bazel Remote
  Caching as a backend; `SCCACHE_WEBDAV_ENDPOINT`).
- bazel-remote: https://github.com/buchgr/bazel-remote (read 2026-11-09: Apache-2.0; HTTP GET/PUT). Its current
  release and maintenance activity were not shown on the page: unverified.
- Gitea API of this instance: `https://dev.decryptology.net/swagger.v1.json`, version 28.3.0 (verified 2026-11-09:
  `POST /repos/{owner}/{repo}/actions/runs/{run}/rerun-failed-jobs` and `.../jobs/{job_id}/rerun` exist; not exercised).
- Gitea's order of dispatch for queued jobs: not documented in what was read; unverified.
