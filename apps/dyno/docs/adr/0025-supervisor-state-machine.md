# 0025. The agent supervisor is a pure state machine with one owner

- Status: Accepted (2026-10-30, by the user: "do it now, before the Dyno 1.0 tag"; the generic mechanism moves to `paddock-proc` after the tag, `CONNECT_GRACE` goes, the 3 s end grace stays)
- Date: 2026-10-30
- Related: ER-21 / Paddock rule 40 (recurring bugs mean a missing design rule), ER-11 (a failing test first), ER-12 (no
  sleeps), ER-19 (the UI never times out a running job), ER-20 / rule 51 (parallelism and cost), rule 42 (share first),
  rule 53 (identity, not reusable names); family ADR 0031 (patience, not wall clocks; its 2026-10-30 amendment, the
  shared start judge), family ADR 0033 (processes by identity); Dyno ADR 0010 §11 (one agent, and who runs it); Pitcrew
  ADR 0001-0003 and `pitcrew-core/src/controller.rs` (the same shape, already in the family).
- Evidence (all read at `origin/fix/dyno-single-agent` faac13c2a and `origin/feat/paddock-proc-start-judge` 7f8fbc3dd
  unless noted). [V] = verified by reading code or running something; [I] = my inference; [E] = external source.

## Amendments at acceptance (2026-10-30)

An independent review kept the design and changed it in the following ways; the sections below are the original text, and
where they disagree this section wins.

1. **The child is held from `Launch` until its end is reported.** Retry in `Starting` moves to `Ending { child }`; nothing is
   looked at for a launch and nothing is launched until `ChildEnded { child }`. Quit decides `stopped_own` from the held
   child, also while a launch is in flight. The adapter runs `Launch` and `EndHeldChild` in order on one launcher thread, so
   the end that follows a launch ends the child that launch made.
2. **Every phase but `Connected`, `Quitting` and the ends counts ticks and shows a cause**, in Dyno's own `Stall` enum
   (`ServiceNotAnswering` for a look or connect that does not answer, `AgentKeepsFailing` for a backoff that repeats
   `FAILED_AFTER` times; the shared `StallCause` only inside `Starting`). A `Quitting` phase is excluded by name: it is bounded
   by the grace of ADR 0031 class S. A count survives a look made from inside a wait (so a stale file beside a holder still
   reaches its note). Retry carries the id of the stall it answers; one for a stall that is over does nothing.
3. **The owner thread never blocks.** `Launch`, `EndHeldChild` and closing the launcher (one launcher thread, in order),
   `Look`, `Connect` (including its file read), `RemoveHint` (file I/O), `AskToExit` and the shell's `connected` callback run
   on workers and answer with events; the sink's notifications go through one notifier thread; `AgentLink::call` asserts it
   is not on the owner thread. What stays on the owner thread is bounded and local: map and mutex bookkeeping and the
   log-line appends. `drop_window` (called from the UI thread) syncs the subscription on a worker, and `sent` is never held
   across a request. Tests: a service probe that never answers, and a sink that blocks, and Quit still
   reaches `Stopped`.
4. **The automatic fallback of ADR 0010 stays**: `WaitingForService` counts rechecks that saw no agent holding the lock and
   starts the app's own agent after `SERVICE_IDLE_RECHECKS`, without Retry. The model proves an own agent is reachable beside
   a service that starts nothing with no Retry.
5. **Looks and child views carry the generation and sequence they are about**; `step` drops stale ones (u64 numbers).
6. **Crash loops back off**: failures in a row are reset only after a connection lasted `CONNECTED_FOR` ticks; a connection
   lost sooner backs off instead of looking again at once.
7. **Owner has two values** (`ThisApp(Generation)`, `Other`): take-back and hand-over need only "ours or not".
8. **The model check fails when it reaches its state bound**, uses parent pointers for traces, and covers the discovery file
   (stale or of another build) and the launcher's queue. Its counts are shrunk (`Tuning`) so a note comes after 3 ticks.
9. **The start judge is fed a real CPU reading** every `CPU_EVERY` ticks (or `Unavailable`), never `NotRead` for ever, through
   one adapter function (`ports::cpu_of`).
10. **Health's repair waits on the published transitions** (`AgentLink::wait_connected`) instead of polling.
11. **Review of the code (2026-10-30):** a launch that fails while Retry is ending it returns to `Looking` (it left `Ending`
    for ever); `Connect` and `RemoveHint` carry the look whose file they are for, and a connect re-checks that the file is of
    the build its purpose needs; the stale-answer rows (an old generation's `Launched`, a pid that is not the held child's)
    are table tests; the model has launch failures, an agent that crashes after it is ready, and a long service-fallback
    scenario with Retry and Quit interleaved; `changes_answered` and `looks` are published so tests count looks, not steps.
12. **Follow-up (not done):** an approval granted in System Settings after `RequiresApproval` is not noticed while connected
    (the probe is re-asked on connect, on a service change and while a service is awaited, not on a timer); and on macOS the
    CLI's `dyno-agent service install` writes a legacy LaunchAgents plist with the label SMAppService uses: it should refuse
    or use SMAppService when the app's item is enabled.
13. `Cargo`: `paddock_proc::StartJudge` derives `Clone` (the machine is cloned by the model check).

Not done here, as decided: the generic part (`crate::supervise`: `Machine`, `run_owner`, `Transitions`, `spawn_ticker`, and
`explore` in tests) has no Dyno type in it and moves to `paddock-proc` after the tag.

## Context

### Five fixes in two days

`apps/dyno/app/src-tauri/src/link.rs` (`AgentLink`) starts, finds, hands over and stops Dyno's agent. On 2026-10-30 it
needed five rounds of fixes, each found on the release path (VM, review or the `dyno-release` gates):

| # | Commit | Symptom | Immediate cause [V] |
|---|---|---|---|
| 1 | 6f77f4691 | Cold VM: two `dyno-agent run` on one `dyno.db`; one orphan after Quit; a busy DB moved aside | A fixed 5 s `SPAWN_WAIT` gave up on a slow agent and launched another; `ProcessLauncher` overwrote the first child's handle, so Quit ended only the newest |
| 2 | fd92274e0 (review P1-1) | "Starting..." for ever in three waits, no cause, no Retry | "Wait forever" replaced the clock in each nested wait loop; each loop needed its own stall code and none had it |
| 3 | fd92274e0 (review P1-2) | A stale `agent.json` counted as ready; exit-3 launch loops, false "Agent stopped" | A decision taken from a file read with no freshness or identity |
| 4 | 03a13d3bb (run 7201, Windows) | Take-back test: `launches 2 != 1` | `refresh_service` on a caller thread asked the former service agent to exit; the link thread saw the connection close and launched before Windows released the agent's `LockFileEx` lock; the launch was refused (exit 3) and repeated |
| 5 | faac13c2a (run 7203, Linux) | `quit()` returned `false` right after logging "the agent is stopped (this app started it)" | `shutdown` decided "stopped" from `matches!(child(), Running)` read *after* sending `Shutdown`; asking the agent to exit ends it, so on Linux the second read found it gone. The fix reads it once, before the action |

The same branch carries `dd9f6996f` (log bound and test guards), so the file changed six times in one day; non-test
code grew from 540 lines on `main` to 1,006 lines, and the shared state from 5 to 12 atomics [V: `git show
origin/main:…/link.rs`, `mod tests` at line 541 vs 1007; `Atomic*::new` counted].

Related races in tests: Dyno `shell.rs` asserted `data/` right after start, before the app wrote it (DY15 in
`issues-research-2026-10-30.md`); Pitwall's quit test passed without the quit wiring and could not fail (PW1). Both are
tests observing a transition they cannot see.

### What the code is today [V]

- **State is spread over 17 places and the call stack.** `AgentLink` holds 12 atomics (`started_by_us`, `running`,
  `ever_connected`, `quit_done`, `service_installed`, `supervised`, `poll_ms`, `retries`, `service_fallback`,
  `stall_shown`, `looks`, `launch_anyway`) and 5 mutexes (`peer`, `votes`, `sent`, `probe`, `stopping`), plus the
  launcher's `Slot`. The most important state, *which wait we are in*, is not stored at all: it is which function the
  link thread is executing (`start_agent`, `wait_for_service`, `wait_for_other_agent`, `establish`'s `for _ in 0..3`,
  `run`'s connected loop; link.rs:456-830).
- **At least five threads write it.** The link thread (`run`); the thread spawned on every connection
  (`shell.rs:304`, `refresh_service`, which can `hand_over`: `Shutdown` plus `launcher.finish(3 s)`, and
  `take_back`); the settings command (`shell.rs:180`, `refresh_service` again; also reached from the `connected`
  callback, which runs on the link thread); the health Retry (`health.rs:99`); the teardown (`lib.rs:162`, `quit`).
  Mutual exclusion is provided piecemeal: the launcher's slot mutex, `launcher.close()` before `stopping`, the swap on
  `quit_done`, `std::mem::replace` on the Retry counter.
- **Decisions are taken from several reads across actions that change what was read.** Bug 5 is the purest case
  (`child()` read, `Shutdown` sent, `child()` read again). Bug 4 is the same across threads: "the peer closed" was read
  as "the agent is gone and its lock is free". `quit()` still computes `started_by_this_app() || running` from two
  sources (link.rs:539-544), safe today only because `running` is read once.
- **Fixed time limits remain** (ADR 0031 instances): `launcher.finish(3 s)` (link.rs:354, 530, 699), `Shutdown` with
  5 s and `wait_gone(5 s)` in `Plan::Replace` (608, 610), `CONNECT_GRACE` 8 s with a 50 ms sleep loop (61, 428), the
  three-pass `establish`, and `backoff`.
- **Tests watch flags by polling.** The 28 link tests use `until(poll)` 16 times and status-channel waits 17 times;
  assertions such as `until(…, || link.connected() && link.started_by_this_app())`, `link.looks() >= 50` and
  `launches == 1` sample derived flags mid-transition. `looks()` and `set_poll()` exist only for tests.

### The structural cause

The coordinator's hypothesis was "a supervisor built as polling loops on a thread with shared mutable flags, observed by
tests mid-transition". The evidence supports it, with two refinements:

1. **Polling is not the root.** Re-reading the launcher slot on every look is in fact what *protects* today's code from
   stale information. The root is that **the supervisor has no single owner and no explicit state**: five threads mutate
   12 flags, and each decision is assembled from several reads (of flags, the launcher, the lock, the discovery file)
   that other threads and the OS change between the reads and the action. Each fix added a flag or a re-ordering for one
   interleaving (`service_fallback`, `launch_anyway`, `stall_shown`, `was_running`); nothing checks the others.
2. **The waits are code locations, not states.** Because "waiting for the service" is a loop in a function, the rules
   "every wait has a cause and Retry" and "nothing launches while a held lock is being released" cannot be stated once
   and checked; each loop implements them by hand, and bug 2 is the loop that did not.

Name of the pattern: **implicit, multi-writer supervisor state** (decisions on re-read shared state, waits as call
stacks). It is pattern C (races) and H (acting on stale names) of the 2026-10-30 issues research, inside one component.

### Is a redesign warranted?

Yes [I, on the evidence above]. Targeted fixes have converged on single interleavings: each round was approved by an
independent review (round 3 was mutation-checked, merge-steward 22:12) and the next interleaving still failed in CI on
another OS. Two more spots of the same kind are visible in the current code without running it [I: read, not
reproduced]: `refresh_service`'s `hand_over` (`launcher.finish`, `started_by_us = false`) runs on a spawned thread and
can interleave with the link thread's `establish` writing `started_by_us` (link.rs:343-354 vs 585), so "ours" can be
wrong for one connection; and `Plan::Replace` still waits a fixed 5 s for an agent of another build to go
(608-610; since 03a13d3bb the following launch is at least guarded by the `is_held` check in `start_agent`). Neither
can be written as a deterministic test against the current shape, which is the point: the shape gives tests no state to
wait on and no way to choose the interleaving.

### A first measurement [V]

A throw-away prototype (pure machine of about 230 lines, a nondeterministic world model of agent processes, the lock,
the login service, delayed probe answers and the person, and an exhaustive breadth-first check of every interleaving)
is in the session scratchpad:
`/private/tmp/claude-501/-Volumes-d8sst7-dev-decryptology-net-paddock/bfc11532-d7b3-48cd-928e-85f725c26a50/scratchpad/proto/src/main.rs`
(not in the repo). Release build, Apple M4 Pro, machine load about 12:

| Bound (per scenario: ticks, Retry, Quit, service removal) | States, 4 scenarios | Time | Result |
|---|---|---|---|
| 5, 1, 1, 1 | 12,345 | about 22 ms | the design: no violation |
| 8, 2, 1, 1 | 124,688 | about 0.37 s (max RSS 107 MB with traces kept) | no violation |
| same, mutant "launch when the old agent's connection closes" (bug 4) | 130,959 | 0.36 s | caught at depth 3 |
| same, mutant "quit outcome read again after the exit" (bug 5) | 124,688 | 0.32 s | caught at depth 3 in all 4 scenarios |

The checker also found a hole in **my first draft of the design**: without a generation number on events, a
`ChildExited` of a child ended by Retry arrived after the next launch and was taken for the new child, and the machine
launched a third agent while the second ran (depth 5; first draft kept as `main_v1.rs.txt` beside it). Today's polling
code does not have that bug; an event-driven rewrite would have, unless events carry identity. That is rule 53 applied
to events, and it is why the design below gives every event the generation or sequence number of the thing it is
about.

## Decision

### 1. One pure machine, one owner, ports for the world

```rust
/// The supervisor's whole state; only `step` changes it.
pub struct Supervisor { phase: Phase, child: Option<Held>, service: ServiceView, look: Seq, wait: Option<WaitInfo>, … }

pub enum Phase {
    Looking { seq: Seq },                         // a look (hint, lock, service) is out
    Connecting { seq: Seq },
    Starting { child: Generation },               // fed to paddock_proc::StartJudge every tick
    WaitingForService,                            // the login service supervises; its agent is awaited
    WaitingForHolder,                             // an agent nobody here started holds agent.lock
    Replacing { peer: PeerId },                   // another build asked to exit; waits for the lock to be free
    TakingBack { peer: PeerId },                  // the service was removed; its agent asked to exit; waits for the lock
    HandingOver { child: Generation },            // the service supervises now; ours asked to exit; waits for its exit
    Connected { peer: PeerId, owner: Owner },     // Owner::ThisApp(Generation) | Service | Other
    BackingOff { failures: u32, ticks_left: u32 },
    Quitting { outcome: QuitOutcome },
    Stopped { outcome: QuitOutcome },             // terminal
}

pub enum Event {
    Tick(Option<Reading>),                         // a host tick (ADR 0031); in Starting, the judge's reading
    Looked { seq: Seq, hint: Option<HintSeen>, lock: LockSeen, service: Option<ServiceState> },
    Launched { child: Generation, result: Result<(), LaunchError> },
    ChildExited { child: Generation, code: Option<i32> },
    Connected { seq: Seq, peer: PeerId, pid: u32 }, ConnectFailed { seq: Seq, why: String },
    PeerClosed { peer: PeerId },
    Retry, Quit, ServiceChanged,                   // the person, the login switch
}

pub enum Command {
    Look { seq: Seq, service: bool },             // read the hint and the lock; ask the OS about the service if `service`
    Launch { child: Generation, anyway: bool },
    Connect { seq: Seq, hint: HintSeen },
    AskToExit { peer: PeerId },                   // the only act towards a process this app did not start: a request
    EndHeldChild { child: Generation },           // ends the held child of that generation, nothing else
    UsePeer(Option<PeerId>),                      // what `call` sends requests to
    Report(AgentStatus), Stall(StallReport), ClearStall,
    Done(QuitOutcome),
}

impl Supervisor {
    pub fn step(&mut self, event: Event) -> Vec<Command>;   // pure: no clock, no I/O, no threads
}
```

- **One owner thread** (the driver) holds the `Supervisor`, takes events from one channel, calls `step`, executes the
  commands through ports and turns their results into events. Nothing else mutates supervisor state. `AgentLink`'s
  public methods stay, as thin facades: `retry()`, `refresh_service()` and `quit()` send `Event::Retry`,
  `Event::ServiceChanged` and `Event::Quit`; `call()` reads the peer published by `UsePeer`.
- **Ports** (traits, fakes in tests): `Launcher` (launch with a generation, end the held child of that generation, child
  exits reported as events by the adapter), `LockProbe` (`paddock_proc::instance::is_held`), `ServiceProbe`,
  `HintReader` (`paddock_proc::hint`, bounded read), `Connector`, `Pace` (the ADR 0031 tick). A slow port (the service
  probe, a connect) runs on a short-lived worker and answers with an event, so the owner never blocks and Quit is
  always taken at once.
- **Stale answers are dropped by construction.** Every look and connect carries a `Seq`; every child event carries its
  `Generation`; every peer event its `PeerId`. `step` ignores an answer whose number is not the current one. This is
  rule 53 for events, and the prototype shows it is necessary (the first-draft hole above).
- **Every transition is published** on a watch channel (`Transitions<Snapshot>`: the phase, the owner, the stall, the
  quit outcome, a transition counter). The UI's status chip, the health view and the tests read it; tests call
  `transitions.wait_until(|s| …)` instead of polling flags. `looks()` and `set_poll()` go.
- **Quit is a command whose result is state.** `step(Quit)` decides `QuitOutcome { stopped_own }` once, from the state
  at that event (`child.is_some() && !service.installed`), emits `AskToExit` and `EndHeldChild` when it is ours, and
  moves to `Quitting { outcome }`; later events cannot change the outcome. `quit()` sends the event and waits for
  `Stopped` on the transitions channel; the teardown's own exit watchdog (ADR 0031 §6, class S) bounds that wait.
- **The start judge does the judging.** In `Starting`, each tick's `Reading` (liveness of the held child, the ready
  handshake: a hint naming that pid written since the start or a live answer, CPU time every `cpu_every` ticks) goes to
  `paddock_proc::StartJudge`; `Stalled(report)` becomes `Command::Stall(report)` with the shared `StallCause`
  (`Slow`, `Spinning`, `Idle`, `Hung`, `WaitingForSupervisor`, `HeldByOther`). Waits outside `Starting` count ticks in
  `WaitInfo` and report their cause after `note_after` ticks.

### 2. Invariants the design guarantees

Each is enforced by the type or by `step`, and checked after every step of the model test (section 4):

1. **At most one agent of this app.** `Launch` is emitted only when no child is held, the phase is not `Quitting` or
   `Stopped`, and the latest look (current `Seq`, issued after the last `AskToExit` or `EndHeldChild`) saw the lock free,
   or Retry chose `anyway` in `WaitingForHolder` (then the agent's own lock makes the loser exit with code 3). The
   agent-side `InstanceLock` stays the backstop for the races no observer can close (two apps, or a service, launching in
   the same instant).
2. **Never end a process this app did not start.** The only ending command is `EndHeldChild { child }`, and the launcher
   ends only the child it holds with that generation (by its handle, rule 53). Towards any other agent the machine can
   only `AskToExit` over a connection.
3. **Every wait has a cause and a Retry.** Every non-terminal phase other than `Connected` maps to a `StallCause` and
   defines `Retry`; a wait that lasted `note_after` ticks has emitted its `Stall`. This is one function over `Phase`, so
   a new wait cannot be added without a cause (exhaustive `match`).
4. **No fixed time limits.** Transitions happen on events; ticks count observations (ADR 0031 §2); the only clocks are
   the S-class bounds of ADR 0031 §6, inside adapters: the grace before `EndHeldChild` kills (today's 3 s) and the
   teardown watchdog. `wait_gone(5 s)`, `CONNECT_GRACE`, the `0..3` pass limit and the 5 s `Shutdown` wait in `Replace`
   go: `Replacing` and `TakingBack` wait for the lock with a stall and Retry like any other wait.
5. **Quit decides once.** The outcome is part of `Quitting`/`Stopped`; no later read can contradict the log line.
6. **One writer.** Only the owner thread calls `step`.
7. **A hint is not proof.** Ready, in any wait, requires a hint written since the wait began that names the held child,
   or a live answer (`paddock_proc::hint::names_live`); a stale `agent.json` changes nothing.

### 3. How each bug becomes impossible or a test

| Bug | Under this design | Test that fails on the old shape |
|---|---|---|
| 1 duplicate agent, lost handle | No clock ends `Starting`; `Launch` requires no held child (invariant 1); the launcher holds one child per generation | Table: `Starting` + any number of `Tick` emits no `Launch`. Model: "a second own agent launched while the first ran" |
| 2 endless silent waits | Invariant 3 is an exhaustive `match` over `Phase`; the stall comes from one place | Table: every wait phase + `note_after` ticks emits `Stall` with its cause; + `Retry` changes phase. Model: "a long wait without a cause shown" |
| 3 stale discovery file | `Looked` carries `HintSeen { pid, started_ms }`; ready needs freshness or an answer (invariant 7) | Table: `WaitingForService` + `Looked` with a stale hint stays waiting, launches nothing |
| 4 launch before the lock is released | `TakingBack`/`Replacing` leave only on a look issued after `AskToExit` that sees the lock free; `PeerClosed` alone moves nothing | Table: `TakingBack` + `PeerClosed` emits no `Launch`. Model mutant "launch on PeerClosed" is caught (measured, depth 3) |
| 5 quit says false after "stopped" | The outcome is decided in `step(Quit)` and carried in the state | Table: `Connected{ThisApp}` + `Quit` → `Quitting{stopped_own: true}`, then `ChildExited` keeps it. Model mutant "outcome read again" is caught (measured) |

The two test races elsewhere get the same remedy: a test waits on a published transition (`Stopped`, "data folder
written"), and a test of a fix is checked with a mutant (the shared mutation step of the issues research, rank 3).

### 4. Test strategy

1. **Table-driven transition tests** on `Supervisor::step`: `(state, event) → (state', commands)`, one row per arrow,
   about 60 rows; no threads, no files, microseconds each. `cargo-mutants --in-diff` on `machine.rs` in review.
2. **A bounded model check** (the prototype's shape, kept as a test): a world model (held child, the other agent, the
   lock with delayed release, the service supervising or broken, answers delivered late and out of order, Retry, Quit,
   the login switch), every interleaving up to a bound, the seven invariants after every step. Each of the five bugs is
   a named mutant that the check must catch, so the check itself is proven able to fail. Default bound in the unit
   tests (about 12k states, tens of milliseconds in release; budget about 10x in a debug test build [I]), a larger bound
   in nightly. No new dependency is needed for exhaustive search; `proptest` (already in Debrief's workspace) is the
   fallback if the state space outgrows exhaustive search.
3. **Integration tests, few**, through the driver with fake ports and the in-process agent of today's rigs, waiting on
   `transitions.wait_until`; the existing 28 link tests are kept and are the specification of behaviour (section 6).
   ADR 0031's harness cap (`HANG_GUARD`, 40 s) stays as the test harness's only bound.

### 5. Where it lives

- **Now, in Dyno:** `apps/dyno/app/src-tauri/src/link/` split into `machine.rs` (pure, no Tauri), `driver.rs` (owner
  thread, ports, transitions), `ports.rs` (real adapters, mostly today's `ProcessLauncher` and probes) and `mod.rs`
  (`AgentLink` facade). It uses the shared `StartJudge`, `StallReport`, `InstanceLock`/`is_held` and `hint` from
  `paddock-proc` (`feat/paddock-proc-start-judge`).
- **The generic mechanism goes to the shared crate after the Dyno tag**, not the Dyno transitions: a `Machine` trait
  (`step(Event) -> Vec<Command>`), the owner-thread `Driver`, `Transitions<S>` with `wait_until`, and the bounded
  explorer for model tests, in `paddock-proc::supervise` (or its own small crate if review prefers). Rule of three for
  the mechanism is met: Pitcrew's controller is already a pure `Input → Action` machine driven by one owner thread "with
  no locks" (`pitcrew-service/src/supervisor.rs` header [V]), Pitwall's `supervise.rs` is a pure core [V], and Dyno is
  the third. The second adopters of the shared mechanism are the starts that the issues research names next:
  `paddock-chromium`/Spotter's browser (driver consolidation ADR) and Pitwall's agent start. The Dyno transitions do
  not generalise (login-service hand-over and take-back are Dyno's), so a "generic Dyno supervisor" is not shared.
- Why not share now: the shared crate is under review for the tag already; adding a generic API under tag pressure,
  with one adopter, is how a shared API gets the wrong shape (YAGNI). Writing Dyno's driver against the trait shape makes
  the later move mechanical.

### 6. Migration

1. Machine and its table and model tests, beside today's code (no behaviour change).
2. Driver and ports; `AgentLink` becomes the facade; the existing 28 tests run unchanged in behaviour, their waits moved
   from `until(…flags…)` to `wait_until` on transitions; `looks()`/`set_poll()` removed; the fixed 3 s/5 s/8 s waits
   other than the S-class kill grace go.
3. Delete `start_progress`, `written_since`, `names_live_agent`, the three wait loops, `service_fallback`,
   `launch_anyway`, `stall_shown`, `retries`, `looks`, `poll_ms` (replaced by `paddock-proc` and `Phase`).
4. Same CI and VM gates as the adoption: `dyno-release` with installers on all three OS, the VM cold start (one agent, no
   orphan after Quit, relaunch), the login switch both ways.

## Parallelism and cost (ER-20)

- **Threads.** One owner thread per app (today: the link thread plus a spawned `refresh_service` thread per connection
  and work on command threads). Slow ports (service probe: `SMAppService` status, `systemctl --user is-active`, a
  connect) run on a short-lived worker each, at most one per kind in flight (a new look is not issued while one is out),
  at UTILITY QoS on macOS (rule on waited work: a person waits on the agent). No parallelism is needed or useful: the
  work is latency-bound and tiny.
- **Per tick.** `step` is arithmetic on a small struct (nanoseconds). A look is one bounded hint read (≤ 16 KiB,
  `hint::MAX_HINT_BYTES`), one `try_lock` probe (open, lock, close) and, every `SERVICE_RECHECK` ticks only, the service
  probe, as today. In `Starting`, the start judge reads CPU time every `cpu_every` ticks, one system call per process of
  the tree (ADR 0031 amendment: about 0.1% of a core). Cheaper than today: no `try_connect` per look to test a hint
  (only when the hint is fresh or names the held pid), and child exit can be an event from the adapter instead of a
  `try_wait` per look.
- **Tests.** Measured above: the model check at the default bound is about 22 ms in release on this machine; the
  larger bound (125k states, 0.37 s, 107 MB with traces) belongs in nightly. Integration tests get faster and steadier:
  they wake on a transition instead of a 20-200 ms poll.
- **Memory.** One `Supervisor` (well under 1 KiB) and one snapshot on the channel; the model check's memory is bounded
  by its state bound (keep traces off by default; print them only on failure).

## Consequences

- The class "two reads across an action", "a wait without a cause", "a launch before the lock is free" and "an answer
  about an old process" becomes a failure of a fast deterministic test instead of a CI run on one OS.
- The supervisor's state is visible: the health chip, the log and tests read the same published phase.
- Cost: a rewrite of about 600 lines of non-test code and a port of 28 tests (section "Effort").
- Risk: a rewrite can lose behaviour the old code had by accident. Mitigation: the 28 tests as the specification, the
  model check, a review with mutants, and the VM cold-start check.
- Requests before the first connection: `CONNECT_GRACE` (8 s) goes; `call` waits on the transitions for `Connected` or
  for a stall (then answers `AgentUnavailable` with the stall's cause). A user-visible change in wording only (needs the
  user's OK, below).

## Alternatives considered

- **Keep fixing instances.** Rejected: five rounds, each approved, each followed by a failure on another OS; two more
  spots of the same kind are visible now (Context).
- **One big mutex around the link.** Removes data races but not the stale-decision class (bug 4 and bug 5 happen within
  one thread's sequence of reads and actions), and blocks Quit behind a slow probe.
- **An async runtime (tokio actor).** Same design with a heavier dependency; the app crate does not need async for one
  owner thread. Not chosen.
- **A state-machine library** (for example a typestate or statechart crate). The machine is small and the family's own
  precedent (Pitcrew) is a plain `enum` + `match`; a library adds a dependency without checking the invariants.

## Effort

| Package | Work | Agent-days |
|---|---|---|
| S1 | `machine.rs`: phases, events, commands, invariant 3 as a function; table tests (about 60 rows) | 1.5 |
| S2 | `driver.rs` + `ports.rs`: owner thread, workers for slow ports, transitions channel, generations/sequences, `StartJudge` in `Starting`; `AgentLink` facade | 1.0 |
| S3 | Model-check test: world model, invariants, five bug mutants as tests | 0.5-1.0 |
| S4 | Port the 28 link tests to `wait_until`; delete the old loops and flags | 1.0 |
| S5 | Review with mutants, one `dyno-release` dispatch with installers, VM cold start and login switch | 1.0 (mostly elapsed) |
| | **Total** | **about 5 agent-days** |

The adoption of the shared crate in today's shape (`dyno/adopt-start-judge`: replace `start_progress`, the lock, the
hints) is about 1-1.5 agent-days and rewrites the same wait loops; so the redesign adds about **3.5 agent-days** over the
adoption alone. With S1 and S2 in parallel (two agents) and S3 beside them, about **2.5-3 days elapsed** to a reviewed
head [I].

## Impact on the Dyno 1.0 tag and recommendation

The tag already waits for `feat/paddock-proc-start-judge` and `dyno/adopt-start-judge` (merge-steward 2026-10-30
22:11, user decision). The adoption replaces exactly the code this ADR replaces.

**Recommendation: do it now, as the shape of the adoption** (`dyno/adopt-start-judge` builds S1-S5 instead of swapping
helpers inside the old loops). Reasons: the adoption rewrites these loops anyway, so the extra cost is about 3.5 days, not
5; adopting into the old shape means rewriting the same code twice and shipping the multi-writer shape to users; and the user's standing instruction is to redesign recurring kinds until stable, with no workarounds.

**Stop rule and fallback:** if S1+S3 (the machine and the model check) are not green and reviewed within 3 agent-days,
tag on the adoption in today's shape with two small structural fixes that close the worst of the class: (a) one writer:
`refresh_service` and `retry` only send a message the link thread acts on (`quit` keeps its single-read decision from
faac13c2a), and (b) `Plan::Replace` waits for the lock like the take-back does (with stall and Retry) instead of a fixed
5 s; then the full machine in 1.0.1.

**The shared mechanism** (`Machine`, `Driver`, `Transitions`, explorer) moves to `paddock-proc` right after the Dyno
tag, with Spotter's browser start or Pitwall's agent start as its second adopter.

## Decisions for the user

1. Now (inside the adoption, tag about 2.5-3 days later) or after the tag (fallback above)? Recommended: now, with the stop
   rule.
2. The generic mechanism in `paddock-proc` after the tag (recommended), now, or never (Dyno only)?
3. `CONNECT_GRACE` goes: a request before the first connection waits for the connection or a shown stall instead of
   8 s. OK?
4. The 3 s grace before ending the held child on Quit and hand-over stays a time limit (ADR 0031 §6, class S: quitting
   must end). OK?
