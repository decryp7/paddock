# 24. Background work: background priority, wake-ups instead of polls, jobs for long work, per-pid monitoring

- Status: Accepted (2026-10-28, after an independent review). **Amends ADR 0020** (its `Class` and tiers now also govern child processes; its WP-12
  lint becomes checks C40-C42), ADR 0008 (every long job has a key) and ADR 0018 (section 5). The user decided on 2026-10-28 that there is one family ADR for background work (D11)
  and that the measured budgets are gates (D7)
- Date: 2026-10-28
- Related: ER-17 to ER-21 (long operations off the UI thread, live progress and Cancel, no UI timeouts, bounded
  parallelism at background priority, recurring bugs mean a missing design rule); rules 10-14, 16, 26, 40, 42, 43, 46;
  ADR 0008 (long operations, `paddock-jobs`), ADR 0018 (bounded loading), ADR 0020 (shared executor; this ADR adds the
  rules that make it binding and the checks over every instance, and covers child processes, which 0020 does not)
- Evidence: Debrief research note
  [2026-10-28-cpu-memory](../../apps/debrief/docs/research/2026-10-28-cpu-memory.md) (measured on synthetic data,
  `9b14d621e`, Mac mini M4 Pro); the poller survey [2026-10-poller-pool](../research/2026-10-poller-pool.md) (classified
  counts); the instance lists below are `grep` over the tree at `2d2fdaadf` and say so where they are raw counts

## Context

Measured on Debrief (research note, section 1):

- **An app that does nothing uses 1.16 cores** with its 12 plugin processes: every 200 ms each child's memory watcher
  builds a table of every process on the machine (1,143 on the test Mac) to read one pid (`sample(1)`: `__sysctl`,
  `__proc_info`, `sysinfo::get_process_infos` under the `debrief-child-memory` threads). 55 wake-ups/s.
- **The store idles at 5.8-11.7 % of a core** (550 MB and 7.6 GB stores): two loops wake every 500 ms and the catalogue
  loop runs three table scans each time.
- **After every import a name watcher polls the whole catalogue every 500 ms**: 1.04 cores at 550 MB, 2.08 cores at
  7.6 GB, for up to 10 minutes (~1,250 CPU-s per 7.6 GB import), because the call it polls fails at that size.
- **Work nobody started runs outside any job**: the post-import catalogue build (5 min and 652 CPU-s at 7.6 GB, no
  progress, no Cancel) and the name-table rebuild (16 s inside a search request at 550 MB; quadratic, ~29 min at its cap).
- **Background child processes run at the user's priority with every core**: Debrief's llama-server gets
  `BELOW_NORMAL_PRIORITY_CLASS`/nice 10 and `--threads` = all logical cores, the same for a chat turn and for background
  work; a user reported a sluggish Windows PC during a background AI job. ADR 0020 gives *threads* two OS tiers through
  `priority::apply_current` (not built yet); nothing covers *child processes*.

These are one design gap seen four ways, in several apps (instances below): each site writes its own loop, thread,
priority call or process scan. Rule 40 / ER-21: name the patterns, give each one shared mechanism and a check over every
instance, and fix the instances through it.

## Decision

Four binding rules for every Paddock app and shared crate, and one amendment to ADR 0018 (section 5). Each rule has one
mechanism, a check that covers every instance, and the list of today's instances.

### 1. Background work runs at background priority, threads bounded (ER-20)

**Mechanism.** One priority model: ADR 0020's `Class` (`Interactive`, `User`, `Background`, `Idle`) and its two OS tiers.
No second enum. ADR 0020's `priority::apply_current(Class)` sets the current thread; this ADR adds the same `Class` to
child-process launch (`paddock_proc::command(...).class(c)`), so a launch site states its class like a task does. (If the
dependency direction needs it, `Class` moves to `paddock-proc` (`paddock_proc::Class`), the lower of the two crates, and
`paddock-jobs` re-exports it; it stays one type.)

**Which class a child gets.** A process that serves interactive requests (the store worker answering UI queries, an
inference server answering chat) is **never launched below `User`**; it tiers its work per thread, as ADR 0020 decides
(tiered workers, WP-9: the class travels with the IPC call). Only a child that does nothing but background work is
launched at `Background` or `Idle`. A child's class is fixed at launch: on Linux an unprivileged process cannot lower its
nice value again (setpriority(2); the reason ADR 0020 fixes worker tiers), so switching a child's class per request is
not portable and is not done. A second instance of a server for the other kind of work is not an option either: for an
inference server it doubles 7-8 GB. Under rule 46 an inference server only serves user-started work, so it is launched
at `User` with bounded threads.

| OS | Child at `Background` / `Idle` (set from the parent on the child handle) | Thread (`apply_current`, ADR 0020) | I/O and memory |
|---|---|---|---|
| macOS | `setpriority(PRIO_DARWIN_PROCESS, pid, PRIO_DARWIN_BG)` (the `taskpolicy -b` behaviour: CPU on efficiency cores, I/O and GPU throttled) | `pthread_set_qos_class_self_np(QOS_CLASS_UTILITY / BACKGROUND)` | included in `PRIO_DARWIN_BG` |
| Linux | nice 10 (`Background`) or nice 19 + `SCHED_IDLE` (`Idle`) in `pre_exec`; never raised again | `setpriority(PRIO_PROCESS, tid, …)`, `SCHED_IDLE` for `Idle` | `ioprio_set` best-effort 7 (`Background`), idle class (`Idle`) |
| Windows | `BELOW_NORMAL_PRIORITY_CLASS` (`Background`) or `IDLE_PRIORITY_CLASS` (`Idle`) at `CreateProcess`, then `SetProcessInformation(child, ProcessMemoryPriority, MEMORY_PRIORITY_LOW / VERY_LOW)` and `SetProcessInformation(child, ProcessPowerThrottling, EXECUTION_SPEED)` (EcoQoS) | `SetThreadPriority(THREAD_MODE_BACKGROUND_BEGIN)` | memory priority as set; I/O priority of *another* process has no public API |

`PROCESS_MODE_BACKGROUND_BEGIN` applies only to the calling process, so it cannot be set on llama-server or any other
third-party child. Our own Rust children may enter it on themselves, which also lowers their I/O priority, but only
through `paddock_proc::priority::enter_background_self()`, and only when they were **launched** at `Background` or
`Idle`: the launch passes the class to the child (`PADDOCK_LAUNCH_CLASS`, set by `paddock_proc::command(...).class(c)`),
and the function refuses (a typed error, no OS call) for any other launch class, so a process launched to serve
interactive requests can never lower itself. It lives in `paddock-proc` because C40 allows `SetPriorityClass` and its
peers only there (`apply_current` is in `paddock-jobs` today). Test: launched at `User`, the call returns the refusal and
`GetPriorityClass` is unchanged; launched at `Background`, it succeeds.

**Thread counts** come from one function, `paddock_proc::threads(kind, class)`, which counts cores by kind, never logical
CPUs (SMT siblings share the core the user is typing on):

- Apple silicon: performance cores (`hw.perflevel0.physicalcpu`) and efficiency cores (`hw.perflevel1.physicalcpu`).
  `User` CPU work: performance cores − 1 (at least 1). `Background`/`Idle`: the efficiency-core count (at least 1),
  because background QoS runs on those cores anyway and more threads only queue there.
- Hybrid x86 (Intel P/E): the same split from `GetLogicalProcessorInformationEx` (`EfficiencyClass`) on Windows and
  `/sys/devices/system/cpu/types` (or `cpu_capacity`) on Linux.
- Uniform CPUs: physical cores − 1 for `User`, physical cores / 2 for `Background`, 1 for `Idle`.
- The ADR 0020 running bound and the app's Performance setting cap the result. A child's thread argument (`--threads`,
  `--threads-batch`, pool sizes) is taken from it.

**Check.** Conformance **C40** (`conformance/src/priority.mjs`, `paddock-rules` once ADR 0022 phase 4 lands): every
`Command::new`/`std::process::Command` in non-test code of `crates/` and `apps/` goes through `paddock_proc::command` with
an explicit `.class(...)`, or carries a dated waiver (below); direct `setpriority`, `SetPriorityClass`,
`pthread_set_qos_class_self_np`, `*_PRIORITY_CLASS` outside `paddock-proc` fail. Tests in `paddock-proc` per OS: a child
launched as `Background` reports the class (`getpriority(PRIO_DARWIN_PROCESS)`, `getpriority` on Linux,
`GetPriorityClass` and `GetProcessInformation` on Windows) and its thread count, on each OS's CI runner; a launch of a
process registered as serving interactive requests below `User` fails a unit test.

**Instances today** (grep at `2d2fdaadf`):

| App | Site | What it does | Change |
|---|---|---|---|
| shared | `crates/paddock-proc/src/command.rs:9,63`; `sys.rs:44-47` | `BELOW_NORMAL` / nice 10 for every quiet command | callers state a `Class` |
| Debrief | `crates/debrief-ai-worker/src/main.rs:12-19`, `child.rs:193-194` | AI worker and llama-server: below-normal / nice 10 for chat and background alike | `User` (it serves chat; rule 46 leaves it no background work) with bounded threads |
| Debrief | `crates/debrief-ai-worker/src/server.rs:283-287` | `--threads` = all logical cores | `threads(Cpu, class)` |
| Debrief | `crates/debrief-ipc/src/process.rs:382-385`, `contain.rs:469` | sandboxed children: nice / below-normal | `paddock-proc` (PH1 moves these anyway) |
| Debrief | `crates/debrief-plugin-api/src/process.rs:15,52-53` | its own copy of the below-normal constant | deleted (PH1) |
| Debrief | `crates/debrief-core/src/threads.rs:116,126`; `crates/debrief-ai/src/models/priority.rs:17,29` | two thread helpers (QoS background vs utility) | `priority::apply_current` |
| Dyno | `crates/dyno-core/src/process.rs:75`; `dyno-platform/src/{unix_control,macos/control,linux/os_control,linux/sys,windows/control}.rs` | priority control *of other processes* (the user's tool) | out of scope (it is the product), waiver kind `product` |
| Pitwall | `crates/pitwall-server/src/process.rs:42` | llama-server below-normal | `User` with bounded threads (it serves interactive requests) |
| Pitcrew, Spotter, Sighter, Pacenotes | – | no priority set on children (Spotter's renderer, Pitcrew's services) | C40 lists them; `Background` for Spotter's renderer only if it serves no interactive render |

### 2. No polling without a wake-up

**Mechanism.** Waits are event-driven: channels, condvars, `Clock::wait(deadline, wake_token)`, file events
(`paddock-watch` `NotifySource`), child-exit waits (`paddock-proc` `wait_child(deadline, cancel)`), or a blocking request
that returns when something changed (a "status after generation N" call). A poll that cannot be avoided (a network peer,
an OS without events) is an ADR 0020 mode-2 check: a declared interval, backoff when nothing changes (×2 up to a declared
maximum), **demand** (paused without subscribers or a visible window) and **idle stop** (it ends when its purpose is met
or after a declared maximum). A store or OS call inside a loop body is work for the check, never for a `sleep` loop.

**Check.** ADR 0020 WP-12's lint, made binding and widened: **C41** finds sleeps and timed waits in non-test code
outside `paddock-jobs::exec` and sorts each into one of these kinds; only the first is a finding:

| Kind | Example | Verdict |
|---|---|---|
| Loop that sleeps and looks | `loop { sleep(500ms); query(); }` | finding: use a wake-up or an executor check |
| Condvar or `Clock::wait` with a timeout as a *deadline* | `wait_timeout(guard, deadline)` on a condvar that is notified on change (what this ADR prescribes) | allowed when the loop body does no store or OS call on timeout; a timeout that leads to work is a finding |
| One-shot delay | a debounce, a fade, a single retry after a failure | waiver kind `delay` (duration, why not an `after()`) |
| Bounded backoff | reconnecting to a peer: ×2 up to a cap, a maximum number of tries or a deadline | waiver kind `backoff` (start, cap, stop) |
| UI clocks | Angular `setInterval`, RxJS `timer(`/`interval(` | finding outside the shared live feed and outside visual clocks (a running "elapsed" label, an animation frame), which get waiver kind `ui-clock` and must stop when hidden |

Waivers are dated entries in `paddock.waivers.toml` with the check id, path, kind, reason and expiry, as ADR 0023 does for
C39; a waiver without a reason or past its expiry is itself an error. A **ratchet** keeps each app's finding count and
waiver count in a committed baseline that may only fall. A PR test per fixed instance counts its work on a
`ManualExecutor`/fake clock: **0 statements, 0 OS calls, 0 wake-ups across N idle turns** (the budget D7 adopted: ≤ 1
wake-up/s and ≤ 0.5 % of a core per idle process; nightly in `debrief-bench.yml` the gate is CPU seconds and wake-ups
over 60 s, never wall time).

**Instances today.** Classified (survey): 44 periodic polls, 17 frontend polls, ~30 copies of a `try_wait`-sleep loop.
Raw grep of `thread::sleep(` in non-test paths (includes inline test modules; the C41 first run gives the exact list):
shared crates 19, Debrief 245, Dyno 68, Pitcrew 17, Spotter 17, Pitwall 8, Sighter 0, Pacenotes 0. The measured ones:

| App | Site | Measured |
|---|---|---|
| Debrief | `app/src-tauri/src/names.rs:38-40, 271-293` (name watcher, 500 ms, full catalogue) | 1.04-2.08 cores for 20 s to 10 min per import |
| Debrief | `crates/debrief-store-duck/src/catalogue.rs:197-253`, `fts.rs:698-735` (500 ms fallbacks, table scans) | 5.8-11.7 % of a core idle, 4.4 wake-ups/s |
| Debrief | `crates/debrief-ipc/src/process.rs:225-241` (200 ms per child) | with rule 4 below: 1.16 cores |
| shared | `crates/paddock-proc/src/supervised.rs:91,132-151` (200 ms per supervised child) | the same pattern for the shared plugin host |
| Debrief | `app/src-tauri/src/names.rs:160-183` (`Polite`, 500 ms) | removed with the alias job (rule 46) |
| others | the survey's A rows: Dyno's eight agent loops, `paddock-watch`'s 50 ms loop, start-up watchdogs, health probes | ADR 0020 WP-5 to WP-8, 10, 11 |

### 3. Long work is a job in the shared executor

**Mechanism.** Anything that can take longer than ~100 ms and that a request handler, a watcher or start-up would
otherwise run inline is submitted to the ADR 0020 executor (mode 1, a class and an owner) as a `paddock-jobs` job (ADR
0008): phase-labelled progress from the first moment, Cancel in every phase, results built then swapped (ER-17, ER-18),
and an **idempotency key** (`JobSpec::key`: kind + scope + canonical request; `submit` returns the queued or running job
with the same key). Requests read the last finished result and never wait for a rebuild; a rebuild is single-flight.

**Check.** **C42**: `thread::spawn`/`Builder::spawn` in non-test code outside `paddock-jobs` fails unless it is one of
ADR 0020's mode-3 lease kinds (pipe readers, lifelines, OS consumers) with a dated waiver of kind `lease`; every job kind
registered with the jobs registry declares its key function and its phases (a registry test enumerates kinds and
asserts both); a PR test per kind submits the same request twice and gets one id. **Inline work on request threads** is
caught by counting, not by the lint. A derived table (Debrief's name table) is registered once with the commands that
read it (`DerivedTable { name, readers: &[command ids], rebuild_job }` in the app's registry); a test walks the app's
command table and fails when a command calls a derived table's read API without being listed as a reader, so the set is
enumerable. Each listed reader (Debrief's `find_values` and the chat turn) has a test with a counting store and a stale table that asserts the command returns the
last table, makes **0 rebuild calls** itself, and that exactly one rebuild job is queued.

**Instances today.** Raw grep of thread spawns in non-test paths: shared 71, Debrief 127, Dyno 49, Pitcrew 24, Spotter
10, Pitwall 7, Sighter 0, Pacenotes 0 (the survey classifies ~65 as one-shot tasks that belong in the executor and ~67 as
lifelong leases). The measured ones:

| App | Site | Measured |
|---|---|---|
| Debrief | `crates/debrief-store-duck/src/catalogue.rs:157-176` (catalogue build thread) | 652 CPU-s / 5 min at 7.6 GB, no progress or Cancel |
| Debrief | `app/src-tauri/src/names.rs:118-131` via `search_everything.rs:151`, `ai/commands.rs:490` (name-table rebuild in the request) | 16.3 s / 31 CPU-s at 550 MB |
| Debrief | `app/src-tauri/src/jobs/model.rs:420-441`, `jobs/queue.rs:249-260` (no key) | duplicate Summarize jobs |
| Debrief | `app/src-tauri/src/jobs/service.rs:518, 859` (a thread per job) | ADR 0020 WP-8 |

### 4. Monitoring a child reads only that child

**Mechanism.** `paddock-proc` gets a per-pid probe and a tree probe that never enumerate the machine:

| OS | One process | Its tree |
|---|---|---|
| macOS | `proc_pid_rusage(pid, RUSAGE_INFO_V4).ri_phys_footprint` (rule 26) | `proc_listchildpids` recursively from the root |
| Linux | `/proc/<pid>/smaps_rollup` PSS (rule 26) | `/proc/<pid>/task/*/children` recursively |
| Windows | `GetProcessMemoryInfo` private working set (rule 26) | the job object the child already runs in: `QueryInformationJobObject` (`JobObjectBasicProcessIdList`, `JobObjectExtendedLimitInformation`) |

`process_memory(pid)` and `tree_memory(root)` use these; `snapshot()` (the whole table) is kept only for process-list
features that show every process (Dyno, Debrief's process monitor) and is renamed `machine_snapshot()` so the cost is in
the name. One watcher serves all of an app's children (an ADR 0020 mode-2 check over the set, 1 s plus a check at each
call's end), not one thread per child.

**Check.** A `paddock-proc` test with a counting OS shim: watching 14 children for 10 fake-clock seconds makes 14 per-pid
reads per tick and 0 machine snapshots; dated waivers of kind `machine-table` name the only callers of `machine_snapshot()`; nightly, the
plugin idle harness (`crates/debrief-plugins/tests/it/perf_plugins.rs`) asserts the host's idle CPU within the D7 budget.

**Instances today:**

| App | Site | Per call | Change |
|---|---|---|---|
| shared | `crates/paddock-proc/src/usage.rs:40-53, 122-125` (`process_memory` builds the whole table) | whole table | per-pid probe |
| shared | `crates/paddock-proc/src/tree.rs:21, 28, 143`; `supervised.rs:135` (default sampler `tree_memory`, every 200 ms per child) | whole table | tree probe |
| shared | `crates/paddock-metrics/src/native.rs:63`, `sampler.rs:100`; `crates/paddock-chromium/src/launch.rs:624` | whole table via `tree_memory` | tree probe |
| Debrief | `crates/debrief-ipc/src/process.rs:242, 630` (every child, 200 ms; own memory) | whole table | per-pid probe (measured 1.16 cores) |
| Debrief | `app/src-tauri/src/ai/core.rs:1247` (`worker_rss_bytes`) | whole table, and RSS (rule 26) | tree probe from the worker's pid |
| Spotter | `crates/spotter-render/src/process.rs:117, 125, 196`; `launch.rs:298` (render watchdog every 4th tick) | whole table | tree probe |
| Pitcrew | `crates/pitcrew-service/src/probe.rs:49` | whole table for its supervised services | tree probe per service, or `machine_snapshot` where it lists all |
| Dyno | `crates/dyno-platform/src/processes.rs:123`; Debrief `crates/debrief-monitor/src/source.rs:85` | whole table, by design (process lists) | waiver kind `machine-table`, `machine_snapshot` |
| Pitwall, Sighter, Pacenotes | – | none found | – |

### 5. Amendment to ADR 0018: bounds hold memory, not only rows; post-import phases stream too

Patterns 6 and 7 of the research note, listed in D11, are recorded here as an amendment to ADR 0018 rather than as new
rules, because ADR 0018 already owns bounded loading and streaming imports; they are not background-work rules.

- **A bounded query bounds memory.** `catalogue(all)` has a 50,000-row limit and still runs out of DuckDB's 192 MB at a
  7.6 GB store (1.58 M catalogue rows; 24 of 24 calls failed). Mechanism: ADR 0018's `bounded_query` test helper runs
  every bounded port method on a large synthetic store (`paddock-testkit` large-fixture generator) under the worker's
  memory limit. Check: the helper's table covers every method in the port table (a test enumerates them). Instance:
  Debrief `crates/debrief-store-duck/src/catalogue.rs:262-268` (1.2 plan P3).
- **Memory caps per process hold for every phase, with streaming.** ADR 0018 and the 1.2 plan cover imports (4.3, 3.5) and
  plugins (2.6, PH4); the store's post-import phases are added: the catalogue phase peaked at 911 MB at 7.6 GB. Check:
  the nightly footprint gate is per phase (budget 512 MB for post-import work). Instance:
  `crates/debrief-store-duck/src/catalogue.rs:59-92` (1.2 plan P4).

## Rollout

1. **Paddock, now (with Debrief 1.2's wave 2):** `Class` on child launch in `paddock-proc`, `threads()`, per-pid and tree
   probes, one watcher for all children (Debrief 1.2 package P1 with 2.2); C40-C42 land in **warning** mode with their
   waivers and ratchet baselines committed.
2. **Debrief 1.2:** packages P0-P5, NR1, BP1, J1 and B1 of the 1.2 plan (section "Performance packages"); PH1 moves its
   children onto `paddock-proc`, which takes rules 1 and 4 with it; C40-C42 become **errors for `apps/debrief`** when its
   findings are fixed or waived.
3. **Other apps, each with its next release** (rule 1, one app at a time): Pitwall (llama-server at `User` with bounded threads,
   ADR 0020 WP-11), Pacenotes and Sighter (born compliant: the checks are errors for them from their first code), Pitcrew
   (`probe.rs`, ADR 0020 WP-10), Spotter (render watchdog and priority), Dyno (agent loops, ADR 0020 WP-5/6). Each turns
   the checks into errors for its folder when its lists are clean.
4. Lessons and rules: on acceptance, rule 13 gains "child processes included" and the four rules here become rules 47-50
   in `docs/rules.md` (with the reasons in `docs/lessons.md`).

**Tooling (ADR 0022).** ADR 0022 §3(a) bans new `.mjs`, `.sh` and `.py` tooling files from the start of its phase 1,
and its phases start after Debrief 1.2 (the agreed order). So C40-C42 are written now as `.mjs` conformance checks
(`conformance/src/{priority,polls,threads}.mjs`), like C1-C39; they join ADR 0022's counted baseline of tooling files and
move to `paddock-repo-rules` in its rules phase. The measurement scripts committed with the evidence
(`apps/debrief/scripts/perf/*.py`, `*.sh`) go on the same baseline and move to `paddock-dev` (or `paddock-bench`) with it.

## Consequences

- Idle apps stop costing CPU: the measured Debrief case goes from ~1.2 cores to the ≤ 0.5 % budget; after-import polling
  (up to ~1,250 CPU-s per import) disappears.
- Background work is slower when the machine is busy, by design; jobs show "Still working" (ER-19), never time out.
- Every launch site states a priority; every long task has a key and phases. More ceremony per call site, checked once.
- Pre-v1, no compatibility shims (rule from the family memory): the old helpers are deleted, not wrapped.

## Risks

| Risk | Mitigation |
|---|---|
| Windows background mode can starve a background job on a busy machine for a long time | jobs show "Still working"; a user-started job is `User`, never `Background`; a test of progress under load on the Windows runner |
| `PRIO_DARWIN_BG` throttles GPU work too (Metal inference for a confirmed batch job) | not applied to inference servers: they serve chat and are launched at `User` (section 1); measured on the Mac runner before any other GPU-using child is launched at `Background` |
| The tree probe misses grandchildren that re-parent (daemonising helpers) | Windows: the job object holds them; macOS and Linux: children of supervised processes are not allowed to daemonise (contain), tested |
| Lints flood with legitimate instances | warning mode first, dated waivers with reasons and expiry, a ratchet that only lets counts fall, errors per app only when its findings are fixed or waived |

## Alternatives considered

- **Fix the measured instances only.** Rejected by rule 40: the same shapes exist in five apps.
- **One OS tier only (below-normal everywhere).** It is what Debrief does today and what the user found sluggish: it does
  not lower I/O, memory or GPU priority and leaves all cores busy.
- **Polling with longer intervals.** Halves the cost and keeps the bug class; the measured worst case (catalogue scans)
  grows with the data, not with the interval.
