# 82. Tests wait on events, not tick windows: a stepped pace for state machines, one until-stable settle, and C71 over counts

- Status: Proposed (2026-11-10). Draft for the user's decision (ER-21); not implemented.
- Date: 2026-11-10
- Related: ER-12 / rule 5 (no sleeps or raised timeouts; make it deterministic), rule 49 (specs never wait a fixed number
  of event-loop turns), ER-21 / rule 40, ER-29 (shuffled nightly suites), ER-33 / rule 71 (a check needs a positive
  control), rule 55 and ADR [0034](0034-one-implementation-per-mechanism.md) (copies), ADR
  [0031](0031-patience-not-wall-clocks.md) (product waits judged on progress), ADR
  [0041](0041-deterministic-timing-in-tests.md) (deterministic timing in tests: proposed rule 60, `Pace`, C71, the starved
  nightly run), ADR [0049](0049-small-merges-behind-switches-and-wire-notes.md) (release freezes), lessons "A turn budget in
  a test wait is a timeout in disguise" (2026-10-28), "Test isolation (Debrief, 2026-10-29)" and "A rule that counts sites
  needs a baseline by text, not by count" (2026-11-04).
- Amends: ADR 0041 §1 (rule 60 lands with this ADR's wording), §2 (`Pace` is built here, as the step harness's base), §3
  (the TypeScript `settle` is shared and takes no count) and §4 (C71's scope and patterns).
- Number: 0082 was free on every `origin` branch on 2026-11-10 (`git log --all -- 'docs/adr/0082*'` is empty).
- Evidence read at `origin/train/2026-11-09-3` `66b600669` (the newest integration line; C71 and its baseline live there,
  not yet on `origin/main` `b51c72509`) and at `origin/fix/dyno-7855-tests`, unless a row names another ref.

## Pattern

**A test waits for a count of something whose rate the host sets, so the count is a wall-clock window.** Whenever a test
drives code that advances on a real clock (a ticker at a test pace of 1 ms, a poll loop with a sleep, an event-loop turn)
and the test, or the code under test, decides after *N* of those steps, *N* steps is *N × (time per step)*, and the time
per step grows with load. On an idle Mac the window is wide enough; on a loaded Windows runner it closes before the event
arrives, and the test fails, or, worse, passes because the window closed (a give-up path ran instead of the one the test
meant to check).

**The invariant that should hold.** A test's verdict depends only on events the code publishes (a state, a note, a
channel message, a resolved promise, Angular's stability) or on time the test advances itself (a fake clock, a stepped
pace). A count may bound a loop only as a hang guard of at least 10 s of real time that fails with what it saw (ADR 0041
§1); it never decides the verdict, and no give-up threshold in the code under test is counted in real-time steps while
the test runs.

ADR 0041 (accepted 2026-11-03) already states this in its proposed rule 60 ("never ... a count of ticks, turns or polls
that host speed sets"). Three pieces of it are missing, and the incidents below came through each gap:

1. Rule 60 is not in `docs/rules.md` (not on `origin/main`, not on `origin/train/2026-11-09-3`).
2. The `Pace` / `ManualPace` seam (ADR 0041 §2, WP-2) was never built: no shared crate has it; Dyno's link and supervisor
   take a `Duration` pace, and tests pass 1 ms.
3. C71's first slice (on `origin/train/2026-11-09-3`) flags `sleep` and `setTimeout` waits in spec files, but not a count:
   it does not see a fixed-turn loop (`for (...; i < n; ...) await Promise.resolve()`), a `settle(fixture, 4)` call, a
   helper defined outside `*.spec.ts` (its `TS_TEST` pattern skips `src/testing/helpers.ts` and `spec-helpers.ts`, where
   the settles live), a real-time pace handed to product code in a Rust test, or a test's tuning of a tick-counted
   machine.

## Context: the incidents

| # | Where | What happened | State |
| --- | --- | --- | --- |
| 1 | Dyno link, `apps/dyno/app/src-tauri/src/link/tests.rs`, CI run 7855 (Windows) | `a_pending_registration_...` launched the app's own agent 0.83 s after connecting. The link's `Tuning` (`link/machine.rs:68`) counts ticks; tests run the real driver at `FAST = 1 ms` (`tests.rs:306`, `AgentLink::paced`, `link/mod.rs:308`). The idle fallback, two rechecks of five ticks (about 10 ms), raced the service agent's start; on the Windows VM under I/O load, 60 looks took over 3 s, past `pending_max` (3000 ticks = 3 s at the test pace): 2 of 10. | Fixed on `origin/fix/dyno-7855-tests` (3c15d2874): waits on the agent's start signal; the give-up is set to "never" in that test and covered by table tests. |
| 2 | Dyno link, CI run 7854 | `a_stale_discovery_file_beside_a_slow_lock_holder_...` counted 100 looks; each probing look costs a paced re-check of the held lock (about 0.4 s): 27 s on an idle Mac against the 40 s hang guard, 77 looks on the loaded runner. | Fixed (3c15d2874): waits for the `OtherAgentHolds` note, then a few looks: 2 s (from the commit message; not a recorded benchmark). |
| 3 | Dyno link, review of #1 | `with_the_service_supervising_the_app_never_starts_an_agent_of_its_own`: idle fallback at 4 rechecks of 50 ticks, about 200 ms at the test pace, raced the service agent. | Fixed (97b746958): tuning never falls back; new table test over 200 stepped ticks (`link/machine/table.rs`). |
| 4 | Debrief `folder-setup-wizard.spec.ts` | Failed 1 of 3 full runs at load 14-25: the wizard opens through `openLazy`; four `setTimeout(0)` rounds of `settle()` did not cover the lazy import. | Fixed with `settleUntil` (7086bbe53) in it and 13 more specs of the same shape. |
| 5 | Debrief `watched-folders.spec.ts`, `outliers-view.spec.ts`, CI run 7825 | A timed-out body resumed in `settle()` and called `TestBed.tick()` on the next file's TestBed (`isolate: false`). | Guarded (24c8d62bf): `settle()` throws once its test ended. |
| 6 | Debrief `import-range.spec.ts` (lessons, 2026-10-29) | `settle(undefined, 2)` before a lazily loaded dialog. | Fixed with `openDialog()`; rule 49 (a). |
| 7 | paddock-ui boot-every-dialog harness and Debrief's dialog spec (lessons, 2026-10-28) | 200 macrotask turns and 400 render rounds to leave Loading; a slow first `import()` on CI failed both. | Fixed per spec. |

Each Debrief full run of the last two weeks surfaced another spec of the shape of #4 to #6 (the coordinator's report for
this ADR; the three fixes above are the ones in history). The Dyno fixes are correct per test, but the next test that
tunes the link, or the next app with a ticked machine, can open the same window: nothing makes the wrong form fail.

## Instances (counts, `origin/train/2026-11-09-3` `66b600669`)

**Method.** TypeScript: `git grep -F 'settle('` over `*.spec.ts`; imports of `settle` from the app's test helpers; calls
that pass a number (`settle(x, 4)`, `settle(8)`); helper definitions named `settle`, `settleAll`, `settled`, `flush`,
`flushAll`, `tick*`, `turn(s)` in any `.ts` (excluding the `Loadable` domain `settle` of Pitcrew and Spotter); and the
fixed-turn idioms `await Promise.resolve()`, `setTimeout(r, 0)` and `for (let i = 0; i < N; i++) await` in specs. Rust: a
scan of test code (`tests/`, `benches/`, `*tests.rs`, and everything after `#[cfg(test)]`) for a `for _ in 0..N` whose next
seven lines sleep (a counted poll), a real-time pace (`paced(`, `with_pace(`, `pace: Duration::from_millis`, a
`const FAST|TICK|PACE: Duration`), and `recv_timeout(Duration::from_millis(n))` with n under 10 s; product code for
tick-counted machines (`ticks_per_second`, `TICKS_PER_SECOND`, `max_looks`, `spawn_ticker`). Scripts are in the scratch
folder, not the repo; WP-0 re-runs them as the check.

### TypeScript

| Area | Spec files | Calling `settle(` | Importing the app's `settle` | `settle(` calls | Calls passing a count | `settle`/`flush`/`tick` helper definitions | `await Promise.resolve()` / `setTimeout(r, 0)` / turn loops in specs | Specs on fake timers |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Debrief | 454 | 229 | 228 | 2,806 | 503 | 12 | 34 / 5 / 9 | 46 |
| Dyno | 199 | 27 | 7 | 574 | 4 | 24 | 9 / 1 / 1 | 26 |
| Pitcrew | 66 | 19 | 19 | 163 | 112 | 2 | 10 / 0 / 0 | 11 |
| Pitwall | 91 | 33 | 24 | 275 | 3 | 12 | 25 / 13 / 10 | 10 |
| Spotter | 68 | 23 | 21 | 169 | 23 | 3 | 0 / 0 / 1 | 7 |
| `ui/` (paddock-ui, paddock-e2e) | 270 | 69 | 36 | 1,694 | 20 | 44 | 60 / 17 / 51 | 22 |
| **Total** | **1,148** | **400** | **335** | **5,681** | **665** | **97** (6 in e2e and scripts) | **138 / 36 / 72** | **122** |

On `origin/main` `b51c72509` Debrief has 234 spec files calling `settle(` (232 importing it) and 2,841 calls. Debrief's
`settle(fixture, rounds = 4)` (`app/src/testing/helpers.ts:182`) runs `rounds` × (`Promise.resolve`, `setTimeout(0)`,
`TestBed.tick`, `whenStable`): a fixed number of real macrotask turns. Pitcrew's (`testing/test-helpers.ts:46`), Spotter's
(`:49`) and Pitwall's (`testing/render.ts:41`) have the same `rounds = 4` shape; the shell's
(`shell/testing/src/shell-testing.ts:207`) runs 8 microtask rounds around `whenStable`; 16 `ui/shell` specs define
their own `settle(rounds = 6 to 12)`. Dyno's (`app/src/testing/settle.ts:34`, 8a3741eb4) is the model this ADR keeps: on a fake
clock, it advances frames until `whenStable` resolves and **throws** if Angular is not stable after a bound, so its count
is a hang guard, not a verdict. C71's baseline (`conformance/baselines/C71.json`) holds 563 sites (Debrief 345, Dyno 68,
Pitwall 45, Spotter 44, `ui` 43, Pitcrew 13, `crates` 5; 294 TypeScript, 269 Rust) and **none** of the helper files above.

### Rust

| Area | Counted polls (`for _ in 0..N` + sleep) | Real-time pace given to product code in tests | Short `recv_timeout` (< 10 s) |
| --- | --- | --- | --- |
| Debrief | 13 in 13 files | 8 in 1 file (`debrief-ai` `browser/research.rs`, on a `FakeClock`: fine) | 4 in 3 files |
| Dyno | 11 in 7 files | 6 in 2 files (`link/tests.rs` `FAST`, `link/mod.rs`; plus `supervise/mod.rs:300` `spawn_ticker(.., 1 ms, ..)`) | 5 in 4 files |
| Pitcrew | 2 in 1 file | 0 | 2 in 2 files |
| Pitwall | 3 in 2 files | 1 (`updates.rs` `with_pace(Duration::ZERO)`: a pause, no count decides: fine) | 0 |
| Spotter | 2 in 2 files | 1 (`spotter-render` `page.rs:753`, a test `TICK` of 10 ms; WP-0 reads what counts it) | 0 |
| `tools/` | 1 | 0 | 0 |
| **Total** | **32 in 26 files** | **16 in 5 files (2 fine)** | **11 in 9 files** |

Tick-counted product machines: Dyno's link (`link/machine.rs` `Tuning`: `note_after`, `service_recheck`, `idle_rechecks`,
`pending_max`, `connected_for`, `cpu_every`, `lock_every`, all in ticks) driven by `supervise::spawn_ticker`; Debrief's
browser readiness (`max_looks()` = `LOOK_FOR / LOOK_EVERY`, 60 looks). The counted polls already sit in C71's baseline
through their sleeps; what C71 cannot see is that the *count*, not the sleep, decides.

## Decision

### 1. Rule 60 lands, with one sentence added (amends ADR 0041 §1)

ADR 0041's rule 60 text goes into `docs/rules.md` as written, plus: *"A count of steps that run on real time (ticks of a
real pace, polls, event-loop turns, render rounds) is a duration: in a test it may only bound a loop as a hang guard that
fails with what it saw; it never decides, and a test never runs a tick-counted machine at a real pace with a finite
give-up."*

### 2. Rust: a stepped pace for state machines

Built in `paddock_jobs::time` (ADR 0041 §2's `Pace`, which this ADR builds rather than restates):

```text
pub trait Pace: Send + Sync { fn next(&self, cancel: &CancelToken) -> Tick; }  // blocks until the next tick
pub struct RealPace { every: Duration, clock: Arc<dyn Clock> }                 // production
pub struct SteppedPace { .. }                                                  // tests: a tick happens only on step()
impl SteppedPace {
    pub fn step(&self);                          // releases one tick, then waits (hang guard) until the machine has
                                                 // taken it and is blocked on the next: one step = one look, at any load
    pub fn step_until(&self, what: &str, cond: impl FnMut() -> bool);  // steps one at a time until cond, with the
                                                 // hang guard; the count of steps taken is returned, never asserted
                                                 // against a window
}
```

- A machine that counts ticks (Dyno's link and supervisor, any future one) takes `impl Pace`, never a `Duration`; so a
  test cannot hand it a real pace without writing `RealPace::every(..)`, which C71 flags in test code (§4).
- **Two kinds of tests, each with one tool.** A *table test* drives the pure transition (`machine::table`, already how
  Dyno tests its give-ups: 200 stepped ticks in `table.rs`) and may assert counts, because the counts are steps, not time.
  An *integration test* runs the real driver with real threads or child processes on `SteppedPace`, and waits on the events
  the machine publishes (notes, launch records, the agent's start signal) before each `step`. When an integration test
  must race a real OS process it cannot step (a launchd job, a real agent start), its tuning sets every give-up to
  "never" (`Tuning::patient()`, as 3c15d2874 and 97b746958 did by hand), and the give-up is proved by a table test.
- Product code keeps no `cfg(test)` branches; the pace is a constructor argument with the production default (ADR 0041
  §2).

### 3. TypeScript: one until-stable `settle`, with no count (amends ADR 0041 §3)

`@decryp7/paddock-ui/testing` exports the only `settle`:

```text
settle(target?: ComponentFixture | 'app'): Promise<void>
  // loop: flush microtasks; TestBed.tick(); await whenStable() (Angular's pending tasks);
  //       under fake timers advance one frame at a time until whenStable resolves (Dyno's model);
  // until a pass changes nothing; throws after the hang guard with the pending task names and the test's name;
  // throws if its test has ended (Debrief's test-lifetime guard, 24c8d62bf, moved here)
settleUntil(target, condition, what): Promise<void>   // settle, then check; repeat until it holds (hang guard)
untilState(signal, predicate, what): Promise<void>   // ADR 0041 §3, unchanged
```

- **No `rounds` parameter.** A count cannot be passed, so the 665 calls that pass one fail to compile once their app
  adopts the shared helper; most become `settle(f)`, the rest `settleUntil` (a lazily loaded chunk, a stub's resolver
  captured late: the 7086bbe53 shape).
- The 97 local helper definitions (`settle`, `settleAll`, `flush`, `tick`, turn loops) go, app by app; each is registered
  in `docs/known-copies.toml` with its app's next tag as `delete_by` until it does (rule 55).
- `whenStable` covers pending tasks that Angular knows of (HTTP, `resource`, `PendingTasks`); a mock backend or a lazy
  import that Angular does not track registers itself with `PendingTasks` in the shared mock helpers, so stability means
  "the work started is done", not "N turns passed".

### 4. The check: C71 sees counts (amends ADR 0041 §4)

C71 (on `origin/train/2026-11-09-3`) gains, with the same baseline-by-text ratchet (lesson 2026-11-04):

| New C71 finding | Scope |
| --- | --- |
| a fixed-turn loop: `for` over a constant or a `rounds` parameter whose body awaits `Promise.resolve()`, `setTimeout(.., 0)`, `TestBed.tick()`, `whenStable()`, `detectChanges()` or `requestAnimationFrame` | specs **and test-support files**: `TS_TEST` widened to `src/testing/**`, `**/testing/**`, `*-helpers.ts`, `*.harness.ts` next to specs |
| a call of `settle(..)` (any import) with a numeric or `rounds` argument | specs and test support |
| a definition of `settle*`, `flush*`, `tick*` outside `@decryp7/paddock-ui/testing` | specs and test support |
| two or more consecutive `await Promise.resolve()` (a turn budget written out) | specs and test support |
| Rust: a real pace given to product code in test code: `RealPace::every(`, `spawn_ticker(` / `paced(` / `with_pace(` with a non-zero `Duration` (until those take `impl Pace`) | Rust test code |
| Rust: a counted poll, `for _ in 0..N` whose body sleeps or receives with a timeout and whose loop end is not a panic naming what it saw | Rust test code |
| Rust: `recv_timeout(< 10 s)` whose `Err` is the asserted outcome ("nothing came"): an absence window, allowed only as `// conformance: observation-window — <why>` (ADR 0041 §1 (b)) | Rust test code |

- **Exemptions** stay per call: a hang guard of at least 10 s with `// conformance: hang-guard — <why>`; a call that takes
  `SteppedPace`, `FakeClock` or `TestSeams`.
- **ESLint** `paddock/no-turn-count` in the family preset gives the TypeScript half in the editor (ADR 0041's
  `paddock/no-real-time-in-specs` grows this rule; one rule, not two).
- **Positive controls (ER-33).** Fixtures that must fail for each row; and a control spec in `paddock-ui/testing` that
  models the slow case (a lazy chunk that resolves after 25 macrotask turns, the lesson of 2026-10-29): the old
  four-round `settle` fails it every time, the shared one passes.

### 5. Ratchet and migration per app

- New sites fail at once (the C71 rule); the baseline only shrinks.
- Each app's migration is mechanical first (a codemod: imports to `@decryp7/paddock-ui/testing`, numeric arguments
  dropped), then a triage of the specs that turn red because they relied on a window: each becomes `settleUntil` on the
  condition it meant, or exposes a real bug that gets its own failing test (rule 5).
- **Order relative to releases (rule 1, ADR 0049 freezes).** The shared pieces (`paddock-jobs` `Pace`, `paddock-ui/testing`)
  land in the first shared window outside a release freeze; the C71 extension (conformance only, no release path) may land
  first, with every current site in the baseline. Then, inside each app's own batches:
  1. **Dyno** (its link and supervisor are the only ticked machines run at a real pace; the 7855 fixes are already
     shaped this way): onto `SteppedPace` and the shared `settle`, before its next tag.
  2. **Pitwall 1.1** (24 files, 3 counted calls, 12 helpers) and **Spotter** (21 files, 23 counted calls) before their
     tags: small.
  3. **Pitcrew 1.1** (19 files, 112 counted calls).
  4. **Debrief 1.2** (228 files, 503 counted calls, 12 helpers): in the 1.2 foundation, by area, after the codemod;
     its tag is not blocked by baseline entries that never flaked (D6), only by the specs that did.
  5. `ui/` shell specs (44 helpers) with the shared helper's own landing (rule 43: a shared change and its adoption land
     together).

### 6. Parallelism and cost (rule 51)

- **Test time goes down, not up.** An until-stable `settle` returns when the work is done, often in fewer turns than four
  fixed `setTimeout(0)` rounds; a stepped pace removes the 1 ms sleeps between looks and the paced lock re-checks that made
  one Dyno test take 27 s (incident 2, from the commit message; to be recorded in `apps/dyno/docs/performance.md` before it
  is quoted elsewhere, rule 48). WP-0 measures the Debrief unit suite before and after the codemod on one area and records
  it.
- **Migration parallelism.** The app migrations touch disjoint files (each app's specs and its testing folder): up to one
  implementer per app, bounded by machine load (rule 41), after the shared helper lands. The codemod is one script reused by
  every app.
- **CI.** No new job: C71 runs in the conformance job in seconds; ESLint in the existing lint step.

## Work packages (raw estimates, one agent)

| WP | Content | Days |
| --- | --- | --- |
| WP-0 | Re-run the survey as C71 fixtures, regenerate the baseline with the new rows, measure one Debrief area before the codemod | 0.5 |
| WP-1 | `Pace`, `RealPace`, `SteppedPace` (`step`, `step_until`, waiting for the taker) in `paddock_jobs::time`; 99 % coverage | 1.5 |
| WP-2 | Dyno link and supervisor take `impl Pace`; integration tests on `SteppedPace` with `Tuning::patient()` where a real process races; Debrief readiness reviewed | 1.5 |
| WP-3 | `@decryp7/paddock-ui/testing`: `settle`, `settleUntil`, `untilState`, the test-lifetime guard, `PendingTasks` in the mock helpers, the control spec; `ui/` adopts it (44 helpers) | 2 |
| WP-4 | C71 extension (seven rows), the ESLint rule, fixtures and positive controls | 2 |
| WP-5 | Pitwall (1), Spotter (0.75), Pitcrew (1), Dyno TypeScript (24 helpers, 1) | 3.75 |
| WP-6 | Debrief: codemod over 228 files and triage of the red specs (the 7086bbe53 batch of 14 specs took one commit; budget for about 50 such specs) | 4 |
| WP-7 | Rust: 32 counted polls and 11 absence windows in 26 and 9 files | 2 |
| WP-8 | Rule 60 text, lesson, ADR 0041 amendment notes, README row | 0.25 |
| | **Mechanism (WP-0 to WP-4, WP-8)** | **7.75** |
| | **Migration (WP-5 to WP-7)** | **9.75** |
| | **Total** | **17.5** |

The migration figures are planning estimates from the counts above, not measurements.

## Decisions for the user

| # | Question | Options | Recommendation |
| --- | --- | --- | --- |
| D1 | Form | (a) amend ADR 0041 and land its rule 60 now with the added sentence; (b) a separate rule | **(a)**: 0041 already decided the principle; this closes its three gaps |
| D2 | Ticked machines in integration tests | (a) `SteppedPace` where the driver is ours, `Tuning::patient()` plus table tests where a real process races; (b) patient tuning only; (c) a real pace with larger counts | **(a)**; (c) is the raised timeout ER-12 forbids |
| D3 | TypeScript `settle` | (a) one shared until-stable `settle` with no count in `@decryp7/paddock-ui/testing`, app copies deleted; (b) per-app helpers, each made until-stable | **(a)**: 97 copies of one mechanism (rule 55) |
| D4 | Pending work Angular cannot see (mock latencies, lazy chunks) | (a) the shared mocks register with `PendingTasks`; (b) every such spec uses `settleUntil` | **(a)** for the shared mocks, (b) for app-specific stubs |
| D5 | Check | (a) extend C71 and its baseline, one ESLint rule; (b) a new check number | **(a)**: one ratchet over test timing |
| D6 | Release gating | (a) a tag is blocked only by specs that flaked in the last 30 days and by new sites; baseline entries burn down in each app's batches; (b) every app at zero before its next tag | **(a)** for Debrief 1.2 (503 counted calls); **(b)** for Pitwall 1.1 and Spotter, where it is about a day |
| D7 | Absence windows (`recv_timeout(short).is_err()`) | (a) replace with an event (a sequence number, a closed channel) where one exists, else the declared observation window; (b) leave in the baseline | **(a)** |

## Consequences

- A tick-counted machine cannot be run at a real pace in a test without a flagged line; a spec cannot pass a turn count.
- Specs that passed because a window happened to be wide enough will fail during migration; each such failure is either a
  missing condition in the test or a real bug, and both are wanted.
- The shared `settle` becomes a dependency of every app's specs (one owner: `ui/`, harness register of ADR 0042).
- The full Debrief unit run should stop surfacing new latent flakes of this shape; the nightly shuffled and starved runs
  (ER-29, ADR 0041 §5) are the measure.

## Alternatives rejected

- **Raise the counts or the test pace** (5 ms instead of 1 ms, 20 rounds instead of 4): moves the window (ER-12).
- **Per-test fixes only** (the state after 3c15d2874 and 7086bbe53): correct, and the next test opens the window again;
  nothing fails on the wrong form.
- **Fake clock everywhere**: right for timers, but Dyno's link races real child processes and file locks that a fake clock
  does not move (lesson 2026-10-28 "A fake clock next to real processes must not run away from the real one"); the stepped
  pace counts looks without pretending time moved.
- **Retry flaky specs**: hides the class (ER-12; flaky stays red).

## The failing tests the changes start with (rule 5 / ER-11; not written in this change)

- `paddock-ui/testing`: `settle_waits_for_a_lazy_chunk_that_takes_25_turns` (fails with any fixed-round settle).
- `paddock_jobs::time`: `a_stepped_pace_gives_one_look_per_step_under_a_starved_machine` (the machine's look count equals
  the steps taken, with the taker slowed by a seam).
- Dyno link: `a_pending_registration_is_not_given_up_while_the_test_steps_slower_than_the_pace` (driver on `SteppedPace`,
  the service agent's start delayed by a gate; fails today with a 1 ms real pace and `pending_max` 60).
- C71 fixtures: `settle(f, 4)`, a turn loop in `src/testing/helpers.ts`, `AgentLink::paced(.., Duration::from_millis(1))`
  in a test, a counted poll without a naming panic: each must be reported.

## Rollback

The new C71 rows can go back to warnings by one line each; the ESLint rule can be switched off in the preset. The shared
`settle` and `Pace` stay (they are improvements on their own); an app whose migration stalls keeps its baseline entries.
