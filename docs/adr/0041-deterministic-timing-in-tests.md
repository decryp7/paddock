# 41. Deterministic timing in tests: injected clocks, paces and seams, one check over every test, and a nightly starved run

- Status: Accepted (2026-11-03), by the user, who approved this redesign on 2026-11-03 (ER-21 redesigns, item 4)
  and decided its open points the same day (Decisions of the user), after the independent review approved it (round 1
  and a delta re-review, 2026-11-03).
- Date: 2026-11-03
- Related: ER-12 / rule 5 (no sleeps or raised timeouts; one retry at most, flaky is red), ER-14 / rule 7, ER-21 / rule 40,
  ER-24 / rule 41 (machine load), ER-28 / rule 57 and ER-29 / rule 58 (a cause confirmed by a run without it; shuffled
  nightly suites; both on branch `docs/rca-rules`, not yet on `main`), rule 48 (recorded measurements), rule 49 (specs
  never wait a count of turns), rule 50 (CI economy), rule 52 (waited work is never starvable); ADR 0031 (patience, not
  wall clocks: product waits judged on progress, C51), ADR 0038 (CI parity before push). Lessons "Tests that measure the
  wall clock fail under load" (2026-10-25), "A turn budget in a test wait is a timeout in disguise" (10-28), "A fake clock
  next to real processes must not run away from the real one" (10-28), "Chromium tests compared a loaded machine with a
  product timeout" (10-28), "A work-stealing test picked its victim by position" (10-29), "A fake server's polling
  timeout belongs after the handshake" (10-29), "A warm module made a chart draw before teardown" (11-01), "A test that
  copies the current status inherits whichever state the race left" (11-01).
- Evidence (read at `origin/main` `9da2ccf59` and on the named branches, 2026-11-03):
  - `conformance/src/checks/C30-wall-clock.mjs`: warns (never fails, except a grace assertion) on Rust `assert!`s over
    `elapsed()` and kin, on `thread::sleep` in tests that use threads, and on TypeScript `Date.now()` differences in
    `expect`. On `main` it reports **203** warnings in five apps (Debrief 152, Dyno 30, Spotter 13, Pitwall 5, Pitcrew 3;
    `paddock-conformance --app <app> --only C30`, run 2026-11-03).
  - In Rust test code (`tests/`, `benches/`, `tests.rs` files and everything after `#[cfg(test)]`), `main` has 299
    `thread::sleep` lines in 144 files, 42 `tokio::time::sleep` in 25, 516 `Instant::now()` in 183, 41 `SystemTime::now()`
    in 33, 165 `recv_timeout(` in 77 and 13 `wait_timeout` in 7; 17 lines carry `conformance: hang-guard`. In 1,366
    TypeScript spec and e2e files: 208 `setTimeout(` in 116 files, 35 `waitForTimeout(` in 21, 42 `Date.now()` in 19,
    21 `clock.install(` in 11 and only 3 `pauseAt(` (grep counts; many `Instant::now()` lines are hang-guard deadlines,
    which is why §4 ratchets rather than bans).
  - `paddock_jobs::time::{Clock, FakeClock}` (re-exported by `paddock-testkit`), `FakeClock::wait_for_waiters`;
    `paddock-testkit`'s `FakeRunner`. ADR 0031's `HostTickClock` and `StartJudge` for product waits.
  - Unmerged and needed here: `ci/flaky-is-red` (`10ab50d10`, one retry with flaky-as-failure, ER-12),
    `docs/rca-rules` (rule 58, `scripts/find-polluter.mjs`, `scripts/bisect-flaky.sh`), and the timer-globals guard
    (`bb10e52a3`, "put back timer globals after each test and fail the test that replaced one", on `steward/s23b-pitwall-1.1`).
  - The coordinator log `/Volumes/d7sst7/dev-targets/handoff/2026-10-29-overnight.md`, cited below as "log line" with its line.
  - nextest configs: `crates/`, `apps/{debrief,dyno,pitcrew,pitwall,spotter}/.config/nextest.toml`, `tools/paddock-dev/`.

## Context

About fifteen tests failed only under load in eight days, and each was fixed one at a time. Earlier ones became lessons;
the later ones are fixes on `fix/*` branches:

| Date | Instance | What made it timing-dependent | Source |
|---|---|---|---|
| 10-25 | Debrief 1.1.1: cancel within 578 ms, chat cancel under 700 ms, "grandchild gone" polled | asserts on elapsed time | lesson |
| 10-25 | Debrief: "four connections are faster than one" (30 of 30 under load) | a speed comparison as a test | lesson |
| 10-28 | paddock-ui dialog harness: 200 turns, Debrief 400 rounds | a turn budget | lesson, rule 49 |
| 10-28 | Pitcrew supervision (run 6628): fake clock 120 s jumps while real processes start | fake clock beside real processes; `sleep 3` jobs | lesson |
| 10-28 | paddock-chromium close within the 5 s grace (6662, 6726); 16 MiB page in a debug build | product grace as an assertion | lesson |
| 10-29 | paddock-download work stealing (CI 7110) | victim chosen by position, not gated | lesson |
| 10-29 | Fake DevTools servers (Spotter, paddock-chromium, Debrief): 10 ms read timeout before the handshake | a tick-sized bound | lesson |
| 10-31 | Debrief chromium-driver start, fixed 20 s (CI 7213, 7250) | a fixed wait in a copy (fix `e4476b9da`) | lesson |
| 10-31 | Dyno `counters::tests::requests_and_the_pinned_reading_run_as_live_work` (ci 7230) | a race; waived once by the user | memory `ci-economy` |
| 11-01 | Dyno disk retry reply (CI 7311) | reply built after the pass is launched; no gate. `5cc23de88`: 30/30 red with a 20 ms pause, 60/60 green gated | commit |
| 11-01 | Dyno disk cancel: a late reader of a cancelled retry | a product race found by the same test (`44c73be25`) | commit |
| 11-01 | Dyno take-back looks | the test counted looks that each cost 0.4 s of real time under a lock (`636bde03d`) | commit |
| 11-01 | debrief-monitor `run_stops_after_duration_and_writes_events` (debrief.yml 7287) | a 700 ms run that needs a second sample 250 ms after a starved first one (`70da7f3b7`, reproduced with `SIGSTOP`) | commit |
| 11-01 | Spotter slow self-test | a real 2 s child set how many 1 ms ticks passed: 1,200 needed, about 445 under load (`851573e56`) | commit |
| 11-01 | Debrief AI `tool_loop_limits_cancellation_and_bad_arguments` (debrief.yml 7348) | 1.3 s unloaded, over the 90 s budget under contention (`925a1762e`) | commit |
| 11-01 | Dyno `tables.spec.ts` keyboard specs | `page.clock.install()` does not stop time; rows held by place (`10a14b480`: 70/120 red with the cause back, 0/680 with the fix) | commit |
| 11-01 | Dyno `widgets-ui.spec.ts` chart dispose; the mock job's fixed 30 ms wait; the disk-map ghost status | module warmth under load; a fixed wait; a copied status (`7a360f66c`, `26fa0c325`) | lessons, commits |
| 11-01 | Debrief `context.spec` DropDialog live count stuck at "Counting…" (main `9da2ccf59`) | open; researcher paused | log line 489 |
| 11-01 | `pitcrew-service` `a_tool_that_hangs…` timeout on Windows (main `9da2ccf59`) | open; researcher paused (`fix/pitcrew-win-hang-test`) | log line 489 |
| 11-01 | **The CPU burner incident**: a flake researcher's 20 unbounded `sh` loops drove the load to 130 while the Mac runner ran Dyno's release tests (ci 7325: 2 FAIL, 3 TIMEOUT); the same head passed at low load (dyno-release 7326) | reproducing load by loading the shared machine | log lines 422, 426 |

The memory note `agent-concurrency.md` records an earlier one (10-29: 28 `yes` loops, load 240).

**Pattern.** A test fails under load whenever its verdict depends on how much real time, or how many host-speed ticks,
pass between two events, instead of on an order of events the test controls. Reproducing such a failure by loading the
shared machine harms everything else on it.

**Invariant.** A test's verdict is a function of its inputs and of the event order it controls. Real time appears in a
test only as a hang guard that stops a test that would never end, with a message, and never decides pass or fail.

What exists and why it is not enough: rule 5 and ER-12 forbid sleeps and raised timeouts, but say nothing about what to
use instead outside the clock; C30 warns on three shapes and is ignored at 203 warnings; ADR 0031 covers **product**
waits (C51), not tests; rule 49 covers turn counts in specs only. Half of the instances since 10-31 were none of C30's
shapes: a real child's run time setting a count, a reply racing a pass with no gate, a duration passed as input, a
Playwright clock that was never paused, a whole test over its budget under contention.

## Decision

### 1. The rule (proposed rule 60)

> **60. Tests control time; they never measure it.** A test drives every wait, interval, timeout, pace and race in the
> code under test through an injected seam: a `Clock` (`FakeClock`), a `Pace` (ticks the test releases), a `Gate` (a named
> point the code reaches and the test opens), or a scripted child or server that finishes when the test says. A test
> never asserts on elapsed time, on a count of ticks, turns or polls that host speed sets, on a product timeout or grace,
> or on the state left by whichever side of a race won; it never sleeps to let something happen and never copies the
> current state to build the next one. Real time in a test is only (a) a hang guard (at least 10 s, scaled by the run's
> starvation factor, under the runner's kill, failing with what it saw), written through the test kit, which stops a
> test that would never end and never decides a verdict; or (b) in a harness that drives a real OS (`vm-smoke`, a
> real-app smoke), a declared observation window: watching for a time, with its reason in the config, for an event that
> must **not** happen (a window closing, a second process starting), never proving that something happened in time.
> Speed is measured in benchmarks (rule 16). A test that fails only in the starved nightly run is a bug of this rule,
> fixed before the app's next tag once the hang-guard scaling has landed (D3). Load is reproduced by starving the test,
> never by loading the machine (§5). Checked by C71. *Why:* about fifteen tests failed only under load in eight days
> (ADR 0041), and a researcher's CPU burners failed a release run.

### 2. The seams in product code

Product code that waits, retries, polls, throttles, paces or races takes its time sources from one place, so a test can
replace them. They live where the clock lives today, `paddock_jobs::time`:
- `Clock` and `FakeClock` (exist), `HostTickClock` (ADR 0031);
- `Pace`, a tick source for loops that count iterations (Spotter's self-test, Dyno's looks, monitor intervals): `RealPace`
  in production (an interval on the clock), `ManualPace` in tests (a tick happens when the test calls `tick()`; the test can
  wait until the code is blocked on it, like `wait_for_waiters`);
- `Seams`, named checkpoints: `seams.reach("disk.retry.reply")`. A component that has seams takes them as a **generic
  parameter** with a default, `struct DiskCentre<S: Seams = NoSeams>`; `NoSeams` is a zero-sized type whose `reach` is an
  empty `#[inline]` function, so production code pays nothing (a `dyn Seams` would cost a virtual call and an allocation
  per component and could not compile away). In tests `TestSeams` can hold a thread at a point until released, or record the order points were reached. Dyno's
  "hold the retried pass at a gate" (`5cc23de88`) and "hold the issue read at its last moment" (`310be6d41`) become this.

Product code keeps no `#[cfg(test)]` timing branches; the seam is a constructor argument or a field with a production
default, as `Clock` is today.

### 3. The test kit

**Rust, `paddock-testkit::time`** (re-exports the above, adds):
- `wait_until(what, cond)`: polls a condition on the event the code under test publishes (or a short real-time backoff for
  external state), with the one hang guard, and on expiry panics with `what` and the last value it saw (ER-27). The
  guard is `HangGuard::DEFAULT` (40 s: over 10 s, under nextest's 60 s CI kill, C30's numbers) **times the starvation
  scale** `PADDOCK_TEST_TIME_SCALE` (1 unless set; the `starved` profile sets it, §5). This is a hang guard, not a
  verdict: no test passes or fails on it, it only stops a hung test with a message. Scaling it is therefore not the
  "raised timeout" ER-12 forbids, which is raising a limit that decides a verdict;
- `ScriptedChild` on top of `FakeRunner`: a fake process that exits, writes or hangs when the test releases it, never
  after a real `sleep N` (the Pitcrew and Spotter lessons);
- `FakeServer` handshake rules (a seconds-long handshake bound, then the polling timeout) as one helper, not per copy;
- a nextest profile `starved` in every workspace's `.config/nextest.toml` (§5) with its **own kill**: `slow-timeout = {
  period = "60s", terminate-after = 16 }` and the environment `PADDOCK_TEST_TIME_SCALE=16`. At a one-CPU quota with four
  test threads a 4 s test takes about 64 s, so the normal CI kill (60 s, `terminate-after = 2` on a 30 s period) would
  kill correct tests; the starved profile keeps kill and hang guard in the same ratio as the normal one. The scale is
  measured in WP-5 (the slowest correct test at the quota) and recorded (rule 48) before it is fixed;
- the flaky-as-failure retry of ER-12
  (`retries = 1`, `flaky-result = "fail"`, from `ci/flaky-is-red`; <https://nexte.st/docs/features/retries/>, read, not
  re-checked today).

**TypeScript, `@decryp7/paddock-ui/testing`** (Vitest) and **`@decryp7/paddock-e2e`** (Playwright):
- `useTestClock()`: fake timers on for the spec, restored after each test, and a failure when a test leaves timer globals
  replaced (`bb10e52a3`, moved here);
- `advance(ms)` that also settles Angular (`whenStable`), and `untilState(signal, predicate)` that waits on a signal or
  store without moving faked timers (unlike `vi.waitFor`, which advances them: the disk-map lesson);
- `frozenClock(page, at)`: `page.clock.install` then `pauseAt`, so time does not flow until `runFor`
  (<https://playwright.dev/docs/clock>, read in `10a14b480`, not re-checked today);
- an ESLint rule in the family preset, `paddock/no-real-time-in-specs`, the editor-time form of C71's TypeScript half.

### 4. The check: C71, tests do not depend on real time

C71 replaces C30 (C30's grace-assertion and hang-guard logic moves into it). It runs over **every** test file in
`crates/`, `apps/*`, `ui/`, `tools/` and `scripts/`: Rust test regions as counted above, and spec, test, e2e and shot files.
A harness that drives a real OS (`scripts/vm-smoke/**`, ADR 0037's real-app smoke) declares its observation windows
through the kit's `observe(window, reason)` helper (or the config's `observeSec`/`settleSec` with a `why`), which C71
accepts as rule 60 (b); any other real-time call in those harnesses is flagged like everywhere else.

Exemptions are **per call**: a waiver comment on the line or the line above, or the call taking a seam
(`FakeClock`/`ManualPace`/`TestSeams`) as an argument. A file that merely mentions a fake clock exempts nothing.

| Flagged in tests | Allowed when |
|---|---|
| Rust `thread::sleep`, `tokio::time::sleep`, `Instant::now()`, `SystemTime::now()`, `recv_timeout(`, `wait_timeout(`, `Deadline::after(` | the call is inside `paddock-testkit` (the one place that owns real time), or the line carries `// conformance: hang-guard — <why>` with a bound of at least 10 s, or that call takes a `FakeClock`/`ManualPace`/`TestSeams` |
| Rust asserts over elapsed time, product graces (C30's rules) | as in C30 |
| TS `setTimeout(`, `new Promise(r => setTimeout`, `waitForTimeout(`, `Date.now()`, `performance.now()` | inside the kit, or that call runs under `useTestClock`/`frozenClock` (the enclosing `describe` or test installs it), or a hang-guard waiver on the line |
| TS `page.clock.install(` without `pauseAt`/`setFixedTime`/`frozenClock` | never |
| TS `vi.waitFor` in a spec with fake timers on | never (use `untilState`) |
| A spread of a live status into a pushed status (`{ ...store.status(), …}`) | never (build it from scratch) |

**Ratchet, not a big bang.** `conformance/baselines/C71.json` holds today's sites as **hashes of the flagged line's
normalised text** (whitespace collapsed) with the file path, so a site is known by what it says, not by a per-file
count that a new site could hide behind after an old one is fixed. A site whose hash is in the baseline passes; any other
is an **error** from the day C71 lands, so no new site appears while the old ones are migrated. Moving a line keeps its
hash; editing it makes it new, so a touched site is migrated when it is touched. The baseline file may only lose entries
(the check fails when it gains one, as C51's waiver list may only shrink).

### 5. The starved nightly run

A run that makes the tests themselves slow, not the machine busy:

| OS | How the tests are starved | Bound |
|---|---|---|
| Linux (CI nightly.yml; locally `./dev test --starved --os linux` in a local Docker container with `--cpus=1`, the reproducible local form) | the test step runs in a cgroup with a CPU quota (`docker run --cpus=1`, or `systemd-run --scope -p CPUQuota=100%`; <https://docs.docker.com/engine/containers/resource_constraints/#cpu>, <https://docs.kernel.org/admin-guide/cgroup-v2.html>, read, not re-checked today), with nextest's `starved` profile running more test threads than the quota allows (4 per CPU) | the job's own container; nothing else on the host slows down |
| Windows (CI, nightly.yml) | a job object with a hard CPU-rate cap around the test process (`JOBOBJECT_CPU_RATE_CONTROL_INFORMATION`, <https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_cpu_rate_control_information>, read, not re-checked today), the same `starved` profile | the job's own processes |
| macOS (local, `./dev test --starved --os macos`, **advisory**) | the test process at `PRIO_DARWIN_BG` (`taskpolicy -b`, man page read on this Mac), the same profile. Measured: fixed work took 9× to 73× longer than at normal priority at load 16-26 (3.0-24.7 s for one fixed loop; [benchmarks](../benchmarks.md#2026-11-03-starving-a-test-process-by-os-priority-on-macos)). That spread is not reproducible, so a macOS starved failure is a lead to reproduce under the Linux Docker form or with a seam, not a verdict, and the tag check does not read it | the process itself; this Mac is also the macOS CI runner, so the run starts only while no CI job runs on it (`./dev` asks Gitea first) |

Vitest suites with shared state run shuffled with a printed seed in the same job (rule 58, ER-29;
<https://vitest.dev/config/#sequence-shuffle>, read, not re-checked today).

**CPU burners are the fallback only**, where none of the above is available, and only as the brief of 2026-11-01 says:
`nice -n 19`, at most 4 threads, killed by a `trap` on every exit path, started only below load 15, and never while a
release run is active. Note that `nice 19` burners barely compete with normal-priority tests under Linux's scheduler
(weight 15 against 1,024 for nice 0 in the kernel's `sched_prio_to_weight`; read, not re-checked today), which is another
reason to starve the tests instead.

**Guards.** The starved job first asks Gitea for running runs of any `*-release.yml` or tag event; if one is active it
skips with a `::notice::` and a summary line (ER-27). It runs at night after the normal nightly jobs, at the lowest runner
priority, and never on a branch (rule 50: one run per night, `main` only).

**Triage.** A test that fails only in the starved run is red in the nightly summary with the starvation mode and the
seed; it is a rule-60 bug, fixed through §2-§3 (before the next tag of its app once the hang-guard scaling has landed,
decision D3), with the failing starved run as its
reproduction (ER-11) and a run without the cause green (rule 57). No retry turns it green (ER-12).

## Checks that make the class detected

| Check | What it enumerates | Fails today on |
|---|---|---|
| **C71** (new, replaces C30) | every real-time call and wall-clock assertion in every test file, against a ratchet baseline | any new site; the baseline holds today's 203 C30 sites and the wider counts above |
| `paddock/no-real-time-in-specs` (ESLint) | the same in the editor for TypeScript | as C71 |
| The starved nightly run | every test of every workspace, starved, shuffled where state is shared | the open Debrief `context.spec` and Pitcrew Windows hang, if they are this class |
| nextest and Playwright flaky-as-failure (ER-12) | every retried test | a test green only on its retry |
| Tag checklist | once the hang-guard scaling (WP-2 and WP-5) is on `main`: the last Linux and Windows starved nightly of the app's tests has no open failure, or each is waived by the user; before that, the open failures are listed in the tag's checklist (D3). The macOS local run is advisory and not read | – |

## Amendment 2026-11-08: C71 first slice

What `conformance/src/checks/C71-test-timing.mjs` implements of §4 today, so the ADR and the check agree:

- **Flagged:** `thread::sleep` and `tokio::time::sleep` in Rust test code (the `#[cfg(test)]` items and test files, not the rest of a
  file); in TypeScript `waitForTimeout(`, a `setTimeout`/`sleep`/`delay` wait, `requestAnimationFrame` turns and a `.count()` used
  later to index (the last two are additions to the table, from the lessons of 2026-10-28 and 11-02).
- **Exemptions, per call, as the table says:** `// conformance: hang-guard — <why>` on the line or above, with a bound of at least
  10 s named in the reason; a TypeScript call under `useTestClock()` or `frozenClock(` installed in an enclosing scope before it;
  a Rust call that takes a `FakeClock`, `ManualPace` or `TestSeams`. The earlier `real-wait` waiver was not in this ADR and is gone.
- **Ratchet:** the baseline has **no expiry** (WP-6 is open-ended); the check, not only the baseline tool, errors on any entry that
  the baseline at the merge base (`origin/main`, or `PADDOCK_BASE_REF`) lacks, and says so when it cannot measure that.
- **Not yet implemented (open work):** `Instant::now()`, `SystemTime::now()`, `recv_timeout(`, `wait_timeout(`, `Deadline::after(`;
  `Date.now()`, `performance.now()`; `page.clock.install(` without `pauseAt`/`setFixedTime`/`frozenClock`; `vi.waitFor` under fake
  timers; the live-status spread; C30's elapsed asserts and graces (C30 is not yet folded in or removed); `paddock-testkit` as
  the one allowed owner (it is skipped by path); `scripts/vm-smoke/**` with `observe()`; the ESLint rule
  `paddock/no-real-time-in-specs`; and wiring C71 into the `--only` list of ci.yml. Each of these adds sites, so each lands with its own
  baseline decision by the user (the baseline may not grow without it).

## Rollout and order

1. **WP-1:** rule 60, this ADR's lesson, AGENTS.md links; land `ci/flaky-is-red` and the rule 57-58 scripts
   (`docs/rca-rules`) first, which this builds on.
2. **WP-2:** `Pace`, `Seams` in `paddock-jobs::time`; `wait_until`, `HangGuard`, `ScriptedChild`, the fake-server helper
   in `paddock-testkit`; 99 % line coverage (the crates gate).
3. **WP-3:** the TypeScript kit and the ESLint rule.
4. **WP-4:** C71 with fixtures, the ratchet baseline generated from `main`, C30 removed.
5. **WP-5:** `./dev test --starved` and the nightly Linux and Windows starved jobs with the release guard and summary.
6. **WP-6:** migrate the open instances and the files with the most sites, app by app, the release app first (ER-1):
   Dyno, then Spotter, Pitwall, Pitcrew, Debrief (152 of the 203 C30 sites). The baseline falls with each.

**Release windows (ADR 0022 G.7).** WP-2 changes `crates/` (`paddock-jobs`, `paddock-testkit`) and WP-3 changes
`ui/` (`@decryp7/paddock-ui/testing`, `@decryp7/paddock-e2e`), which every app's release builds from, so both land
**outside release windows**: after the Dyno 1.1.0 tag and between the Pitwall 1.1 and Spotter windows, or in the first
gap the coordinator names, and with at most two tooling agents while a release is in flight. WP-1, WP-4 and WP-5 touch
only docs, `conformance/` and nightly jobs that no release path runs, and may go first. WP-6 runs inside each app's own
batches, because it touches only that app's tests, and only once WP-2 and WP-3 are on `main`.

**Rollback.** C71 can go back to warnings by one line; the starved jobs are separate nightly jobs that can be switched off.

## Costs (agent-days, estimates)

| WP | Days |
|---|---|
| WP-1 rule, lesson, land the prerequisites | 0.5 |
| WP-2 Rust seams and kit | 2 |
| WP-3 TypeScript kit and ESLint rule | 1.5 |
| WP-4 C71, ratchet, C30 folded in | 1.5 |
| WP-5 starved runner and nightly jobs | 2 |
| WP-5 addition: the starvation scale, its measurement and the `starved` profile's kill | 0.25 |
| **Subtotal (mechanism)** | **7.75** |
| WP-6 migration, open instances: the two open failures (1 each) | 2 |
| WP-6 migration, the baseline: about 1,100 flagged Rust lines (299 `thread::sleep`, 516 `Instant::now()`, 165 `recv_timeout(` and the rest, from the counts above) and about 300 TypeScript lines. Most are mechanical (a sleep loop or a deadline becomes `wait_until`) at about 60 lines a day for an implementer; seams in product code need a researcher. About 23 agent-days if done at once. The ratchet makes it **open-ended by design**: sites are migrated when their file is touched or by budgeted sweeps, release app first, and nothing waits for the baseline to reach zero | (about 23, open-ended) |
| **Total committed** (mechanism plus the open instances) | **9.75** |

The migration figure is a planning estimate from the counts above, not a measurement.

## Consequences

- New timing-dependent tests cannot land; old ones fall file by file, visible in the baseline.
- Product code gains seams where it waits or races. They are small and default to production behaviour, but they are
  API: each is documented, and a seam that a test never uses is removed.
- The nightly gets one more job per OS (Linux and Windows), sized to its quota; the Mac's starved run stays local.
- A hang guard still exists in every test that waits; it is generous and only names the hang.

## Alternatives considered

- **Raise the timeouts or add retries.** Forbidden by ER-12; every earlier raise was followed by the next failure (lesson
  2026-10-25). Rejected.
- **Keep C30 and make it an error.** It covers three shapes; most instances since 10-31 were other shapes. Rejected as
  insufficient; folded into C71.
- **A global fake clock** (tokio's `time::pause`, Vitest's global fake timers). It does not cover threads, child
  processes, OS clocks or a real browser, which are where the failures were (<https://docs.rs/tokio/latest/tokio/time/fn.pause.html>,
  read, not re-checked today). Used where it fits, under the same rule.
- **Starve with CPU burners on the shared machine.** It harms CI and other agents (the incident above), and nice-19
  burners barely starve normal-priority tests. Rejected except as the bounded fallback.
- **Run the whole suite under a deterministic simulator** (loom, shuttle) everywhere. Valuable for a few lock-heavy
  modules (the store-duck sink deadlock), far too costly as the default. Kept as a per-module choice.

## Decisions of the user (2026-11-03)

The user accepted the reviewer's recommendations on 2026-11-03. The delta re-review approved the ADR with them.
- **D1:** C71 is an **error** over the ratchet baseline from the day it lands (§4).
- **D2:** the starved run is **nightly in CI on Linux and Windows**; the macOS local run is **advisory** (§5).
- **D3:** a starved-run failure **blocks the app's tag only after the hang-guard scaling has landed** (WP-2's scaled
  `HangGuard` and WP-5's `starved` profile); until then the open failures are listed in the tag checklist.
