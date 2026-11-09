# 80. CI does only the necessary, once, in the cheapest place: a necessity audit, Linux first, failed-only fix rounds

- Status: Proposed (2026-11-09), revision 2. Drafted at the user's request of 2026-11-09 ("i want the CI to be really
  optimized and intelligent and at the speed of light"). Revision 2 answers the independent review of revision 1
  (`45253615a`) and adds the user's two later requests of the same day: "most of the test should be running on linux,
  only OS specific test running on the specific OS" (§4, D11) and "make the CI really optimized and only do the
  necessary" (the governing principle and the necessity audit, §3). Needs the user's decisions D1 to D13 before any build.
- Date: 2026-11-09
- Related: ER-36 (speed and quality together; engineering-rules `checklists/speed-and-quality.md`), ER-7/ER-8 and rule 3
  (affected tests, full suite once), ER-9/ER-10 and rule 50 (h) and (i) (local checks overlap CI; fix rounds rerun only
  what failed, then one full run), ER-12 (no retries that turn red green), ER-20 / rule 51 (parallelism and cost),
  ER-24 / rule 41 (machine load), ER-31 (per-app release judgement), ER-33 / rule 71 (a check counts only with a verified
  precondition), rule 48 (benchmarks recorded), rule 54 (installers before landing); ADR 0014 §6 (affected CI), ADR 0022
  (`./dev`, the Rust planner, the prebuilt binary), ADR 0025 (CI job isolation, §8 one job per Linux host), ADR 0038 (CI
  parity before push), ADR 0060 (every check runs somewhere: the gate register `docs/gates.toml`), ADR 0073 (no test
  doubles in a shipped build), ADR 0074 (declared hosts and machine leases, D2 and WP-B3: a per-job sccache cache on the
  Linux host; on `train/2026-11-09-2`); the research notes [2026-10-ci-speed.md](../research/2026-10-ci-speed.md) and
  [2026-10-ci-timings.md](../research/2026-10-ci-timings.md).
- In flight, not re-decided here: the Rust planner (`feat/dev-plan-port`), the prebuilt `./dev` (`ci/dev-prebuilt`), the
  kept-clone sparse checkout (on `train/2026-11-09-2`: Windows checkout 102 s to 34 s), the private build-inputs store
  (`ci/build-inputs-wp1`), `./dev plan --since-run` and `.gitea/` precision in the Rust planner (planned).

## Governing principle

**CI does only what is necessary, once, in the cheapest place.** A check runs when its inputs changed, on the one
operating system where its result can differ, in the job that already has what it needs; a check whose inputs did not
change does not run; the same check never runs twice for one tree. Nothing is dropped because it is quiet: a check that
never fails in the window keeps running, made cheaper or moved to the nightly and the release gates, and every move that
removes coverage from a run is a user decision with a stated reason. Speed never buys a weaker gate (ER-36): the full
suite on every operating system still runs nightly and in each app's release gates (rule 54, ER-31), and the run that
lands a change is always a full, ungated run.

## Summary

CI today is not slow because jobs are slow; it is slow because jobs wait, and because a good part of what runs is not
needed for the change at hand. Over 5.5 days the median `ci.yml` run took 45 min, while its critical path without any
queueing was 20 min; the last job of a run waited a median 34 min for a runner. The waits come from bursts: whenever a
Linux job waited, all 4 Linux lanes were busy in 74 % of those minutes, and in 56 % two or more runs were in play,
mostly train pairs (`ci.yml` and `debrief.yml` on one head). Between bursts the Linux host is idle 61 % of the time, by
day and by night alike. Of 17,597 runner minutes, 59 % went to runs that failed and 20 % to runs that were cancelled;
inside a failed run only 24 % of the minutes were in the jobs that failed.

The necessity audit (§3) finds about 2,350 job-minutes in 5.5 days (13 % of all runner minutes) spent on duplicates and
on checks whose inputs had not changed, before any change to fix rounds. Linux-first testing (§4) moves the generic tests
off the macOS and Windows runners and shortens Debrief's macOS job, the longest one left.

The plan, in order of measured effect: (1) a fix round reruns only the failed jobs and tests (rule 50 (i)), (2) each
check runs once, where its inputs changed (the audit), (3) the Linux lane gets lighter (Playwright workers, merged tiny
jobs) and, after a measured spike, wider, (4) generic tests run on Linux only in branch, fix and train runs, (5) nothing is
downloaded from GitHub during a run.

Targets: a typical fix round **under 10 min** (today a fix round is a new full run, median 45 min; the Debrief macOS
job, 19 to 44 min today, meets it only through D1 and D11, see §4.3); a full `ci.yml` run **about 22 min** after phases
0 and 1, and a full-run critical path **about 15 min** after phase 2.

## 1. Measured baseline

All numbers come from the Gitea Actions API (job and step timestamps, runner names) and the job logs, read-only. The
sample is the 216 runs 7480 to 7825 (2026-11-04 05:00 to 2026-11-09 18:38 +08). Of the 5,005 jobs they list, 3,957
were not skipped and 3,624 have both a start and an end time; the job counts and minutes here use those 3,624 (a job
cancelled before it started has no minutes). Logs were read for runs 7696, 7697, 7796, 7797, 7803, 7810, 7812, 7813,
7819, 7821 and 7822; runs 7824 and 7825 were still running at 18:38 and were read, not waited for. The results block,
with method and limits, is in
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
| Train pair 7824 / 7825 at 16.7 min | 19 `ci.yml` jobs still queued: 17 Linux, 2 macOS |
| Linux job time by phase (6 runs of logs) | Playwright 47 %, cargo compile 15 %, nextest run 3 %, the rest npm, Angular, checks and setup |
| macOS / Windows job time by phase | compile 50 % / 54 %, nextest run 18 % / 13 %; macOS and Windows jobs run only Rust (clippy, nextest, doc tests) |
| Setup share (pre-step, checkout, npm, tools, upload, cleanup) | Linux 20 % (checkout median 10 s; 54 % of Linux jobs last under 2 min); Windows 43 % (checkout median 105 s before the kept clone); macOS 31 % (checkout median 57 s, p90 268 s) |
| Runner concurrency observed | `d-gtr9` runs 4 jobs at once (47 % of the last 24 h at exactly 4); Windows ran 1 job at a time until the afternoon of 2026-11-09, 3 since; macOS 3 runners since 2026-11-09 01:03 |
| Queue wait, last 72 h | Linux p50 7 min, p90 35 min; Windows p50 21 min, p90 80 min; macOS p50 20 min, p90 73 min |
| Linux lanes while a Linux job waited, last 72 h | all 4 busy in 74 % of those minutes; two or more runs in play in 56 % |
| Linux host idle, last 72 h | 61 % of the time; 62 % between 00:00 and 08:00, 61 % between 08:00 and 24:00. The mean of 1.3 busy lanes of 4 is low because of the idle stretches, not because the bursts fit |
| sccache (local disk per host) | Linux hit rate 91 % (34,027 hits, 3,217 misses); macOS 24 %, Windows 27 %. On the host runners the kept build folders already skip unchanged crates, so the low rate is mostly genuine changes: `crate` missed 78 on Linux and 79 on macOS in run 7821. Cold slots are the exception: run 7810 `app dyno (windows)` on slot `s3` missed 1,020 |
| Repeated compiles | 7821's 10 Linux Rust jobs made 12,729 compile requests (one per crate built), each job building its own dependency graph from cache |
| Downloads from GitHub during a run | `taiki-e/install-action` for `cargo-about` took 277 s, 279 s and 276 s in 7821/7822 (eight HTTP 504 retries each; 1 to 19 s when GitHub answers). Action downloads (`Prepare all required actions`) took up to 88 s. The CI image pins DuckDB 1.5.6 (`ci-image/Dockerfile`), so a job that needs DuckDB 2.0.0 downloads it |
| Runner minutes by run outcome (5.5 days) | 17,597 in all. Linux 9,547: failed runs 59 %, cancelled 20 %, green 20 %. Windows 3,839: 64 / 15 / 21 %. macOS 4,211: 59 / 24 / 16 % |
| Failed runs: minutes in the failed jobs | 24 % (45 failed `ci.yml`/`debrief.yml` runs); median share of jobs failing 9 % |
| Slowest tests | `debrief-store-duck` suites on macOS: 60 to 126 s each, three timeouts at 180 s in 7822; `paddock-dev` golden 196 s on Windows; tests over 10 s are 37 % of all test time in the sample |
| Found only by CI (12 failed runs, 49 failed jobs) | 7 failed on static checks that run in seconds locally (C13 `clippy.toml` in three jobs, `check-wording`, C99, conformance, the `ui` job's check); 2 on the job-isolation holder (infrastructure); most of the rest on tests that pass alone and fail under the runner's load or on one OS: `paddock-chromium` real browser (3 runs, macOS), `dyno-bench` disk (4 runs, macOS), `dyno-platform` locks (2, macOS), `paddock-mcp` listener (Windows), `debrief-store-duck` writer (2, macOS) |

Three of these facts reorder the old plan in [2026-10-ci-speed.md](../research/2026-10-ci-speed.md):

1. **The critical path is the Linux queue in bursts, not a compile.** Without queueing a full run would take about 20
   min; a train pair's 159 Linux job-minutes on 4 lanes take at least 40 min. Bursts, mostly train pairs, fill all 4
   lanes. Anything that cuts Linux job-minutes per run or adds Linux lanes shortens the run; speeding up a macOS or
   Windows job only helps once that is done, except the Debrief macOS job, which is `debrief.yml`'s critical path.
2. **Most minutes are spent on runs that fail.** Fix rounds that rerun only what failed cut the biggest pile of minutes,
   with no gate removed, because the full run still follows.
3. **Test-level selection by coverage has little to win.** The nextest run phase is 3 % of Linux job time and 13 to
   18 % on the host runners. Duplicated jobs, unchanged inputs, Playwright and queueing are where the time is.

## 2. Options evaluated

Savings are estimates derived from the measurements unless marked measured. "Quality" says what happens to the gates.

| Option | Expected saving | Cost | Risk | Quality |
| --- | --- | --- | --- | --- |
| **A. Fix rounds rerun only the failed jobs and failed tests** (rule 50 (i), D1) | about 76 % of a fix round's runner minutes (measured share outside the failed jobs); wall about 8 to 12 min | 2 agent-days on top of the planned `--since-run` | a fix that breaks something that passed before is caught by the full run that follows | unchanged: the full plan still runs once on the final tree |
| B. Gitea's `rerun-failed-jobs` on the same commit | none for a fix (it reruns the old commit); allowed only to retry a suspected flake | 0.25 day | must never turn red green (ER-12) | the rerun's result is reported beside the first, never replaces it |
| **C. Playwright workers per job from the host declaration** (3 on Linux), specs made isolated, shards rebalanced; the five app configs moved onto the `@decryp7/paddock-e2e` preset | Linux Playwright is 59 of 159 job-min per train pair; 3 workers save an estimated 30 to 40 job-min, 8 to 10 min of Linux floor | 3 agent-days | specs that share mock-server state fail in parallel; each suite runs 20 times under load before the switch | fixes ER-12 in five configs (D6) |
| **D. Wider Linux lane**: one shared cache service, then 6 lanes, after a spike | 6 lanes turn 159 job-min into a 27 min floor instead of 40 (with C: about 21) | 0.5-day spike, then 2 agent-days and a host action | memory, I/O and flakes at 6 lanes; the WebDAV backend against `bazel-remote` is unverified | pass conditions in D2 |
| **E. No GitHub at run time** (actions mirror, the build-inputs store, libduckdb and the image) | removes the 4.6 min 504 tails and up to 88 s of action download per job | 1.5 agent-days after `ci/build-inputs-wp1`, with the image rebuild | a mirror that falls behind; Renovate keeps the pins | stronger: pinned, hashed inputs |
| **F. One preflight job** for the cheap static checks, replacing 7 tiny Linux jobs | 117 job-min of tiny jobs in 5.5 days become one container per run (6 lane slots per run); a red static check stops about 90 Linux job-min in a gated run | 1.5 agent-days | 2 to 3 min before the Linux jobs of a gated run; never gates the run that lands (D3) | failures arrive sooner |
| **G. Necessity audit: each check once, only when its inputs changed** (§3) | about 2,350 job-min in 5.5 days | 4 agent-days | a declared input list that misses an input; the nightly and release runs are the net | every removal of per-run coverage is a user decision (D11, D12) |
| **H. Linux first, OS-specific tests on the host runners** (§4) | about 520 host job-min in 5.5 days (12 % of the Rust host jobs); Debrief macOS job from 19 to about 12 to 15 min | 3.5 agent-days | a generic-looking test that only fails on one OS slips to the nightly or the release gates (§4.4) | the full suite still runs on every OS nightly and before each release |
| I. Build once, test many (a nextest archive per OS) | small: on each OS the Rust jobs build different workspaces or profiles; only Debrief's macOS tests had a run phase over 6 min, and H shortens it | 2 days | archive and checkout must match | unchanged |
| J. Partition the Debrief macOS tests across two Mac slots | after H and the slow-suite fixes, probably not needed | 2 days | nextest has no timing-based partitioning | unchanged |
| K. Test-level selection from a per-test coverage map | at most the run phase: 3 % of Linux, 13 to 18 % of host job time | 6 to 10 days, plus upkeep | silent misses: tests read files, goldens and environment | weaker |
| **L. Package-level selection inside a workspace** (`rdeps()` of the changed crates) on branch runs and fix rounds | the test build and run of crates the change cannot reach | 2 agent-days | a test that reads another crate's file; the planner's declared doc inputs mark those | the train and main still run everything |
| M. Critical path first | Gitea's dispatch order for queued jobs is not documented (unverified); job order is the only lever | 0.5 day experiment | none | unchanged |
| N. A fuller CI image (node_modules, more tools) | small: Linux `npm ci` is a median 5 s; the needed change is DuckDB 2.0.0 (E, D9) | — | — | — |

Not chosen: K (cost and silent misses for a few percent), a cross-host remote cache (no cross-OS hits; the host runners'
misses are mostly genuine), N beyond the DuckDB rebuild, I and J unless H leaves Debrief's macOS run phase over 6 min.

## 3. Necessity audit

Every job of the measured window, by workflow, with what it caught (failed jobs in 5.5 days), what it cost (job-minutes),
how often it was the last job of its run (a proxy for the critical path) and whether something else checks the same.
Jobs under 5 job-minutes are left out and covered by the step list. A failure count includes infrastructure failures and
flakes; it is evidence that a job can see something, not a count of real bugs.

### 3.1 Jobs

| Workflow: job | Runs | Job-min | Failed | Last | Catches (examples from the window) | Duplicate of | Verdict |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `ci.yml` `crate (macos / windows / ubuntu)` | 66 / 60 / 66 | 679 / 642 / 108 | 13 / 22 / 13 | 2 / 1 / 1 | macOS: `paddock-chromium` real browser; Windows: `paddock-mcp` listener, `paddock-limits` golden (line ends); Linux: the features-off build (failed alone 11 times) | doc tests and the features-off build run on all three OS; on Windows they never failed alone, on macOS once each | keep; doc tests and features-off on Linux only in branch, fix and train runs (D11) |
| `ci.yml` `crates` (fmt, hakari, machete, coverage, deny) | 66 | 114 | 20 | 0 | coverage gate, unused dependencies | — | keep |
| `ci.yml` `app dyno / spotter / pitcrew / pitwall` on macOS and Windows | 50 to 61 each | 1,061 macOS, 1,029 Windows | 3 to 19 | 33 of them `app spotter (windows)` | OS adapters: Dyno platform locks, Pitwall admin key ACL, Spotter folder watch | their generic tests also run on Linux | keep; OS set only in branch, fix and train runs (D11) |
| `ci.yml` `app <app> (ubuntu)` (six apps) | 57 to 64 | 1,161 | 1 to 15 | 1 to 3 | C13, wording, nextest failures | `npm run build -w ui` in every Linux leg (14 to 19 s, 9 times a run) | keep |
| `ci.yml` app and reference e2e legs (Chromium) | 55 to 62 each | 2,035 | 1 to 6 | 1 | Dyno, Pitwall, Spotter, reference apps | — | keep; 3 workers (C) |
| `ci.yml` WebKit legs (`app * (ubuntu, e2e-webkit)`, `reference apps in WebKit (macOS)`) | 19 to 22 | 664 | 8 | 2 | WebKit-only rendering | the same WebKit legs in `dyno-release.yml` (301) and `nightly.yml` (265) | once per main tree: nightly and release only (D12) |
| `ci.yml` `paddock-dev (macos / windows / ubuntu)` | 58 / 59 / 68 | 291 / 356 / 74 | 17 / 13 / 5 | 4 / 4 / 2 | planner goldens, wrapper tests | — | keep; the tool's OS set on hosts (D11) |
| `ci.yml` `ui` | 63 | 134 | 8 | 0 | tokens, components, publish dry run | — | keep; publish dry run only when its inputs change (§3.3) |
| `ci.yml` `conformance`, `icons`, `eslint preset`, `tools`, `C99`, `endpoint register` | 2 to 73 | 117 | 14 | 3 | C13, C99, C100 | each pays its own container, checkout and npm | merge into one preflight job (F) |
| `ci.yml` `commit messages` | 81 | 57 | 0 | 2 | Conventional Commits, no attribution | `debrief.yml` `licences, notices + commits`, `dyno-release.yml` `commit messages` (14 job-min), and rule 50 (g) locally before landing | once per run, in the preflight job |
| `ci.yml` `plan` | 86 | 135 | 1 | 3 | — | each workflow plans the same head again (`debrief.yml` 52, `dyno-release.yml` 71 job-min) | keep; cheap once the prebuilt `./dev` lands |
| `debrief.yml` `rust tests (macOS)` | 17 | 259 | 8 | 6 | store writer deadlock, plugin harness, EMFILE at 256 open files (ADR 0074 I3) | its generic suites also run in `rust tests` on Linux | OS set plus a declared store smoke (§4) |
| `debrief.yml` `end-to-end (shard 1-4/4)` | 17 to 20 | 740 | 6 | 3 | Debrief UI flows | — | keep; already 2 workers |
| `debrief.yml` `rust tests`, `rust format + clippy`, `frontend`, `plugin SDK templates`, `licences, notices + commits` | 21 each | 475 | 7 | 3 | notices, clippy, plugin SDK | commit lint (above) | keep; plugin SDK templates and notices only when their inputs change (§3.3) |
| `debrief.yml` `real-app smoke`, `bundle` (3 OS) | 9 to 10 | 581 | 4 | 8 | installers, smoke | — | keep (full runs only, as today) |
| `dyno-release.yml` gates (`app dyno` ubuntu, macOS, Windows, e2e-1, e2e-2, e2e-webkit) | 19 to 20 | 903 | 29 | 1 | the failures `ci.yml` saw on the same heads | `ci.yml` `app dyno` legs on the same commit | installers only when `ci.yml` was green on the same commit (D13) |
| `dyno-release.yml` `bundle` (3 OS) | 35 | 1,304 | 7 | 1 | installers (rule 54) | `nightly.yml` bundles on main | keep (rule 54) |
| `nightly.yml` (all jobs) | 8 to 40 | 2,285 | 30 | 0 | the full suite and bundles on main | — | keep: it is the net for D11, D12 and §3.3 |
| `debrief-fuzz.yml` (6 shards) | 6 each | 1,534 | 10 | 6 | fuzz crashes (shards 5 and 6) | — | keep; a shard runs when its target crates changed since its last green night (§3.3) |
| `debrief-bench.yml` `store benchmarks (10M events)` | 6 | 37 | 6 | 6 | none: it failed every run | — | fix or quarantine with an owner: a check that always fails tells nothing (ER-33) |
| `service-smoke.yml` macOS VM update smoke | 8 | 152 | 5 | 7 | the update path | — | keep; its failure rate is its own work item |

### 3.2 Steps

| Step | Runs | Min | Failed | Note |
| --- | --- | --- | --- | --- |
| `actions/checkout` | 3,641 | 2,090 | 21 | the kept clone (on the train) is the fix; the 21 are infrastructure |
| `stable-workspace` | 2,015 | 804 | 34 | the macOS cross-volume copy (ADR 0074 I4); the kept clone replaces it |
| `Tests with the optional features off` (`crate`) | 204 | 193 | 24 | macOS 97 min, Windows 68, Linux 28; failed alone 11 times on Linux, once on macOS, never on Windows |
| `Doc tests (workspace, every feature)` (`crate`) | 204 | 181 | 3 | macOS 83, Windows 71, Linux 27; failed alone once (macOS) |
| `Third-party notices are up to date` (Debrief) and `notices.sh --check` in each Linux app job | 21 + about 240 | 49 + about 170 | 4 | inputs are the lockfiles and the notice templates |
| `Memory budget` (Debrief) | 21 | 16 | 0 | runs with `continue-on-error`, so it can never fail: it does not count as a check (ER-33) |
| `Publish dry run` (`ui`) | 67 | 11 | 0 | inputs: `ui/` package manifests and the public API |
| `Lockfile is current` | 199 | 9 | 0 | quiet, cheap: keep |
| `Lint` (ESLint) | 88 | 22 | 0 | quiet, cheap: keep |
| `Build the library (all entry points)` | 67 | 15 | 0 | quiet; a prerequisite of the specs: keep |
| `plugin SDK templates` (job) | 21 | 56 | 0 | inputs: the plugin SDK crates and `templates/` |
| `cargo check --locked --lib --bins` (Dyno, ADR 0073, on the train) | 3 so far | 53 s Linux, 195 s Windows, 266 s macOS each | — | builds without dev-dependencies, which clippy `--all-targets` cannot; keep, measure after a week |

### 3.3 What a run needs: planner rules

Each check in ADR 0060's register (`docs/gates.toml`) gets an `inputs` list (paths or a project) and a `where` (Linux,
OS set, every OS). The planner runs a check only when one of its inputs changed since the last green run of that check
on the same line, and only where its result can differ. The rules from the audit:

- WebKit e2e: on the nightly and in the release gates, not on every main push (D12).
- Notices (`notices.sh --check`, Debrief's notices step): when a lockfile or a notice template changed.
- `Publish dry run`: when a `ui/` package manifest or `public-api.ts` changed.
- `plugin SDK templates`: when the plugin SDK crates or `templates/` changed.
- Fuzz: a shard runs when its target crates changed since its last green night.
- Commit lint: once per run, in the preflight job; a release workflow reads that result for the same commit.
- Doc tests and the features-off build: Linux only in branch, fix and train runs (D11).
- A release workflow dispatched for rule 54 on a commit whose `ci.yml` run was green builds the installers only (D13).
- Never by input alone: the run that lands a change, the nightly and the release gates run their whole plan.

Estimated saving of §3 over the 5.5 days measured: the duplicated Dyno gates 903 job-min, WebKit on main pushes 664,
doc tests and features-off on the host runners 319, input-driven skips about 290 (notices, publish dry run, plugin SDK
templates), merged tiny jobs and one commit lint up to 180: about 2,350 job-min, 13 % of the 17,597 measured. The fuzz rule
saves up to 1,534 job-min on nights when the fuzzed crates did not change; it is not counted.

### 3.4 Quiet checks are kept

No check is dropped for being quiet. `Lockfile is current`, `Lint`, `Build the library`, `Publish dry run`, and the
`icons`, `eslint preset`, `tools` and `commit messages` jobs never failed in the window; they protect against lockfile
drift, ESLint rule breaks, broken library entry points, a failing npm publish, stale installed icon copies, a broken lint
preset, broken shared scripts and bad commit history. They stay, made cheaper (one preflight container) or input-driven.
The `Memory budget` step is the opposite case: it looks quiet because it cannot fail. It becomes a counted check with
its budget or moves to `debrief-bench.yml` (WP-10).

## 4. Linux first: OS-specific tests on the host runners

### 4.1 Proposal

Branch runs, fix rounds and train runs run the whole suite on Linux. The macOS and Windows legs run clippy (the
OS-specific code only compiles there) and only the OS set of tests. The nightly and each app's release gates run the
full suite on every OS (rule 54, ER-31), so nothing ships untested on its OS. A generic test's host-OS run therefore
happens at the next nightly after it reaches main, or at the app's release gates, whichever comes first.

### 4.2 How the OS set is selected: declared, then checked

A test belongs to the OS set when any of these holds; the planner builds a nextest filterset from them per workspace:

1. **It exists only on that OS** (`#[cfg(windows)]`, `#[cfg(target_os = "macos")]`, `#[cfg(unix)]` on the test or its
   module): it is not compiled on Linux, so it always runs where it compiles.
2. **Its test binary touches the OS**: any source file of the binary, shared harness files such as
   `tests/it/common/mod.rs` included, has `cfg(target_os | windows | unix | target_family)` code, uses a declared
   OS-touching API, or depends directly on a declared OS-touching crate. The list: processes and signals
   (`std::process`, `tokio::process`, `paddock-proc`), sockets and pipes (`TcpListener`, `TcpStream`, `UnixStream`,
   named pipes) and HTTP clients that connect, file permissions and ACLs (`PermissionsExt`, `set_permissions`, the
   `paddock-secrets` and `paddock-storage` private-folder calls), line endings in goldens (`\r\n`, checked-out text
   compared byte for byte), installers and services (`launchctl`, `systemctl`, the registry, NSIS includes), real browsers
   (`paddock-chromium`).
3. **Its package is a platform adapter**, declared in `paddock.ci.json` as `osAdapters`. Today's candidates by `cfg`
   density (at least one `cfg(os)` per source file): `paddock-proc`, `paddock-service`, `paddock-ipc`, `paddock-secrets`,
   `paddock-mcp`, `paddock-dialogs`, `dyno-platform`, `dyno-app`, `dyno-helper`, `dyno-helper-service`, `dyno-bench`,
   `pitcrew-service`, `spotter-sources`, `chromium-driver`.
4. **It is declared** in the workspace's `.config/nextest.toml` as `os-specific` (a test group with a filterset), for what
   rules 1 to 3 cannot see: resource limits (a large-folder store test for the macOS open-files limit, ADR 0074 I3), file
   watching, time zones.

A new conformance check (registered under ADR 0060) fails when a test with a `cfg(os)` attribute, or in a binary that
rule 2 marks, is outside the generated OS set, and when an `osAdapters` package has no `cfg(os)` code left. The
generator and the check share one implementation in `./dev` (rule 55).

### 4.3 Measurements

Test time below is the sum of nextest's per-test durations (summed over parallel tests, not wall) from the logs of runs
7696, 7697, 7810, 7813, 7821 and 7822. A test's class here is taken from its own source file, a cheap stand-in for rule 2:
it misses shared harness files and dependencies on OS-touching crates (see 4.4), so the generic share is an upper bound.

| Measure | macOS | Windows |
| --- | --- | --- |
| Test time sampled | 347 test-min | 103 test-min |
| Adapter packages (rule 3) | 15 % | 34 % |
| Source file has `cfg(os)` (rule 2) | 13 % | 11 % |
| Source file uses a listed OS-touching API (rule 2) | 35 % | 14 % |
| Unknown package (`paddock-dev`, outside the scan) | 2 % | 13 % |
| **Generic (none of these)** | **34 %** | **29 %** |
| Packages with any `cfg(os)` code | 75 of 134 packages; 73 % of macOS and 80 % of Windows test time | |

A package-level rule ("any `cfg(os)` in the package, plus its reverse dependencies") would keep 73 to 80 % of the host
test time and save little; the per-binary rule above is needed.

What Linux first saves in host minutes (5.5 days; the Rust host jobs, `crate`, `app *`, `paddock-dev` and Debrief's macOS
tests, took about 4,300 host job-min):

| Item | Host job-min today | Saved | How |
| --- | --- | --- | --- |
| `crate` doc tests and features-off build on macOS and Windows | 319 (macOS 180, Windows 139) | 319 | Linux only in branch, fix and train runs |
| Generic tests in the host nextest runs (about 30 % of a run phase that is 13 to 18 % of job time) | part of the 4,300 | about 200 | the filterset; test binaries with only generic tests (26 of 83 in Debrief, 2 % of its test time) are not built |
| **Total** | | **about 520 host job-min, 12 % of the Rust host jobs** | |

Compile stays: the host jobs still build the workspace for clippy and the OS-set test binaries, and compile is about
50 % of their time. Linux first is therefore a moderate saving in host minutes; its main value is on the critical path
of `debrief.yml`.

**Worked example: Debrief `rust tests (macOS)`.** Wall 8.3, 19.1 and 46.0 min in runs 7697, 7822 and 7813 (the last
under load); compile 6.6 min in 7822. Its test time over the three runs is 208 test-min: 34 % generic by the file rule,
13 % `cfg(os)` files, 51 % files that start processes or open connections, 2 % adapter packages. The slowest generic
suites are data logic: `debrief-store-duck::it` `differential` (23 test-min), `large_folder` (10), `analysis` (8),
`segments` (6), `debrief-core::it` `properties` (7) and `survey` (5). In branch, fix and train runs these run on Linux
only, with one declared large-folder store test kept on macOS for the open-files limit. Estimate: the job drops from 19
min to about 12 to 15 min, and below 10 min with WP-6's slow-suite fixes. In a fix round it runs only its failed tests
(D1): compile plus the failed tests, about 8 to 9 min. That is what lets the fix-round target hold; without D11 and D1
the Debrief macOS job alone breaks it.

### 4.4 Would it still catch today's host-only failures?

The combined train (`train/2026-11-09-2`) failed on Windows on three generic-looking causes, and the macOS legs on
load-sensitive tests. Checked against the selection:

| Failure | Where the fix went | Selected by | Caught in a branch, fix or train run? |
| --- | --- | --- | --- |
| CRLF checkout in the `per_machine` include test (`44561f6bc`) | `crates/paddock-service/src/per_machine/tests.rs` | rule 3 (`paddock-service` is an adapter) and rule 2 (line-ending comparison) | yes |
| Windows temp-folder ACL for test secrets (`a0cb3d36f`) | `crates/paddock-secrets/src/file.rs` (33 `cfg(os)` uses), `dyno-platform` | rules 2 and 3 | yes: the test-only ACL path is in an adapter package with `cfg(windows)` code |
| Pitwall's `admin.key` on a Windows runner (`000685e63`): about 100 `pitwall-server::it` tests failed | `pitwall-server/src/auth.rs` (`cfg(os)`) and `tests/it/common/mod.rs` (processes, sockets) | rule 2 at binary level: the shared harness marks the whole `it` binary. At file level 51 of these failures looked generic, which is why the rule is per binary | yes |
| Pitwall's 2.2 s refused-loopback connect (`50772feb8`, `pitwall-server` `library::tests::unreachable_...`) | `pitwall-server/src/egress.rs`: no `cfg(os)`, no listed API in the file; the package is below the adapter threshold | rule 2 only through the "HTTP clients that connect" entry, or a rule 4 declaration | yes with that entry; without it, it slips |
| macOS: `paddock-chromium` real browser, `dyno-bench` disk, `dyno-platform` locks, `debrief-store-duck` writer deadlock, `dyno-agent` term | browsers, processes, adapters | rules 2 and 3 | yes |
| macOS: `debrief-plugin-e2e` store worker, `spotter-render` real browser | the tests start a worker or a browser through a library | rule 2 through the direct dependency on `paddock-proc` and `paddock-chromium` | yes with those entries; slips without |
| macOS: DuckDB EMFILE at 256 open files (ADR 0074 I3) | a runner limit seen by a generic store test | rule 4 (the declared large-folder store test) | only through the declaration |

**What slips.** A generic-looking test whose OS behaviour comes through code no rule names: a new HTTP client crate, a
new way of starting a process, a library that behaves differently by OS without `cfg` in Paddock's code (DuckDB, case
sensitivity of the file system, path length limits) and resource limits not declared under rule 4. Such a failure is found
by the next nightly (within 24 h of the change reaching main; the nightly runs when main changed) or by the app's
release gates, whichever comes first, never after a release. Each slip found that way adds its pattern to rule 2's list
or a rule 4 declaration in the same fix (ER-21), and the conformance check keeps the list honest.

## 5. Decision

1. **Fix rounds rerun only what failed** (rule 50 (i), ER-10): only the failed jobs and, inside them, only the failed
   tests; nothing that passed reruns. The fix round is dispatched with `base` set to the last tested commit. Gitea's
   `rerun-failed-jobs` on the same commit (present in this instance's API, version 28.3.0) may retry a suspected flake,
   never to turn red green: its result is reported beside the first. After the last fix round the full affected plan
   runs once on the final tree, and that run is the one that lands.
2. **Each check runs once, where its inputs changed** (§3): declared `inputs` and `where` in the gate register, the
   planner rules of §3.3, the duplicated jobs removed.
3. **Linux first** (§4): generic tests on Linux only in branch, fix and train runs; the OS set on the host runners from
   the declared mechanism and its conformance check; the full suite on every OS nightly and in the release gates.
4. **The Linux lane gets lighter, then wider.** Playwright workers from the host declaration and merged tiny jobs; then,
   after a spike in which a second build hits the shared cache, one cache service and 6 lanes under D2's pass conditions.
5. **Nothing comes from GitHub during a run**, libduckdb included: actions mirrored, tools from the build-inputs store,
   `paddock-ci` rebuilt as v3 with DuckDB 2.0.0. A run that would need the internet fails at the step that asks, with an
   `::error::` that names the input (ER-27).
6. **Cheap checks first, never in place of the full run.** One preflight job runs every static check. In fix rounds and
   branch runs the heavy Linux jobs need it; in a fix round it reruns only the static checks that failed. A run where any
   job was skipped by gating never counts as the run that lands (ER-33).
7. **Measured, always.** `./dev ci timings` prints the baseline table and the audit for a run range; every work package
   records a before and after block in `docs/benchmarks.md` (rule 48), in pairs.

## 6. Parallelism and cost (rule 51)

- **More parallelism:** Linux lanes 4 to 6 (D2), Playwright workers 1 to 3 per job (D6).
- **Cheaper:** failed-only fix rounds (D1), each check once and only on changed inputs (D12, D13, §3.3), generic tests on
  Linux only (D11), no downloads (D8, D9), merged static jobs (D3), package-level selection (D5), faster slow tests (D7).

## 7. Work packages and estimates (agent-days)

| WP | Content | Phase | Days | Depends on |
| --- | --- | --- | --- | --- |
| WP-0 | `./dev ci timings`: baseline table and audit for a run range (queue, lanes, phases, critical path, sccache, failures, per-check cost) | 0 | 1.5 | prebuilt `./dev` |
| WP-1 | Actions mirror, the remaining tools and libduckdb through the build-inputs store; `paddock-ci` v3 with DuckDB 2.0.0; a check that no workflow names a GitHub URL | 0 | 1.5 | `ci/build-inputs-wp1` |
| WP-2 | Fix-round mode: failed jobs and failed tests only, `base` = last tested commit, nextest and Playwright filters, run summary; `./dev ci rerun --failed` for suspected flakes, reported beside the first result | 0 | 2 | `--since-run`, Rust planner |
| WP-3 | Preflight job (7 tiny Linux jobs and the commit lint merged); `needs` on fix rounds and branch runs only; a gated run marked as not landable; C100 entries | 1 | 1.5 | ADR 0060 register |
| WP-4 | Playwright: the five app configs (Debrief `retries: CI ? 2 : 0`, already 2 workers; Pitwall `retries: CI ? 2 : 0`; Dyno, Spotter, Pitcrew `retries: 1` without `failOnFlakyTests`) moved onto the `@decryp7/paddock-e2e` preset (retries 0, or 1 with `failOnFlakyTests`); workers from the declaration; spec isolation; shard balance | 1 | 3 | ADR 0074 inventory |
| WP-5 | Spike (0.5 day): sccache's WebDAV backend against `bazel-remote` on `d-gtr9`, a second build hits; then the service, D2's measurements at 4 and 6 lanes, the lane change | 1 | 0.5 + 2 | ADR 0074 WP-B3, spike result, user host action |
| WP-6 | Debrief macOS: slow `debrief-store-duck` and `debrief-core` suites made faster (time-boxed) | 2 | 2 | — |
| WP-7 | Package-level selection (`rdeps`) per workspace on branch runs and fix rounds | 2 | 2 | Rust planner |
| WP-8 | Critical path first: job order experiment, kept only if a measured pair shows it | 2 | 0.5 | WP-0 |
| WP-9 | Local pre-push parity (ADR 0038 WP-2, WP-3) | 2 | as ADR 0038 | ADR 0038 |
| WP-10 | Necessity audit into the planner: `inputs` and `where` per check in `docs/gates.toml`, the §3.3 rules, release workflows read `ci.yml`'s verdict for the same commit (D13), WebKit to nightly and release (D12), the `Memory budget` step counted or moved, the always-red store benchmark fixed or quarantined | 1 | 4 | ADR 0060 register, Rust planner |
| WP-11 | Linux first: the OS-set generator (rules 1 to 4), `osAdapters` in `paddock.ci.json`, `os-specific` groups in the seven `nextest.toml`, the conformance check, doc tests and features-off Linux-only on branch runs, the Debrief macOS leg on the OS set | 1 | 3.5 | WP-0, Rust planner |

Total new work: about 27.5 agent-days. Phase 0: 5 (WP-0, 1, 2). Phase 1: 16 (WP-3, 4, 5, 10, 11). Phase 2: 6.5
(WP-6, 7, 8; WP-9 counted in ADR 0038).

## 8. What each phase saves (estimates, to be measured)

| Phase | Fix round wall | Full `ci.yml` wall (median) | Runner minutes |
| --- | --- | --- | --- |
| Today (measured) | a new full run: median 45 min | 45 min (no-queue path 20 min) | 17,597 in 5.5 days; 59 % in failed runs |
| 0: WP-0, 1, 2 | about 8 to 12 min (a Debrief macOS fix round 8 to 9 min) | 40 to 43 min (the 504 tails off the critical path) | fix rounds about 76 % cheaper |
| 1: WP-3, 4, 5, 10, 11 | under 10 min | about 22 min (Linux floor about 21 min; `debrief.yml` path about 15 min) | about 2,350 job-min (13 %) from the audit, about 200 more host job-min from Linux first |
| 2: WP-6, 7, 8, 9 | about 6 to 8 min | about 15 min critical path | fewer test builds on branch runs; fewer CI-only failures |

## 9. Decisions for the user

- **D1. Fix rounds** follow rule 50 (i) exactly: only the failed jobs and failed tests, nothing that passed; dispatched
  with `base` = the last tested commit; a same-commit rerun only to retry a suspected flake and never to turn red green;
  then one full run, which is the run that lands. *Recommended.*
- **D1a (optional extension).** In a fix round, also run whatever the fix's own diff reaches (the affected plan of
  `base..head`), on top of the failed set. It finds a regression the fix caused one round earlier, at the cost of a larger
  fix round. *Not recommended by default*: the full run before landing already catches it.
- **D2. Linux lanes 4 to 6**, after the WP-5 spike, with pass conditions: a CPU budget of at most 15 busy threads on the
  16-core host (lanes × build jobs plus test threads, from the ADR 0074 declaration); peak memory measured at 4 lanes
  first (not measured here: no host access) and projected for 6 within the host's 128 GB less a reserve; a flake rate at
  6 lanes no higher than at 4 over 20 paired runs. *Recommended.* Alternative: stay at 4 (Linux floor about 30 min instead
  of 21).
- **D3. Preflight gating** on fix rounds and branch runs only; in a fix round only the failed static checks rerun; a run
  with any job skipped by gating never lands; main, the train, tags and the nightly are never gated. *Recommended.*
- **D4. Compile cache backend:** a spike first: sccache's WebDAV backend against `bazel-remote` (Apache-2.0) on `d-gtr9`'s
  local disk, unverified until a second build shows hits. It is the shared-cache follow-up that ADR 0074's D2 names as
  option (b) "later": ADR 0074's accepted D2 (a) and WP-B3 give each Linux job its own cache on the mount, which removes
  the race but leaves up to four colder caches; the service would make them one warm cache again, which 6 lanes need.
  *Recommended: spike after WP-B3 lands; WP-B3 stays the fallback.* Alternatives: Valkey (BSD); not Redis 7.4 or later
  (its licences are outside the allow-list; not re-verified here).
- **D5. Test selection:** package level (`rdeps`), no per-test coverage map. *Recommended.*
- **D6. Playwright:** the five app configs onto the shared preset (ER-12), 3 workers per Linux job. *Recommended.*
- **D7. Debrief macOS tests:** fix the slow suites (WP-6) and use Linux first (D11); partition only if its run phase stays
  over 6 min. *Recommended.*
- **D8. No GitHub at run time:** actions mirrored into Gitea, tools and libduckdb from the build-inputs store.
  *Recommended.* It needs a mirror owner (the coordinator) and Renovate on the mirror pins.
- **D9. CI image:** rebuild `paddock-ci` as v3 with DuckDB 2.0.0 (the pending rebuild), its downloads through the mirror;
  no further baking (node_modules, Angular caches: measured small). *Recommended.*
- **D10. Order:** phase 0 now, alongside the in-flight planner and prebuilt `./dev`; phase 1 after the train lands; phase
  2 after Dyno's next tag. Each phase ends with a measured results block. *Recommended.*
- **D11. Linux first:** branch runs, fix rounds and train runs run generic tests on Linux only and the declared OS set on
  macOS and Windows; the nightly and the release gates run everything everywhere. It removes per-run coverage of generic
  tests on the host OS: a generic-looking host-only failure is found up to one nightly later, never after a release.
  *Recommended*, with the HTTP-client, `paddock-proc` and `paddock-chromium` entries in rule 2 from the start.
- **D12. WebKit e2e** once per main tree (nightly and release gates), not on every main push or in `dyno-release.yml`'s
  gates. It removes per-push WebKit coverage; a WebKit regression is found up to one nightly later. Saves about 664
  job-min in the window (plus 301 inside D13). *Recommended.*
- **D13. Release workflows dispatched for rule 54** build the installers only when `ci.yml` was green on the same commit
  (the verdict checks the commit, ER-33); otherwise they run their gates as today. Saves about 903 job-min in the window
  and removes nothing: the same gates ran on the same commit. *Recommended.*

## 10. Risks

- Parallel Playwright exposes shared state in specs; the switch waits for 20 runs per suite under load (checklist §1).
- A failed-only fix round misses a regression the fix caused elsewhere; the full run before landing catches it (rule 50
  (i)), so it costs time, not quality.
- Linux first lets a generic-looking host-only failure reach main; the nightly or the release gates find it (§4.4), and
  each slip adds its pattern to the declaration.
- Input-driven skips depend on complete `inputs` lists; a missing input skips a check that should have run. The nightly
  and release runs ignore the rules, and C100 checks that every check has a declared input list.
- More Linux lanes raise CPU, memory and I/O pressure on `d-gtr9`; D2's pass conditions decide, and the change is one
  number to roll back.
- The cache service is a new component on the host; if it is down, sccache must compile locally (slower, not red). This
  fallback is verified in the WP-5 spike with a positive control (ER-33).
- Estimates come from one busy week; the 2026-10 note showed load changes timings by up to 60 %. Every phase is judged on
  its own measured pair.

## 11. Consequences

- Rule 50 (i) gets its mechanism; ER-12 breaches in five Playwright configs are fixed on the way.
- ADR 0060's gate register gains `inputs` and `where` per check; the planner reads them.
- ADR 0025 §8 ("one job per Linux host") is superseded for Linux by ADR 0074 WP-B3 and, if D4's spike passes, the cache
  service.
- `docs/benchmarks.md` carries the CI baseline and each phase's results; `./dev ci timings` replaces ad hoc scripts.

## 12. Alternatives considered

- **Per-test coverage selection** (K): little to win, silent misses.
- **Package-level OS selection** ("a package with any `cfg(os)` and its reverse dependencies"): keeps 73 to 80 % of host
  test time, so it saves little; the per-binary rule of §4.2 is needed.
- **One cross-host remote cache**: no cross-OS hits, a network hop on every hit.
- **More Windows and macOS runners first**: their average busy lanes are 0.5 and 0.7; Linux is the bottleneck.
- **Fail-fast on every run**: on main and the train every failure must be reported in one run.

## Sources

- nextest archiving: https://nexte.st/docs/ci-features/archiving/ (read 2026-11-09: archives omit sources; same revision
  required; `--workspace-remap`).
- nextest partitioning: https://nexte.st/docs/ci-features/partitioning/ (read 2026-11-09: `slice`, `hash`, deprecated
  `count`; no timing-based mode described).
- nextest filtersets: https://nexte.st/docs/filtersets/reference/ (read 2026-11-09: `deps()`, `rdeps()`).
- sccache local storage: https://github.com/mozilla/sccache/blob/main/docs/Local.md (read 2026-11-09: "The local storage
  only supports a single sccache server at a time").
- sccache WebDAV: https://github.com/mozilla/sccache/blob/main/docs/Webdav.md (read 2026-11-09: lists Bazel Remote
  Caching as a backend; `SCCACHE_WEBDAV_ENDPOINT`). That it works with `bazel-remote` here is unverified (D4 spike).
- bazel-remote: https://github.com/buchgr/bazel-remote (read 2026-11-09: Apache-2.0; HTTP GET/PUT). Its current release
  and maintenance activity were not shown on the page: unverified.
- Gitea API of this instance: `https://dev.decryptology.net/swagger.v1.json`, version 28.3.0 (verified 2026-11-09:
  `POST /repos/{owner}/{repo}/actions/runs/{run}/rerun-failed-jobs` and `.../jobs/{job_id}/rerun` exist; not exercised).
- Playwright `failOnFlakyTests` in 1.64.1: from the independent review of revision 1; not re-checked here.
- Gitea's order of dispatch for queued jobs: not documented in what was read; unverified.
