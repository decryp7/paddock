# 4. CPU taming: rules with an escalation ladder, and the per-OS ways of holding a process back

- Status: superseded by ADR 0021 (2026-10-28); Dyno no longer changes processes. Kept for history
- Related: ADR 0002, 0003 (groups), 0006 (observe-only), `dyno_core::rules`, `dyno_core::protect`,
  `dyno_platform::unix_control`, `dyno_platform::windows::control`

## Context

The user wants Dyno to be able to calm a runaway or background program without killing it: a sync client eating a
core, an indexer during a game, a compiler starving a call. Windows, macOS and Linux offer different mechanisms with
different strength and different failure modes, and some programs (antivirus, the audio engine) must never be touched.
Anything Dyno holds back must be released when the cause is gone, when Dyno stops, and even after a crash.

## Decision

### Rules (pure, in the core)

1. **A rule is data**: a condition (CPU threshold of an app, target, optional context such as user idle or on
   battery), a ladder of steps (`LowerPriority`, `EfficiencyMode`, `CpuCap(percent)`, `Notify`) with delays, and a
   release time. Rules ship **disabled**; profiles (Normal, Gaming, Meeting, On battery, Building code) say which rules
   are active.
2. **The engine is a pure state machine per (rule, app)** fed one tick at a time. An app above the threshold for
   `for_secs` gets the first step; each later step follows `after_secs` after the previous while the condition holds.
   Steps add up. After the app stays below the threshold for `release_after_secs`, or when the rule stops applying
   (disabled, paused, other profile, context false, the app comes to the foreground), every step is undone in reverse
   order. Processes joining the group later get the steps already taken. A manual undo snoozes the pair until the app
   drops below the threshold. The engine returns `Command`s; the agent executes them through `ProcessControl`.
3. **The protected list is absolute** (`dyno_core::protect`): critical OS processes, the antivirus (Defender, XProtect),
   the audio engine, Dyno's own executables, and the user's entries. Rules and clicks both pass through it, and
   rules never touch the foreground app.

### Methods per OS (`ProcessAction`)

| Action | Windows | macOS | Linux |
| --- | --- | --- | --- |
| Lower priority | `SetPriorityClass` (Below normal, …) | `setpriority` (nice) | `setpriority` (nice) |
| Efficiency mode | EcoQoS (`PROCESS_POWER_THROTTLING_EXECUTION_SPEED`) plus idle priority, as Task Manager does | background policy through `setpriority(PRIO_DARWIN_PROCESS, PRIO_DARWIN_BG)` | nice 10 at least (never raises an already lower priority) plus idle I/O class (best effort) |
| Hard CPU cap | a **named job object** per target with CPU rate control (`JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP`, 1/100 %) | **duty cycle**: `SIGSTOP`/`SIGCONT` from one thread per capped process, **100 ms period** | same duty cycle |
| Suspend / resume | every thread (`SuspendThread` via a Toolhelp snapshot), stacked suspensions undone on resume | `SIGSTOP`/`SIGCONT` | `SIGSTOP`/`SIGCONT` |
| Affinity | `SetProcessAffinityMask` | unsupported (the system decides) | `sched_setaffinity` on online cores |
| End | close windows, wait 2 s, then `TerminateProcess` | `SIGTERM`, 2.5 s, then `SIGKILL` | same |

Every action first compares the live process's start time with the `ProcessRef`; a reused pid is `NotFound`.

### Why these methods

- **Job objects** are the only supported hard CPU limit on Windows that does not need the process's cooperation or a
  driver. A process cannot leave a job, so *releasing* a cap switches the job's rate control off rather than moving the
  process.
- **A SIGSTOP duty cycle** is the only portable cap on Unix without cgroups (which need root or systemd delegation).
  At 100 ms the process runs `pct` ms of every 100 ms. It is coarse: a capped GUI app visibly stutters, and a
  process holding a lock or a network peer waiting on it sees stalls. That is why the ladder tries priority and
  efficiency mode first and why a cap is the last step.
- **Background QoS / EcoQoS / nice** move work to efficiency cores or lower its scheduling priority without stopping it.
- **Suspending a process that holds locks or talks to others can wedge them**; suspend is an explicit click, never a
  rule step.

### Antivirus and tamper implications

Security software treats "another process changing mine" as suspicious and may block or alert on handles opened with
suspend or terminate rights. Dyno therefore never acts on the security products (protected list), asks for the
smallest access it needs per action (`PROCESS_SET_INFORMATION` and `PROCESS_SET_QUOTA` for priority, caps and EcoQoS;
thread suspend rights only for suspend), and does not inject code or hook anything. Where the OS refuses (another
user's process, protected processes) the action fails with `AccessDenied` and a message saying the privileged helper
would be needed (ADR 0006); Dyno does not try to bypass it.

### Crash recovery and undo

- What outlives the call (caps, suspensions) is recorded in a versioned **control-state file** next to the agent's
  data; `release_all` is idempotent and also reads the file, so what a crashed earlier run left behind is released at
  the next start. Entries are bounded (`MAX_ENTRIES`) and a missing or corrupt file is an empty state.
- **Windows**: a job's name exists only while some handle to it is open, so a crashed Dyno would leave an unreachable
  cap. The agent therefore duplicates a **query-only handle to the job into the capped process itself**; the name lives
  as long as the process, and the next `release_all` re-opens it by name (`job_name(ProcessRef)`) and turns the rate
  control off.
- **macOS and Linux**: if the agent dies while a capped process is in the *stopped* half of the cycle, that process stays
  stopped until the next run's `release_all` reads the state file and sends `SIGCONT` (only to a process whose start
  time still matches). `Drop` of the control also ends every cap thread so a clean exit never leaves one stopped.
  The service restarts the agent within seconds (ADR 0010), which bounds the stall.
- Every applied step is an `Activity` with its inverse (`dyno_core::activity`), shown in the Activity list; rule steps
  are undone when the rule releases, when the app exits and when the agent stops.

## Consequences

- A cap on Unix is approximate and visible; the UI says so and recommends priority or efficiency mode first.
- On Linux `setpriority` and `sched_setaffinity` act per *thread*. The Linux efficiency mode and affinity walk the
  threads, but the shared Unix `SetPriority` changes only the main thread (a known gap, docs/roadmap.md).
- Tests act only on dummy processes the test spawns (AGENTS.md "Safety"); the CI macOS runner is a person's real Mac.
- Adding a method (cgroup v2 `cpu.max` for a systemd user scope, `taskpolicy`) is a new `OsControl` or a new
  `ProcessAction` mapping, not a change to the engine.
