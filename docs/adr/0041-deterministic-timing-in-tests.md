# 41. Deterministic timing in tests: injected clocks, paces and seams, one check over every test, and a nightly starved run

- Status: Proposed (2026-11-03). The user approved drafting it on 2026-11-03 (ER-21 redesigns, item 4). No work starts
  before it is accepted.
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
> current state to build the next one. Real time in a test is only a hang guard (at least 10 s, under the runner's kill,
> failing with what it saw), written through the test kit. Speed is measured in benchmarks (rule 16). A test that fails
> only in the starved nightly run is a bug of this rule, fixed before the app's next tag. Load is reproduced by starving
> the test, never by loading the machine (§5). Checked by C71. *Why:* about fifteen tests failed only under load in eight
> days (ADR 0041), and a researcher's CPU burners failed a release run.

### 2. The seams in product code

Product code that waits, retries, polls, throttles, paces or races takes its time sources from one place, so a test can
replace them. They live where the clock lives today, `paddock_jobs::time`:
- `Clock` and `FakeClock` (exist), `HostTickClock` (ADR 0031);
- `Pace`, a tick source for loops that count iterations (Spotter's self-test, Dyno's looks, monitor intervals): `RealPace`
  in production (an interval on the clock), `ManualPace` in tests (a tick happens when the test calls `tick()`; the test can
  wait until the code is blocked on it, like `wait_for_waiters`);
- `Seams`, named checkpoints: `seams.reach("disk.retry.reply")`. In production it is a zero-sized no-op that compiles away;
  in tests `TestSeams` can hold a thread at a point until released, or record the order points were reached. Dyno's
  "hold the retried pass at a gate" (`5cc23de88`) and "hold the issue read at its last moment" (`310be6d41`) become this.

Product code keeps no `#[cfg(test)]` timing branches; the seam is a constructor argument or a field with a production
default, as `Clock` is today.

### 3. The test kit

**Rust, `paddock-testkit::time`** (re-exports the above, adds):
- `wait_until(what, cond)`: polls a condition on the event the code under test publishes (or a short real-time backoff for
  external state), with the one hang guard (`HangGuard::DEFAULT`, 40 s: over 10 s, under nextest's 60 s CI kill; C30's
  numbers), and on expiry panics with `what` and the last value it saw (ER-27);
- `ScriptedChild` on top of `FakeRunner`: a fake process that exits, writes or hangs when the test releases it, never
  after a real `sleep N` (the Pitcrew and Spotter lessons);
- `FakeServer` handshake rules (a seconds-long handshake bound, then the polling timeout) as one helper, not per copy;
- a nextest profile `starved` in every workspace's `.config/nextest.toml` (§5) and the flaky-as-failure retry of ER-12
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

| Flagged in tests | Allowed when |
|---|---|
| Rust `thread::sleep`, `tokio::time::sleep`, `Instant::now()`, `SystemTime::now()`, `recv_timeout(`, `wait_timeout(`, `Deadline::after(` | the call is inside `paddock-testkit` (the one place that owns real time), or the line carries `// conformance: hang-guard — <why>` with a bound of at least 10 s, or the file passes a `FakeClock`/`ManualPace`/`TestSeams` to the call that uses it |
| Rust asserts over elapsed time, product graces (C30's rules) | as in C30 |
| TS `setTimeout(`, `new Promise(r => setTimeout`, `waitForTimeout(`, `Date.now()`, `performance.now()` | inside the kit, or the spec uses `useTestClock`/`frozenClock`, or a hang-guard waiver |
| TS `page.clock.install(` without `pauseAt`/`setFixedTime`/`frozenClock` | never |
| TS `vi.waitFor` in a spec with fake timers on | never (use `untilState`) |
| A spread of a live status into a pushed status (`{ ...store.status(), …}`) | never (build it from scratch) |

**Ratchet, not a big bang.** `conformance/baselines/C71.json` holds today's count per file. A file's count may only
fall; a new file starts at zero; C71 is an **error** for anything over the baseline from the day it lands, so no new site
appears while the old ones are migrated. The baseline file changes only downwards (the check fails on an increase of the
baseline itself, as C51's waiver list may only shrink).

### 5. The starved nightly run

A run that makes the tests themselves slow, not the machine busy:

| OS | How the tests are starved | Bound |
|---|---|---|
| Linux (CI, nightly.yml) | the test step runs in a cgroup with a CPU quota (`docker run --cpus=1`, or `systemd-run --scope -p CPUQuota=100%`; <https://docs.docker.com/engine/containers/resource_constraints/#cpu>, <https://docs.kernel.org/admin-guide/cgroup-v2.html>, read, not re-checked today), with nextest's `starved` profile running more test threads than the quota allows (4 per CPU) | the job's own container; nothing else on the host slows down |
| Windows (CI, nightly.yml) | a job object with a hard CPU-rate cap around the test process (`JOBOBJECT_CPU_RATE_CONTROL_INFORMATION`, <https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_cpu_rate_control_information>, read, not re-checked today), the same `starved` profile | the job's own processes |
| macOS (local, `./dev test --starved`, before a tag) | the test process at `PRIO_DARWIN_BG` (`taskpolicy -b`, man page read on this Mac), the same profile. Measured: fixed work took 9× to 73× longer than at normal priority at load 16-26 ([benchmarks](../benchmarks.md#2026-11-03-starving-a-test-process-by-os-priority-on-macos)); the variance is the point, it is a loaded runner | the process itself; never on the CI Mac, which is this machine |

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
seed; it is a rule-60 bug, fixed before the next tag of its app, through §2-§3, with the failing starved run as its
reproduction (ER-11) and a run without the cause green (rule 57). No retry turns it green (ER-12).

## Checks that make the class detected

| Check | What it enumerates | Fails today on |
|---|---|---|
| **C71** (new, replaces C30) | every real-time call and wall-clock assertion in every test file, against a ratchet baseline | any new site; the baseline holds today's 203 C30 sites and the wider counts above |
| `paddock/no-real-time-in-specs` (ESLint) | the same in the editor for TypeScript | as C71 |
| The starved nightly run | every test of every workspace, starved, shuffled where state is shared | the open Debrief `context.spec` and Pitcrew Windows hang, if they are this class |
| nextest and Playwright flaky-as-failure (ER-12) | every retried test | a test green only on its retry |
| Tag checklist | the last starved nightly of the app's tests has no open failure, or each is waived by the user | – |

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

WP-1 to WP-5 are about 7 agent-days and come first. WP-6 runs alongside the release work, inside each app's own batches,
because it touches only that app's tests.

**Rollback.** C71 can go back to warnings by one line; the starved jobs are separate nightly jobs that can be switched off.

## Costs (agent-days, estimates)

| WP | Days |
|---|---|
| WP-1 rule, lesson, land the prerequisites | 0.5 |
| WP-2 Rust seams and kit | 2 |
| WP-3 TypeScript kit and ESLint rule | 1.5 |
| WP-4 C71, ratchet, C30 folded in | 1.5 |
| WP-5 starved runner and nightly jobs | 2 |
| **Subtotal (mechanism)** | **7.5** |
| WP-6 migration: the two open instances (1 each) and about 300 flagged sites, mostly mechanical (`thread::sleep` loops → `wait_until`) at about 40 per day for an implementer, with harder seams by a researcher | 2 + 7.5 |
| **Total** | **17** |

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

## Decisions for the user

- **D1:** C71 as an error over a ratchet baseline from day one (recommended), or a warning for a release first?
- **D2:** the starved run in CI nightly on Linux and Windows plus locally on macOS before a tag (recommended), or local
  only on all three OSes (no CI cost, but it depends on someone running it)?
- **D3:** does an open starved-run failure block the app's tag (recommended), or only get listed in the release notes?
