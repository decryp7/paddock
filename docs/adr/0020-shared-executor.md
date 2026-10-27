# 20. One shared executor: tasks, periodic checks and leases, with priorities

- Status: Proposed (2026-10-27; not accepted)
- Date: 2026-10-27
- Related: rules 10–14 (background work, progress, never time out, bounded parallelism at background priority,
  pipelines), rule 40 (recurring bugs mean a missing design rule), rule 42 (share first), rule 43 (shared behaviour tested
  once); ADR 0008 (long operations, `paddock-jobs`), ADR 0010 (notifications and service health), ADR 0012 (app shell);
  Pitwall's scheduler (`apps/pitwall/crates/pitwall-core/src/scheduler.rs`)
- Evidence: [research note 2026-10 poller pool](../research/2026-10-poller-pool.md) (survey of every wait, poll and
  background thread in the monorepo; counts, file:line, wake-up estimates, priority sites, OS facts)

## Context

The survey found the same few shapes written dozens of times, each with its own thread:

- **44 periodic polls (A)**, mostly "sleep N in small slices, look, repeat" on a dedicated thread: eight in dyno-agent
  alone, one per supervised child for memory caps (twice: `debrief-ipc` and `paddock-proc`), a 50 ms wake loop per
  watched folder in `paddock-watch`, start-up watchdogs, health probes, update checks. Idle cost per app: 5–16 threads
  and 5–30 wake-ups a second.
- **~65 thread-per-task or one-shot-timer sites (E)**: a new OS thread per job, action, guarded plugin call, handshake,
  fade or deadline; one of them (`apps/debrief/app/src-tauri/src/browser/research.rs:463`) sleeps its full timeout after
  the work ended.
- **~67 lifelong blocking threads (F)** that must stay dedicated (pipe and socket readers, lifelines, OS trace consumers,
  DB writers, abandonable plugin and OS calls) but are unnamed, uncapped and invisible to the user.
- **17 frontend polls (D)** of state the engine already has.
- **Priority** exists only in Pitwall's scheduler and in three helpers that lower a thread differently per OS. Debrief's
  import pools run at normal priority against interactive queries (rule 13), Spotter's "Check now" and scheduled checks
  each build their own semaphore and together exceed the render cap, Debrief's chat turn waits behind queued background AI
  work, and no job model has "run next" or child jobs.
- Abort is cooperative (`CancelToken`) everywhere, but a task that ignores cancel is silent; the only hard stop is a
  supervised child process (`paddock-proc`).

`paddock-jobs` already has the parts: `Clock`/`SystemClock`/`FakeClock`, `CancelToken` with deadlines, `Deadline`,
`Backoff`, `JobPool`, `Workers`, `schedule` (user-facing schedules, minimum 10 s) and the job registry and reporter.
`paddock-guard` has panic containment and quarantine; `paddock-proc` has supervised children; `paddock-live` has hubs
with subscriber counts. What is missing is one mechanism that uses them together.

This is a rule-40 case: the recurring bug kinds (idle wake-ups, leaked sleeping threads, silent stuck work, work at the
wrong priority, cancel that waits for a sliced sleep, tests that sleep) come from every site hand-rolling its own loop.

## Decision (proposed)

Add **one executor per process** to `paddock-jobs` (module `exec`). It has three modes and one priority model, and every
background activity in the family goes through it, except the declared exemptions below.

### Mode 1: `submit` — short tasks on a bounded worker pool

- Tasks carry a **class**, an **owner** (for containment and attribution), an optional **group** (per-group concurrency
  limit, e.g. `"render"`, `"import:<window>"`, `"ai"`) and an optional parent task.
- Workers are created lazily up to the bound and retire after 30 s idle (as tokio's blocking pool does), so an idle app
  holds no worker threads beyond the reserved one.
- Subsumes `JobPool` (its `hold`/`assert_nonblocking` keep working) and every E site that runs a short action on a new
  thread.

### Mode 2: `every` / `after` — periodic checks and one-shot timers on one scheduler thread

- **One scheduler thread per process** holds all checks in a due-time queue and sleeps with
  `clock.wait(until_next_due, wake_token)` until the next due time or a new registration (a fresh token per wait, so a
  new check wakes it).
- **Coalescing:** a check's due time is snapped to an aligned grid (1 s, 5 s, 30 s, chosen as the largest grid not
  above the interval) and may move within its **tolerance**, so checks due close together run in one wake-up.
- **Weight:** `Inline` checks (a few reads of shared state, budget 2 ms) run on the scheduler thread; `Worker` checks
  (system calls, process tables, network) are submitted to Mode 1 with the check's class (default `Background`). An inline
  check that overruns its budget twice is moved to `Worker` automatically and reported.
- **Never overlapping:** a check still running when it is due again is skipped, not queued; the skip is counted; the
  next due time is computed from when it finished.
- **Demand:** a check declares what it serves: `Always`, `Subscribers(...)` (a `paddock-live` hub's `subscribers()`) or
  `Window(...)`. No subscriber → paused; subscribers but no visible window → `idle_interval` (slower). Demand changes wake
  the scheduler.
- **Lateness:** each run gets `late_by` (how much later than due it ran), so `paddock-startup`'s "the app was paused"
  detection keeps working.
- **Snapshot + diff:** a `Probe` returns a keyed snapshot; the executor diffs it with the previous one and calls the
  sink with `Change::{Added, Removed, Changed}` only when something changed. Folder watching reaches it through a
  poll-backed `paddock_watch::ChangeSource`; UIs reach it through a `paddock-live` sink.
- **`after(deadline)`** is a one-shot check: it replaces every thread that exists only to wait for a deadline (guard
  timers, handshake, browser and exit-stall watchdogs, fades, draft timeouts), and is cancelled when the work ends.
- Intervals are not clamped to `schedule::MIN_INTERVAL_SECS`; a check may use a `schedule::Schedule` as its interval
  function for user-facing schedules (Spotter watches, Pitwall update checks).
- Same mechanism on every OS: no OS event APIs in the executor.

### Mode 3: `lease` — a dedicated, named, capped thread for lifelong blocking waits

- For work that blocks for its whole life on a pipe, socket, OS wait or a resource it must own (child stderr readers,
  lifelines, accept loops, ETW/EventPipe consumers, the DuckDB index workers, actors, CDP sockets).
- Leases are named, counted per owner with a cap (default 64 per process), visible in introspection, and **cached**: a
  returned lease thread is parked for reuse for 30 s before it exits.
- A lease inherits the class of its creator and sets its OS priority once at start.
- `lease_abandonable` is the variant for calls that may hang in the OS or a plugin (guarded plugin calls, counter reads,
  `getaddrinfo`, blocking HTTP): when its deadline passes the caller gets an error and the thread is written off
  (counted, capped, reported), never reused.

### Priorities

| Class | For | macOS | Linux | Windows |
|---|---|---|---|---|
| `Interactive` | what a user is waiting for right now: queries that paint, "Check now", chat turn, opening a file | `QOS_CLASS_USER_INITIATED` | nice 0 | normal |
| `User` | work the user started and watches: imports, exports, analyses started by a click | `QOS_CLASS_UTILITY` | nice 5 (lowered once) | `THREAD_PRIORITY_BELOW_NORMAL` |
| `Background` | work nobody waits for: indexing, mining, periodic checks (default), downloads | `QOS_CLASS_UTILITY` for checks, `QOS_CLASS_BACKGROUND` for bulk | nice 10 + `ioprio` best-effort 7 | `THREAD_MODE_BACKGROUND_BEGIN` |
| `Idle` | maintenance that may wait for hours: compaction, cache sweeps, pruning | `QOS_CLASS_BACKGROUND` | `SCHED_IDLE` + `ioprio` idle | `THREAD_MODE_BACKGROUND_BEGIN` + `THREAD_PRIORITY_IDLE` |

One helper, `paddock_jobs::exec::priority::apply_current(Class)`, replaces the three thread helpers and the two process
helpers (`debrief-core/src/threads.rs`, `debrief-ai/src/models/priority.rs`, `paddock-proc/src/sys.rs`,
`debrief-ipc/src/process.rs`).

**Workers have a fixed OS priority per tier.** Linux does not let an unprivileged thread raise its nice value again
(setpriority(2), sched(7)), so a worker is never lowered for one task and raised for the next. There are two tiers:
*foreground* workers (Interactive and User) and *background* workers (Background and Idle; Idle sets its extra level per
task only where the OS can undo it, Windows `THREAD_MODE_BACKGROUND_END`). Each tier is bounded (cores − 1 by default,
the Performance setting where an app has one); a process-wide **running bound** (cores − 1) caps how many run at once
across both tiers, plus the reserved worker.

**Queues:** one priority queue per class, FIFO within a class. Dispatch takes the highest **effective** class whose
group has room (a full group never blocks the head of the queue: the next eligible task runs).

**Reserved Interactive:** at least one foreground worker (default 1) takes only `Interactive` tasks and does not count
against the running bound, so a user action never waits behind bulk work.

**Aging** (from Pitwall's scheduler, lifted rather than rewritten): a queued task's effective class rises one step per
`aging` interval waited (default 30 s), so `Idle` and `Background` never starve for a slot. Aging changes queue order and
slot share only; it never changes the OS priority tier a task runs on (aged bulk work does not become foreground CPU
load).

**Re-prioritising:** `TaskHandle::set_class(c)` and `bump()` ("run next": the head of `Interactive`'s queue). A bump
raises the task's queued children too and is inherited by every subtask and lease it creates afterwards. A running task's
thread keeps its tier (OS limits above); on macOS and Windows a later WP may move it where the OS allows undoing.

**Inheritance:** subtasks, checks registered by a task and leases inherit the creator's class unless they ask for a
lower one; periodic checks default to `Background`.

**Groups and priority:** a group limit applies to all classes together; `Interactive` tasks may use the group's
`reserved_interactive` slots (default 0, 1 for `"render"`). Priority never bypasses a group limit, because group limits
protect a scarce resource (one browser profile, one model, Chromium processes).

**Preemption is cooperative only.** A running lower-class task yields at its checkpoints:
`cx.checkpoint()?` checks cancel and, when a higher-class task is waiting for a slot it could take, releases its slot
and waits (in class order) to get one back. The thread is held but the slot and the CPU are free. Nothing is interrupted
outside a checkpoint. The write-lock steps of Debrief's FTS and catalogue services (`try_lock` and yield) already work
this way and become checkpoints.

**Priority across processes:** where interactive and bulk work meet in another process (Debrief's store worker), the
class travels with the IPC call and the serving thread applies it. A reserved worker in the app does not help there: the
contention is CPU inside the worker's engines (research note, Priority today).

### Abort

- **Cooperative** through `CancelToken`, triggered by the user, a deadline, `cancel_when(predicate)` (a predicate the
  scheduler evaluates as an inline check) or the **parent** (child tokens: `token.child()`; new reasons `Parent`,
  `Condition`, `Shutdown`).
- **Watchdog:** a built-in check scans running tasks every second; a task still running `grace` after cancel (default
  2 s; `CANCEL_LATENCY_TARGET_MS` is 250 ms) is marked **Stuck**: reported (event, log line, Jobs pane, owner's
  quarantine count), never silent. The pool writes the worker off and starts a replacement; written-off threads per
  process are capped (default 8), after which the owner is quarantined.
- **Hard abort** only for work run as a supervised child: `exec.submit_child(cmd, ...)` runs it under
  `paddock_proc::Supervised`, and cancel past grace ends the process tree.

### Containment

Every task, check and lease runs under `paddock_guard::catch` with its owner: a panic becomes `TaskError::Crashed`
attributed to the owner; failures (panic, stuck) count in the owner's `Quarantine`; a quarantined owner's submits and
checks are refused with a message that names it. A panicking check is skipped for that run and stays registered.

### Introspection

`exec.snapshot()` returns lanes (workers per tier, busy, reserved, written-off), tasks (id, name, owner, group, class,
effective class, state `Queued | Running | Yielded | Stuck | WrittenOff`, since, parent, linked `JobId`), checks (name,
interval, grid, last run, last duration, next due, paused by demand, skipped overlaps, late-by) and leases (name, owner,
since). A `paddock-live` feed publishes it for the Jobs pane and the plugin task manager. User-facing jobs stay in
`JobRegistry` (titles, phases, progress); a task links to its `JobId`.

### Tests

`Executor::manual(FakeClock)` is a **step mode with zero threads**: `run_until_idle()` runs queued tasks inline in
dispatch order (priority, aging, groups, reserved slots); `advance(d)` moves the clock and runs every due check inline in
due order; `snapshot()` shows the state. The same scenario tests run against the threaded driver with a `FakeClock`
(rule 43: shared behaviour tested once, in `paddock-jobs`). Leases are real threads by definition and are tested with
fake sources.

### Drivers

The core (`Timetable`: due queue, grid, tolerance, demand, overlap skip; `Dispatch`: class queues, aging, groups,
reservation) is pure and thread-free. Two drivers: the std driver (scheduler thread + tiered workers) and a **tokio
driver** (feature `tokio`: the timetable on one tokio task, workers via `spawn_blocking` with priority applied per call),
so pitwall-server and Spotter register checks without adding threads.

### Declared exemptions (stay raw threads, listed in a lint allow-list)

Real-time loops (drag cursor trackers, Dyno's CPU-cap duty cycle, stack samplers), watchdogs that must work when the
executor is wedged (Debrief's plugin budget watchdog, the exit watchdog: each a `lease` with its own thread, never a
pool worker), separate small processes that are themselves the loop (shell-demo agent, Windows SCM hosts), and CLIs.

### API sketch

```rust
// paddock_jobs::exec
pub enum Class { Interactive, User, Background, Idle }

pub struct ExecConfig {
    pub foreground: Workers, pub background: Workers, pub running: Workers, // bounds
    pub reserved_interactive: usize,        // default 1
    pub aging: Duration,                    // default 30 s
    pub stuck_grace: Duration,              // default 2 s
    pub written_off_cap: usize,             // default 8
    pub lease_cap: usize,                   // default 64
    pub groups: Vec<(GroupId, GroupLimit)>, // GroupLimit { max: usize, reserved_interactive: usize }
}

pub struct Executor { /* Arc inner */ }
impl Executor {
    pub fn start(cfg: ExecConfig, clock: Arc<dyn Clock>, guard: Arc<paddock_guard::Guard>) -> Executor; // std driver
    pub fn manual(cfg: ExecConfig, clock: FakeClock) -> ManualExecutor;                                   // step mode
    #[cfg(feature = "tokio")] pub fn start_tokio(cfg: ExecConfig, clock: Arc<dyn Clock>, rt: tokio::runtime::Handle) -> Executor;

    // Mode 1
    pub fn submit<T: Send + 'static>(&self, spec: TaskSpec, f: impl FnOnce(&TaskCx) -> Result<T, TaskError> + Send + 'static)
        -> TaskHandle<T>;
    pub fn submit_child(&self, spec: TaskSpec, cmd: std::process::Command, cap: paddock_proc::MemoryCap)
        -> TaskHandle<std::process::ExitStatus>;

    // Mode 2
    pub fn every(&self, spec: CheckSpec, run: impl FnMut(&CheckCx) + Send + 'static) -> CheckHandle;
    pub fn watch<P: Probe>(&self, spec: CheckSpec, probe: P,
        sink: impl FnMut(&[Change<P::Key, P::Value>]) + Send + 'static) -> CheckHandle;
    pub fn after(&self, at: Duration /* on the clock's monotonic reading */, spec: TaskSpec,
        f: impl FnOnce(&TaskCx) + Send + 'static) -> CheckHandle;

    // Mode 3
    pub fn lease(&self, spec: LeaseSpec, f: impl FnOnce(&LeaseCx) + Send + 'static) -> Result<LeaseHandle, LeaseError>;
    pub fn lease_abandonable<T: Send + 'static>(&self, spec: LeaseSpec, deadline: Duration,
        f: impl FnOnce() -> T + Send + 'static) -> Result<T, Abandoned>;

    pub fn snapshot(&self) -> ExecSnapshot;
    pub fn shutdown(&self, grace: Duration) -> ShutdownReport;
}

pub struct TaskSpec { pub name: &'static str, pub owner: Owner, pub class: Class,
                      pub group: Option<GroupId>, pub parent: Option<TaskId>, pub job: Option<JobId>,
                      pub deadline: Option<Duration>, pub cancel: Option<CancelToken> }

pub struct TaskCx { /* id, class, token */ }
impl TaskCx {
    pub fn checkpoint(&self) -> Result<(), Cancelled>;  // cancel check + cooperative yield
    pub fn token(&self) -> &CancelToken;
    pub fn spawn<T: Send + 'static>(&self, spec: TaskSpec, f: impl FnOnce(&TaskCx) -> Result<T, TaskError> + Send + 'static)
        -> TaskHandle<T>;                               // inherits class and parent
}

pub struct TaskHandle<T> { /* id, result slot */ }
impl<T> TaskHandle<T> {
    pub fn id(&self) -> TaskId;
    pub fn cancel(&self);
    pub fn cancel_when(&self, pred: impl Fn() -> bool + Send + 'static); // evaluated by the scheduler
    pub fn set_class(&self, class: Class);                              // queued: re-queue; children follow
    pub fn bump(&self);                                                 // run next
    pub fn join(self) -> Result<T, TaskError>;
    pub fn try_result(&self) -> Option<Result<T, TaskError>>;
}

pub struct CheckSpec { pub name: &'static str, pub owner: Owner, pub interval: Interval,
                       pub tolerance: Duration, pub first_after: Duration, pub weight: Weight,
                       pub class: Class /* default Background */, pub demand: Demand,
                       pub idle_interval: Option<Duration>, pub max_runs: Option<u32> }
pub enum Interval { Every(Duration), Dynamic(Box<dyn Fn() -> Duration + Send>), Schedule(crate::schedule::Schedule) }
pub enum Weight { Inline, Worker }
pub enum Demand { Always, Subscribers(Arc<dyn Fn() -> usize + Send + Sync>), Window(Arc<AtomicBool>) }

pub struct CheckCx { /* late_by, run count, token */ }
impl CheckCx { pub fn late_by(&self) -> Duration; pub fn token(&self) -> &CancelToken; pub fn stop(&self); }

pub trait Probe: Send + 'static {
    type Key: Ord + Clone + Send + 'static;
    type Value: PartialEq + Clone + Send + 'static;
    fn snapshot(&mut self, cx: &CheckCx) -> Result<std::collections::BTreeMap<Self::Key, Self::Value>, CheckError>;
}
pub enum Change<K, V> { Added(K, V), Removed(K), Changed(K, V) }

pub mod priority { pub fn apply_current(class: super::Class) -> Result<(), PriorityError>; }
```

Sketches for the neighbours: `paddock_proc::table::ProcTable: Probe<Key = (Pid, StartTime), Value = Identity>` with
`details(pid)` on demand and `memory(pids)` for caps; `paddock_proc::wait_child(&mut Child, deadline, &CancelToken)`;
`paddock_watch::PollSource: ChangeSource` built on `scan_folder`; `paddock_live::demand(&Hub<S>) -> Demand` and
`paddock_live::DiffSink`.

### Validation against the surveyed sites

| Site group (research note) | Mode | Fits? | Gap the design closes |
|---|---|---|---|
| Sleep-loop polls: dyno-agent ×8, Debrief probes/updates/idle unload, shared health digest, updater, ExeWatch, Pitcrew supervisor tick | `every` | yes | grid + tolerance coalesce them; cancel is immediate instead of a sliced sleep |
| Dyno process list, Pitwall live loop, `paddock-metrics::Sampler` | `watch` / `every` with `Demand::Subscribers` | yes | snapshot + diff; pause without subscribers |
| Per-child memory caps (`debrief-ipc`, `paddock-proc`), Spotter render memory | `watch` over `ProcTable` | yes | one table read serves every cap; needs `ProcTable` (new) |
| Start-up watchdogs (`paddock-startup`, Debrief) | `every` | yes, with `late_by` | `late_by` is new |
| Folder watch (`paddock-watch`, Debrief `watched.rs`, Dyno sweeper) | `every`/`watch` + `PollSource` | yes | poll-backed `ChangeSource` is new |
| One-shot deadlines: guard timer, handshake, chromium/exit-stall watchdogs, fades, draft timeout, SQL console timeout | `after` | yes | `after` is new; cancelled when work ends |
| Thread per job/action (Debrief jobs, Pitcrew ops, Dyno network refresh, shared commands) | `submit` | yes | priority classes, groups for Debrief's per-kind limits |
| Spotter "Check now" vs scheduled checks | `submit` with group `"render"` | yes | shared group limit with a reserved Interactive slot (fixes the overrun) |
| Debrief AI queue (chat vs use cases) | `submit` with group `"ai"` | yes | class order replaces FIFO + manual up/down; `bump` for "run next" |
| Debrief import vs queries in the store worker | `submit` in the store worker + class on the IPC call | partly | needs the class on the wire (Debrief contract change) and DuckDB has no statement priority |
| Pipe/socket readers, lifelines, accept loops, ETW, DB writers, actors | `lease` | yes | names, caps, introspection |
| Guarded plugin calls, counter reads, `getaddrinfo`, blocking HTTP | `lease_abandonable` | yes | written-off threads are counted and capped |
| Spotter scheduler, Pitwall checks (tokio) | tokio driver | yes | tokio driver is new |
| Plugin budget watchdog, exit watchdog, real-time trackers, duty cycle | exemption (own `lease` or raw thread) | by design | allow-list |
| Reconnect/back-off loops (`paddock-live` runner, Pitwall/Dyno/Pitcrew links) | stay on `runner::run` inside a `lease` | yes | not periodic; no change |
| Unix accept 5 ms busy-poll (`paddock-ipc`, `dyno-ipc`) | none | n/a | fixed in `paddock-ipc` by a blocking accept; independent of this ADR |
| Frontend polls (D) | `watch` + `paddock-live` feed | yes | needs each app on `paddock-live` |

Nothing surveyed needs a fourth mode. Missing today and added by this ADR: `after`, `late_by`, demand, diff, child
cancel tokens and reasons, the stuck watchdog, tiered workers, the tokio driver, and `ProcTable`.

## Consequences

- An idle app runs one scheduler thread, one reserved foreground worker and its leases; estimated savings in the research
  note's summary (Debrief ~6–7 threads and ~22 wake-ups/s, dyno-agent ~6 threads and ~24 wake-ups/s, plus 200/s from the
  separate accept fix).
- Every background activity has a name, an owner, a class and a place in the Jobs pane / task manager; stuck work is
  visible.
- Tests of timing logic run in step mode without threads or sleeps.
- `JobPool` becomes the executor's Mode 1 (kept as a thin wrapper until its three users move).
- Apps that do not use `paddock-jobs` today (Pitwall, Spotter) take it as their first shared dependency.

## Risks

1. **One scheduler thread is a single point of stall.** An inline check that blocks stalls every check. Mitigation:
   inline budget with automatic demotion to `Worker`, the stuck watchdog runs on the scheduler but reports through a
   lease-based fallback if the scheduler itself is late by more than 5 s (detected by `late_by`).
2. **Latency regressions** where a site relies on its own fast wake (memory caps at 200 ms, browser liveness at 200 ms).
   Mitigation: tolerance per check; caps keep their interval; the grid only applies at or above 1 s.
3. **OS priority semantics differ** (Linux cannot raise; macOS `BACKGROUND` throttles I/O hard; the macOS raise-back is
   unverified). Mitigation: fixed tiers; `Background` checks use `UTILITY` on macOS; measure in WP-2.
4. **Priority inversion** on shared locks: `std::sync::Mutex` gets no priority inheritance on Linux, and whether it gets
   macOS QoS promotion is unverified. Mitigation: keep background critical sections short (checkpoints already bound
   them); document it.
5. **Written-off threads leak** until the OS call returns. Mitigation: cap and quarantine; out-of-process for anything
   that hangs often.
6. **Migration churn** across five apps that do not use the shared crates yet; a half-migrated app has two mechanisms.
   Mitigation: per-app WPs, a lint (C-check) that flags new `thread::spawn`/`sleep` loops outside the allow-list.
7. **Abstraction weight** (KISS): one more concept for every contributor. Mitigation: three verbs, step-mode tests, and
   deleting the per-site loops it replaces.
8. **Step mode and the threaded driver can drift.** Mitigation: the same scenario suite runs against both.

## Work packages (proposed order)

| WP | Size | Content |
|---|---|---|
| 0 | S | `paddock-ipc` (and `dyno-ipc`) blocking Unix accept instead of the 5 ms busy-poll; failing regression test first that counts accept attempts while nobody connects. Independent; biggest single win. |
| 1 | M | `paddock-jobs::exec` core: `Timetable` (grid, tolerance, overlap skip, demand, `late_by`, `after`), `Probe` + diff, `ManualExecutor` step mode, std scheduler thread; child `CancelToken`s and new reasons. |
| 2 | M | Dispatch: classes, per-class queues, aging (lifted from `pitwall-core/src/scheduler.rs`), groups, reserved Interactive, tiered workers, `priority::apply_current` replacing the three helpers, `checkpoint` yield, `bump`/`set_class` with children. Measure tier behaviour on macOS/Linux/Windows runners. |
| 3 | S | Leases (cached, capped, abandonable), stuck watchdog, `paddock-guard` containment, `snapshot()` + `paddock-live` feed. |
| 4 | M | `paddock-proc::ProcTable` (pid + start time identity pass, details and memory on demand) and `wait_child(deadline, cancel)`. |
| 5 | M | **Dyno process list first:** dyno-agent sampler split into a system check and a `ProcTable` watch; deltas to the window via `paddock-live`; details for new, shown and top processes only. |
| 6 | M | Rest of dyno-agent: the seven loop threads and boot settle → checks; network refresh and DNS → `submit`; Dyno app heartbeat and link poll; Dyno frontend D polls → feeds. |
| 7 | M | Shared crates: `paddock-watch` (blocking recv + one-shot, `PollSource`), `paddock-startup` (`late_by`), health digest, updater, `ExeWatch`, `paddock-proc` caps on `ProcTable`, guard timer / handshake / chromium / window one-shots → `after`, writer threads → `after` + `submit`. |
| 8 | L | Debrief: child memory caps, probes, AI updates, idle unload, start-up watchdog (adopt `paddock-startup`), `watched.rs` → `paddock-watch`, `jobs/service.rs` → executor (groups = job kinds, AI group with classes), plugin watchdog on a condvar, store-duck condvar fallbacks, the `research.rs:463` leaked timeout (failing test first), frontend D polls. |
| 9 | M | Priority across the store-worker IPC: a class on each call, serving thread applies it; import pools at `User`; FTS/mining at `Background` (Debrief contract change in `api.types.ts` only if the UI sends it). |
| 10 | M | Pitcrew: supervisor tick, main-thread oneshot, repair check, `ProcTable` instead of its own sysinfo table, frontend D polls (service health from `paddock-health`). |
| 11 | L | Pitwall and Spotter: adopt `paddock-jobs` (first shared dependency), tokio driver, Pitwall checks with demand (live loop), syscalls instead of `sysctl`/`pmset` spawns, Spotter's scheduler on the timetable, its schedule copy retired for `paddock-jobs::schedule`, render group with a reserved slot, render watchdog via `paddock-chromium`. |
| 12 | S | Lint: flag `thread::spawn`, `Builder::spawn` and sleep loops outside `exec` and the allow-list; record lessons. |

## Open decisions for the user

1. **Folder watching:** keep the OS `NotifySource` (event-driven, low latency) and only remove the 50 ms wake loop, or
   move folders onto the poll-backed source too ("same mechanism on every OS"), accepting up to one interval of delay?
2. **Tiers:** two OS tiers (foreground, background) as proposed, or a third for `Idle`?
3. **Defaults:** aging 30 s, stuck grace 2 s, reserved Interactive 1, written-off cap 8, lease cap 64.
4. **Store-worker priority (WP-9):** a wire-contract change in Debrief; do it with this ADR or later?
5. **Pitwall/Spotter adoption (WP-11):** now, or after Debrief and Dyno?
