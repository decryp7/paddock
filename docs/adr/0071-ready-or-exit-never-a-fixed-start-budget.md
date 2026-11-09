# 71. Ready or exit, never a fixed start budget: tests wait for a started program or page until it answers or ends, warm a fresh binary once, and C71 checks every start wait

- Status: Accepted 2026-11-09 by the user: D1–D6 as recommended. Revision 2 after the independent review of revision 1
  (CHANGES) and its re-review (ACCEPTABLE, 2026-11-09), with the re-review's P2s applied.
- Date: 2026-11-09
- Related: ER-11 / rule 5 (failing test first), ER-12 (no raised timeouts or sleeps; flaky is red), ER-20 / rule 51
  (parallelism and cost), ER-21 / rule 40 (recurring bugs mean a missing design rule), ER-22 / rules 42-43 (share first,
  shared behaviour tested once), ER-27 (logs explain every failure), ER-28 (a cause is confirmed by a run without it),
  ER-33 / rule 71 (a check counts only when its probe has shown it can see the thing), rule 48 (recorded measurements),
  rule 49 (no turn counts in specs), rule 55 (one implementation per mechanism); ADR 0031 (patience, not wall clocks:
  product starts judged by `paddock_proc::StartJudge`, check C51), ADR 0041 (deterministic timing in tests, proposed rule
  60, check C71, `paddock-testkit::wait`), ADR 0042 (one owner per shared harness), ADR 0048 (test in the users' launch
  context), ADR 0072 (`docs/adr-0072-ordered-close`: every connection end says why; keeps the IPC handshake bound).
- Measurements (rule 48; every number below comes from one of these):
  - [M1] [benchmarks, 2026-11-09, the first start of a freshly linked program on macOS](../benchmarks.md#2026-11-09-the-first-start-of-a-freshly-linked-program-on-macos-docsadr-0071-ready-or-exit)
  - [M2] [benchmarks, 2026-11-09, test durations around the first-start failures of 2026-11-08](../benchmarks.md#2026-11-09-test-durations-around-the-first-start-failures-of-2026-11-08-ci-job-logs)
  - [M3] [Pitwall performance, 2026-11-09, first starts of a freshly copied `pitwall-server`](../../apps/pitwall/docs/performance.md#2026-11-09-first-starts-of-a-freshly-copied-pitwall-server-and-the-child-engine-tests-fixpitwall-e2e-engine-health)
- Evidence (read at `origin/main` `b51c72509` and on the named branches, 2026-11-09):
  - Pitwall: CI run 7775, job 32436, two `pitwall-server::e2e` child-engine tests failed with "the child engine did not
    become healthy" [M2]. Diagnosis and fix: `81c3ac92f`, `9c42a60ef`, `7a73cdc59` on `fix/pitwall-e2e-engine-health`, all
    three in `train/2026-11-08-1`, not on `main`.
  - Debrief: job 32417, `debrief-plugin-e2e::it store_worker::a_plugin_the_app_did_not_load_reads_nothing_in_the_worker`
    and `store_worker::an_installed_importer_reads_files_in_the_store_worker` failed with `Engine("the store worker
    stopped (it crashed (the store worker did not connect within 15 s)); try again")` [M2].
  - UI kit: job 32097 (`reference apps in WebKit (macOS)`), the first `shell-demo-webkit` test,
    `shell-demo-a11y.spec.ts:12`, failed at `ShellDialogs.loaded()` (`ui/projects/paddock-e2e/src/shell.ts:284`,
    `expect(locator).toBeVisible()`, 5,000 ms, element not found) [M2].
  - Dyno: jobs 31671 and 31816, `dyno-bench::process disk_measures_the_mechanics_and_leaves_nothing_behind` TIMEOUT at
    60 s with no output; cause unknown (row 12).
  - Code: `crates/paddock-proc/src/start.rs` (`StartJudge`, `StartWatch`, `Pace`, `Patience`, `Reading`, `ExitInfo`);
    `crates/paddock-testkit` (on `main` without `wait.rs`; `wait.rs` with `wait_until`, `wait_for`, `poll_for` and
    `HANG_GUARD` = 40 s is on `train/2026-11-08-1`, `f38846973`); `conformance/src/checks/C71-test-timing.mjs` and
    `conformance/baselines/C71.json` (606 sites) on `train/2026-11-08-1`, not on `main`;
    `conformance/src/checks/C51-start-waits.mjs` (on `main`); `apps/debrief/crates/debrief-ipc/src/process.rs:95`
    (`SpawnSpec::new` sets `connect_window` 15 s; `UNTIL_CONNECTED`).
  - The check's prototype: `/Volumes/d7sst7/dev-targets/scratch/adr-0071/scan2.mjs` (scratch) over an extract of
    `b51c72509`; its counts are in "Instances". The reviewer's scan of revision 1's kinds is in
    `/Volumes/d7sst7/dev-targets/scratch/review-adr-0071/`.

## Context

On 2026-11-08 the macOS CI host went from one runner to three on one Mac. Within a day, tests of Pitwall, Debrief and the
shared UI kit failed with one shape: a test started a program that had just been built (or opened a page in a cold
browser) and gave up after a fixed number of polls or a fixed deadline.

**What is confirmed by the fix's author (Pitwall).** The first start of a freshly linked binary on macOS waits for the system's check of that
binary (`syspolicyd`; a stalled child was sampled in `_dyld_start` with 32 KB resident, using no CPU). With the binary
copied to a new file before each iteration, the old test double (200 health polls 50 ms apart) left failure logs in 6
and 3 iterations of two stress runs, both tests failing at 10.7-10.9 s; with the fix, 0 of 8 iterations failed while the
same binary's first start took 0.49-10.08 s, and the slowest iteration only made the tests slower (37 s) [M3]. On CI, 10
of the 12 `pitwall-cli` tests, each the first start of the fresh `pitwall` binary, ended within 57 ms of each other at
16.8 s, against 1.4 s earlier the same day [M2]. Ruled out by the fix's author (commit `81c3ac92f`): a port collision, a
leftover listener, CPU starvation alone, work before the fake engine starts. The with-and-without comparison is the
author's; no independent control run (the same stress script with and without re-creating the file, ER-28) is recorded
yet, so "confirmed" here means confirmed by the fix's author until WP-0b records one.

**What is measured about the check, and what is not.** A new program's first start costs about 100 times its second
start even on a lightly loaded host (258-2,864 ms against 2.4-6.6 ms for a one-line program), and six different new
programs started together took about as long as one: the checks of different binaries ran in parallel there, not one
after another [M1]. The CI pattern (many processes of one binary finishing together) fits every start of that binary
waiting on the single check of that binary. Why the check of `pitwall-server` took 10 s in a stress iteration and about
15 s on CI, while a one-line program's took under 3 s, is **not measured**: the size of the binary, the scan, contention
inside the checking service with other jobs' fresh binaries, and host load are all candidates (WP-0b measures it). The
decision below does not depend on the answer: whatever makes a start slow, a test must not turn slowness into a verdict.
Secondary sources describe the check as an XProtect scan plus a notarisation-ticket lookup on the first run of a new
executable (<https://eclecticlight.co/2025/05/04/last-week-on-my-mac-checking-code-can-take-longer-now/>,
<https://developer.apple.com/forums/thread/666452>; read 2026-11-09, not verified beyond [M1]).

**The same shape elsewhere, cause not confirmed.** The Debrief store-worker failure ran on the same host the same evening
and hit the product's default connect window (15 s) for a fresh worker binary; the WebKit failure was the first page of a
cold browser project under a 5 s assertion (the next test took 6.3 s, the rest of the file 0.33-0.66 s) [M2]. Neither is
confirmed by a run without its cause (ER-28), and the WebKit binary is not freshly built, so that one is a cold browser
and bundle, not the first-start check. Both are fixed budgets that start counting before the thing waited for could be
ready. Earlier instances of the class: CI 7096 (2026-10-29; the reason ADR 0031 and `UNTIL_CONNECTED` exist), the
Debrief chromium-driver fixed 20 s start (CI 7213, 7250) and Spotter's self-test count set by a real child (both in ADR
0041's table).

**Why ADR 0041 and C71 did not stop it.** ADR 0041 forbids counts of polls that host speed sets and allows real time only
as a hang guard of at least 10 s, but C71's first slice flags only `sleep` calls and a few TypeScript shapes: not the
count, not a deadline, not a start bound passed as configuration, and nothing in a test-support crate's `src/` or in the
e2e kit's sources. Pitwall's `sleep` line is in C71's baseline while the 200-poll count that decided the verdict was
never looked at. The store worker's 15 s is a product default the test reaches through a constructor, and C51 (product
code, report-only) does not list it: its name list is START, SPAWN, LAUNCH and READY, not `connect_window`.

## The pattern

> **A start wait fails whenever a test gives a program, a worker or a page a fixed number of polls or a fixed deadline
> to become ready, and the start takes longer than that: a first-start check, a cold browser, a starved host.**

**Invariant.** A test waits for anything it started (a process, a worker, a browser page or window) until it is ready or
has ended, and reports an end at once with its reason. Elapsed time never decides that verdict. Real time appears only as
a hang guard of at least `HANG_GUARD` (40 s), measured on the wall clock, whose expiry is reported as a hang, not as a slow
start.

## Instances (`origin/main` `b51c72509`, 2026-11-09)

**Method.** The kinds of §4 were prototyped (`scan2.mjs`) and run over an extract of `b51c72509` (`apps`, `crates`,
`tools`, `ui`, `scripts`); every hit was read in context. The prototype ignores waivers (none exist yet) and is a
heuristic; the check itself (WP-3) is tested against fixtures and the history controls of §4. The per-kind waiver column
is this author's reading; the re-review read two more sites as waivers, so the totals are given as about 15 waivers and
about 84 migrations.

| Kind | Hits | Of which waived or exempt (expected) | To migrate |
|---|---|---|---|
| R1 fixed-count poll in a start function | 3 | 0 | 3 (one already fixed on the train) |
| R2 start bound under 40 s in test configuration | 13 | 5: three windows that a test examines (`debrief-ipc` `launch.rs:120`, `:133`, `server_backend.rs:362`), a parsed configuration value (`paddock-startup` `config.rs:174`), one on a fake system (`paddock-service` `install_tests.rs:1025`, the seam exemption) | 8 |
| R3 time-bounded wait in a start function | 25 | 3: a grace under test (Dyno `link/tests.rs:1503`), a recording window (`real_runtime.rs:242`), an end after a user action on a fake browser (`chromium-driver` `launch.rs:1110`) | 22 |
| R4 product start-window constructor in a test without its test form | 31 | 5: a program that does not exist (`debrief-ipc` `launch.rs:258`, `debrief-plugin-host` `builtins.rs:243`, `debrief-store-worker` `worker.rs:659`) and two constructions that start nothing (`dyno-plugin-host` `host.rs:555` on a fake launcher and clock, `debrief-ipc` `process.rs:663`) | 26 |
| T1 a ready-marker selector outside the kit's `ready.ts` | 27 | 0 | 27 |
| T2 counted attempts in a page-opening function | 0 | 0 | 0 |
| **Total** | **99** | **about 15** | **about 84** |

Revision 1 counted about 44 sites. Its kinds were not limited to starts, so they would have reached about 160 sites (the
reviewer's scan); revision 2 limits R1, R3 and T2 to functions that start a process or open a page, adds R4 and the new T1,
and leaves fixed-count polls that are not starts (about 45 in the reviewer's scan, including Pitwall `e2e.rs:599`, `:692`,
`:808`, Dyno `tests/common/mod.rs:25` and `paddock-e2e` `svg.ts:41`) to ADR 0041's ratchet and its WP-6.

The rows, with their migration (every site is in the prototype's output, `scan2.txt`, which WP-3 turns into the check's
first run):

| # | Where | Instance | Status | Kind | Migration |
|---|---|---|---|---|---|
| 1 | `apps/pitwall/crates/pitwall-server/tests/common/mod.rs:476` | `ChildEngine::start`: `for _ in 0..200` `/health` polls 50 ms apart, blind to the child's exit | broken (job 32436; cause confirmed by the fix's author [M3]) | R1 | land `fix/pitwall-e2e-engine-health` as is; then move its loop and `runnable_server`'s warm-up onto the kit (WP-4a), keeping the double's `StartOutcome` recording |
| 2 | `apps/debrief/crates/debrief-plugin-e2e/tests/it/store_worker.rs:49` and every `WorkerConfig::new`/`StoreWorker::new` in tests (`debrief-store-worker/tests/it/parity.rs`, `worker.rs`, the app's `health/commands/tests.rs`, `debrief-store-client` unit tests) | the product default `connect_window` 15 s (`debrief-ipc/src/process.rs:95`), which `WorkerConfig` cannot change | broken (job 32417; cause probable, not confirmed) | R4 | `WorkerConfig` gains `connect_window`; `WorkerConfig::for_tests(exe)` sets `UNTIL_CONNECTED` and the tests call `warm_first_start`. Failing test first: `assert_eq!(WorkerConfig::for_tests(WORKER).connect_window, UNTIL_CONNECTED)` (the form Debrief's `lib.rs:1116` uses for plugins) |
| 3 | `ui/projects/paddock-e2e/src/shell.ts:267` (the `pd-dialog-shell` locator) and `:284` (`loaded()`) | the first wait on a new window inherits the 5 s expect timeout | broken (job 32097; cause not confirmed) | T1 | `loaded()` first awaits `readyWindow(page)`; the selector moves into `ready.ts`; the WebKit projects get `warmApp`. Failing test first: a kit unit test runs `loaded()` against a recording locator and `expect` seam and asserts its first wait has no time limit of its own (red today). A spec that reproduces a cold start needs real time and is not written (ADR 0041) |
| 4 | `apps/debrief/crates/debrief-plugin-test/src/lib.rs:455`, `crates/paddock-plugin-test/src/fake.rs:224` | the plugin test harnesses set a 15 s connect window and a 30 s start timeout for every plugin under test | at risk | R2 | `UNTIL_CONNECTED` (or the kit's test `Patience`) and `warm_first_start` before a program's first launch |
| 5 | `apps/debrief/crates/debrief-ipc/tests/it/launch.rs:19`, `child_log.rs:14`, `launch.rs:471` | a 10 s connect window for the fresh fake child; `launch.rs:127` (`--exit-now`) relies on it, so a slow start reads as "did not connect" instead of "exited while starting"; a 10 s wait for memory events after launch | at risk | R2, R3 | `UNTIL_CONNECTED`, warm-up in `spec()`, `wait_for` through the kit; `:120` and `:133` (`--no-connect`) keep their short windows with `expiry-under-test` |
| 6 | `apps/debrief/crates/debrief-ai/tests/it/worker.rs:36-37`, `worker_memory.rs:35` | `hello_timeout` and `load_timeout` 10 s for the fresh fake worker; a 10 s deadline from `launch()` to the memory watcher's first event | at risk | R2, R3 | the test `Patience`, warm-up, `wait_ready_or_exit` with the event as probe |
| 7 | `apps/debrief/crates/debrief-ai-worker/tests/it/server_backend.rs:65` and its four `ServerBackend::new` uses | `Timeouts { ready: 20 s }` and the default ready bound for a fake `llama-server` built by the test | at risk | R2, R4 | `ServerBackend::for_tests`; `:362` (`never_ready`, 1 s) keeps `expiry-under-test` |
| 8 | `apps/debrief/crates/debrief-plugins/tests/it/{common/mod.rs:35, evtx_memory.rs:120, :210, perf_plugins.rs:22}`, `debrief-plugin-host/tests/it/remote.rs:22`, `debrief-store-worker/tests/it/parity.rs` (4) | `Builtins::load` with the product's 15 s window; two of them then wait 20-30 s for every pid and measure a half-started set if one is missing | at risk | R4, R3 | `Builtins::load_with(.., UNTIL_CONNECTED)` (as the Debrief app's tests already do), `wait_for` each pid |
| 9 | `apps/dyno/crates/dyno-plugin-host/tests/processes.rs:28` | `Timeouts { start: 20 s }` for fake plugin processes | at risk | R2 | the test `Patience`, warm-up |
| 10 | `apps/dyno/crates/dyno-agent/tests/term.rs:65`, `:100`, `:170` | readiness on the discovery file under a 40 s guard, blind to the child's exit: an agent that dies at start reads as "never became ready" after 40 s (ER-27) | at risk | R3 | `wait_ready_or_exit(child, probe: the discovery file)` |
| 11 | `apps/dyno/app/src-tauri/src/link/tests.rs:145`, `apps/dyno/crates/dyno-platform/src/network.rs:295` | waits of 60 s and 20 s on a real child's exit or socket | at risk | R3 | the kit's `wait_exit` / `wait_ready_or_exit` |
| 12 | `apps/dyno/crates/dyno-bench/tests/process.rs:30` (`bin()`) | starts the fresh `dyno-bench` with `.output()`; only nextest's 60 s kill bounds it; jobs 31671 and 31816 timed out in the disk test | candidate, cause unknown | none | ER-28 first (a first start, the disk work or a hang?); a `warm_first_start` in `bin()` with its printed duration is the first logging step. Job 32410's failure of the same test ("the drive has 266 GB free; the disk test needs 401 GB" for a 16 MiB file) is a different bug |
| 13 | `crates/paddock-chromium/tests/it/real_browser.rs:176`, `:431`; `apps/spotter/crates/spotter-render/tests/real_browser.rs:212`, `:296`, `:319`; `apps/debrief/crates/chromium-driver/tests/it/real_browser.rs:65`, `src/launch.rs:907` | 20-40 s receives and deadlines after launching Chrome for Testing; a 40-evaluation count | at risk | R1, R3 | the kit's waits with the browser's process as the liveness |
| 14 | `crates/paddock-setup/tests/vm/windows.rs:78`, `:904`, `:972`, `:1249`; `tools/paddock-dev/tests/it/slots.rs:289`, `:335`; `apps/debrief/crates/debrief-plugin-windows/tests/it/eventlog_live.rs:72`; `apps/debrief/crates/debrief-plugin-dotnet/tests/it/real_runtime.rs:114`; `apps/debrief/crates/debrief-monitor/src/dotnet_win_tests.rs:199` | 20-60 s deadlines and a 50-poll count on installers, scripts and runtimes the test started (Windows VM tests among them) | at risk | R1, R3 | the kit's waits; the VM tests run in WP-4d's Windows VM pass |
| 15 | 27 selector sites in 17 files: `ui/projects/paddock-e2e/src/` `dialog-fits.ts` (2), `size-to-content.ts` (3), `shell.ts` (1); `ui/e2e/shell-demo.spec.ts` (2), `shell-demo-theme.spec.ts` (1); Dyno `e2e/` `menu-bar-items-fit` (3), `screenshots-helper` (3), `layout` (2), `rule-dialog` (2), `dialogs`, `screenshots-dialogs`, `screenshots-docs-dialogs`, `screenshots-network`, `screenshots-shell`, `screenshots-wake`, `screenshots-wol`, `widget-drag-ghost` (1 each) | ready markers waited on directly, most with a fixed 15 s | at risk | T1 | `await readyWindow(page)`; `dialog-fits.ts:46-47` also keep 15 s waits right after the ready wait, which go too |

**Related finding, routed separately by the coordinator (ER-12).** Playwright retries without flaky-as-failure: Pitwall
and Debrief `retries: CI ? 2 : 0`, Pitcrew, Spotter and Dyno `retries: CI ? 1 : 0`, none with `failOnFlakyTests`
(`apps/*/app/playwright.config.ts` at `b51c72509`). Such a retry can turn exactly this class green. Debrief's local
nextest profile has `retries = 2` (CI 0).

## Decision

### 1. One mechanism in Rust: `paddock-testkit::start`

A new module beside `wait.rs` (ADR 0041), built on the product's start judge so the family has one judgement of "ready
or exit" (rule 55, ER-22):

- **`warm_first_start(exe: &Path, args: &[&str]) -> Duration`**: runs `exe args` (`--version` by convention) once to its
  end under the hang guard, and prints `the first start of <exe> took <s> s` to the test's stderr each time it runs
  (ER-27). Memoised per process by the file's identity (canonical path, length, modification time; on Unix also device
  and inode), so repeated calls in one test binary cost one start. A non-zero exit fails with the program's stdout and
  stderr tails: a program that cannot print its version is broken, not slow. **`warm_first_starts(&[(exe, args)])`**
  starts several programs at once (a thread per program, bounded by the list, at the test's own priority, rule 52), for a
  test that starts several fresh programs (Debrief's plugin tests). The warm-up moves the first-start check before any
  step that still has a bound the test cannot remove (a product's HTTP client, a harness request). It is never what
  makes a test correct: the wait below is.
- **`wait_ready_or_exit(child, what, probe) -> StartOutcome<T>`**: returns `StartOutcome::Ready(T)` as soon as `probe`
  gives `Some(T)`, or `StartOutcome::Exited(ExitInfo)` (status and stderr tail, `paddock_proc::ExitInfo`) as soon as the
  child has ended. It drives `paddock_proc::StartWatch` with:
  - readings with `Cpu::NotRead` (the probe reads no CPU time in tests). With such readings the judge cannot judge spin
    or idle: it counts the wait as blind and reports `Stalled(Hung)` once `Patience::hang_guard` worth of ticks has
    passed. The test `Patience` therefore sets `hang_guard` (and `spin`, `idle`) above the wall guard, so that only the
    `DeadlinePace` below ends a wait. A `Stalled(Paused)` (the child is stopped by the OS) is not an end either: the
    kit calls `wait` again, and the wall deadline keeps running;
  - a **wall-clock** `Pace` (`DeadlinePace`): it waits `wait.rs`'s backoff (200 µs doubling to 20 ms), and its `wait`
    returns `false` once `HANG_GUARD` has passed on the wall clock since the wait began, so the judge's tick counting
    (which counts a paused host as one tick, ADR 0031 §2) cannot stretch the guard past nextest's kill;
  - its own progress note: the kit prints "<what> (pid N) is still starting after N s" every 10 s of wall time (the
    judge's `note_after` fires once, so the kit does not rely on it).
  `Watched::Cancelled` from the `DeadlinePace` (and any `Stalled` other than `Paused`, should one come first) is reported
  as a hang: a panic with the pid, whether the process is alive,
  a zombie or paused (`paddock_proc::run_state`), the last probe result and the stderr tail, worded "never became ready
  or ended", never as a slow start.
- **`wait_exit(child, what) -> ExitInfo`**: the same loop with no ready probe, for a test that waits for a child it
  started to end on its own (rows 11 and 14).
- **`try_wait_ready_or_exit`**: returns `Err(HangGuardExpired)` instead of panicking, for test doubles that implement a
  product port and must return an error into the code under test (Pitwall's `ChildEngine`).
- **Async**: `StartWatch` is synchronous, so `wait_ready_or_exit_async` (behind a `tokio` feature, for
  `tokio::process::Child` and async probes) is its own small loop over the pure `StartJudge` with `tokio::time::sleep`
  and the same wall deadline and notes. Both loops share the judge, the `Patience`, the messages and the tests' scripted
  program.
- **Dependencies.** `paddock-testkit` gains `paddock-proc` as an optional dependency behind a `start` feature (default
  off). `paddock-proc` already has `paddock-testkit` as a dev-dependency (`tests/it/never_panics.rs` uses
  `assert_never_panics`). Cargo allows that dev-dependency cycle; within `paddock-proc`'s own tests the testkit's copy of
  `paddock-proc` is a different crate whose types do not unify with the crate under test. That is harmless while
  `paddock-proc`'s tests never pass its types to the testkit, so they test `StartWatch` directly and never use
  `paddock_testkit::start`; a C71 fixture asserts that rule (a `use paddock_testkit::start` in `crates/paddock-proc/` is
  an error).
- **Tests of the module** (rule 43: shared behaviour tested once here; apps test only their wiring): a scripted program
  (a test binary of the crate: `--ready-when <file>`, `--exit <code>`, `--never-ready`) proves (a) a child that becomes
  ready only after the test has seen more probes than any budget this ADR removes (200, as in `9c42a60ef`) is waited
  for, with no real duration involved; (b) a child that exits before it is ready is reported at once with its stderr;
  (c) the mutant "liveness always says running" fails the exit test within a few polls (as `7a73cdc59` did: the first
  child has ended and the wait polled it more than 5 more times); (d) `warm_first_start` runs the program once per
  identity and again after the file is replaced; (e) the `DeadlinePace` ends the wait on a stopped (`SIGSTOP`) child at
  the guard with the hang message, with an injected clock for the deadline so the test does not take 40 s. Line
  coverage at least 99 % (the crates gate).

### 2. One mechanism in TypeScript: `@decryp7/paddock-e2e`

- **`ready.ts`** is the one file that names ready markers (`html[data-dialog-ready]`, `pd-dialog-shell`, an app's
  `data-*-ready`). It exports:
  - **`readyWindow(page, marker?)`**: waits for the window's ready marker with no time limit of its own
    (`locator.waitFor({ timeout: 0 })`: the test's timeout is the guard), and on failure attaches the console errors and
    `pageerror`s it collected while waiting. `ShellDialogs.loaded()` calls it first; `ShellDialogs`' shell root takes its
    locator from `ready.ts` and becomes private.
  - **`warmApp`**: a worker-scoped automatic fixture, enabled per project with `use: { warm: { path, marker } }`. Before
    a worker's first test it opens the base URL, waits for the marker under the worker-fixture timeout, records the
    duration as an annotation and in the log, and closes the page. A cold browser and a cold bundle are paid there,
    not inside a test's 5 s assertion.
- **Guard floor.** A project that uses `readyWindow` or `warmApp` has a test timeout of at least 40 s, so the guard is
  never below `HANG_GUARD`. Pitwall's Playwright config sets no test timeout (Playwright's default, 30 s) and gets 60 s
  (WP-2); Debrief's is 60 s on CI but 30 s locally (`CI ? 60_000 : 30_000`) and gets 60 s locally too, so the floor
  holds in local runs and not only on CI (WP-2); Dyno's is 60 s, the kit's `config.ts` default 60 s.
- Unit tests in the kit (`*.test.ts`) and a selftest spec whose page sets its ready marker only when the spec releases
  it (the TypeScript twin of `--ready-when`).
- `pointInShape` (`svg.ts`) is not a start; it moves to `expect.poll` under ADR 0041, not this ADR.

### 3. How a hang is still bounded

- Rust: the guard is `paddock_testkit::wait::HANG_GUARD` (40 s) of wall time from the start of the wait (the
  `DeadlinePace`), times `PADDOCK_TEST_TIME_SCALE` once ADR 0041's scaling lands (its §3 and WP-5; `wait.rs` on the train
  has the constant without the scale; the `starved` profile scales the kill in the same ratio). It is above every first
  start recorded (10.08 s in a stress iteration [M3], about 16.8 s on CI [M2]) and under the CI nextest kill
  (`[profile.ci]` `slow-timeout` 30 s × 2 = 60 s in `crates/`, Dyno, Pitwall, Pitcrew and Spotter; × 3 = 90 s in
  Debrief), so a real hang ends with the kit's message, not the runner's.
- TypeScript: the guard is the test timeout (at least 40 s, §2) or, for `warmApp`, the worker-fixture timeout.
- The guard never decides a verdict: it only ends a wait that would never end. Ready and exit are the only outcomes a
  test asserts. A test that asserts a start's speed is a benchmark (rule 16), not a test.
- The warm-up has its own guard. A warm-up, a readiness wait and the test's own work together can pass nextest's 60 s
  kill; nextest then ends the test with TIMEOUT, and the warm-up's printed duration and the 10 s notes in the test's
  output say why (ER-27). So "a slow start makes a test slower, never red" holds while the whole test stays under the
  runner's kill.
- A test that examines a product window's own expiry with a child scripted never to be ready (`--no-connect`,
  `never_ready`) keeps a short window: no slower start can change its verdict. It carries `// conformance:
  expiry-under-test — <the scripted mode>`.

### 4. The check: C71 gains start kinds, C51 gains connect windows

**Scope.** C71 (ADR 0041 §4, on `train/2026-11-08-1`) reads Rust test code (`tests/`, `benches/`, `tests.rs`,
`*_test(s).rs`, `#[cfg(test)]` items) and TypeScript specs and e2e files. For the start kinds it also reads **test-support
sources**: `crates/*-plugin-test/src/**`, `apps/*/crates/*-plugin-test/src/**`, `**/src/testing.rs`, `**/src/testing/**`,
and **`ui/projects/paddock-e2e/src/**`**. It skips only the files that implement the mechanism:
`crates/paddock-testkit/src/{start,wait}.rs` and `ui/projects/paddock-e2e/src/ready.ts`.

**Start function.** A Rust function whose body starts a process (`.spawn()`, `Command::new(`, `CARGO_BIN_EXE_`, a
`launch(` or `launch_child(` call, `ChildProcess::spawn(`) or calls a function of the same file that does (one level).
`Type::start(` of an in-process type is not a process start. **Page-opening function**: a TypeScript test or function
whose body calls `goto(`, `newPage(`, `.launch(` or a kit opener (`open…Window(`, `openDialog(`).

| Kind | Flags | Passes when |
|---|---|---|
| **R1 fixed-count poll** (Rust) | in a start function, a `for _ in 0..N` loop whose body has an early `break`, `return` or `?` | the loop goes through the kit; or `// conformance: bounded-search — <why the count is not time>` |
| **R2 fixed start bound in test configuration** (Rust) | an assignment or field initialiser named `*connect*`, `*hello*`, `*start*`, `*ready*`, `*spawn*`, `*launch*` or `*load*` ending in `window`, `timeout`, `deadline`, `limit` or `budget` (or exactly `ready`/`start`), set to a `Duration` literal under 40 s | the value is `UNTIL_CONNECTED`, `HANG_GUARD` or the kit's test `Patience`; the function uses a fake seam (`FakeClock`, `ManualPace`, `TestSeams`, `FakeSystem`, `FakeRunner`); the name is a network client's `connect_timeout` (HTTP/TLS, not a start); or `// conformance: expiry-under-test — <scripted mode>` |
| **R3 time-bounded wait in a start function** (Rust) | in a start function, `Instant::now() + d`, `elapsed() < d` in a loop condition, `recv_timeout(d)` or `wait_timeout(d)`, **whatever `d`** (a 40 s guard that is blind to the child's exit is flagged too: row 10) | the wait goes through the kit (`wait_ready_or_exit`, `wait_exit`, `wait_until`); it follows a stop request within six lines (`stop(`, `term(`, `kill`, `close(`, `shutdown(`, `unload(`); it is an elapsed-time assertion (ADR 0041's C30 rules own those); or `// conformance: timing-under-test — <the grace or window the test examines>` |
| **R4 product start window reached through a constructor** (Rust) | a constructor listed in `conformance/start-windows.json` (crate, constructor, the window it sets, its test form) used in test code without the test form in the same function: today `debrief_ipc::process::SpawnSpec::new` (when the function also launches), `debrief_store_client::WorkerConfig::new` and `StoreWorker::new`, `debrief_plugin_host::Builtins::load`, `debrief_ai_worker::ServerBackend::new`, `dyno_plugin_host::HostConfig::new` (when the function also starts the host) | the test form (`for_tests(…)`, `load_with(…, UNTIL_CONNECTED)`, `connect_window = UNTIL_CONNECTED`); or `expiry-under-test` (a program that does not exist, a never-connecting child). A product crate that adds a start window adds its line to the list in the same change (the list is data, open/closed) |
| **T1 a ready marker named outside `ready.ts`** (TypeScript) | a string literal containing `data-…ready` or `pd-dialog-shell` in Playwright code (e2e folders, `*.e2e.ts`, the kit), read from the unmasked text (C71's `maskTs` blanks strings), **whatever the timeout** | the code calls `readyWindow`/`ShellDialogs.loaded()` instead; Vitest component specs (a component's own ready attribute under test) are outside T1's scope |
| **T2 counted attempts** (TypeScript) | in a page-opening function, `for (let i = 0; i < N; …)` with an early `return`/`break` | `expect.poll`/`toPass` instead; a key walk (`keyboard.press` in the body) is a count of steps, not polls; or `bounded-search` |

The 10 s floor of ADR 0041's hang-guard waiver stays for C71's other kinds; R1-R4 accept no hang-guard waiver at all
(the kit's guard is the only one).

**Residual gap.** T1 sees selectors, not locators handed around: `expect(someLocatorFromAGetter).toBeVisible()` on a
ready root is not seen. The kit closes the common path by keeping `ShellDialogs`' shell root private behind `loaded()`.
R4 sees only the constructors on its list; a new product start window enters the check when its line is added, which C51
(below) prompts.

**C51** (product code, report-only, ADR 0031 §8) widens its name list from START, SPAWN, LAUNCH and READY to also
CONNECT (window-style names such as `connect_window`, not network clients' `connect_timeout`) and HELLO. HANDSHAKE stays
out: `paddock-ipc`'s handshake bound starts after the client is already running and ADR 0072 keeps it on purpose
(`Timeout { phase: Hello }` on an injected clock). The product's own 15 s store-worker and plugin windows then appear in
C51's report for ADR 0031's migration; they are not changed by this ADR (D4).

**Errors from day one, only for the start kinds.** R1-R4 and T1-T2 as defined here are errors from the day they land,
in the same change as the about 84 migrations and about 15 waivers above, so `conformance/baselines/C71.json` gains no entry.
Fixed-count polls that are not starts stay where ADR 0041 put them: C71's existing kinds and its shrink-only ratchet,
migrated by ADR 0041's WP-6 (D3).

**Positive controls (ER-33, rule 71).** C71's test file gains, per kind, fixtures that must be flagged and their fixed
twins that must pass, and these controls from history, one per broken instance plus the exit-blind wait:
- R1: `git show 81c3ac92f^:apps/pitwall/crates/pitwall-server/tests/common/mod.rs` is flagged at the `for _ in 0..200`
  line; `7a73cdc59`'s version is not (both verified with the prototype);
- R4: `apps/debrief/crates/debrief-plugin-e2e/tests/it/store_worker.rs` at `b51c72509` is flagged at line 49 (verified);
- T1: `ui/projects/paddock-e2e/src/shell.ts` at `b51c72509` is flagged at line 267 and `dialog-fits.ts` at lines 43 and
  100 (verified);
- R3: `apps/dyno/crates/dyno-agent/tests/term.rs` at `b51c72509` is flagged at lines 65, 100 and 170 (verified).

In the repository run, C71 prints how many start functions, page-opening functions, listed constructors and ready-marker
strings it scanned; when any of these counts is zero the run reports `check broken` (an error), so a scanner that stops
seeing starts cannot pass silently.

### 5. Rule text

No new rule. Proposed rule 60 (ADR 0041, not yet on `main`) gains one sentence when it lands: "A test waits for a
process, worker or page it started until it is ready or has ended, and reports an end at once; a fresh binary is warmed
once (`warm_first_start`) before any step with a bound the test cannot remove; a start's hang guard is the kit's, at
least `HANG_GUARD` of wall time." A lesson records the incident (ER-6).

### 6. The nextest setup-script warm-up: not used

nextest can run a setup script before the tests a filter selects and pass variables to them, but the feature is marked
"Experimental: This feature is not yet stable" and needs `experimental = ["setup-scripts"]`
(<https://nexte.st/docs/configuration/setup-scripts/>, read 2026-11-09; tracking issue #978). Decided against, for now:
- It would be a second, CI-only home of the warm-up (rule 55, ADR 0042): `cargo test`, Playwright's backends, VM runs and
  the local `./dev` gates would not get it, and the in-test warm-up is needed there anyway.
- It does not remove the class: the guarantee is ready-or-exit; a warm-up only moves a delay, and a script that warms
  everything would hide a missing ready-or-exit wait until the next cold start elsewhere.
- The script has to find every fresh binary itself (`CARGO_BIN_EXE_*` is set for tests, not for scripts); a non-zero exit
  or timeout of the script ends the whole run; an experimental key can change under a nextest update (Renovate updates
  the pinned version).
- It would save little: one already-checked start per test that calls the warm-up (milliseconds for a small program [M1]).

Revisit when the feature is stable, as an optimisation that calls `warm_first_start`, never as the mechanism.

## Parallelism and cost (ER-20, rule 51)

- **Where the time goes.** The check of a new binary is paid once per binary and host, by whoever starts it first; the
  warm-up does not add it, it moves it before the test's bounded steps. Its own cost is one extra start of an
  already-checked binary, 2.4-6.6 ms for a one-line program [M1]; for larger binaries it is not measured (WP-0b). With
  nextest's process per test, the per-process memo pays that once per test that calls it.
- **More parallelism.** The checks of different binaries ran in parallel in [M1] (six took about as long as one), so a
  test that starts several fresh programs warms them together (`warm_first_starts`), costing about one check instead of
  several in a row. A run-wide warm-up of every binary would do the same across tests; it is the setup-script idea (§6)
  and is not done now. One binary per test needs nothing more: its first `warm_first_start` is its only check.
- **Cheaper?** (a) Skip the warm-up across processes with a marker file per binary identity: saves milliseconds per test
  and adds a cached identity (rule 53's extension); not done. (b) A nextest setup script: §6. (c) Exempt the runner from
  the check (the "Developer Tools" exemption, secondary sources above; its effect on a runner started by launchd is
  unverified): removes the delay on that host, but tests would no longer start programs as a user's Mac does (ADR 0048),
  and it does nothing for Windows (Defender scans a just-started program: `crates/paddock-setup/tests/vm/windows.rs:77`),
  Linux under load or browsers (D5). (d) Fewer fresh binaries per job (test-binary consolidation, lessons: 239 → 99)
  already reduces the number of checks.
- **Polling cost.** `wait_ready_or_exit` polls with `wait.rs`'s backoff (200 µs doubling to 20 ms): about 50 cheap probes
  a second once the backoff has grown, each a `try_wait` and one connect or file read, and no CPU-time reads.
  `readyWindow` uses Playwright's own locator polling.
- **What it saves.** Each red macOS run of this class cost a re-run of the app's macOS job plus a diagnosis; there were
  three on the evening of 2026-11-08 [M2].

## Consequences

- A test cannot give a started program, worker or page a count or a deadline any more, and cannot reach a product's fixed
  start window through a listed constructor; the check fails the change that tries. A slow start makes a test slower
  and is red only when the whole test passes the runner's kill (§3). A start that ends is reported at once (ER-27).
- The family has one ready-or-exit judgement for product and tests (`paddock_proc::StartJudge`), one warm-up and one
  window wait; Pitwall's double, Debrief's harnesses and Dyno's agent tests lose their copies.
- Tests that hit a product start window set it to wait until ready or exit, as the Debrief app's tests already do for
  plugins. The product's own fixed windows are listed by the widened C51 for ADR 0031's migration, not changed here.
- A real hang now takes up to 40 s (scaled) of wall time to report instead of 5-20 s.
- Every new product start window needs a line in `conformance/start-windows.json`.

## Alternatives considered

- **Raise the budgets** (200 polls to 900, 15 s to 60 s). Forbidden by ER-12, and every number is wrong on the next slower
  host; Pitwall's `tick_until` already says "Generous: loaded CI runners are slow" at 900 polls.
- **Warm-up only.** Fixes the first-start check on macOS and nothing else (a cold browser, Windows Defender, a starved
  Linux container); a test that still counts polls stays wrong. Kept as the complement, not the fix.
- **Retry the failed test.** One retry with flaky-as-failure keeps the run red (ER-12); retries that turn it green hide
  the class (the related finding above).
- **A nextest setup-script warm-up.** Experimental; CI-only; §6.
- **Exempt the CI host from the first-start check.** Hides a real-user behaviour from the tests (ADR 0048) and leaves the
  other causes; D5.
- **Kinds over all test code, not only starts** (revision 1). About 160 sites, most of them polls ADR 0041 already owns;
  rejected for this ADR (D3).
- **A per-app helper in each app.** That is how it grew (Pitwall's `CHILD_HANG_GUARD`, Dyno's `term.rs` `HANG_GUARD`,
  Debrief's `wait_for` copies of 10-20 s); rule 55 and ER-22 say one shared implementation.
- **Use `StartWatch` directly in tests.** Every test would build a probe, a pace and a patience and choose the guard;
  the wrapper fixes the guard, the wall deadline and the messages once.

## Work packages

Estimates in agent-days, from the measured counts above (planning estimates, not measurements).

| WP | What | Owner (role) | Days |
|---|---|---|---|
| WP-0 | Rule-48 blocks for every number quoted here | done in this revision ([M1]-[M3]) | 0 |
| WP-0b | Measure why `pitwall-server`'s first start takes seconds (binary size, scan, contention, load), outside CI's busy hours, without CPU burners; record the block | researcher | 0.5 |
| WP-1 | `paddock-testkit::start` (`start` feature): `warm_first_start(s)`, `wait_ready_or_exit` (sync on `StartWatch` with `DeadlinePace`; async on `StartJudge`), `wait_exit`, `try_…`, `StartOutcome`, the test `Patience`, the scripted program, tests (a)-(e), 99 % coverage, CHANGELOG | implementer, reviewed by reviewer | 2.5 |
| WP-2 | `@decryp7/paddock-e2e`: `ready.ts` (`readyWindow`, `warmApp`, the markers), `ShellDialogs.loaded()`; the Pitwall and Debrief Playwright test timeouts (60 s, locally too); unit tests and the selftest page with a released marker | implementer, reviewed | 1.5 |
| WP-3 | C71 start kinds R1-R4, T1-T2 with the widened scopes, `conformance/start-windows.json`, fixtures, the history controls and the scan counts (`check broken` at zero), the `paddock-proc` rule; C51's CONNECT and HELLO; conformance README | implementer, reviewed | 2 |
| WP-4a | Pitwall: land `fix/pitwall-e2e-engine-health` (in `train/2026-11-08-1`); then `ChildEngine` and `runnable_server` on the kit (row 1) | implementer | 0.75 |
| WP-4b | Debrief (about 47 sites, waivers included): `WorkerConfig::connect_window` and `for_tests`, `ServerBackend::for_tests`, the plugin-test harness, `debrief-ipc`, AI worker, plugins, chromium-driver, plugin runtimes (rows 2, 4-8, 13-14); a failing test first per broken row (ER-11) | implementer | 3 |
| WP-4c | Dyno (about 9 sites): `term.rs`, link and network tests, plugin host (rows 9-11); `dyno-bench` `bin()` warm-up and the ER-28 cause list for the 60 s TIMEOUT (row 12) | implementer; researcher for row 12 | 1.5 |
| WP-4d | Shared crates, Spotter and tools (about 17 sites): `paddock-plugin-test`, `paddock-plugin-host` tests, `paddock-chromium`, Spotter's real-browser tests, `paddock-setup` VM tests (run in the Windows VM), `paddock-dev` slots (rows 4, 13-14) | implementer | 1.5 |
| WP-4e | TypeScript (27 sites): `readyWindow` everywhere, the WebKit projects' `warmApp` (rows 3, 15) | implementer | 1 |
| WP-5 | Lesson, the sentence for proposed rule 60, the Wiki notes for the coordinator | implementer | 0.25 |
| **Total** | | | **14.5** |

**Order and release windows (ER-1, ADR 0041's rollout).** WP-0b and WP-4a's landing of the existing fix are independent
and may go first. WP-1 and WP-2 change `crates/` and `ui/`, which every app's release builds from: they land after
`train/2026-11-08-1` (it carries C71 and `wait.rs`) is on `main`, outside a release window, with at most two tooling
agents while a release is in flight. WP-3 lands together with WP-4b-e (the error kinds and their migrations in one
change, so nothing is baselined); if that change is too large for one batch, the kinds land per app with each app's
migration, each kind scoped to the apps already migrated. WP-4 touches only each app's tests and runs in that app's
batch.

**Rollback.** The new C71 kinds can be switched to warnings by one line; the kit functions are additive; each app's
migration is its own commit and reverts alone.

## Decisions for the user

- **D1. Build the test wait on the product's start judge** (`paddock-testkit`'s optional `start` feature depends on
  `paddock-proc`; a wall-clock `DeadlinePace`, the kit's own 10 s notes, an async loop over `StartJudge`, and
  `paddock-proc`'s own tests kept off `paddock_testkit::start` because of the dev-dependency cycle). *Recommended: yes.*
  Alternative: a small separate loop in the kit (no new dependency, but a second implementation of the judgement under
  rule 55).
- **D2. The nextest setup-script warm-up.** *Recommended: no, not while it is experimental; revisit when stable, as an
  optimisation calling `warm_first_start`* (§6).
- **D3. Which kinds are errors from day one.** *Recommended:* only the start kinds as defined in §4 (R1 and R3 in start
  functions, R2, R4, T1, T2 in page-opening functions), with their about 84 migrations and about 15 waivers in the same change and
  no baseline growth; fixed-count polls that are not starts stay in ADR 0041's shrink-only ratchet (WP-6). Alternatives:
  the start kinds as warnings first, turned to errors app by app as each app migrates; or a user-approved one-time
  baseline growth by about 84 sites, migrated later.
- **D4. Widen C51 to CONNECT (window-style names) and HELLO**, report-only, not HANDSHAKE (ADR 0072 keeps that bound) and
  not network `connect_timeout`s. *Recommended: yes.* The product's own 15 s store-worker and plugin windows meet the same
  first-start check on a user's Mac after an update (not measured); *recommended* that the coordinator gives that an
  owner and a line in the Debrief 1.2 plan under ADR 0031, rather than leaving it to the C51 report.
- **D5. Exempt the macOS CI runners from the first-start check** (Developer Tools in Privacy & Security). *Recommended:
  no*: the tests should start programs as a user's Mac does (ADR 0048), and it would not cover Windows, Linux or browsers.
  Its effect on a runner started by launchd is unverified; if CI time ever matters more, measure it on one runner first
  (rule 48) and keep this ADR's mechanism regardless.
- **D6. Order.** *Recommended:* land the Pitwall fix and WP-0b now; everything else after `train/2026-11-08-1` reaches
  `main`, outside a release window.
