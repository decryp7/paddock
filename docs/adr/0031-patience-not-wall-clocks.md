# 31. Patience, not wall clocks: stalls judged on progress, CPU time and run state, one injected policy, tests wait on conditions

- Status: Accepted (2026-10-29), by the user, after independent review rounds 1-3. The user's decisions are at the end
  (Decisions of the user).
- Date: 2026-10-29
- Related: ER-1 (one release at a time), ER-12 (no sleeps or raised timeouts in tests), ER-18 and ER-19 (live phases,
  the UI never times out), ER-20 and rule 51 (bounded parallelism), ER-21 / rule 40 (recurring bugs mean a missing design
  rule), rule 42 (share first), rule 48 (recorded measurements); ADR 0008 (long operations: phases, progress, cancel),
  ADR 0019 (limits explain themselves), ADR 0020 (shared executor, stuck watchdog), ADR 0021 (portable tests, condition
  waits), ADR 0024 (background work, per-pid monitoring), ADR 0026 (dialogs shown at a ready handshake), ADR 0029 (the
  shell owns the start; accepted, on `fix/dyno-startup-deadlock`); conformance checks C51 and up (C49-C50 are left free)
- Evidence: CI run 7096 (job 21092), its reproduction and fix on `fix/debrief-ai-tests-load` (`5ed40bdba`, `e576d0f8f`,
  `a9ea3cbd2`), the before/after CI runs recorded in
  [apps/debrief/docs/performance.md](../../apps/debrief/docs/performance.md) (rule 48, on that branch). Instance counts
  are a regular-expression scan of non-test Rust at `4109de2fd` (section 8 says how); they are raw line counts.

## Context

**What happened.** In CI run 7096 five `debrief-app` `ai::tests` ran past nextest's 90 s on the Mac runner (load about
119 on 14 cores). Alone each takes 2-4 s. A `sample(1)` of a reproduction showed the test waiting for its import job and
the import's parse threads waiting in the plugin supervisor's `lease` and in `debrief_ipc::launch` for a plugin process
to connect.

**Why slowness became failure.** The import waits on plugin processes under wall-clock bounds:

- the connect window: a child must connect within 15 s of being started (`SpawnSpec::connect_window`);
- the call deadline: 90 % of the call kind's budget (`deadline_for`), 4.5 s for a sniff, after which the process is
  stopped and the call counts as a failure of the plugin;
- the in-process watchdog: a frame older than its budget marks the plugin hung (`debrief_core::plugin::watchdog`);
- the supervisor: after a failure it restarts with backoff (0.5-8 s) and quarantines after 3 failures in 10 minutes.

A plugin that starts or answers late is stopped, restarted (late again, with one more process starting), and finally
quarantined; the import waits through every round. With plugins started 16 s late through a wrapper (no CPU load), one
test took **215 s**; with the test harness waiting on conditions, 146 s (nine cold starts, no restarts).

**The pattern.** The same shape caused earlier CI timing flakes (rule 40's budgets and Debrief's nextest waivers, CI 6381)
and hides in the product. On the user's Mac, App Tamer stops apps in the background. While Debrief or a child is stopped,
the monotonic clock keeps running; when it resumes, every open window and deadline has expired at once, children are
restarted, plugins quarantined, and files show as "not imported" although nothing was wrong. A loaded laptop does the
same more slowly.

**What a liveness bound must tell apart.** These states look alike on a wall clock:

| State of the child | Gets CPU? | Makes progress? | Verdict |
|---|---|---|---|
| Working | yes | yes | fine |
| Starved (runnable, the machine is busy) | little or none | little or none | fine: wait |
| Paused (stopped by the OS or App Tamer; or the host is paused) | no | no | fine: wait, say so |
| Spinning (busy loop) | yes | no | stuck |
| Deadlocked (blocked, waiting on itself or on nothing the host owes it) | no | no | stuck |

Elapsed time cannot separate these. Progress, CPU time and run state together can.

## Decision

### 1. Two kinds of bounds

- **Liveness bound:** "is this still working?" Connect windows, call deadlines, watchdog budgets, handshakes with a local
  child, our own binaries describing themselves. It becomes a **stall judgement** (sections 2-4) and never fires only
  because time passed.
- **Cap:** a limit the user would recognise as a product rule, with its value in Settings or in a spec, or a bound on
  something that cannot report progress (external I/O, shutdown). Caps stay time limits (section 6), but they run on
  host ticks so a pause does not spend them (shutdown and external bounds excepted, section 6), and when one fires it
  explains itself (ADR 0019).

### 2. The stall judgement for a child process

**Host tick.** The monitor thread (ADR 0024's per-pid monitor, one per host) wakes every *beat* (default 1 s). **Every
wake-up is exactly one tick**, however late it comes: a host paused for an hour counts one tick when it resumes, not
3,600, and a starved host that wakes every 3 beats still moves forward, one tick per wake-up. Ticks are the only unit of
liveness; seconds are never compared.

**Signals, read at each tick for each child with a call in flight or a start pending:**

1. **Call progress** (the first signal), from the call's own work, never from a timer: an item streamed, a callback
   answered, a lent file read, a `ctx.progress()` from the plugin's code, the handshake of a starting child. The SDK
   sends a `Progress` frame from those points only; it has no keep-alive thread. A child that sends frames without call
   progress is not making progress.
2. **CPU time** (the second signal): the child's user + system CPU time from `paddock-proc`'s per-pid probe.
3. **Run state:** runnable, blocked, waiting for I/O, stopped, exited (section 4 says how each OS reports it).

**The process's state is the combination of its threads' states:** any thread runnable means **Runnable**; no thread
runnable and at least one in uninterruptible wait means **IoWait**; every thread blocked means **Blocked**; the process
stopped means **Stopped** (section 4 says where each OS reports thread states).

**Counters are per call.** A child may serve several calls at once (a sniff beside an import). Each call has its own
blocked and spin counters, and a call gains a blocked tick or spin CPU only when **that call** made no progress since
the last tick; another call's progress does not reset it. The process's run state and CPU growth are shared by its
calls; the CPU growth of a tick is counted toward a call's spin limit only while that call is the one without progress
(with several calls without progress, each counts it: the limit is an upper bound, never an exact share).

**Judgement, per tick, for each call without progress since the last tick:**

- **Stopped** (paused): no tick counts; the job's phase says "Paused by macOS or another app" (the user's wording).
  Cancel still works (section 4).
- **Runnable and its CPU time did not grow** (starved), or **waiting for I/O**: no tick counts. A runnable child that got
  no CPU is not stuck; a busy disk is load.
- **Blocked** (sleeping): the tick counts toward the **blocked limit** N. If the host owes the call something (a callback
  in the pump, an outstanding lent-file read), the child is waiting on the host and no tick counts.
- **CPU time grew** (it ran and still made no progress): the CPU used since the last progress counts toward the
  **spin limit** C, in CPU-seconds, which load does not inflate.
- **Stuck** = blocked ticks reach N, or spin CPU-seconds reach C. Then it is handled as today: the call fails as a
  failure of the plugin, the process is stopped, the supervisor restarts with backoff and quarantines after 3.
- Any call progress resets both counters.

A child that is **alive and still starting** is judged the same way, with its start as the call: it is waited for while
it is starved or stopped; the fixed 15 s window goes.

### 3. In-process calls (the watchdog)

This covers Debrief's `plugin::watchdog` and `paddock-guard`'s call budget (`Guard`, `Failure::Hang`), both **L**
instances. An in-process plugin call runs on one of the host's threads; it cannot be stopped, only marked hung so that further
calls fail fast. Reading another thread's run state is not portable (Windows has none), and a deadlocked thread and a
starved thread both show no CPU growth. Decision: **the watchdog counts host ticks, not elapsed time: it is pause-proof
but stays load-sensitive.** This is acceptable because (a) the consequence is "fail fast", not a kill or restart that
feeds the load, (b) the watchdog thread runs at the same background class as the calls (ADR 0024), so on a starved host
it ticks less as well, and (c) the per-record transform budget (250 ms today) becomes a tick count of at least 3, far
from one beat. Where a platform reads the calling thread's CPU time cheaply (Linux `CLOCK_THREAD_CPUTIME_ID` through
`pthread_getcpuclockid`, macOS `thread_info(THREAD_BASIC_INFO)` on the thread's Mach port, Windows `GetThreadTimes`), a
frame whose thread used no CPU in a tick does not count that tick; this narrows the load sensitivity without making the
judgement depend on it.

### 4. Reading the run state (paddock-proc)

Read in the platform documentation and headers, not yet verified on every OS version; WP-A2's tests pin what each
returns.

- **macOS:** `proc_pidinfo(PROC_PIDTBSDINFO)`: `pbi_status == SSTOP` (4) is stopped. The process status is coarse
  (`SRUN` for most live processes), so runnable or blocked comes from the threads: `proc_pidinfo(PROC_PIDLISTTHREADS)`
  for the thread ids, then `PROC_PIDTHREADINFO` per thread, `pth_run_state`: `TH_STATE_RUNNING` is runnable,
  `TH_STATE_WAITING` blocked, **`TH_STATE_UNINTERRUPTIBLE` is IoWait** (`TH_STATE_STOPPED` and `TH_STATE_HALTED` count
  as stopped). CPU time:
  `PROC_PIDTASKINFO` `pti_total_user + pti_total_system`, in Mach absolute time units, converted with
  `mach_timebase_info` (on Apple silicon the units are not nanoseconds).
- **Linux:** the threads' states from `/proc/<pid>/task/*/stat` (field 3: `R` runnable, `S` blocked, `D` IoWait,
  `T`/`t` stopped), combined as above; `/proc/<pid>/stat` for `Z` (exited) and for the process's `utime + stime`
  (fields 14 and 15) over `sysconf(_SC_CLK_TCK)`.
- **Windows:** no stopped state for ordinary processes. A process frozen by the system reports `IsFrozen` in
  `PROCESS_EXTENDED_BASIC_INFORMATION` (`NtQueryInformationProcess`, `ProcessBasicInformation` with the extended size).
  CPU time: `GetProcessTimes`. Runnable against blocked is not available per process: on Windows a child without CPU
  growth counts toward the blocked limit only when the host is not starved itself (its own tick gaps and CPU growth say
  so), and the limit is set higher. This is the weakest platform, and the ADR says so. `paddock-proc::run_state` returns `Unknown` there for now: reading `IsFrozen` is deferred.
- **TODO:** verify which mechanism App Tamer uses to stop an app (a signal, or suspending the Mach task). If it suspends tasks rather than signalling, `pbi_status` may not read `SSTOP`; add the per-thread `TH_STATE_STOPPED` fallback (`thread_info`) then. Until verified, only a SIGSTOP-stopped process is known to be detected.
- **Cancel of a stopped child:** Cancel never sends `SIGCONT` (the OS or the user paused it on purpose). It closes the
  lifeline and kills after `KILL_GRACE` as today: `SIGKILL` ends a stopped process, while `SIGTERM` would stay pending
  until it is continued. On Windows `TerminateProcess` ends a frozen process.

### 5. One injected policy: `Patience`, on the existing time types

**The pure policy in `paddock-jobs`** (module `patience`), built on `paddock-jobs::time` instead of beside it:

```rust
/// Which bound is judged: an opaque key the app defines ("plugin.sniff", "worker.start"), never an app's own enum.
pub struct CallKey(pub std::borrow::Cow<'static, str>);

/// The thresholds of one key; `None`: wait on the condition only (tests).
pub struct Stall { pub blocked_ticks: Option<u32>, pub spin_cpu: Option<Duration> }

pub trait Patience: Send + Sync {
    fn beat(&self) -> Duration;
    fn stall(&self, key: &CallKey) -> Stall;
}

/// Port: ticks of the host, as section 2 defines them.
pub trait TickSource: Send + Sync {
    /// Ticks counted so far (a paused or starved host adds none).
    fn ticks(&self) -> u64;
}

/// Port: what the OS says about a process (paddock-proc's probe, adapted in paddock-ipc).
pub trait RunStateProbe: Send + Sync {
    fn sample(&self, pid: u32) -> Option<RunSample>;
}
pub struct RunSample { pub state: RunState, pub cpu: Duration }
pub enum RunState { Runnable, Blocked, IoWait, Stopped, Exited }

/// The judge: fed one observation per tick per watched call, answers Working, Waiting(why) or Stuck(why).
pub struct StallJudge { /* pure state machine: no I/O, no clock */ }
```

- **`HostTickClock`**, an implementation of the existing `Clock` whose `monotonic()` advances by one beat per host tick.
  It makes the existing `Deadline` and `Backoff` pause-proof without touching their code. **Only waits the host does
  itself run on it:** the user-facing caps (section 6) and the supervisors' backoff. Shutdown bounds (S) stay on the
  real monotonic clock (quitting must end in real time). External bounds (X) keep the OS's or the library's own
  timeouts (a socket or HTTP timeout, a tool's run limit); the host-tick clock is not passed into them. The simpler option, "only a host-tick clock everywhere", was considered and rejected as
  the whole answer: it fixes pauses but not starvation, which is what CI 7096 was.
- **`Deadline`** stays for caps (on the host-tick clock) and is not used for liveness. **`Backoff`** is reused unchanged.
- **`Patience::product()`** holds the thresholds per key. N (blocked ticks) starts from today's values (a child start:
  15 ticks). **C (the spin limit) is set per key from measurements:** WP-A3 measures, for every built-in call kind, the
  largest CPU time between two progress reports on the synthetic corpora, and C is that with a margin of 4x (recorded
  under rule 48). Both are tunable in Settings under rule 51 (Auto default, validated in the engine).
  **`Patience::until_condition()`** returns no thresholds for any key: tests.
- **Layering and dependency edges.** `paddock-jobs::patience` is pure (policy, ports, judge) and adds no dependency.
  The OS probe goes in `paddock-proc` (`paddock_proc::runstate`, with its own types; `paddock-proc` gains no dependency).
  `paddock-ipc` already depends on both and adapts the probe to the port. `paddock-plugin-host` and `paddock-guard` take
  `Arc<dyn Patience>` from `paddock-jobs`, on which they already depend. New edges: **`paddock-startup -> paddock-jobs`**
  (WP-B3; `paddock-startup` depends on `paddock-commands`, `paddock-env` and `paddock-log` today, and `paddock-jobs`
  depends on none of them back, so no cycle), and in Debrief `debrief-ipc -> paddock-jobs` (WP-B4; it already depends on
  `paddock-proc`). No other edge among the shared crates.

### 6. Bounds that stay time limits

- **User-facing caps (C), on host ticks:** the elevated-call cap (`MAX_ELEVATED_CALL`, 1 h), a tool operation's declared budget
  (`OpSpec::budget`, `DEFAULT_OP_BUDGET`), the store client's `LONG_TIMEOUT` and `WAIT_TIMEOUT`, `paddock-plugin-host`'s
  `NO_BUDGET` (1 h, the longest a call without a budget is waited for), import size, memory and count caps.
- **External I/O (X), on the OS's or library's own timeouts:** I/O that cannot report progress: other hosts over the network (`paddock-net`, the updater's
  `MANIFEST_TIMEOUT`, downloads, Spotter's `REQUEST_TIMEOUT` and mail timeouts), OS tools we run but do not control
  (`paddock-files` `OPENER_TIMEOUT`, `paddock-download` `TOOL_TIMEOUT`, `paddock-proc` `SHELL_TIMEOUT`, Dyno's platform
  tool timeouts), the privileged helper and services (`paddock-service` `HELPER_TIMEOUT`), the MCP loopback handshake
  (`paddock-mcp` `HANDSHAKE_TIMEOUT`: the other end is a client we do not ship), notification channels
  (`paddock-channels` `DEFAULT_TIMEOUT`). They fail with Retry and never count toward a quarantine.
- **Shutdown and teardown (S), on the real monotonic clock:** `KILL_GRACE`, `EXIT_WAIT`, `CANCEL_GRACE`, `paddock-window` `TEARDOWN_STEP_LIMIT` and
  `EXIT_STALL_LIMIT`, Pitwall's `EXIT_STALL_LIMIT`. Quitting must end.
- **UI handshakes that change only what is shown (U), on host ticks:** `paddock-dialogs` `SHOW_DEADLINE`, `paddock-startup`
  `dialog_show_deadline` (ADR 0026: the window is shown anyway).
- **True hangs:** still found, by section 2.

### 7. Instances, family-wide

Classes: **L** liveness (becomes a stall judgement), **C** cap (stays, on host ticks), **X** external I/O (stays),
**S** shutdown (stays), **U** UI handshake (stays), **T** test harness (`until_condition()`). The rows are the bounds
that decide restarts, failures or quarantine, and their neighbours; section 8's check covers every site.

| Where | Bound | Class | Note |
|---|---|---|---|
| Debrief `debrief-ipc` `SpawnSpec::connect_window` (15 s), `launch` | child start | L | CI 7096 |
| Debrief `debrief-ipc` `link::HANDSHAKE_TIMEOUT` (10 s), host and child side | handshake | L | on the child side, the host is what may be paused |
| Debrief `debrief-ipc` `supervisor` `crash_window` (600 s) | quarantine window | C | on host ticks |
| Debrief `debrief-plugin-host` `remote::deadline_for` (90 % of the budget) | every plugin call | L | one key per call kind |
| Debrief `debrief-plugin-host` `remote::DEFAULT_DEADLINE` (10 s) | config sniff, settings | L | |
| Debrief `debrief-plugin-host` `document.rs`, `profile.rs` `SNIFF_DEADLINE` (10 s) | sniff | L | |
| Debrief `debrief-plugin-host` restore `ApplySettings` (10 s) | after a restart | L | |
| Debrief `debrief-plugin-host` `MANIFESTS_TIMEOUT`, `installed::PROBE_TIMEOUT` (30 s) | our binary describing itself | L | |
| Debrief `debrief-plugin-host` `PUMP_WAIT` (120 s) | a pumped callback | L | the host owes the child: no blocked ticks for the child |
| Debrief `debrief-core` `plugin::watchdog` with `guard::Budgets` | in-process call | L | section 3 |
| Debrief `debrief-plugin-host` `PluginSpec::connect_window` (15 s, copied into `SpawnSpec`) | plugin process start | L | |
| Debrief app `startup_watchdog::READY_TIMEOUT` (10 s) | a window's page-ready handshake | L | the webview is ours; judged on the host's ticks |
| Debrief `debrief-plugin-api` `serve::REPORT_TIMEOUT` (10 s), child side | task reports to the host | L | judged on the host's ticks |
| Debrief app `ai::runtimes::REPORT_TIMEOUT` (90 s) | the AI worker checking every installed build | L | `llama-server --list-devices` per build |
| Debrief app `engine::import::BUSY_WAIT` (300 s) | an import waiting for the store's running import | L | the running import's progress is the progress |
| Debrief `debrief-store-duck` `tail::TAIL_WAIT` (30 s) | live tail waiting for a running import | L | same |
| Debrief `debrief-ai` `worker::frames::CONNECT_WINDOW` (15 s) | AI worker start | L | |
| Debrief `debrief-store-client` `CALL_TIMEOUT` (120 s) | store call | L | `LONG_TIMEOUT`, `WAIT_TIMEOUT`: C |
| Debrief, child side: `debrief-plugin-api` `host_file::READ_TIMEOUT` (60 s), `output::ACCEPT_WAIT` (30 s) | the child waiting on the host | L | judged on the host's own ticks and run state |
| Debrief, child side: `debrief-plugin-api` `serve::ELEVATION_PROMPT_WAIT` (120 s) | the user's elevation prompt | C | |
| Debrief `debrief-plugin-dotnet` `RUNDOWN_TIMEOUT`, `DRAIN_TIMEOUT`, `connect_timeout`, elevated `CALL_TIMEOUT` | a .NET target process | X | the target is the user's process |
| Debrief `debrief-plugin-test` `CALL_TIMEOUT` (60 s), `connect_window` (15 s), conformance `IMPORT_TIMEOUT` | SDK test kit | T | |
| Debrief test apps (`test_support`) | `UNTIL_CONNECTED`, `Budgets::unlimited()` | T | become `until_condition()` |
| `paddock-ipc` `handshake::HANDSHAKE_TIMEOUT` (10 s) | handshake | L | |
| `paddock-ipc` `peer::CANCEL_GRACE` (2 s) | cancel | S | |
| `paddock-plugin-host` per-call budgets; `NO_BUDGET` (1 h) | plugin call | L; C | |
| `paddock-guard` call budget (`Guard` `budget`, `Failure::Hang`) | an in-process guarded call | L | judged like section 3 |
| `paddock-guard` quarantine window | quarantine | C | already on the injected `Clock`: give it the host-tick clock |
| `paddock-startup` `ready_timeout` (10 s), `terminate_window` (60 s), `dialog_show_deadline` (1 s) | the page-ready handshake of a starting window (ready), the end of a stalled start (terminate), a dialog shown anyway (show) | L; S; U | ADR 0026, 0029 |
| `paddock-chromium` `START_LIMIT` (20 s), `START_PAGE_WAIT` (5 s), `SELF_TEST_LIMIT` (30 s); `CLOSE_GRACE` (5 s) | the research and test browser | L; S | Chromium is ours to start: judged on its run state |
| `paddock-service` `HELPER_TIMEOUT` (30 s), `ELEVATED_TIMEOUT` (120 s), `STOP_VERB_TIMEOUT` (10 s) | privileged helper, OS service manager | X | the OS and the user's prompt |
| `paddock-mcp` `loopback::HANDSHAKE_TIMEOUT` (10 s) | MCP client | X | |
| `paddock-channels` `dispatch::DEFAULT_TIMEOUT` (15 s) | notification channels | X | |
| Dyno app `link` `REQUEST_TIMEOUT` (15 s), `SPAWN_WAIT` (5 s), `CONNECT_GRACE` (8 s); `dyno-ipc` `CONNECT_TIMEOUT`, `PUSH_TIMEOUT` (5 s) | app to agent | L | the agent is our child or service |
| Dyno `link` `LOG_READ_TIMEOUT` (150 s), `EXTRAS_TIMEOUT` (120 s); `service_repair` `GONE_WAIT`, `START_WAIT`; `health` `REPAIR_WAIT` | logs, repair | C | user-started |
| Dyno `dyno-agent` `CATALOG_DEADLINE`, `READ_DEADLINE`, `RESOLVE_DEADLINE`, `STAT_LIMIT`; `dyno-platform` tool timeouts; `dyno-cli` `CALL_TIMEOUT`, `LIVE_TIMEOUT` | OS counters and tools; the CLI | X; L | OS: X; CLI to agent: L |
| Pitcrew `pitcrew-service` `HELLO_TIMEOUT` (30 s); `VERSION_TIMEOUT`, `REGISTER_TIMEOUT`, hygiene `LIST_TIMEOUT`, `PRUNE_TIMEOUT`; `service_repair` waits | runner service | L; X; C | hello: L; runner and Docker tools: X; repair: C |
| Pitwall `pitwall-loopback` `connect_timeout` (3 s); `pitwall-server` engine start (300 s), `serving` timeouts; `live::OPEN_TIMEOUT` | inference service and engines | L | engines judged on their run state |
| Pitwall `quit::EXIT_STALL_LIMIT`; `service_repair` waits | quit, repair | S; C | |
| Pitwall `updates::REPO_TIMEOUT`; `pitwall-cli` `DEFAULT_TIMEOUT`, `LONG_TIMEOUT` | model hosts; the CLI | X; C | |
| Spotter `spotter-render` `START_LIMIT` (20 s); `CLOSE_GRACE` | renderer | L; S | |
| Spotter `spotter-engine` `CHANNEL_TIMEOUT`, mail `STEP_TIMEOUT`, `TOTAL_TIMEOUT`; `spotter-sources` `REQUEST_TIMEOUT`, `LIST_TIMEOUT`; app `notifier::COMMAND_TIMEOUT` (300 s) | channels, sources, the user's command | X; C | |

### 8. The check over every instance

A conformance check, **C51**, over all non-test Rust in `crates/`, `apps/*/crates`, `apps/*/app/src-tauri` and `tools/`.
It flags:

- a wait with a `Duration`: `recv_timeout`, `wait_timeout`, `wait_timeout_while`, `tokio::time::timeout`;
- a call with a time bound: `.request(.., Some(d))`, `.call(.., Some(d))` and the like;
- `Deadline::after`, `Clock::wait` (`clock.wait(d, ..)`);
- `Instant::now() + d` and comparisons with `.elapsed()`;
- constants **and struct fields** of type `Duration` or `Option<Duration>` whose name contains TIMEOUT, DEADLINE, WINDOW,
  WAIT, LIMIT, GRACE or BUDGET.

A flagged site passes when its bound comes from `Patience`, or when the line or the item above it carries `// cap:`,
`// external:`, `// shutdown:`, `// ui:`, `// hang-guard:` (a test-only guard against a hang) or **`// pace:`** with a reason; the
comment may be anywhere in the comment block directly above the line. `pace` is for throttles and intervals that bound
nothing (a poll interval, a progress throttle, a debounce): they decide how often, not how long. Test code is exempt
from the comment, not from ER-12. Today's sites are a waiver list that may only shrink, as C40-C42 do, with targets:

- end of phase A (WP-A4): no Debrief site left uncommented or unjudged (Debrief's 349 seed sites);
- end of WP-B3: no shared-crate site left (the shared crates' share of the seed);
- end of phase B (WP-B5): the list is empty; C51 then fails on any new uncommented site.

**Seed of the waiver list** (`4109de2fd`; raw line counts of the patterns above, with in-file `#[cfg(test)]` modules cut
off and `tests/`, `benches/`, `examples/` and `fuzz/` skipped; a line matching two patterns counts twice; the check's own
list is authoritative): **909** sites. Named constants and fields 576, `Instant + d` 105, `recv_timeout` and
`wait_timeout` 87, `elapsed` comparisons 85, `Deadline` and `Clock::wait` 22, `request(.., Some(d))` 17,
`tokio::time::timeout` 17. By area: Debrief 349, Dyno 148, Pitwall 54, Spotter 46, Pitcrew 46, `paddock-ipc` 44,
`paddock-chromium` 28, `paddock-download` 21, `paddock-service` 19, `paddock-jobs` 18, `paddock-net` 15,
`paddock-plugin-host` 14, `paddock-proc` 11, the other shared crates and `tools/` 76.

### 9. Mixed versions

Services and apps update separately (Pitwall's service, Dyno's agent, Pitcrew's service), and Debrief's third-party
plugins are built with older SDKs. The `Hello` frame gains a capability bit, `progress`. A peer that does not set it is
never refused: its calls are judged on the reply and on what the OS says (CPU time and run state still give the spin and
blocked limits without the peer's help), and its start on run state and CPU time. The `Progress` frame is sent only to a
host that set the bit. No frame version bump is needed for a capability bit; the protocol version is raised once, in
WP-B2, for the new frame type.

## Parallelism and cost (rule 51)

- **Cost of the judgement:** one `Progress` frame where the call's code already does work (no timer), and per watched
  child per beat, on the monitor thread ADR 0024 already runs (no new thread), only while the child has a call in flight
  or a start pending:
  - macOS: one `PROC_PIDTBSDINFO`, one `PROC_PIDTASKINFO`, one `PROC_PIDLISTTHREADS` and one `PROC_PIDTHREADINFO` per
    thread: system calls only, no table of all processes (the cost ADR 0024 measured and removed);
  - Linux: one read of `/proc/<pid>/stat` and one of `/proc/<pid>/task/<tid>/stat` per thread;
  - Windows: one `GetProcessTimes` and one `NtQueryInformationProcess`.

  A plugin process has a handful of threads, so a beat costs tens of system calls per watched child. WP-A2 measures it
  and records it (rule 48), with a budget of 0.1 % of a core per watched child.
- **Waiting instead of restarting is cheaper on a busy machine:** in the reproduction the restarts cost 69 s of 215 s and
  started extra processes.
- **Plugin reuse (rule 51 b, "can it be cheaper?").** Most of the remaining 146 s were plugin processes started one after
  another on first use, as a file is sniffed by each importer in turn. In the product the processes are shared by every
  window and warmed up in parallel at start (`Builtins::warm_up`). Two options, measured in WP-A5 before either is built:
  (a) sniff only with the plugins whose manifest says they could match the file (extension or magic bytes), so a plugin
  that cannot match is never started; (b) start the matching plugins of an import in parallel, not in turn (rule 51 a).
  Tests cannot reuse plugin processes across tests (one process per test); they bound their load instead
  (`app-plugin-processes`, at most 6, on `fix/debrief-ai-tests-load`).

## Consequences

- A paused or busy app waits, says it is waiting, and goes on. Cancel works in every phase (ER-18) and never resumes a
  paused child.
- A real hang is still found: a deadlock after N blocked ticks, a busy loop after C CPU-seconds without progress.
  Neither depends on how busy the machine is (Windows: section 4).
- The IPC protocols gain a `Progress` frame, a "starting" frame and a capability bit (section 9).
- Tests stop failing because the machine is busy; a hung test is still stopped by nextest's budget (rule 40).

## Sequencing (ER-1)

- **Phase A, Debrief only, now** (Debrief 1.2): WP-A1 to WP-A5. The judge, the ports and the probe are written in
  `debrief-ipc` (`debrief_ipc::patience`, `debrief_ipc::runstate`) with the API of section 5, so the later move is a
  rename. That copies what will be shared, for one release: a rule 42 exception, recorded here with its end, WP-B4.
- **Phase B, shared crates, only after the Dyno, Spotter and Pitwall 1.1 tags:** WP-B1 to WP-B5, including the
  `paddock-ipc` protocol change. Nothing in phase B is on those releases' paths.

## Alternatives considered

- **Raise the timeouts.** Rejected: ER-12, and it only moves the threshold.
- **Retry on timeout.** Rejected: the retries are the amplifier.
- **Scale timeouts by the load average.** Rejected: load does not tell a paused process from a running one.
- **A host-tick clock alone.** Kept as part of the answer (caps and backoff run on it), rejected as the whole answer: it
  does not handle a starved child.
- **CPU time alone.** Rejected: a deadlocked child and a stopped one both use none; run state tells them apart.
- **SDK keep-alive frames.** Rejected: a timer thread keeps sending while the call's code is stuck.

## Work packages

Phase A, Debrief (about 9.5 days):

1. **WP-A1** `debrief_ipc::patience`: `CallKey`, `Stall`, `Patience` (`product()`, `until_condition()`), `TickSource`,
   `RunStateProbe`, `StallJudge`, `HostTickClock`. Tests with a hand-driven tick source and a **fake probe**: starved
   (runnable, no CPU growth) is not stuck; stopped is not stuck; waiting for I/O is not stuck; blocked for N ticks is
   stuck; spinning for C CPU-seconds is stuck; a **heartbeat without call progress** is stuck; a paused host counts one
   tick on resuming; **a host waking every 3 beats still reaches N**; **a stuck call beside a progressing call is still
   stuck**; thread states combine as section 2 says; the host owing a callback counts no ticks; Cancel of a stopped
   child kills it and sends no `SIGCONT`. About 2.5 days.
2. **WP-A2** `debrief_ipc::runstate` for macOS, Linux and Windows (section 4), each tested on its CI runner with a real
   child that is stopped, blocked, spinning and in uninterruptible wait; the tests pin the mapping of each OS state,
   **`TH_STATE_UNINTERRUPTIBLE` to IoWait on macOS** and `D` to IoWait on Linux, and measure the cost per beat (rule 48).
   About 2 days.
3. **WP-A3** Debrief's SDK and IPC: `Progress` and "starting" frames from the call's work, the `progress` capability bit;
   the host's launch, supervisor and plugin-host deadlines, `SNIFF_DEADLINE`, `DEFAULT_DEADLINE`, the restore call,
   `MANIFESTS_TIMEOUT`, `PROBE_TIMEOUT`, `PluginSpec::connect_window`, the AI and store worker launchers, the app's
   `startup_watchdog::READY_TIMEOUT`, the SDK's `REPORT_TIMEOUT`, the AI runtimes' `REPORT_TIMEOUT`, `BUSY_WAIT` and
   `TAIL_WAIT` on the judge; caps on `HostTickClock`; the watchdog on ticks (section 3); the "Paused by macOS or
   another app" phase. Measure C per
   key (section 5) and add a test that **every built-in long call reports `ctx.progress()` within its C** on the
   synthetic corpora. About 3.5 days.
4. **WP-A4** Debrief's tests on `until_condition()`; C51 with Debrief's waiver list. About 1 day.
5. **WP-A5** Verify on the user's Mac (a local VM or the real app, never CI): App Tamer pausing Debrief during an import,
   a loaded machine; measure the plugin-reuse options and record them (rule 48). About 0.5 day.

Phase B, shared crates, after the tags (about 9 days):

6. **WP-B1** `paddock-jobs::patience` (moved from WP-A1) and `paddock_proc::runstate` (moved from WP-A2). About 1.5 days.
7. **WP-B2** `paddock-ipc`: `Progress` and "starting" frames, the capability bit, the judge in launch and peer, the probe
   adapter; the protocol version raised once. About 2 days.
8. **WP-B3** `paddock-plugin-host`, `paddock-guard` (host-tick clock), `paddock-startup`, `paddock-chromium`. About 2 days.
9. **WP-B4** Debrief onto the shared crates (`debrief-ipc -> paddock-jobs`), removing the phase A copies. About 1 day.
10. **WP-B5** Dyno, Pitcrew, Pitwall and Spotter: their L rows of section 7 on the judge, their C, X, S and U rows
    commented; C51 over the whole family. About 2.5 days.

Total about 18.5 days.

## Decisions of the user (2026-10-29)

(a) **Third-party plugins without `Progress` frames** (built with an older SDK) are judged on CPU time and run state only
    (section 9): accepted. A long silent call of theirs counts toward the spin limit.
(b) **The phase shown while a child or the app is stopped:** "Paused by macOS or another app".
(c) **Debrief first:** phase A in Debrief 1.2; phase B after the Dyno, Spotter and Pitwall 1.1 tags.

## Amendment 2026-10-30: phase A is built in the shared crates

**Why.** Debrief 1.2 moves onto the shared plugin platform (family ADRs [0001](0001-plugin-platform-crates.md) and
[0003](0003-debrief-migration-and-users.md)), and its package PH1 deletes `debrief-ipc`. Phase A's judge and probe would
be written into a crate that the same release deletes, and then copied again in WP-B4. ER-22 (share first) asks for one
shared implementation instead. The "rule 42 exception" under Sequencing is therefore withdrawn.

**What changes.** Phase A's stall judge is built in the shared crates (`paddock-jobs`, `paddock-proc`, `paddock-ipc`,
`paddock-plugin-host`, `paddock-guard`), not in `debrief-ipc`, as packages of the
[Debrief 1.2 plan](../plans/debrief-1-2.md):

| Work package here | Plan package | Notes |
|---|---|---|
| WP-A1 (judge, ports) and WP-A2 (run state per OS) | **2.6c** | written directly as WP-B1: `paddock-jobs::patience`, `paddock_proc::runstate` |
| WP-A3, plugin path; WP-B2 (`paddock-ipc` frames); WP-B3 for `paddock-plugin-host` and `paddock-guard` | **2.6d** | the `Progress` frames land before Debrief 1.2 ships protocol 2.0, so the protocol stays 2.0 (family ADR 0001, "Versioning and compatibility") |
| WP-A3, the rest: store and AI worker bounds, the AI runtimes' check, `BUSY_WAIT`, `TAIL_WAIT`, the ready handshake, the in-process watchdog, the "Paused" phase, C per key | **LV1** | |
| WP-A4 (tests on `until_condition()`, C51 for Debrief) | **LV2** | |
| WP-A5 (verification on the user's Mac) | **LV3** | before Debrief 1.2's tag |
| WP-B4 (Debrief onto the shared crates) | none | nothing to move: there are no phase A copies |
| WP-B3 for `paddock-startup` and `paddock-chromium`; WP-B5 (the other apps) | unchanged | phase B, each app with its next release; `paddock-startup`'s `ready_timeout` goes with LV1 if Debrief is on the shell by then |

**Sequencing.** Decision (c) is kept in substance. The shared work happens in Debrief 1.2, which comes after the Dyno,
Spotter and Pitwall 1.1 tags in the agreed order, so nothing here sits on those releases' paths. Decision (a) no longer
applies to Debrief's old plugins: plugins built for protocol 1 are removed and refused (family ADR 0003), and every
protocol-2.0 plugin is built with the `Progress` frames. The plan counts the work as 2.6c, 2.6d and LV1-LV3, about
13 days, against about 14 for phase A (9.5), WP-B1 (1.5), WP-B2 (2) and WP-B4 (1) done separately; part of WP-B3 is in
2.6d.

**Correction to §9 (2026-10-30).** §9 says a capability bit in `Hello` needs no frame version bump. That is wrong on this
wire: postcard has no field count, so a field appended to `Hello` fails the handshake both ways (an older host reports
"trailing bytes" and counts a crash; a newer host reading an older `Hello` reports "Hit the end of buffer";
[ipc-wire.md](../ipc-wire.md) §16, measured). The capability is carried as decided in family ADR 0001 ("Versioning and
compatibility"): frame version 3, whose `Hello` and `Welcome` end with a `caps: u32` bit set (bit 0 `progress`); a
version-3 host accepts version-2 peers without capabilities, and a version-3 client refused with `version` reconnects
once as version 2. The rest of §9 stands: a peer without the bit is never refused and is judged on CPU time and run
state. Package 2.6d builds it.

## Amendment 2026-10-30: the shared start judge (`paddock-proc`)

**Decision of the user (2026-10-30):** build the shared part for starts now, so every app uses it (Dyno 1.0 first, then
Spotter and the browser driver). It is deliberately small: what Dyno, Spotter and `paddock-chromium` each wrote beside
each other (Dyno `link.rs`, Spotter `StartJudge`, `paddock-chromium`'s fixed 20 s `START_LIMIT`). The rest of phase B
(`paddock-jobs::patience`, the plugin path) is unchanged.

**API (all in `paddock-proc`, pure core and thin edges).**

- `StartJudge` (pure): one `Reading` per tick (`Liveness` Running/Paused/Exited{status, stderr tail}, the `ready`
  handshake, an app-defined `progress` counter, `Cpu` NotRead/Unavailable/Used) gives a `StartState`: Starting, Ready,
  Stalled(`StallReport`), Exited(`ExitInfo`). Spin (CPU used without becoming ready), idle (no CPU and no progress) and
  the note after N ticks are counted in observations, never wall seconds: a paused host counts one tick (section 2).
  `Patience` holds the limits. The only time-based bound is `hang_guard`, used when the OS gives no CPU reading at all;
  so a start is always bounded, also without a process identity, except while the process is stopped (the caller's cap bounds that). A hung process with timer threads can keep using a
  little CPU and reset the idle count: the caller's cap is the bound for that.
- `StartWatch` (edge): a `StartProbe` (liveness, ready, progress, CPU) and a `Pace` (the clock) drive the judge until
  Ready, Exited, Stalled or Cancelled; after a stall it can be called again.
- `StallReport { cause, action, message }`: `StallCause` Slow, Spinning, Idle, Hung, Paused (stopped by the OS or another app: not stuck, Wait), WaitingForSupervisor, HeldByOther;
  `StallAction` Wait, Retry, ShowLog. Apps map it to their health chip and Retry.
- `InstanceLock::claim(path)` gives `Claim::Acquired | Held | Unsupported` from an OS lock (`File::try_lock`: `flock`,
  `LockFileEx`); `is_held(path)`. The lock is the authority.
- `hint`: a discovery file is only a hint (rule 53). `Hint` (pid, start), `written_since`, `names_live` (new, or
  answers), `remove_if_same`, `remove_if_mine`, `write_private_atomic`, `read_bounded`.
- `tree_cpu_time_from(source, root)`, `ProcessSource::cpu_micros(pid)`: the CPU time of a tree by identity, one pid at
  a time (macOS `proc_pidinfo(PROC_PIDTASKINFO)` with the timebase, Linux `/proc/<pid>/stat` fields 14 and 15, Windows
  `GetProcessTimes`), `tree_cpu_time_of` for a job's members.

**What it replaces.** Dyno: `instance.rs`'s lock, `start_progress`, `written_since`/`names_live_agent`,
`Discovery::remove_if_same/mine`, the "still starting" note. Spotter: `StartJudge`/`StartPatience`. `paddock-chromium`:
`START_LIMIT`/`start_limit` and the port-file wait. Pitwall, Pitcrew, Debrief: their fixed spawn waits, as C51 lists them.

**Parallelism and cost (ER-20).** The judge is arithmetic on one reading per tick. The stopped-state read (`run_state`: one `proc_pidinfo` or `/proc/<pid>/stat` read) runs on the same `cpu_every` cadence as the CPU time and is kept between reads. The probe's cheap signals (a liveness
check, a file read) run every tick (50 ms); the CPU time is read every `cpu_every` ticks (1 s) and costs one system call
per process of the tree, never a table scan (ADR 0024 rule 4): about 0.1% of a core for a browser's tree, against about
2% for a table scan per beat (Spotter's first version). Nothing runs in parallel: one watcher thread per starting
process, at the caller's priority. Cheaper than reading the CPU is not reading it while the handshake is near: the watch
reads the handshake first. Measurements are recorded when the apps adopt it (rule 48).

**Conformance C51** starts as the narrow first step of section 8, in report-only mode (warn): it flags fixed *start and
spawn* waits only (names with START, SPAWN, LAUNCH or READY, and waits inside `start*`, `spawn*`, `launch*` and `boot*`
functions), not every `Duration` wait the section lists. It widens to section 8's patterns and turns to failing with the
waiver-list targets above.

*Amendment 2026-10-31 (C51 widened, still report-only).* C51 also flags a readiness poll by shape in any function (a
deadline in a loop that checks a child's liveness or reads readiness), the start-like names connect, handshake, wait_for,
ready and await, and `Timeout("... start ...")` literals. A run limit that kills its child on timeout is still a fixed wait:
it says so with `// cap: <why>`. The skips (cdp.rs, run/kill/close/stop-style function names, test files and items,
fake and fixture binaries) are listed in `conformance/README.md` and the check's header. Cause: a fixed 20 s in a browser
driver's `connect()` failed CI three times and the check missed it.

**Semantics of the shared judge, for adopters.** A stall is reported once per episode (the slow note once ever), so a
watch that returned a stall waits for the next event when called again. The hang guard also counts ticks in which the
caller never read the CPU time, so a caller that passes no CPU reading is still bounded. A start that is slow but alive
is no longer failed by the browser driver: a broken browser that does nothing is now given up on after the idle patience
(60 s) or, when it cannot be observed, the hang guard (300 s), not after 20 s.
