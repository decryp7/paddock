# 31. Patience, not wall clocks: liveness from progress, one injected policy, tests wait on conditions

- Status: Proposed (2026-10-29). Needs an independent review before the user decides.
- Date: 2026-10-29
- Related: ER-12 (no sleeps or raised timeouts in tests), ER-19 (the UI never times out a running job), ER-20 and
  rule 51 (bounded parallelism), ER-21 / rule 40 (recurring bugs mean a missing design rule), rule 48 (recorded
  measurements); ADR 0020 (shared executor, stuck watchdog), ADR 0021 (portable tests, condition waits), ADR 0024
  (background work: wake-ups instead of polls, per-pid monitoring)
- Evidence: CI run 7096 (job 21092, Debrief macOS Rust tests), its reproduction and fix on branch
  `fix/debrief-ai-tests-load` (`5ed40bdba`, `e576d0f8f`, `a9ea3cbd2`); the measurements are recorded in
  [apps/debrief/docs/performance.md](../../apps/debrief/docs/performance.md) (rule 48) on that branch. Instance lists
  below are `grep` over `crates/` and `apps/debrief/crates/` at `5c646222a` and say so where they are partial.

## Context

**What happened.** In CI run 7096 five `debrief-app` `ai::tests` ran past nextest's 90 s budget on the Mac runner (load
about 119 on 14 cores). Alone, each takes 2-4 s. A `sample(1)` of a reproduction showed the test waiting for its
import job, the job following the import, and the import's parse threads waiting in the plugin supervisor's `lease` and
in `debrief_ipc::launch` for a plugin process to connect.

**Why slowness became failure.** The import waits on plugin processes under wall-clock bounds:

- the connect window: a child must connect within 15 s of being started (`SpawnSpec::connect_window`);
- the call deadline: 90 % of the call kind's budget (`deadline_for`), so 4.5 s for a sniff, after which the process is
  stopped and the call counts as a failure of the plugin;
- the watchdog: an in-process frame older than its budget marks the plugin hung (`debrief_core::plugin::watchdog`);
- the supervisor: after a failure it restarts with backoff (0.5-8 s) and quarantines after 3 failures in 10 minutes.

A plugin that starts or answers late is stopped, restarted (late again, because the machine is still busy, and now with
one more process starting), and finally quarantined. The import waits through every round, plugin by plugin. Started
16 s late on an idle machine (a wrapper as `DEBRIEF_PLUGINS`, no CPU load), one test took **215 s**. With the test
harness waiting on conditions instead, the same test took 146 s: nine cold starts of 16 s, no restarts. Every bound
did what it was written to do. Together they turn "the machine is busy" into "the plugin is broken", and the restarts
make the machine busier.

**This is a pattern, not an instance.** The same shape has shown up before as timing flakes on CI (the 90 s budgets of
rule 40 and the waiver list in Debrief's `nextest.toml`, CI 6381), and it hides in
the product. On the user's Mac, App Tamer pauses apps that are in the background: while Debrief or one of its children is
stopped, the monotonic clock keeps running. When the app resumes, every window and deadline that was open has expired at
once. Children are stopped and restarted, plugins are quarantined, and files show as "not imported" although nothing was
wrong. A loaded laptop has the same effect, slower.

**What a bound is for.** The bounds above exist to detect a child or call that will never finish (a deadlock, a busy
loop, a child that never connects). Wall-clock time is a poor signal for that: it cannot tell "slow" or "paused" from
"stuck". The signal that can is progress: a child that is alive, scheduled and sends nothing for a long run of host
observations is stuck. A child that is stopped (paused by the OS or by a tool such as App Tamer), or whose host is
stopped, is not.

## Decision

### 1. Two kinds of bounds, and only one of them is wall-clock time

Every time bound in the family is one of these, and the code says which:

- **Liveness bound:** "is this still working?" (connect windows, call deadlines, watchdog budgets, handshake waits for a
  local child, manifest and probe runs of our own binaries). It becomes a **progress check** (section 2). It never fires
  only because time passed.
- **Cap:** a limit the user would recognise as a rule of the product, with its value in Settings or in the spec. These
  stay real wall-clock bounds (section 5). A cap that fires explains itself (ADR 0019): which cap, its value, where to
  change it.

### 2. Liveness comes from progress, observed in host ticks

- **Heartbeats.** Every long call and every child start reports progress: a call streams items or a heartbeat frame
  (the IPC peer already has frames; a `Progress` frame every *beat* while a call runs), and a starting child reports
  "starting" as soon as its `main` runs (one frame on the endpoint before the handshake), then its handshake.
- **Host ticks, not elapsed time.** A host thread (the existing watchdog or `debrief-child-memory` loop, ADR 0024's
  per-pid monitor) counts *ticks*: one per beat that the host itself was running. A stall is "N consecutive ticks with
  no progress while the child is running". A paused host counts no ticks, so a resumed app does not see every child as
  stuck at once.
- **Stopped is not stuck.** Before counting a tick against a child, the monitor reads the child's run state (macOS
  `proc_pidinfo` `pbi_status == SSTOP`, Linux `/proc/<pid>/stat` state `T`, Windows: all threads suspended). A stopped
  child is waited for, and its job shows "Paused by the system" (ER-18: phase-labelled, never a static 0).
- **A child that is alive and still starting is waited for.** The connect window becomes "until the child connects,
  exits, or stalls" (no progress for N ticks while running). The 15 s fixed window goes.
- **Stuck is still detected.** A deadlocked or looping child makes no progress while running and is stopped after N
  ticks, counted as a failure, restarted with backoff, and quarantined as today. The supervisor and quarantine logic
  stay; only what feeds them changes.

### 3. One injected policy: `Patience`

One type in a shared crate (`paddock-jobs::patience`, next to the executor's watchdog of ADR 0020, which needs the same
thing) owns every liveness bound:

```rust
/// How long liveness checks wait: in ticks of no progress while running, never in elapsed time.
pub trait Patience: Send + Sync {
    /// The beat at which progress is observed (and a tick counted).
    fn beat(&self) -> Duration;
    /// Ticks without progress, while running, after which `what` is stuck; `None`: never (wait on the condition).
    fn stall_ticks(&self, what: Liveness) -> Option<u32>;
}
pub enum Liveness { ChildStart, Handshake, Call(CallKind), Probe }
```

- `Patience::product()` holds today's values converted to ticks (for example a beat of 1 s and 15 ticks for a child
  start), tunable in Settings under rule 51 (Auto default, validated in the engine).
- `Patience::until_condition()` returns `None` everywhere: the wait ends on the condition (connected, answered, exited).
  Tests use it; the runner's hang detector (nextest's `slow-timeout`) ends a real hang.
- It is injected where the bounds are used today: `SpawnSpec` (`connect_window` becomes `patience: Arc<dyn Patience>`),
  the plugin host's `Ctx::deadline`, the guard's `Budgets`, the supervisor, `load_manifests`, the installed-plugin probe,
  the AI and store worker launchers. No module keeps its own `const ..._TIMEOUT` for a liveness bound.

### 4. Tests choose "until a condition"

- Test helpers that build an app, a host or a worker (Debrief's `test_support::manage_in`, `debrief-plugin-test`, the
  store and AI worker harnesses, `paddock-plugin-test`) inject `Patience::until_condition()`.
- The bounds themselves are tested where they are the subject, with a fake child that stops progressing and a
  hand-driven tick source: "stuck after N ticks", "stopped is not stuck", "a paused host counts no ticks". No such test
  sleeps (ER-12).
- `fix/debrief-ai-tests-load` is the first step and the model: `debrief_ipc::process::UNTIL_CONNECTED` and
  `Budgets::unlimited()` in Debrief's test apps. With `Patience` those two become `Patience::until_condition()`.

### 5. Bounds that stay real

- **User-facing caps**, in Settings or in the spec, explained when they fire (ADR 0019): the elevated-call cap
  (`MAX_ELEVATED_CALL`, 1 h), a tool operation's declared budget (`OpSpec::budget`, `DEFAULT_OP_BUDGET`), the store
  client's `LONG_TIMEOUT`/`WAIT_TIMEOUT`, import size and memory caps.
- **External I/O that cannot report progress:** network requests to other hosts (`paddock-net`, the updater's
  `MANIFEST_TIMEOUT`, downloads), OS tools run as subprocesses we do not control (`paddock-files` `OPENER_TIMEOUT`,
  `paddock-download` `TOOL_TIMEOUT`, `paddock-proc` `SHELL_TIMEOUT`). They keep a wall-clock bound and go through
  `paddock-proc`'s bounded runner. A paused host can still make these fail; they are retried by the user (Retry), not
  quarantined.
- **Shutdown and teardown:** `KILL_GRACE`, `EXIT_WAIT`, `paddock-window`'s `TEARDOWN_STEP_LIMIT`/`EXIT_STALL_LIMIT`.
  Quitting must end.
- **UI handshakes that only change what is shown, never fail work:** `paddock-dialogs` `SHOW_DEADLINE` (the window is
  shown anyway, ADR 0026).
- **True hangs**: still detected, through section 2.

### 6. Instances (liveness bounds today)

`grep` at `5c646222a`; Debrief's are complete for the crates named, the family list covers the shared crates.

| Where | Bound | What happens when it fires | Target |
|---|---|---|---|
| `debrief-ipc` `SpawnSpec::connect_window` (15 s), used by `launch` | child start | start fails, supervisor counts a failure and restarts | `Patience` `ChildStart` |
| `debrief-ipc` `link::HANDSHAKE_TIMEOUT` (10 s) | handshake with a local child | start fails | `Handshake` |
| `debrief-plugin-host` `remote::deadline_for` (90 % of the budget; sniff 4.5 s) | every plugin call | process stopped, failure, quarantine after 3 | `Call(kind)` with heartbeats |
| `debrief-plugin-host` `remote::DEFAULT_DEADLINE` (10 s) | config sniff, settings | same | `Call` |
| `debrief-plugin-host` `document.rs` and `profile.rs` `SNIFF_DEADLINE` (10 s) | document and profile sniff | same | `Call` |
| `debrief-plugin-host` `PluginProcess::create` restore: `ApplySettings` (10 s) | settings re-applied after a restart | restart fails, one more failure | `Call` |
| `debrief-plugin-host` `builtins::MANIFESTS_TIMEOUT` (30 s), `installed::PROBE_TIMEOUT` (30 s) | our binary describing itself | no plugins at all for the session, or install refused | `Probe` |
| `debrief-plugin-host` `process::PUMP_WAIT` (120 s) | a pumped callback | call fails | `Call` |
| `debrief-core` `plugin::watchdog` with `guard::Budgets` | in-process plugin call | plugin marked hung | ticks (ADR 0020 watchdog) |
| `debrief-ai` `worker::frames::CONNECT_WINDOW` (15 s) | AI worker start | start fails | `ChildStart` |
| `debrief-store-client` `CALL_TIMEOUT` (120 s) | an ordinary store call | call fails | `Call` with heartbeats (the long ones stay caps, section 5) |
| `debrief-plugin-test` `connect_window` (15 s) | test harness | test fails under load | `until_condition()` |
| `paddock-ipc` `handshake::HANDSHAKE_TIMEOUT` (10 s), `paddock-mcp` `loopback::HANDSHAKE_TIMEOUT` (10 s) | handshake | start fails | `Handshake` |
| `paddock-plugin-host` `supervisor::NO_BUDGET` (1 h) and per-call budgets | plugin call | failure, quarantine (`paddock-guard`) | `Call` |
| `paddock-channels` `dispatch::DEFAULT_TIMEOUT` (15 s) | channel call | call fails | decide in WP-1 (local or external) |
| `paddock-service` `HELPER_TIMEOUT` (30 s), `ELEVATED_TIMEOUT` (120 s) | privileged helper | operation fails | `ChildStart`/`Call`; the elevated total stays a cap |

### 7. The check over every instance

A conformance check (next free C number) over non-test Rust in `crates/` and `apps/*/crates`: a `Duration` constant or
literal passed to `recv_timeout`, `wait_timeout`, `tokio::time::timeout`, or compared with `elapsed()` must either come
from `Patience`, or carry a `// cap:` (section 5) or `// external:` comment naming why it is real time. Tests are exempt
from the comment but not from ER-12. The check lists today's instances as a shrinking waiver list, as C40-C42 do.

## Parallelism and cost (rule 51)

- The heartbeat costs one small frame per beat per running call, and one run-state read per child per beat on the
  monitor thread ADR 0024 already runs. It adds no thread.
- Waiting for a slow child instead of restarting it is cheaper on a busy machine: in the reproduction the restarts cost
  69 s of 215 s and started extra processes.
- The test side bounds its own load instead of its patience: Debrief's shell tests run at most six at a time, with the
  stores in low memory mode (`fix/debrief-ai-tests-load`, recorded in performance.md).

## Consequences

- A paused or busy app waits, says it is waiting ("Paused by the system", "Starting the plugins"), and goes on. Cancel
  still works in every phase (ER-18).
- A real hang takes N ticks of no progress while running to detect. With a 1 s beat and today's numbers that is about
  the same time as today on an idle machine.
- The IPC protocol gains a `Progress` frame and a "starting" frame. The plugin SDK sends them from its runtime, so
  plugin authors write nothing. Old plugins that never send a heartbeat are judged by the call's reply only, with the
  product `Patience` converted to ticks.
- Tests stop failing because the machine is busy. A test that hangs is still stopped by nextest's budget (rule 40).

## Alternatives considered

- **Raise the timeouts** (or raise them in tests). Rejected: ER-12, and it only moves the threshold. The reproduction
  needs only a 16 s start to fail; a paused app needs any pause longer than the largest timeout.
- **Retry on timeout.** Rejected: that is what the supervisor does today, and the retries are the amplifier.
- **Scale timeouts by the load average.** Rejected: load does not tell a paused process from a running one, and it is
  not portable.
- **Budgets in CPU time of the child.** Rejected as the only signal: a deadlocked child uses no CPU, like a stopped one.
  Run state plus progress tells them apart.

## Work packages

1. **WP-1 `Patience` in `paddock-jobs`** with `product()`, `until_condition()`, the tick source and the run-state probe
   (three OS), and tests with a hand-driven tick source. Decide the `paddock-channels` case. About 2 days.
2. **WP-2 IPC progress frames** in `debrief-ipc` and `paddock-ipc` (`Progress`, "starting"), the SDK runtimes send them;
   protocol version bump. About 2 days.
3. **WP-3 Debrief host:** `SpawnSpec`, `launch`, the plugin host's deadlines, `SNIFF_DEADLINE`, `DEFAULT_DEADLINE`, the
   restore call, `MANIFESTS_TIMEOUT`, `PROBE_TIMEOUT`, the AI and store worker launchers on `Patience`; "Paused by the
   system" in job phases. About 3 days.
4. **WP-4 Family:** `paddock-plugin-host`, `paddock-guard` feeding, `paddock-service`, `paddock-mcp`. About 2 days.
5. **WP-5 Tests and check:** every harness on `until_condition()`; the conformance check with its waiver list;
   remove `UNTIL_CONNECTED`/`Budgets::unlimited()` from Debrief's test apps in favour of `Patience`. About 1 day.
6. **WP-6 Verify on the user's Mac** (local VM or the real app, never CI): App Tamer pausing Debrief during an import,
   and a loaded machine; no quarantine, the job says it was paused. About 0.5 day.

## Open questions for the user

1. The beat and the tick counts for the product (`Patience::product()`): today's values converted 1:1 (a 1 s beat,
   15 ticks to start), or longer now that only a stuck, running child is stopped?
2. Should a stopped child's job say "Paused by the system", or stay silent and only not fail?
