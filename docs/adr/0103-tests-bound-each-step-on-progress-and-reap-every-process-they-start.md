# 0103. Tests bound each step on progress and reap every process they start

- Status: Accepted 2026-11-15 (user), with D1-D4 as taken (recorded under "Decisions"). Before that: proposed 2026-11-14,
  revised 2026-11-15 after the first independent review (verdict NOT YET; its P1s and P2s are answered below) and again after the
  second (one P1: the reaper's children escaped nextest's group kill; fixed below). The user approved building the test-side
  redesign (about 1-2 agent-days) for the class "tests time out on the loaded Mac" on the night of 2026-11-14 (relayed in the
  designer's brief), and the coordinator decided D1-D4 within that approval on 2026-11-15. Built on
  `feat/test-timing-redesign`, merged in train 12.
- Date: 2026-11-14 (revised 2026-11-15)
- Related: ER-12 (no limit raised without a measured cause), ER-14 (no process outlives its test), ER-20, ER-21 / rule 40 (a
  recurring class gets a redesign), ER-22 (share first), ER-27, ER-28, ER-37 / rule 76, ER-41 (new checks start report-only);
  rule 53 (act only on a process proven to be one's own; the kill guard on train 11, 0cda11713); ADR 0041 (deterministic timing
  in tests; C71), ADR 0071 (ready or exit, never a fixed start budget; `guarded`, `Progress`), ADR 0019 (limits explain
  themselves), ADR 0100 (tests start programs through one builder; on `feat/test-env-builder`, not yet on a train), ADR 0102 (the
  machine side: the heavy priority lane, in train 11; not redone here).
- Number: 0103. On 2026-11-14 the highest ADR on `origin/main` was 0097 and on any local or `origin` branch 0102
  (`git ls-tree docs/adr/` over every branch: 0098 `docs/adr-combined-tree-before-approval`, 0099 `docs/adr-path-asserts`, 0100
  `feat/test-env-builder`, 0101 `feat/adr-0089-typed-outcomes`, 0102 `feat/heavy-priority-lane`).
- Check id: **C126**, assigned by the coordinator on 2026-11-15 (C125 is the disk-path check on `fix/c125-and-arm64-tests`).

Evidence heads read (ADR 0091), re-read before the commit: `origin/main` 3a342cb42, `origin/train/2026-11-14-11` 31cdde673 (merged
into this branch), `feat/heavy-priority-lane` 80cf9505d, `feat/test-env-builder` 73f41d468. Shipped code: none of the commits
cited below is in a tag (`git tag --contains` of 82d5ee496, e0fdd0286, 2680ad22f and 737b4c841 is empty); every claim about code
reads train 11's head.

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
| 8061 (37749) | `debrief-app workspace_commands::tests::a_session_reopens_its_store` | `TMT [92.5s]` | three imports of a sample folder one after the other |
| 8060 (37731) | `spotter-render::real_browser a_missing_element_times_out_as_failed` | `FAIL [29.9s]` "the browser did not answer in time (a command)" | a fixed 10 s command deadline inside the page's 15 s budget (fixed on train 11 by 737b4c841) |
| 8060 (37722), 8066 (37873), 8099 (38404) | `paddock-dev::it golden::live_workflow_action_and_script_edits_narrow_exactly_as_affected_mjs_does` | `SLOW [80-121 s]` (passed under its 480 s waiver) | 25 edits on one copy, each a Node run, one after the other |
| 2026-11-14 vault run2, train 10 (`gen check`, local) | `ts-debrief-wire` generator | the 1800 s hang guard ended a cold `cargo test` build | a build inside the run's guard (2680ad22f: the build is its own phase) |

Earlier fixes on train 11 touched the same class: `fix/kept-clone-maintenance-lock` (a72dc183d), `fix/vm-tooling-and-kept-clone-tests`
(82d5ee496, e0fdd0286: the parity runs side by side, one test per case, `bounded.sh` naps 0.05 s for a phase's first second,
holders ended by their owner folder) and 737b4c841 (Spotter's readiness looks bounded by the page's budget). Each fixed its
instances by hand. The lessons file has the same class five times before (docs/lessons.md: "Tests that measure the wall clock fail
under load", 2026-10-25; "A turn budget in a test wait is a timeout in disguise", 2026-10-28; "Chromium tests compared a loaded
machine with a product timeout", 2026-10-28; "Two Dyno vitest specs that lose to a loaded runner", 2026-11-04; "Start waits kept
coming back as fixed budgets", 2026-11-11).

**The pattern:** a test fails on a loaded host whenever its one time bound covers a sum of steps (or a step the host can slow ten
times), its waits poll at a fixed pace, or a process it started survives it.

**The invariant, stated as far as it really holds:** every test still has one total bound, nextest's (or `node --test`'s), and
this ADR does not remove it. What it changes is what fits under it: independent cases run side by side, the work of a test does
not grow with a product constant, a CPU-bound test that has no smaller unit gets a measured size class, and waits look at a
condition with a backoff. Separately, every process a test started is gone when the test ends, pass or fail, and nothing the
test did not start is touched.

### What exists already (ER-22) and what it does not cover

- `paddock_testkit::wait` (on main): `poll_for`, `wait_until`, `wait_for` poll with a backoff (200 µs doubling to 20 ms) under a
  hang guard (`HANG_GUARD`, 40 s); `guarded` + `Progress` + `StallGuard` (ADR 0071) run work on a thread and fail only on no
  progress. Nothing for Node.
- `paddock_testkit_start` (on main): `wait_ready_or_exit`, `wait_exit`, `Logged` for one started program. Nothing ends **what a
  program started** (a detached grandchild leaves the child's tree when its parent exits, so `kill_tree` of the child no longer
  reaches it).
- `paddock_proc` (on main): `kill_tree(ProcessId)`, `descendants_of`, `end(ProcessId)`, `processes_mentioning(needle)`. The first
  version of this ADR used the last one as the reaper's marker; the review showed that a match by command line alone is not
  proof that a process is the test's (and the incident below proved it), so the rebuilt reaper never acts on a match alone.
- C71 (`conformance/src/checks/C71-test-timing.mjs`, on main): flags `thread::sleep` in Rust tests, short `Duration` literals in
  waits and short `{ timeout }` in TypeScript specs, with shrink-only baselines (`conformance/baselines/C71*.json`; a listed site
  that is gone is a warning, an entry the merge base lacks is an error, `reconcile`/`baselineGrowth` in
  `conformance/src/util/baseline.mjs`). It does not look at loops of steps, at Node polls in `.mjs` tests, or at spawns.
- C114 (on main): every nextest workspace bounds every test; an override needs a `# waiver:` comment and at most 480 s.

## Common practice (ER-37)

| Tool or vendor | What it does | Source |
|---|---|---|
| cargo-nextest | `slow-timeout = { period, terminate-after }` per profile and per test override; on a timeout SIGTERM then SIGKILL to **the test's process group** (Unix; a job object on Windows), after a 10 s `grace-period`. Leak detection: after the test exits, stdout/stderr must close within `leak-timeout` (100 ms default), `result = "fail"` since 0.9.95 makes a leak a failure; "subprocesses whose stdout and stderr are redirected to null" escape it, which the docs call "a very difficult problem to solve". Test groups (`max-threads`) serialize tests that share a resource. | nexte.st/docs/features/slow-tests/, nexte.st/docs/features/leaky-tests/, nexte.st/docs/configuration/test-groups/ (read 2026-11-14) |
| Bazel | Tests declare a **size** (small, medium, large, enormous) mapping to timeouts of 60, 300, 900 and 3600 s; "Tests may create subprocesses, but not process groups or sessions", "the test runner may kill any stray processes. Tests should not leak processes in this fashion." | bazel.build/reference/test-encyclopedia (read 2026-11-14) |
| Go | `go test -timeout` bounds the whole test binary ("The default is 10 minutes (10m)", cmd/go); `T.Deadline` tells a test when that is; `t.Cleanup` runs in reverse order when a test ends, pass or fail. `exec.Cmd.Cancel`/`WaitDelay` bound a child, but grandchildren that keep the pipes are only cut loose, not killed. | pkg.go.dev/cmd/go, pkg.go.dev/testing, pkg.go.dev/os/exec (read 2026-11-14) |
| Playwright | `expect.poll` and `toPass` retry an assertion at intervals of 100, 250, 500, 1000 ms (a backoff) under a timeout; "Prefer auto-retrying assertions whenever possible"; `page.waitForTimeout`: "Never wait for timeout in production. Tests that wait for time are inherently flaky." | playwright.dev/docs/test-assertions, playwright.dev/docs/api/class-page (read 2026-11-14) |
| Linux / macOS / Windows | Linux has `PR_SET_CHILD_SUBREAPER`, so a process can adopt the orphans of its tree; Windows has kill-on-close job objects; macOS has neither, and (measured 2026-11-15 on macOS 27) shows no other process's environment, not even the same user's (`ps -E` and sysinfo's `environ()` both come back without it). | prctl(2); Microsoft's job object docs; measurement in this ADR |

What the common practice adds up to, and how this ADR compares, honestly:

- **The total bound stays.** Bazel, nextest and Go all bound a test's total time, sized by a declared class. So does this ADR: it
  keeps nextest's bound and adds a measured size class for the one CPU-bound test family. The first version claimed a "bound on
  progress instead of total time"; that was not so. `wait_progressing` judges one wait inside a test on progress, but the test's
  total bound is unchanged, and no Rust test calls `wait_progressing` yet.
- **What fixed the incidents is common practice too:** independent cases side by side (`both`/`run_cases`; any test framework's
  parallel subtests, Go's `t.Parallel`), smaller workloads for limit tests, size classes, and backoff polls (Playwright). The kit
  makes these one call each.
- **Leaks:** nextest's leak detection and Bazel's stray-process kill cover children that keep the test's output or stay in its
  group. The leak that broke run 8099 was a detached holder with its output redirected, which both miss. The reaper covers that
  case, and only on proof (below); that is where this ADR goes beyond common practice.
- **The check:** no comparable tool lints for these shapes. It stays report-only (D4).

## Decision

### 1. The mechanism: shared test helpers

**Rust, `paddock_testkit` (no process code, so `paddock-proc` can keep using it):**

- `cases::both(a, b)` and `cases::run_cases(what, cases)` (`crates/paddock-testkit/src/cases.rs`): independent cases side by side
  (scoped threads; `run_cases` on at most `case_threads()` threads: `PADDOCK_TEST_CASE_THREADS`, else half the cores, at least two),
  every case runs even when one fails, each case's time goes to stderr (ER-27) and the panic lists every failed case with its
  message and time. A parity check takes the slower implementation, not the sum. The working fix of this ADR.
- `wait_progressing(what, no_progress, look)` (`crates/paddock-testkit/src/wait.rs`): `look` returns `Look::Done(value)` or
  `Look::At(level)`; the wait polls with the kit's backoff (`FIRST` 200 µs doubling to `LAST` 20 ms) and panics only when the level
  has not changed for `no_progress`. It is a wait inside a test, under the test's total bound; it has no Rust caller yet (the Node
  form has: the VM tests' waits). Its schedule and stall rule are tested on an injected clock and sleep, with no wall-clock bound.

**Rust, `paddock_testkit_start::Reaper`** (`crates/paddock-testkit-start/src/reap.rs`), on `paddock_proc::own`
(`crates/paddock-proc/src/own.rs`, new): it acts only on processes it can prove the test created.

- **A test helper never makes a new group or session while the test leads one** (after Bazel: "Tests may create subprocesses,
  but not process groups or sessions"). nextest starts every test in a process group of its own and, on a timeout, SIGTERMs and
  SIGKILLs that group; no drop runs then, so a child in a group of its own would survive (the run-8099 cascade). `Reaper::new(what)`
  gets a random token and, on Linux, makes the test process a child subreaper (orphans of its tree stay its children).
  `spawn(&mut Command)` starts the child with `PADDOCK_REAPER_ID=<token>` in its environment and in a kill-on-close job (Windows,
  `own::KillOnClose`); it stays in the test's group when the test leads one (`own::leads_own_group`: `getpgid(0) == getpid()`,
  true under nextest), and leads a group of its own only when the test does not (a plain `cargo test`, which kills no group). A
  test with a fake runner (`a_runner_that_ends_the_tests_group_also_ends_the_reapers_children`: this test binary started as the
  test in a group of its own, its group SIGKILLed) checks it; red with an always-new group, the child survived. `scratch_in(parent)` makes a
  folder named with the token. `observe()`, `candidates()` (what it would end, and why) and `reap()`; `Drop` reaps too, so a test
  that panics partway ends its children.
- The candidates (the pure rule `choose`, unit-tested on rows): the reaper's own children; the descendants a sampler saw below
  them (10 ms backing off to 0.5 s; a look at the table is 18 ms with 890 processes at load 60, about 4 % of a core per live
  reaper), and the groups any of them leads; then, only for a process of this user started after the reaper (start times from
  `ProcessId`, with one second of slack for Linux's whole-second boot time): one in a group the reaper made or saw made (a group
  is kept by its leader's identity and counts while that leader is still the process seen, or while no process holds the leader's
  pid: a group keeps its id while it has members), one that
  carries the token (readable on Linux only), or one whose command line names a folder the reaper made while it is an orphan or a
  child of the test process. Zombies and the test process itself never. A command-line match alone proves nothing.
- A child is ended with its tree (`kill_tree`: by identity, and its group only when it leads one the reaper made); every other candidate alone, by identity
  (`paddock_proc::end`), never a group or a pattern. On Linux an ended orphan re-parented to the test is collected
  (`own::reap_zombie`), and only one seen below a child (so it was re-parented): never a process the test started itself, whose
  exit status a `Child` may still be waiting for. `own::look` reads the identities first and the facts after (as
  `processes_mentioning` does, ADR 0033) and drops a row whose identity no longer holds once its group was read. The reaper then waits until they are gone (`GONE_WITHIN`
  15 s, a bound that only stops a process the OS does not end) and panics on a survivor.
- This is the one place Rust tests end other processes (rule 55); it calls `paddock_proc` and never `kill(2)` itself. It follows
  the rule of the kill guard on train 11 (0cda11713): act only on a process proven to be one's own.

**Node, `scripts/test-kit.mjs`**, the same rules (a second implementation by language, named here; `docs/known-copies.toml` lists
only non-test source with a delete-by tag):

- `nextPause` (1 ms doubling to 100 ms) and `waitProgressing(what, done, { noProgressMs, progress, now, sleep })`; `now` defaults
  to `performance.now()`, and progress levels compare by value (`sameLevel`: numbers, strings and NaN by value, objects by JSON).
- `processTable()` fails closed: it throws when `ps` fails or lists nothing, and on Windows (the kit's tests skip there with the
  reason); zombies are left out (`stat` Z). A reap whose gone-wait cannot read the table fails.
- `Reaper(what, { inherit })`: children by `spawn`, with the token and never detached: they stay in the test's process group
  (under `node --test` a test file's process leads no group, measured, so a new group would only move them away from a group
  kill of whatever runs the tests); a fake-runner test kills a test's group and checks the reaper's child is gone (red with
  `detached: true`, it survived); groups are kept by their leader's identity as in Rust; `scratchIn`;
  `choose` is the same rule as Rust's; each candidate is ended alone with `SIGKILL` after its start time is checked again, never
  `kill(-pgid)`. With `inherit: true` the token goes into this process's environment, so every child a test file starts carries it.
- `scripts/vm-test-support.mjs`: `endHolders` (unchanged rule: a holder ends when its owner folder goes) and `fileReaper(what,
  parent)`: a file's reaper made first, with `inherit`, and its scratch folder made by the reaper.

**Declared size class instead of a raised default.** A test that is CPU-bound by design and has no unit to run side by side (the
crash model) gets a named class in its workspace's nextest config, Bazel's practice: an override with a `# waiver:` line that
cites the measured quiet time, the measured loaded time, the run, n and what was not measured, at most the default profile's own
120 s.

**A test of a limit uses a small limit.** A test that reaches a product constant by doing the work (50 001 files) calls the code
with a smaller limit through a private seam and checks once that the public entry passes the product's constant.

**nextest's own leak detection (D2).** Every workspace's ci profile sets `leak-timeout = { period = "500ms", result = "pass" }`:
report-only (ER-41) until the first green CI run with it on main, recorded beside it by run id; then `result = "fail"`.

### 2. The check: C126

`conformance/src/checks/C126-test-steps.mjs` (JavaScript, like C114). In test code only, with comments and strings masked:

- **A, serial steps:** a loop whose body runs a program to completion itself (Rust `.output()`, `.wait_with_output()`, and a
  `.status()` whose statement is a Command chain or whose receiver was made from a Command; Node `spawnSync`, `execFileSync`,
  `execSync`). `impl Trait for Type` and `for<'a>` are not loops. **Never gated** (D4): a heuristic, it lists fixture loops of
  two or three commands too, and it cannot see a program run inside a helper the loop calls.
- **B, fixed poll (Node):** a `setTimeout(..., <any pause>)` in a loop's header or body, or an awaited sleep from
  `node:timers/promises` (under any imported name) or named `sleep`/`delay`. Rust's fixed sleeps stay C71's.
- **C, unguarded spawn:** Rust `spawn_detached(`, or a Command's `.spawn()` that is not waited for, ended or handed on by name
  (`name.wait(`, `name.kill(`, a call or match that takes `name`, a wrapper or struct field that owns it), or that is asserted on
  before that; Node `detached: true` in a file that calls no `new Reaper(`, `fileReaper(`, `reapUnder(` or `endHolders(`.
- A site that is fine carries `// conformance: test-steps — <why>` (on the site's line, the loop's line, or the line above either).

Known gaps, named in the check's header: a program run in an iterator chain (`cases.iter().map(|c| Command::new(c).output())`),
and a Node `spawn` without `detached` that is left running. It is **report-only** (ER-41: warnings, `mode: 'report-only'`, a
`[[report_only]]` entry in `docs/gates.toml`). On this branch's
tree it lists **60 sites in 41 files** (24 A, 36 C, 0 B). The review's probe cases (`impl` blocks, an HTTP `.status()`, a spawn
asserted on before its wait, a leaked second child, a timers/promises sleep, a pause held in a name, a file that names "Reaper"
without making one) are tests in `conformance/test/c126-test-steps.test.mjs`.

### 3. The migration list

Done (before and after, measured as recorded in docs/benchmarks.md, "2026-11-14, test timing redesign" and "2026-11-15, test
timing redesign, review round 1"):

| Test | Change | Before | After |
|---|---|---|---|
| `debrief-core watch::tests::a_scan_stops_at_the_entry_limit_and_says_so` (run 8061) | a private `scan_folder_within(.., max_entries)`; 21 files; `scan_folder` passes `MAX_SCAN_ENTRIES` | 4.85 s quiet, 90.4 s TMT in CI | 0.013 s |
| `paddock-service::it model::the_crash_model_*` (run 8066) | size class "cpu-heavy" in the ci profile, 120 s, with the measured waiver (n = 1; the CI runner's load not recorded) | 5-7 s quiet, 8.5 s at load 30, 59-60 s in CI | same test, a 2x margin over the measured 59 s |
| `paddock-dev::it host_steps::*` (runs 8054-8099), 37 tests | every step through a `Reaper`: wait for the step, reap, then read its pipes; `same`, `run_kept_pair`, `kept_fixtures` on `both` | 15.3 s for the 37 | 12.8-16.7 s (machine noise), no leak found |
| `paddock-dev::it golden::live_range_plans_match_affected_mjs_on_this_repositorys_first_parent_history` | the ranges side by side (`run_cases`), script and planner at once (`both`) | 85.8 s at load 27 | 33.2 s at load 29 |
| `paddock-dev::it golden::live_workflow_action_and_script_edits_narrow_exactly_as_affected_mjs_does` (SLOW 80-121 s in runs 8060, 8066, 8099) | the 25 edits split over up to four copies side by side (`run_cases`) | 21.0 s at load 27 | 12.3 s at load 29 |
| `scripts/vm-lock.test.mjs`, `scripts/vm-win.test.mjs` (run 8099) | `endHolders` and the four 100 ms polls on `waitProgressing`; each file's `fileReaper` reaps in `after` and fails on a survivor | 98 s, 71/71 | 71/71, nothing reaped (98 s on a committed tree; 138-154 s while tools/paddock-dev had uncommitted changes, which makes the tests run the `./dev` wrapper) |
| `spotter-render::real_browser a_missing_element_times_out_as_failed` (run 8060) | none needed here: 737b4c841 (train 11) bounds every readiness look by the page's budget | 29.9 s FAIL | in train 11 |
| `debrief-app workspace_commands::tests::a_session_reopens_its_store` (run 8061) | split in two tests, an import and one step each (`a_session_reopens_its_store`, `a_session_whose_store_is_gone_imports_its_files_again`) | 1.4-2.1 s quiet; 92.5 s TMT in CI, its wait log: import 29.6 s, reopen 12.4 s, import again 20.0 s | 1.0 s and 1.3 s quiet; at the CI run's per-step times about 42 s and 53 s each (an estimate from the log, under the 90 s bound); the timeout did not reproduce locally at load 19-27 |

The other C126 sites are fixed when their test next fails (D3 b).

### 4. Alternatives

- **Raise the limits** (60 s to 180 s in the ci profile). Rejected by ER-12 without a measured cause per test, and it hides a hang
  for longer in every test of the workspace.
- **Only nextest's own tools** (leak-timeout, test groups, size classes, one test per case): the cheaper common-practice option.
  It misses the leak that broke run 8099 (a detached `slot hold` with its output redirected) and does nothing for Node tests.
- **A marker by command line alone** (the first version): rejected after the review and the incident below; a match is a filter,
  never proof.
- **A test group for CPU-heavy tests** (two at a time): measured for the crash model; it cut the slowest from 8.5 s to 6.8 s and
  tripled the wall time (8 s to 22 s). Not chosen.

## Parallelism and cost (ER-20)

- **More parallelism:** `both` and `run_cases` run independent cases side by side, bounded by `case_threads()` (half the cores, at
  least two) because nextest already runs one test per core. Measured: kept-clone 25.6-33.1 s to 2.1-5.2 s per test (e0fdd0286),
  the history comparison 85.8 s to 33.2 s, the workflow edits 21.0 s to 12.3 s. CPU-bound cases (the crash model) are not
  parallelised inside the test: the machine is the bound.
- **Cheaper work:** a limit tested with a small limit (50 001 files to 21: 4.85 s to 0.013 s); the binary instead of the `./dev`
  wrapper per call (82d5ee496: 0.17-0.4 s per call to 0.01 s); a build moved out of a run's guard (2680ad22f).
- **The helpers' own cost:** a reaper's look at the table is 18 ms at load 60 with 890 processes (Rust) and about 29 ms for one
  `ps` (Node, 863 processes); both samplers back off to one look per 0.5 s. The waits' backoff caps at 20 ms (Rust) and 100 ms
  (Node).

## Cost (ER-37)

| Option | Agent-days |
|---|---|
| Common practice only: `leak-timeout` in every nextest config, size classes as waivered overrides, one test per case by hand | 0.5 |
| This ADR as built, both rounds: helpers, the rebuilt reaper, the check, the migrations above | about 2.0 (spent) |
| The other C126 sites, when their tests next fail (D3 b) | not planned |

The extra over common practice is the reaper (the leak nextest cannot see) and the check. The total stays within the user's
approved 1-2 agent-days only if the first round's day and this round's day are counted as the 2.

## Decisions (coordinator, 2026-11-15, within the user's approval)

- **D1:** (a) as modified by the review: the reaper rebuilt to act only on proof, the kits' tests without wall-clock bounds, the
  Node table failing closed, this restatement.
- **D2:** yes: nextest `leak-timeout` in every workspace's ci profile, report-only until its first green run (ER-41).
- **D3:** (b): the open incidents' tests migrated now (the debrief-app session test, the Spotter browser test, the golden live
  tests); the other C126 sites only when a test fails.
- **D4:** C126 stays report-only. Rule A is never gated; B and the ownership-based C are gated only after a green run.

## Incident while building it

On 2026-11-15 between 00:12:04 and 00:12:40 (+08), the red run of a test for the first version's marker minimum called
`Reaper::marked("/tmp")` before that guard existed (three runs). Its drop ended every process on the machine whose command line
contained `/tmp`, by identity: the agent's own shell (the run's output stopped at "Starting 1 test") and, very likely, other
agents' running shell commands, whose wrappers name `/tmp/claude-…` files. The window comes from file times: the log of the
branch's first push, last written at 00:12:04 just before the red runs, and the third run's own output file, last written at
00:12:40 when the run was ended with the shell (the first two runs wrote no file). The CI runners, OrbStack and the agent sessions
were still running afterwards. The rebuilt reaper cannot do this: a command-line match is only a filter, and the folder it matches
is one the reaper made with a random name. Its tests signal nothing but their own children, and the grandchildren they make sleep
20 s and end by themselves, so a red run leaks a sleep and ends no other process (lesson in docs/lessons.md).

## Consequences

- A new test that sums steps, polls at a fixed pace or starts a process nothing ends is listed by C126 (a warning).
- Tests that start programs through a reaper pay a few ms per step; a leak fails the test that made it, with the pid and the
  reason it was the test's.
- On macOS the token cannot be read, so a detached grandchild that left its tree before the sampler saw it, outside the reaper's
  groups and naming no reaper folder, is not found there (it is on Linux by its token, and on Windows by its job).

## Rollback

Each part stands alone: remove the `[[report_only]]` entry and the check from `conformance/src/checks/index.mjs`; the helpers are
additive (`paddock_proc::own` is a new module, no existing API changed); the migrations revert per file.
