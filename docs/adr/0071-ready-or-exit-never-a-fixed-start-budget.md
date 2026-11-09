# 71. Ready or exit, never a fixed start budget: tests wait for a started program or page until it answers or ends, warm a fresh binary once, and C71 checks every start wait

- Status: Proposed (2026-11-09). Not accepted; nothing in "Work packages" starts before the user accepts (ER-21 step 7).
- Date: 2026-11-09
- Related: ER-11 / rule 5 (failing test first), ER-12 (no raised timeouts or sleeps; flaky is red), ER-20 / rule 51
  (parallelism and cost), ER-21 / rule 40 (recurring bugs mean a missing design rule), ER-22 / rules 42-43 (share first,
  shared behaviour tested once), ER-27 (logs explain every failure), ER-28 (a cause is confirmed by a run without it),
  ER-33 / rule 71 (a check counts only when its probe has shown it can see the thing), rule 48 (recorded measurements),
  rule 49 (no turn counts in specs), rule 55 (one implementation per mechanism); ADR 0031 (patience, not wall clocks:
  product starts judged by `paddock_proc::StartJudge`, check C51), ADR 0041 (deterministic timing in tests, proposed rule
  60, check C71, `paddock-testkit::wait`), ADR 0042 (one owner per shared harness), ADR 0048 (test in the users' launch
  context).
- Evidence (read at `origin/main` `b51c72509` and on the named branches, 2026-11-09):
  - CI run 7775, job 32436 (`d7-mac-mini-2`): `pitwall-server::e2e` `a_crashing_engine_is_restarted_with_backoff_then_given_up_on_until_retry`
    and `stopping_serving_unloads_every_model_ends_the_engine_processes_and_resuming_serves_again` failed after 10.5-11.1 s
    with "the child engine did not become healthy". Root cause and reproduction in commit `81c3ac92f` on
    `fix/pitwall-e2e-engine-health` (with `9c42a60ef`, `7a73cdc59`; all three are in `train/2026-11-08-1`, not on `main`).
  - Job 32417: `debrief-plugin-e2e::it store_worker::a_plugin_the_app_did_not_load_reads_nothing_in_the_worker` (29.97 s)
    and `store_worker::an_installed_importer_reads_files_in_the_store_worker` (17.14 s) failed with `Engine("the store
    worker stopped (it crashed (the store worker did not connect within 15 s)); try again")`.
  - Job 32097 (`reference apps in WebKit (macOS)`): the first `shell-demo-webkit` test,
    `shell-demo-a11y.spec.ts:12` "every dialog window passes axe in both themes", failed in 5.4 s at
    `ShellDialogs.loaded()` (`ui/projects/paddock-e2e/src/shell.ts:284`, `expect(locator).toBeVisible()`, 5,000 ms, element
    not found); the next test of that project took 6.3 s and the following ones 0.3-0.7 s; 434 passed.
  - Dyno jobs 31671 and 31816 (2026-11-08): `dyno-bench::process disk_measures_the_mechanics_and_leaves_nothing_behind`
    TIMEOUT at 60 s with no output; cause not known (see the instance table).
  - The job logs and the stress scripts: `/Volumes/d7sst7/dev-targets/scratch/main-after-7761/logs/` and
    `/Volumes/d7sst7/dev-targets/scratch/pitwall-e2e-health/` (a stalled child sampled at `_dyld_start` with 32 KB RSS in
    `evidence-sample-stalled-child.txt`). These folders are scratch, not kept; the facts used here are quoted above.
  - Measurement: [benchmarks, 2026-11-09, the first start of a freshly linked program on macOS](../benchmarks.md#2026-11-09-the-first-start-of-a-freshly-linked-program-on-macos).
  - Code: `crates/paddock-proc/src/start.rs` (`StartJudge`, `StartWatch`, `Patience`, `ExitInfo`); `crates/paddock-testkit`
    (on `main` without `wait.rs`; `wait.rs` with `wait_until`, `wait_for`, `poll_for`, `HANG_GUARD` = 40 s is on
    `train/2026-11-08-1`, commit `f38846973`); `conformance/src/checks/C71-test-timing.mjs` and
    `conformance/baselines/C71.json` (606 sites) on `train/2026-11-08-1`, not on `main`; `conformance/src/checks/C51-start-waits.mjs`
    (on `main`); `apps/debrief/crates/debrief-ipc/src/process.rs` (`SpawnSpec::new` sets `connect_window` 15 s;
    `UNTIL_CONNECTED`).

## Context

On 2026-11-08 the macOS CI host went from one runner to three on one Mac. Within a day, tests in three apps and the
shared UI kit failed with the same shape: a test started a program (or opened a page) that had just been built, and gave
up after a fixed number of polls or a fixed deadline.

**The confirmed cause (Pitwall).** The first start of a freshly linked binary on macOS waits for the system's check of
new programs (`syspolicyd`; the child sits in `_dyld_start` with no CPU use while it waits). The checks of concurrent
first starts queue behind each other, so on a host where three jobs each start fresh binaries, a first start that takes
a few hundred milliseconds alone can take tens of seconds. Commit `81c3ac92f` records: all 11 `pitwall-cli` tests in job
32436 (each the first start of the fresh `pitwall` binary) took 16.8 s and ended within 50 ms of each other (1.4 s in run
7774); under a stress run that re-created `pitwall-server` as a new file before each run, with CPU burners, the old test
code failed 2/10, 6/10 and 3/10 runs with the CI message, 10/10 green without re-creating the file, and the fix 20/20
green with first starts up to 27.7 s. Ruled out there: a port collision, a leftover listener, CPU starvation alone (an
already-checked binary started in 0.06 s under the same load) and work before the fake engine starts. A synthetic
measurement on the same host today (benchmarks, 2026-11-09) shows the effect unloaded: a new program's first start took
258-2,864 ms (median 304 ms) against 2.4-6.6 ms for its second start, and six concurrent first starts ended together in
each round. Secondary sources describe the same check (an XProtect scan plus a notarization-ticket lookup on the first
run of any new executable): <https://eclecticlight.co/2025/05/04/last-week-on-my-mac-checking-code-can-take-longer-now/>,
<https://developer.apple.com/forums/thread/666452> (read 2026-11-09, not verified beyond what is measured above).

**The same shape elsewhere.** The Debrief store-worker failure ran on the same host the same evening and hit the product
default connect window (15 s) of a fresh worker binary; the WebKit failure was the first page of a cold browser project
under a 5 s assertion. Neither cause is confirmed by a run without it (ER-28); both are failures of a fixed budget that
starts counting before the thing waited for could possibly be ready. Earlier instances of the same class: CI 7096
(2026-10-29, the reason ADR 0031 and `debrief_ipc::process::UNTIL_CONNECTED` exist: "a fixed window then stopped a child
that was still starting"), the Debrief chromium-driver fixed 20 s start (CI 7213, 7250; ADR 0041's table), and Spotter's
self-test count set by a real child (ADR 0041's table).

**Why ADR 0041 and C71 did not stop it.** ADR 0041 forbids counts of polls that host speed sets and allows real time
only as a hang guard of at least 10 s, but C71's first slice flags only `sleep` calls (and a few TypeScript shapes), not
the count, not a deadline, and not a start bound passed as configuration. Pitwall's loop slept between polls, so its
`sleep` line is in C71's baseline while the 200-poll count that decided the verdict was never looked at. The store
worker's 15 s is not in the test at all (a product default), and C51, which watches product start bounds, misses it:
it matches names with START, SPAWN, LAUNCH or READY, not `connect_window`. And a "hang guard" of 10-20 s is shorter than
a first start measured at 27.7 s, so even a correctly waived bound decided verdicts.

## The pattern

> **A start wait fails whenever a test gives a program, a worker or a page a fixed number of polls or a fixed deadline
> to become ready, and the start takes longer than that: a first-start check, a cold browser, a starved host.**

**Invariant.** A test waits for anything it started (a process, a worker, a browser page or window) until it is ready
or has ended, and reports an end with its reason at once. Elapsed time never decides that verdict. Real time appears only
as a hang guard that is longer than any correct start, never shorter than `HANG_GUARD` (40 s; scaled by
`PADDOCK_TEST_TIME_SCALE` once ADR 0041 §3's scaling lands), and whose expiry is reported as a hang, not as a slow start.

## Instances (grep of `origin/main` `b51c72509`, 2026-11-09)

Method: every Rust test file that starts a process (`CARGO_BIN_EXE_`, `.spawn()`, `Command::new`, `launch(`,
`StoreWorker::new`, `Builtins::load`, the plugin-test harness: 75 files) was searched for fixed-count loops with an early
exit, `Instant::now() + <30 s or less>`, `elapsed() < <30 s or less>` and start bounds set in test configuration
(`connect_window`, `hello_timeout`, `load_timeout`, `ready:`, `start:`); each hit was read in context. TypeScript: every
e2e spec and the `@decryp7/paddock-e2e` sources were searched for explicit `timeout:` values (190 lines), readiness
markers and counted attempt loops. Status: **broken** (failed in CI), **at risk** (same shape, not yet seen failing),
**not a start** (a fixed-count poll of something else, also flagged by the check's R1), **fine**.

| # | Where | Instance | Status | Migration |
|---|---|---|---|---|
| 1 | `apps/pitwall/crates/pitwall-server/tests/common/mod.rs:476` | `ChildEngine::start`: `for _ in 0..200` `/health` polls 50 ms apart (10 s), exit-blind | broken (job 32436, cause confirmed) | Land `fix/pitwall-e2e-engine-health` as is (ready-or-exit, `--version` warm-up, `StartOutcome`, `--ready-when`); then move its loop and `runnable_server` warm-up onto the kit (WP-4a), keeping its double-specific `StartOutcome` recording |
| 2 | `apps/debrief/crates/debrief-store-client/src/client.rs:275` via `debrief-plugin-e2e/tests/it/store_worker.rs:48` | `StoreWorker::new(WorkerConfig::new(WORKER))` takes `SpawnSpec::new`'s product default `connect_window` 15 s (`debrief-ipc/src/process.rs:96`); the test cannot set it (`WorkerConfig` has no field) | broken (job 32417; cause probable, not confirmed) | `WorkerConfig` gains `connect_window`; a `WorkerConfig::for_tests` sets `UNTIL_CONNECTED`; the tests call `warm_first_start(WORKER)`. Failing test first: `assert_eq!(WorkerConfig::for_tests(..).connect_window, UNTIL_CONNECTED)` in the e2e crate (the form Debrief's `lib.rs:1116` already uses for plugins). Same change for every other `WorkerConfig::new` in tests (`debrief-store-worker/tests/it/*.rs`) |
| 3 | `ui/projects/paddock-e2e/src/shell.ts:284` | `ShellDialogs.loaded()`: `expect(this.shell).toBeVisible()` under the 5 s expect timeout, the first wait on a freshly opened window | broken (job 32097, first test of the WebKit project; cause not confirmed) | `loaded()` first awaits `readyWindow(page)` (kit, §2), then its two assertions; the project gets the `warmApp` fixture. Failing test first: a kit unit test runs `loaded()` against a recording locator and `expect` seam and asserts that its first wait has no time limit of its own (`timeout: 0`, the test timeout is the guard); red today, where the first wait inherits the 5 s expect timeout. A spec that reproduces the cold start needs real time and is not written (ADR 0041) |
| 4 | `apps/debrief/crates/debrief-plugin-test/src/lib.rs:455` | the plugin test harness sets `connect_window` 15 s for every plugin under test (the five example plugins and Debrief's plugins) | at risk | `UNTIL_CONNECTED` and `warm_first_start(exe)` before the first launch of each program |
| 5 | `apps/debrief/crates/debrief-ipc/tests/it/launch.rs:19`, `child_log.rs:14` | `connect_window` 10 s for the fresh `debrief-ipc-fake-child`; `launch.rs:127` (`--exit-now`) relies on it: a slow start reads as "did not connect" instead of "exited while starting" | at risk | `UNTIL_CONNECTED` plus `warm_first_start` in `spec()`; `launch.rs:120` and `:133` (`--no-connect`, 300 and 500 ms) test the window's own expiry with a child scripted never to connect: they keep it with `// conformance: expiry-under-test — <why>` |
| 6 | `apps/debrief/crates/debrief-ai/tests/it/worker.rs:36-37` | `hello_timeout` and `load_timeout` 10 s for the fresh fake worker | at risk | test config without a start bound (`Patience::until_condition()`, ADR 0031 §5) or `HANG_GUARD`-scaled, plus warm-up; `stall_timeout` 600 ms and `cancel_grace` are behaviour under test (ADR 0041's domain), not starts |
| 7 | `apps/debrief/crates/debrief-ai/tests/it/worker_memory.rs:35` | a 10 s deadline from `launch()` until the memory watcher's first event | at risk | `wait_ready_or_exit` on the launched worker with the event as the probe |
| 8 | `apps/debrief/crates/debrief-ai-worker/tests/it/server_backend.rs:65` | `Timeouts { ready: 20 s }` for a fake `llama-server` binary built by the test | at risk | no fixed `ready` in the test config (the product's own judge, ADR 0031), warm-up of the fake; `:362` (`ready: 1 s`, `never_ready` script) is expiry under test and keeps it with the waiver |
| 9 | `apps/debrief/crates/debrief-plugins/tests/it/evtx_memory.rs:213`, `perf_plugins.rs:25` | wait 20 s / 30 s for every built-in plugin to have a pid, then measure anyway | at risk (silently measures a half-started set) | `wait_for` each pid through the kit; a missing pid fails with which plugin |
| 10 | `apps/dyno/crates/dyno-plugin-host/tests/processes.rs:28` | `Timeouts { start: 20 s }` for fake plugin processes | at risk | no fixed start bound in the test config; warm-up |
| 11 | `apps/dyno/crates/dyno-agent/tests/term.rs:65`, `:100`, `:170` | readiness on the discovery file under the 40 s guard, exit-blind: an agent that dies at start reads as "never became ready" after 40 s | at risk (wrong message, ER-27) | `wait_ready_or_exit(child, probe: discovery file)` |
| 12 | `apps/dyno/crates/dyno-bench/tests/process.rs:30` (`bin()`) | starts the fresh `dyno-bench` with `.output()`; only nextest's 60 s kill bounds it; jobs 31671 and 31816 timed out in the disk test | candidate, cause unknown | not migrated on a guess: ER-28 first (does the TIMEOUT follow a first start, the disk work, or a hang?); `warm_first_start` in `bin()` is cheap and is added with the first logging step so the next TIMEOUT shows the start's duration. Job 32410's failure of the same test ("the drive has 266 GB free; the disk test needs 401 GB" for a 16 MiB file) is a different bug, not this class |
| 13 | `apps/debrief/crates/chromium-driver/tests/it/real_browser.rs:65` | `for _ in 0..40` evaluations of the page's list until it has 3 items, no pace: a count set by host speed | at risk | `expect`-style `wait_for` on the evaluation through the kit |
| 14 | 20 sites in 12 files: `ui/projects/paddock-e2e/src/dialog-fits.ts` (2), `size-to-content.ts` (3); Dyno `e2e/layout.spec.ts` (2), `menu-bar-items-fit.spec.ts` (3), `screenshots-dialogs.spec.ts`, `screenshots-docs-dialogs.spec.ts`, `screenshots-helper.spec.ts` (3), `screenshots-network.spec.ts`, `screenshots-shell.spec.ts`, `screenshots-wake.spec.ts`, `screenshots-wol.spec.ts`, `widget-drag-ghost.spec.ts` (1 each) | `page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 })`: a fixed 15 s for a freshly opened window | at risk (the first window of a cold project is the one that is slow) | `await readyWindow(page)` (kit, §2) |
| 15 | `ui/projects/paddock-e2e/src/svg.ts:41` | `pointInShape`: 20 unpaced attempts until the shape is drawn | at risk | `expect.poll` on the measurement (the test timeout is the guard) |
| 16 | `apps/pitwall/crates/pitwall-server/tests/e2e.rs:599` (900 × 100 ms), `:692` (100 × 100 ms), `:808` (100 × 50 ms, waits for the engine pid to end) | fixed-count polls of a product condition | not a start (R1 flags them) | the kit's `wait_until` / `wait_for` (ADR 0041 WP-6) |
| 17 | `apps/dyno/crates/dyno-agent/tests/common/mod.rs:25` | `wait_for_alarm`: 100 × 100 ms, in-process agent | not a start (R1 flags it; still on `train/2026-11-08-1`) | the kit's `wait_for` |
| - | Pitcrew `supervision.rs` `for _ in 0..N { pump(...) }` (6 loops), Pitwall `tests/common/mod.rs` HTTP client 20 s per request (after the warm-up), Playwright `webServer.timeout` 180 s, `waitForFunction` without a timeout, `paddock-chromium`/Spotter `real_browser.rs` (a pinned Chrome for Testing, started through the product's `StartJudge`) | fake-clock steps, a per-request bound after readiness, generous server guards, test-timeout waits | fine | none (R1 skips a loop without an early exit; the per-request bound is not a start bound) |

Totals: 3 broken (2 Rust, 1 TypeScript); 11 rows at risk with 35 sites (14 Rust, 21 TypeScript), besides the four
expiry-under-test windows that keep a waiver (`launch.rs:120`, `:133`, `server_backend.rs:362` and the AI worker's
behaviour timeouts are not starts); 1 candidate; 5 fixed-count polls that are not starts (rows 16-17). Exit and shutdown waits under 40 s
(`debrief-ai worker.rs:106` `wait_gone`, `server_backend.rs:288`, `launch.rs:66`, Spotter's leftover checks) are ADR
0041's domain and stay in its ratchet; R3 below does not look at them.

## Decision

### 1. One mechanism in Rust: `paddock-testkit::start`

A new module beside `wait.rs` (ADR 0041), built on the product's start judge so there is one implementation of "ready
or exit" in the family (rule 55, ER-22):

- **`warm_first_start(exe: &Path, args: &[&str]) -> Duration`**: runs `exe args` (`--version` by convention) once to its
  end, under its own hang guard, and prints `the first start of <exe> took <s> s` to the test's stderr every time it runs
  (ER-27). It is memoised per process by the file's identity (canonical path, length, modification time; on Unix also
  device and inode), so repeated calls in one test binary cost one start. A non-zero exit fails with the program's
  stdout and stderr tails: a program that cannot print its version is broken, not slow. Its purpose is to move the
  first-start check before any step that still has a time bound the test cannot remove (a product's HTTP client, a
  harness request). It is never what makes a test correct: the wait below is.
- **`wait_ready_or_exit(child, what, probe) -> StartOutcome<T>`**: polls `probe` (returns `Some(T)` when ready) and the
  child's liveness, with `wait.rs`'s backoff (1 ms doubling to 50 ms), and returns `StartOutcome::Ready(T)` or
  `StartOutcome::Exited(ExitInfo)` (status and stderr tail, `paddock_proc::ExitInfo`) as soon as either happens. It prints
  "<what> (pid N) is still starting after N s" every 10 s. On the hang guard (§3) it panics with the pid, whether the
  process is alive, a zombie or paused (`paddock_proc::run_state`), the last probe result and the stderr tail, worded as
  a hang ("never became ready or ended"), never as a slow start. Implemented as a `paddock_proc::StartWatch` with a
  test `Patience` (spin and idle equal to the guard, `note_after` 10 s, `Cpu::NotRead`): the product's judge and the test's
  wait share the loop, the liveness reading and the exit report.
- **`try_wait_ready_or_exit`**: the same, returning `Err(HangGuardExpired)` instead of panicking, for test doubles that
  implement a product port and must return an error into the code under test (Pitwall's `ChildEngine`).
- **`Liveness` for `std::process::Child`**, and for `tokio::process::Child` behind a `tokio` feature with an async
  `wait_ready_or_exit_async` (Pitwall's double is async).
- Tests of the module (rule 43: shared behaviour tested once here; apps test only their wiring): a scripted fake
  program (a test binary of the crate, `--ready-when <file>`, `--exit <code>`, `--never-ready`) proves (a) a child that
  becomes ready only after the test has seen more probes than any budget this ADR removes (200, as in `9c42a60ef`) is
  waited for, with no real duration involved; (b) a child that exits before it is ready is reported at once with its
  stderr; (c) the mutant "liveness always says running" fails the exit test within a few polls (as `7a73cdc59` did, by
  checking that the first child has ended and the wait polled it more than 5 more times); (d) `warm_first_start` runs the
  program once per identity and again after the file is replaced. Line coverage at least 99 % (the crates gate).

`paddock-testkit` gains a dependency on `paddock-proc` (pure core, already a dependency of six app crates). No `cfg(test)`
branches in product code.

### 2. One mechanism in TypeScript: `@decryp7/paddock-e2e`

- **`readyWindow(page, marker = 'html[data-dialog-ready]')`**: waits for the window's ready marker with no explicit
  timeout (`locator.waitFor({ timeout: 0 })`, so the test's own timeout is the guard), and on failure attaches the
  page's console errors and `pageerror`s it collected while waiting. `ShellDialogs.loaded()` calls it first.
- **`warmApp`**: a worker-scoped automatic fixture, declared once in the kit's `test` and enabled per project through
  `use: { warm: { path, marker } }`. Before the first test of a worker it opens the project's base URL, waits for the
  marker under the worker-fixture timeout (the guard), records the duration as a test annotation and in the log, and
  closes the page. A cold browser and a cold bundle are paid there, not inside a test's 5 s assertion.
- `pointInShape` polls with `expect.poll` instead of 20 attempts.
- Unit tests in the kit (`*.test.ts`) and a selftest spec with a page that sets its ready marker only when the spec
  releases it (the TypeScript twin of `--ready-when`).

### 3. How a hang is still bounded

- The guard is `paddock_testkit::wait::HANG_GUARD` (40 s), counted from the start of the wait, times
  `PADDOCK_TEST_TIME_SCALE` once ADR 0041's scaling lands (its §3 and WP-5; `wait.rs` on `train/2026-11-08-1` has the
  constant without the scale). 40 s is above the longest first start measured (27.7 s under CPU burners, `81c3ac92f`)
  and under CI's nextest kill (`[profile.ci]` `slow-timeout` 30 s with `terminate-after = 2`, so 60 s, in Dyno and
  Pitwall; 3, so 90 s, in Debrief), so a real hang ends with the kit's message, not the runner's.
- The guard never decides a verdict: it only ends a wait that would never end. Ready and exit are the only outcomes a
  test asserts. A test that needs to assert a start's *speed* is a benchmark (rule 16), not a test.
- The warm-up has its own guard (the same constant). A warm-up near its guard followed by a readiness wait near its
  guard can together exceed nextest's 60 s kill; nextest then ends the test with TIMEOUT, and the warm-up's printed
  duration in the test's output says why (ER-27). Accepted: it needs a first start over 20 s and a hang after it.
- TypeScript: the guard is the test's timeout (60-120 s per config) or, in `warmApp`, the worker-fixture timeout.
- A test that examines a product window's own expiry (a child scripted never to connect) keeps a short window; it is
  not a start wait, because no slower start can change its verdict. It carries `// conformance: expiry-under-test —
  <the scripted mode>`.

### 4. The check: C71 gains start-wait kinds, C51 gains connect windows

C71 (ADR 0041 §4, on `train/2026-11-08-1`) scans every test file in `crates/`, `apps/*`, `ui/`, `tools/` and
`scripts/`. It gains these kinds, each an **error**, with the same per-call waivers and the same skip of
`paddock-testkit` and the kit sources that own the mechanism:

| Kind | Flags, in test code | Passes when |
|---|---|---|
| **R1 fixed-count poll** (Rust) | a `for _ in 0..N` (or a counted `while`) loop whose body has an early `break`, `return` or `?` on a condition: the count is the budget, whatever pace it has | the loop has no early exit (repeating work N times); or `// conformance: hang-guard — <why>, <bound>` naming at least 40 s of total budget |
| **R2 fixed start bound in test configuration** (Rust) | an assignment or field initialiser whose name contains `connect`, `hello`, `handshake`, `start`, `ready`, `spawn`, `launch` or `load` and ends in `window`, `timeout`, `deadline`, `limit` or `budget` (or is exactly `ready`/`start`), with a `Duration` literal under 40 s | the value is `UNTIL_CONNECTED`, `HANG_GUARD` or a `Patience` from the kit; or `// conformance: expiry-under-test — <scripted mode>` |
| **R3 deadline after a start** (Rust) | `Instant::now() + d`, `elapsed() < d`, `recv_timeout(d)`, `wait_timeout(d)` with `d` under 40 s, in a function that also starts a process (`.spawn()`, `Command::new(`, `CARGO_BIN_EXE_`, `launch(`, `::start(`) | the wait goes through `wait_ready_or_exit`/`warm_first_start`; or a hang-guard waiver naming at least 40 s; exit and shutdown waits after a stop request (`stop()`, `term(`, `kill`) on the lines before are skipped |
| **T1 readiness with a fixed timeout** (TypeScript) | `waitFor`, `toBeVisible`, `toHaveAttribute`, `toBeAttached` with an explicit `timeout` on a locator naming a ready marker (`data-*ready`, `pd-dialog-shell`, `loaded`) | it calls `readyWindow`/`ShellDialogs.loaded()`, or runs under `warmApp` with no explicit timeout |
| **T2 counted attempts** (TypeScript) | `for (let i = 0; i < N; …)` with an early `return`/`break` on a measurement or locator result in e2e and kit code | `expect.poll`/`toPass` instead |

The hang-guard waiver keeps ADR 0041's 10 s floor for C71's other kinds; for R1-R3 the floor is 40 s, because a bound
between 10 and 40 s is shorter than a measured first start and so decides verdicts.

**C51** (product code, report-only, ADR 0031 §8) widens its name list from START, SPAWN, LAUNCH, READY to also CONNECT,
HELLO and HANDSHAKE, so `SpawnSpec::connect_window`, the AI worker's `hello_timeout` and the like are listed for ADR
0031's migration. That is where the product's own 15 s store-worker window belongs (a first start after an update meets
the same check on a user's Mac; not measured here), not in this ADR.

**No baseline growth.** The new kinds land in the same change as the migration of every row of the table (WP-3 with
WP-4), so `conformance/baselines/C71.json` gains no entry (its shrink-only rule stands) and the new kinds are errors from
the first day.

**Positive controls (ER-33, rule 71).** C71's test file gains, per kind, fixtures that must be flagged and their fixed
twins that must pass, and two controls taken from history: the check run over
`git show 81c3ac92f^:apps/pitwall/crates/pitwall-server/tests/common/mod.rs` must report R1 at the `for _ in 0..200`
line, and over `7a73cdc59`'s version must not; `dialog-fits.ts` at `b51c72509` must report T1 twice. In the repository
run, C71 prints how many start functions (R3's scope) and readiness locators (T1's scope) it scanned; when either count is
zero the run reports `check broken` (an error), so a scanner that stops seeing starts cannot pass silently.

### 5. Rule text

No new rule. Proposed rule 60 (ADR 0041, not yet on `main`) gains one sentence when it lands: "A test waits for a
process, worker or page it started until it is ready or has ended, and reports an end at once; a fresh binary is warmed
once (`warm_first_start`) before any step with a bound the test cannot remove; a hang guard on a start is at least
`HANG_GUARD`." A lesson records the incident (ER-6).

### 6. The nextest setup-script warm-up: not used

nextest can run a setup script before the tests a filter selects and pass variables to them, but the feature is marked
"Experimental: This feature is not yet stable" and must be enabled with `experimental = ["setup-scripts"]`
(<https://nexte.st/docs/configuration/setup-scripts/>, read 2026-11-09; tracking issue #978). Decided against, for now:
- It would be a second, CI-only home of the warm-up (rule 55, ADR 0042): `cargo test`, Playwright's backends, VM runs
  and the local `./dev` gates would not get it, and the in-test warm-up is needed there anyway.
- It does not remove the class: the guarantee is ready-or-exit; the warm-up only moves a delay. A setup script that
  warms everything would hide a missing ready-or-exit wait until the next cold start elsewhere.
- The script has to find every fresh binary itself (`CARGO_BIN_EXE_*` is set for tests, not for scripts), any non-zero
  exit or timeout of the script ends the whole run, and an experimental configuration key can change under a nextest
  update (Renovate updates the pinned version).
- What it would save is small (see "Parallelism and cost"). Revisit when the feature is stable, as an optimisation that
  calls the kit's `warm_first_start`, never as the mechanism.

## Parallelism and cost (ER-20, rule 51)

- **Where the time goes.** The first-start check is paid once per new binary per host, whoever pays it; the warm-up does
  not add it, it moves it before the test's bounded steps. Its own cost is one extra start of an already-checked binary:
  2.4-6.6 ms for a small program (benchmarks, 2026-11-09), 6-60 ms for `pitwall-server` (`--version` and fake-engine
  starts, commit `81c3ac92f`). nextest runs each test in its own process, so the per-process memo pays once per test that
  calls it: for Pitwall's e2e binary (24 tests) under 1.5 s of process starts in total, spread over the run.
- **More parallelism?** No gain: concurrent first starts finish together (six concurrent first starts ended within
  3-74 ms of each other in each round, benchmarks 2026-11-09), so warming several binaries in parallel does not shorten
  the check, and warming them early in parallel with other tests would only add load to a shared host. The warm-up runs
  on the test's own thread, at the test's priority (rule 52: a test is waited-for work).
- **Cheaper?** (a) Skip the warm-up across processes with a marker file per binary identity: saves one start per test,
  but adds a cached identity (rule 53's extension) for milliseconds; not done. (b) A nextest setup script: saves the same
  milliseconds once per run; not now (§6). (c) Exempt the runner from the check (the "Developer Tools" exemption in
  Privacy & Security, secondary sources above): removes the delay on that host, but tests would then no longer start
  programs the way a user's Mac does (ADR 0048), and the class remains on Windows (Defender scans a just-started
  program: `crates/paddock-setup/tests/vm/windows.rs:77`), on Linux under load and in browsers; a decision for the user (D5).
  (d) Fewer fresh binaries per job (test-binary consolidation, lessons: 239 → 99) already reduces the number of checks.
- **Polling cost.** `wait_ready_or_exit` polls with a 1-50 ms backoff: about 20 cheap probes a second at most per waiting
  test (a `try_wait` and one connect or file read), and reads no CPU time (`Cpu::NotRead`). `readyWindow` uses
  Playwright's own locator polling.
- **What it saves.** Each red macOS run of this class cost a re-run of the app's macOS job (its nextest step alone ran
  478 s in job 32417) plus a diagnosis; there were three such failures on the evening of 2026-11-08.

## Consequences

- A test cannot give a started program, worker or page a count or a deadline any more; the check fails the change that
  tries. A slow start makes a test slower, never red. A start that ends is reported at once with its reason (ER-27).
- The family has one ready-or-exit loop for product and tests (`paddock_proc::StartWatch`), one warm-up and one window
  wait; Pitwall's double, Debrief's harnesses and Dyno's agent tests lose their copies.
- Tests that hit a product start window (Debrief's store worker, plugins, AI worker) set it to wait until ready or exit,
  as the Debrief app tests already do for plugins; the product's own fixed windows are listed by the widened C51 for ADR
  0031's migration, not changed here.
- C71's R1 also flags fixed-count polls that are not starts (rows 16-17); they are migrated in the same change through
  the kit's existing `wait_until`/`wait_for`, so no baseline grows.
- A real hang now takes up to 40 s (scaled) to report instead of 5-20 s. That is the price of never failing a slow start.

## Alternatives considered

- **Raise the budgets** (200 polls to 900, 15 s to 60 s). Forbidden by ER-12, and every number is wrong on the next
  slower host; Pitwall's `tick_until` already says "Generous: loaded CI runners are slow" at 900 polls.
- **Warm-up only.** Fixes the first-start check on macOS and nothing else (a cold browser, Windows Defender, a starved
  Linux container); a test that still counts polls stays wrong. Kept as the complement, not the fix.
- **Retry the failed test.** One retry with flaky-as-failure keeps the run red (ER-12); retries that turn it green hide
  the class.
- **A nextest setup-script warm-up.** Experimental; CI-only; §6.
- **Exempt the CI host from the first-start check.** Hides a real-user behaviour from the tests (ADR 0048) and leaves the
  other causes; D5.
- **A per-app helper in each app.** That is how it grew (Pitwall's `CHILD_HANG_GUARD`, Dyno's `term.rs` `HANG_GUARD`,
  Debrief's `wait_for` copies with 10-20 s); rule 55 and ER-22 say one shared implementation.
- **Use `paddock_proc::StartWatch` directly in tests without a kit wrapper.** Possible, but every test would build a
  probe, a pace and a patience, and choose the guard; the wrapper fixes the guard and the messages once.

## Work packages

Estimates in agent-days, from this ADR's instance table (planning estimates, not measurements).

| WP | What | Owner (role) | Days |
|---|---|---|---|
| WP-0 | Record `81c3ac92f`'s stress numbers (first starts up to 27.7 s, 2/10-6/10 red) as a rule-48 block in `apps/pitwall/docs/performance.md` and link it here | implementer | 0.25 |
| WP-1 | `paddock-testkit::start`: `warm_first_start`, `wait_ready_or_exit` (sync, `tokio` feature), `try_…`, `StartOutcome`, the test `Patience`, the scripted fake program, tests (a)-(d), 99 % coverage, CHANGELOG | implementer, reviewed by reviewer | 2 |
| WP-2 | `@decryp7/paddock-e2e`: `readyWindow`, `warmApp` fixture, `ShellDialogs.loaded()`, `pointInShape`; unit tests and the selftest page with a released marker | implementer, reviewed | 1.5 |
| WP-3 | C71 kinds R1-R3, T1-T2 with fixtures, the two history controls and the scan counts (`check broken` at zero); C51's name list; conformance README | implementer, reviewed | 1.5 |
| WP-4a | Pitwall: land `fix/pitwall-e2e-engine-health` (already reviewed, in `train/2026-11-08-1`); then `ChildEngine` and `runnable_server` on the kit; rows 16 | implementer | 0.75 |
| WP-4b | Debrief: `WorkerConfig::connect_window` + `for_tests`, the plugin-test harness, `debrief-ipc` tests, AI worker and AI-worker tests, plugin pid waits, chromium-driver count (rows 2, 4-9, 13); failing test first per row (ER-11) | implementer | 2 |
| WP-4c | Dyno: `term.rs` on `wait_ready_or_exit` (row 11), `wait_for_alarm` (row 17), `processes.rs` (row 10); `dyno-bench` `bin()` warm-up with its printed duration and the ER-28 cause list for the 60 s TIMEOUT (row 12) | implementer; researcher for row 12 | 1 |
| WP-4d | TypeScript: the 20 `data-dialog-ready` sites and the WebKit project's `warmApp` (rows 3, 14, 15) | implementer | 0.5 |
| WP-5 | Lesson, the sentence for proposed rule 60, ADR index line; the Wiki notes for the coordinator | implementer | 0.25 |
| **Total** | | | **9.75** |

**Order and release windows (ER-1, ADR 0041's rollout).** WP-0 and WP-4a's landing of the existing fix go first and are
independent. WP-1 and WP-2 change `crates/` and `ui/`, which every app's release builds from: they land after
`train/2026-11-08-1` (it carries C71 and `wait.rs`, which this builds on) is on `main`, outside a release window, with at
most two tooling agents while a release is in flight. WP-3 lands together with WP-4b-d (the error kinds and their
migration in one change, so nothing is baselined). WP-4 touches only each app's tests and runs in that app's batch.

**Rollback.** The new C71 kinds can be switched to warnings by one line; the kit functions are additive; each app's
migration is its own commit and reverts alone.

## Decisions for the user

- **D1. Build the test wait on the product's start judge** (`paddock-testkit` depends on `paddock-proc`; one ready-or-exit
  loop in the family). *Recommended: yes.* Alternative: a small separate loop in the kit (no new dependency, but a second
  implementation of the same mechanism under rule 55).
- **D2. The nextest setup-script warm-up.** *Recommended: no, not while it is experimental; revisit when stable, as an
  optimisation calling `warm_first_start`* (§6).
- **D3. The new C71 kinds are errors from the day they land, with every listed site migrated in the same change** (no
  baseline growth). *Recommended: yes.* Alternative: approve a one-time growth of `C71.json` by about 35 entries and
  migrate later (faster to land, but leaves at-risk sites red-capable on the shared host).
- **D4. Widen C51 to CONNECT, HELLO and HANDSHAKE windows** (report-only, product code), so Debrief's 15 s store-worker
  and plugin windows join ADR 0031's migration. *Recommended: yes*; the product change itself stays with ADR 0031 and
  Debrief 1.2's plan.
- **D5. Exempt the macOS CI runners from the first-start check** (Developer Tools in Privacy & Security). *Recommended:
  no*: the tests should start programs as a user's Mac does (ADR 0048), and it would not cover Windows, Linux or browsers.
  If CI time matters more, measure it on one runner first (rule 48) and keep this ADR's mechanism regardless.
- **D6. Order.** *Recommended:* land the Pitwall fix now (WP-0, WP-4a's first step); everything else after
  `train/2026-11-08-1` reaches `main`, outside a release window.
