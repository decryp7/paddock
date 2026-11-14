# 0103. Tests bound each step on progress and reap every process they start

- Status: Proposed 2026-11-14. The user approved building the test-side redesign (about 1-2 agent-days) for the class "tests time
  out on the loaded Mac" on the night of 2026-11-14 (relayed in the designer's brief); it becomes Accepted after its one independent
  review (arranged by the coordinator) and the user's answer to D1-D4. Built on `feat/test-timing-redesign` (train 12).
- Date: 2026-11-14
- Related: ER-12 (no limit raised without a measured cause), ER-14 (no process outlives its test), ER-20, ER-21 / rule 40 (a
  recurring class gets a redesign), ER-22 (share first), ER-27, ER-28, ER-37 / rule 76, ER-41 (new checks start report-only);
  ADR 0041 (deterministic timing in tests; C71), ADR 0071 (ready or exit, never a fixed start budget; `guarded`, `Progress`),
  ADR 0019 (limits explain themselves), ADR 0100 (tests start programs through one builder; on `feat/test-env-builder`, not yet on
  a train), ADR 0102 (the machine side: the heavy priority lane, in train 11; not redone here).
- Number: 0103. On 2026-11-14 the highest ADR on `origin/main` was 0097 and on any local or `origin` branch 0102
  (`git ls-tree docs/adr/` over every branch: 0098 `docs/adr-combined-tree-before-approval`, 0099 `docs/adr-path-asserts`, 0100
  `feat/test-env-builder`, 0101 `feat/adr-0089-typed-outcomes`, 0102 `feat/heavy-priority-lane`).
- Check id: **C1xx (to assign)** by the coordinator. The code uses the provisional id `C125` (no branch or commit names C125,
  `git log --all -G 'C125\b'`, 2026-11-14), renamed when the number is assigned.

Evidence heads read (ADR 0091), re-read before the commit: `origin/main` 3a342cb42, `origin/train/2026-11-14-11` 1ecc53eee (=
the local `train/2026-11-14-11`), `feat/heavy-priority-lane` 80cf9505d, `feat/test-env-builder` 73f41d468. Shipped code: none of
the commits cited below is in a tag (`git tag --contains` of 82d5ee496, e0fdd0286 and 2680ad22f is empty); every claim about
code reads train 11's head.

## Context: the class

A test's work is a sum of steps (a program started, a fixture made, a job run, a file written), and nextest or `node --test`
bounds the **whole test** with one wall-clock limit (`crates/.config/nextest.toml` `[profile.ci]`: `slow-timeout = { period =
"30s", terminate-after = 2 }`, 60 s; C114 holds every workspace to at most 120 s in the default profile and 480 s in an override,
with a `# waiver:` line). On a quiet Mac each step is cheap and the sum fits. The macOS CI runners share the dev machine with
other agents' builds (load 30-140 is normal), so every step's cost grows several times and the sum passes the limit, although
no step is hung. Three companions make it worse: a fixed poll interval adds its whole pause to every step; a process a test
started and did not end (because an assertion failed first) keeps a lock or a CPU and fails the next tests; and a product limit
the test sets small (a 1 s phase limit) is met by a step that is only slow.

The instances, each read from the job log (`./dev ci triage <run> --no-history`, then the failed job's log through the Gitea API;
every row is a real failure: a `TMT` or a failed assertion, none from a green run):

| Run (job) | Test | What the log shows | Shape |
|---|---|---|---|
| 8054 (37645), 8060 (37722), 8066 (37873), 8090 (38111), 8099 (38404) | `paddock-dev::it host_steps::kept_clone_makes_the_clone_again_when_it_is_foreign_stale_locked_unmarked_or_too_big` | `TMT [60.0s]` twice per run (the retry too) | 24 kept-clone jobs, shell and Rust one after the other, under one 60 s bound (25.6 s quiet) |
| 8099 (38404) | `host_steps::kept_clone_each_phase_is_bounded_by_its_own_limit` | `TMT [60.0s]` twice | 20 jobs one after the other, a hung phase paying its 1 s limit twice per job, a 0.2 s nap per poll in `bounded.sh` |
| 8050 (37553), 8060 (37722), 8066 (37873) | `host_steps::kept_clone_a_phase_that_hangs_ends_in_one_warning_and_never_fails_the_job` | `FAIL` in 4.9-13.5 s: the shell and Rust differ (one made the clone within the 1 s limit, the other did not) | a product limit set to 1 s for every phase, met by a slow clone (fixed on train 9 by per-phase limits, a72dc183d) |
| 8099 (38394, VM tooling, macOS) | `vm-lock.test.mjs` / `vm-win.test.mjs`, 15 tests | `✖` at 60-105 s, then nine "held by vm-win-admin-setup (pid 41403)" | a `slot hold` (detached, its own process group) and a `vm-win.sh` left running by a test that failed partway; the leak failed the tests after it (82d5ee496) |
| 8066 (37869) | `paddock-service::it model::the_crash_model_*` (4 of 8) | `TMT [60.1s]`; the other four passed at 58.8-59.2 s | pure CPU, 5-7 s each quiet: a 9x slowdown from other jobs' load |
| 8061 (37749) | `debrief-core watch::tests::a_scan_stops_at_the_entry_limit_and_says_so` | `TMT [90.4s]` | 50 001 file creations to reach a product constant (4.8 s quiet) |
| 8061 (37749) | `debrief-app workspace_commands::tests::a_session_reopens_its_store` | `TMT [92.5s]` | three imports of a sample folder one after the other (not migrated here, see the migration list) |
| 2026-11-14 vault run2, train 10 (`gen check`, local) | `ts-debrief-wire` generator | the 1800 s hang guard ended a cold `cargo test` build | a build inside the run's guard (2680ad22f: the build is its own phase) |

Two more fixes on train 11 touched the same class: `fix/kept-clone-maintenance-lock` (a72dc183d: a limit per phase, so a slow
clone never meets the hung phase's limit) and `fix/vm-tooling-and-kept-clone-tests` (82d5ee496, e0fdd0286: the parity runs side
by side, one test per case, `bounded.sh` naps 0.05 s for a phase's first second, holders ended by their owner folder). Each
fixed its instances by hand; nothing stops the next test from summing its steps again. The lessons file has the same class five
times before (docs/lessons.md: "Tests that measure the wall clock fail under load", 2026-10-25; "A turn budget in a test wait is a
timeout in disguise", 2026-10-28; "Chromium tests compared a loaded machine with a product timeout", 2026-10-28; "Two Dyno vitest
specs that lose to a loaded runner", 2026-11-04; "Start waits kept coming back as fixed budgets", 2026-11-11).

**The pattern:** a test fails on a loaded host whenever its time bound covers a sum of steps (or a step the host can slow ten
times), its waits poll at a fixed pace, or a process it started survives it. **The invariant:** a test's verdict depends only on
whether each step made progress, independent cases of a test run side by side, and every process a test started is gone when
it ends, pass or fail.

### What exists already (ER-22) and what it does not cover

- `paddock_testkit::wait` (on main): `poll_for`, `wait_until`, `wait_for` poll with a backoff (200 µs doubling to 20 ms) under a
  hang guard (`HANG_GUARD`, 40 s); `guarded` + `Progress` + `StallGuard` (ADR 0071) run work on a thread and fail only on no
  progress. What is missing: a **same-thread wait on a condition with a no-progress bound** (the shape of "wait until the job's
  log has its last line, while its line count grows"), and nothing for Node.
- `paddock_testkit_start` (on main): `wait_ready_or_exit`, `wait_exit`, `Logged` for one started program. What is missing: ending
  **what a program started** (a detached grandchild leaves the child's tree when its parent exits, so `kill_tree` of the child no
  longer reaches it).
- `paddock_proc` (on main): `kill_tree(ProcessId)` (ends a tree by identity, the group first), `descendants_of`,
  `processes_mentioning(needle)`, `end(ProcessId)`. These are the means; nothing in the test kits drives them for a test.
- C71 (`conformance/src/checks/C71-test-timing.mjs`, on main): flags `thread::sleep` in Rust tests, short `Duration` literals in
  waits and short `{ timeout }` in TypeScript specs, with shrink-only baselines (`conformance/baselines/C71*.json`: a listed site
  that is gone is a warning, an entry the merge base lacks is an error, `reconcile`/`baselineGrowth` in
  `conformance/src/util/baseline.mjs`). It does not look at loops of steps, at Node `setTimeout` polls in `.mjs` tests, or at spawns.
- C114 (on main): every nextest workspace bounds every test; an override needs a `# waiver:` comment and at most 480 s.

## Common practice (ER-37)

| Tool or vendor | What it does | Source |
|---|---|---|
| cargo-nextest | `slow-timeout = { period, terminate-after }` per profile and per test override; on a timeout SIGTERM then SIGKILL to **the test's process group** (Unix; a job object on Windows), after a 10 s `grace-period`. Leak detection: after the test exits, stdout/stderr must close within `leak-timeout` (100 ms default), `result = "fail"` since 0.9.95 makes a leak a failure; "subprocesses whose stdout and stderr are redirected to null" escape it, which the docs call "a very difficult problem to solve". Test groups (`max-threads`) serialize tests that share a resource. | nexte.st/docs/features/slow-tests/, nexte.st/docs/features/leaky-tests/, nexte.st/docs/configuration/test-groups/ (read 2026-11-14) |
| Bazel | Tests declare a **size** (small, medium, large, enormous) mapping to timeouts of 60, 300, 900 and 3600 s; "Tests may create subprocesses, but not process groups or sessions", "the test runner may kill any stray processes. Tests should not leak processes in this fashion." | bazel.build/reference/test-encyclopedia (read 2026-11-14) |
| Go | `go test -timeout` bounds the whole test binary ("The default is 10 minutes (10m)", cmd/go); `T.Deadline` tells a test when that is; `t.Cleanup` runs in reverse order when a test ends, pass or fail. `exec.Cmd.Cancel`/`WaitDelay` bound a child, but grandchildren that keep the pipes are only cut loose, not killed. | pkg.go.dev/cmd/go, pkg.go.dev/testing, pkg.go.dev/os/exec (read 2026-11-14) |
| Playwright | `expect.poll` and `toPass` retry an assertion at intervals of 100, 250, 500, 1000 ms (a backoff) under a timeout; "Prefer auto-retrying assertions whenever possible"; `page.waitForTimeout`: "Never wait for timeout in production. Tests that wait for time are inherently flaky." | playwright.dev/docs/test-assertions, playwright.dev/docs/api/class-page (read 2026-11-14) |
| Linux / macOS | Linux has `PR_SET_CHILD_SUBREAPER`, so a runner can adopt orphans; macOS has no equivalent, so an orphaned detached process is found only by a table walk. | prctl(2); Apple has no documented subreaper API |

What the common practice adds up to: **a per-test bound, sized by a declared class; kill the test's process group on a timeout;
detect leaks that hold the test's output; poll with a backoff; clean up in a per-test hook.** It does not offer: a bound on
progress instead of total time (Bazel and nextest bound total time only), a way to end a detached process with redirected output
(nextest says so itself), or a check that finds the shape in code.

## Decision

### 1. The mechanism: shared test helpers

**Rust, `paddock_testkit` (no process code, so `paddock-proc` can keep using it):**

- `wait_progressing(what, no_progress, look)` (`crates/paddock-testkit/src/wait.rs`): `look` returns `Look::Done(value)` or
  `Look::At(level)`; the wait polls with the kit's backoff (`FIRST` 200 µs doubling to `LAST` 20 ms) and panics only when the level
  has not changed for `no_progress`, naming the level, the stall and the total. It uses the existing `StallGuard`. This is the wait
  for several steps in a row; the outer bound stays nextest's, and `no_progress` is kept well under it so a stall fails with this
  message first.
- `cases::both(a, b)` and `cases::run_cases(what, cases)` (`crates/paddock-testkit/src/cases.rs`): independent cases side by side
  (scoped threads; `run_cases` on at most `case_threads()` threads: `PADDOCK_TEST_CASE_THREADS`, else half the cores, at least two),
  every case runs even when one fails, each case's time goes to stderr (ER-27) and the panic lists every failed case with its
  message and time. A parity check takes the slower implementation, not the sum.

**Rust, `paddock_testkit_start::Reaper`** (`crates/paddock-testkit-start/src/reap.rs`; this crate already depends on `paddock-proc`):

- `Reaper::new(what)` / `Reaper::marked(what, marker)`; `spawn(&mut Command)`, `adopt(&Child)`, `adopt_pid(pid)`, `observe()`,
  `reap() -> Vec<Ended>`; `Drop` reaps too, so a test that panics partway ends its children.
- It keeps the identity (`ProcessId`: pid plus start time, rule 53) of each child, and a sampler thread remembers every descendant
  (`paddock_proc::descendants_of`) with a backoff from 10 ms to 0.5 s. One look reads the whole process table, 18 ms with 890
  processes at load 60 (measured below), so the sampler costs about 4 % of a core per live reaper. On reap it ends every child,
  every descendant it saw and, with a marker, every process whose command line names the marker (`paddock_proc::processes_mentioning`;
  a test's scratch folder is unique and named by everything started there): `kill_tree` then `end`, each by identity, then waits
  until they are gone (the kit's backoff, `GONE_WITHIN` 15 s, a bound that only stops a process the OS does not end) and panics on a
  survivor. What it had to end is printed and returned, so a test asserts that it left nothing.
- A marker must be at least `MARKER_MIN` (12) characters: a short text such as `/tmp` is in the command lines of unrelated
  processes (other agents' shells), and a reap by it ends them. This guard came from an incident while building it (below).
- This is the one place Rust tests end processes (rule 55); it calls `paddock_proc` and never `kill(2)` itself. The Node `Reaper`
  is a second implementation by language (Node tests cannot call Rust), with the same rules and tests; it is named here rather
  than in `docs/known-copies.toml`, whose copies have a delete-by tag and count only non-test source.

**Node, `scripts/test-kit.mjs`** (for `scripts/*.test.mjs` and `conformance/test`; the Node side cannot call `paddock_proc`, so
it reads `ps` and checks the same identity, pid plus `lstart`):

- `nextPause(previous)`: 1 ms doubling to 100 ms (the replaced polls' own pace as the worst case); `waitProgressing(what, done, { noProgressMs, progress })`: the same rule as
  the Rust wait.
- `Reaper` (`spawn`, `adoptPid`, `adoptIdentity`, `observe`, `stillRunning`, `reap`): the same rules and the same marker minimum;
  it kills a process group only when the process still leads it and is still the same process. One `ps` of the table blocks the
  event loop about 29 ms (863 processes), so its sampler backs off from 10 ms to 0.5 s (about 6 % of the loop).
- `scripts/vm-test-support.mjs` keeps its holder-specific `endHolders` (a holder is ended by removing its owner folder, as the lock
  means it to end), now waiting with `waitProgressing`, and adds `reapUnder(scratch)` for each file's `after`.

**Declared size class instead of a raised limit.** A test that is CPU-bound by design and has no unit a progress wait could watch
(the crash model) gets a named class in its workspace's nextest config, Bazel's practice: an override with a `# waiver:` line that
cites the measured quiet time, the measured loaded time and the run, at most the default profile's own 120 s. That is ER-12's
"named, measured cause recorded next to it", and C114 already enforces the waiver line and the 480 s cap.

**A test of a limit uses a small limit.** A test that reaches a product constant by doing the work (50 001 files) calls the code
with a smaller limit through a private seam and checks once that the public entry passes the product's constant (ADR 0019's
`limit_cases!` is the same idea for limit tables).

### 2. The check: C1xx (to assign), provisional id C125

`conformance/src/checks/C125-test-steps.mjs` (JavaScript, like C114; the Rust host port follows the conformance port plan and is
not part of this ADR). In test code only (Rust `tests/`, `tests.rs`, `*_tests.rs`, `#[cfg(test)] mod`; Node `*.test.mjs`,
`*test-support.mjs`, `conformance/test/helpers.mjs` and `lib/`), with comments and strings masked:

- **A, serial steps:** a loop whose body runs a program to completion itself (Rust `.output()`, `.status()`, `.wait_with_output()`
  in a file that builds a `Command`; Node `spawnSync`, `execFileSync`, `execSync`).
- **B, fixed poll (Node):** a `setTimeout(..., <number>)` in a loop's header or body. Rust's fixed sleeps stay C71's.
- **C, unguarded spawn:** Rust `spawn_detached(`, or `.spawn()` in a function that names no `Reaper`, `Supervised`, `wait_exit`,
  `wait_ready_or_exit`, `.kill(`, `kill_tree`, `.wait(` or `wait_with_output`; Node `detached: true` in a file that names no
  `Reaper`, `reapUnder` or `endHolders`.
- A site that is fine carries `// conformance: test-steps — <why>`. The kits themselves are skipped.

It starts **report-only** (ER-41: warnings, `mode: 'report-only'`, a `[[report_only]]` entry in `docs/gates.toml`) and blocks only
after its first green run on the real tree, recorded there by run id. On train 11's head (1ecc53eee) it lists **67 sites in 50
files** (35 A, 5 B, 27 C); it fires on the kept-clone loop that timed out (`host_steps.rs:1406` at e0fdd0286^) and on the old
`endHolders` poll. It is a heuristic: rule A also lists fixture loops of two or three git commands, which are fine and take a
waiver, and it cannot see a program run inside a helper the loop calls. After this change's migrations and two waivers, **60 sites**
remain (`node conformance/bin/paddock-conformance.mjs --app paddock --root . --only C125`). It is not baselined: the findings are
the migration list below, and the gate flips once they are migrated or waived.

### 3. The migration list

Done in this change (before and after, measured as recorded in docs/benchmarks.md, "2026-11-14, test timing redesign"):

| Test | Change | Before | After |
|---|---|---|---|
| `debrief-core watch::tests::a_scan_stops_at_the_entry_limit_and_says_so` (run 8061) | a private `scan_folder_within(.., max_entries)`; the test makes 21 files, and checks that `scan_folder` passes `MAX_SCAN_ENTRIES` | 4.85 s quiet, 90.4 s TMT in CI | 0.013 s |
| `paddock-service::it model::the_crash_model_*` (run 8066) | size class "cpu-heavy" in the ci profile, 120 s, with the measured waiver | 5-7 s quiet, 8.5 s at load 30, 59-60 s in CI | same test, a 2x margin over the measured 59 s |
| `paddock-dev::it host_steps::*` (runs 8054-8099), 37 tests | every step through a marked `Reaper` that fails the test on a leak; `same`, `run_kept_pair` and `kept_fixtures` on `paddock_testkit::both` | 15.3 s for the 37 at load 30-60 | 13.0-16.7 s at load 30-78 (noise of the shared machine; the kept-clone cases were already side by side since e0fdd0286), no leak found |
| `scripts/vm-lock.test.mjs`, `scripts/vm-win.test.mjs` (run 8099) | `endHolders` and the four 100 ms polls wait with `waitProgressing`; each file's `after` reaps its scratch folder and fails on a survivor | 98 s, 71/71 (load 25) | 98 s on the committed tree (load 11-13), 71/71, nothing reaped |

Open (each a small change through the helpers; in the order of the incidents, then by C125's count):

1. `debrief-app workspace_commands::tests::a_session_reopens_its_store` (run 8061, 92.5 s): three imports in a row; measure each
   import, then split into one test per case or wait on the import job's progress.
2. `spotter-render::real_browser a_missing_element_times_out_as_failed` (run 8060): a browser command answered later than the
   driver's per-command budget under load; the test examines an expiry, so it belongs under ADR 0071's start judge.
3. `paddock-dev::it golden::live_*` (SLOW 60-121 s in runs 8060, 8066, 8099): parity cases one after the other; `run_cases`.
4. The 60 remaining C125 sites in 46 files, largest first: `apps/debrief/app/src-tauri/src/opener.rs` (4),
   `crates/paddock-proc/src/detach.rs` (3), `crates/paddock-env/src/private/dir.rs` (3),
   `apps/pitcrew/crates/pitcrew-service/tests/supervision.rs` (3), `tools/paddock-dev/tests/it/golden.rs` (2),
   `tools/heavy/tests/it/procutil.rs` (2), then 40 files with one or two (the check's output is the list).

### 4. Alternatives

- **Raise the limits** (60 s to 180 s in the ci profile). Rejected by ER-12 without a measured cause per test, and it hides a hang
  for longer in every test of the workspace.
- **Only nextest's own tools** (leak-timeout with `result = "fail"`, test groups, one test per case): the cheaper common-practice
  option, offered as D1. It misses the leak that actually broke run 8099 (a detached `slot hold` with its output redirected, the
  case nextest's docs say it cannot see) and does nothing for Node tests.
- **A test group for CPU-heavy tests** (two at a time): measured for the crash model; it cut the slowest from 8.5 s to 6.8 s and
  tripled the wall time (8 s to 22 s), and under other jobs' load the CPU share, not the tests' own number, sets the cost. Not chosen.
- **A Linux subreaper**: not available on macOS, where the failures are.

## Parallelism and cost (ER-20)

- **More parallelism:** `both` and `run_cases` run independent cases side by side, bounded by `case_threads()` (half the cores, at
  least two) because nextest already runs one test per core: a test's cases share its slot with the tests beside it. The
  kept-clone tests went from 25.6-33.1 s quiet to 2.1-5.2 s each this way (e0fdd0286's measurement), the first instance of the
  helper. CPU-bound cases (the crash model) are not parallelised inside the test: the machine is the bound.
- **Cheaper work:** a limit tested with a small limit (50 001 files to 21: 4.85 s to 0.013 s); the binary instead of the `./dev`
  wrapper per call (82d5ee496: 0.17-0.4 s per call to 0.01 s); a build moved out of a run's guard (2680ad22f).
- **The helpers' own cost:** a reaper's look at the process table is 18 ms at load 60 with 890 processes; the sampler backs off
  to one look per 0.5 s (about 4 % of a core per live reaper), and a reap with a marker adds one table scan of about 25-30 ms. The
  waits' backoff caps at 20 ms (Rust) and 100 ms (Node), so a quick condition is seen within milliseconds and a slow one is looked
  at a few times per second.

## Cost (ER-37)

| Option | Agent-days |
|---|---|
| Common practice only (D1 b): `leak-timeout = { period = "500ms", result = "fail" }` in every nextest config, size classes as waivered overrides, one test per case by hand | 0.5 |
| This ADR's helpers, check and the migrations above (built on this branch) | 1.0 (spent) |
| The open migration list (items 1-4, about 50 files) | 1.0-1.5 |
| Total for the full redesign | 2.0-2.5, about 1.5-2 beyond the common-practice option |

The extra over common practice is the Reaper (the leak nextest cannot see, which cascaded into nine failures in run 8099), the
progress wait (nextest and Bazel bound total time only) and the check. The user approved 1-2 agent-days; the open list pushes the
total to 2.0-2.5, which D3 asks about.

## Decisions for the user

- **D1. Scope.** (a) This ADR as built: helpers, report-only check, the four migrations; the open list as follow-ups. (b) The
  common-practice option instead: nextest `leak-timeout` with `result = "fail"`, size classes and one test per case by hand;
  remove the Reaper, the Node kit and C125 (0.5 agent-day; leaks with redirected output stay unseen). Recommended: (a).
- **D2. nextest leak detection on top** (`leak-timeout = { period = "500ms", result = "fail" }` in every workspace's ci profile):
  cheap, catches children that hold the test's output, a first green run needed (ER-41). Recommended: yes, as a separate small change.
- **D3. The open migration list** (1.0-1.5 agent-days, past the approved 1-2): (a) now, in train 12; (b) as each test next fails;
  (c) when C125 turns into a gate. Recommended: (c), items 1-3 first.
- **D4. C125 to gate:** after its first green run on main and the open list migrated or waived (the `[[report_only]]` entry's
  review date, 2026-12-14).

## Incident while building it

On 2026-11-15 about 00:20-00:25 (+08), the red run of the marker-minimum test called `Reaper::marked("/tmp")` before the guard
existed (three runs). Its drop ended every process on the machine whose command line contained `/tmp`, by identity: the agent's
own shell (the run's output stopped at "Starting 1 test") and, very likely, other agents' running shell commands, whose wrappers
name `/tmp/claude-…` files. The CI runners, OrbStack and the agent sessions were still running afterwards. The test now uses a
short marker that matches no process, so it is harmless while red too, and the guard refuses short markers in both kits. Lesson
recorded (docs/lessons.md): a failing-first test must be safe in its failing state.

## Consequences

- A new test that sums steps, polls at a fixed pace or starts a process nothing ends is listed by C125 (a warning now).
- Tests that start programs take a few ms per step for the reaper; a leak fails the test that made it, with the pid and how it was
  found, instead of a later test on the runner.
- `wait_progressing` and the Node kit make "a wait for several steps" one call; a total budget over the steps has no helper.

## Rollback

Each part stands alone: remove the `[[report_only]]` entry and the check from `conformance/src/checks/index.mjs`; the helpers are
additive (no existing API changed); the migrations revert per file.
