# 24. Background work: background priority, wake-ups instead of polls, jobs for long work, per-pid monitoring

- Status: Proposed (2026-10-28); the user decided on 2026-10-28 that there is one family ADR for background work (D11)
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

Four binding rules for every Paddock app and shared crate. Each has one mechanism, a check that covers every instance,
and the list of today's instances.

### 1. Background work runs at background priority, threads bounded (ER-20)

**Mechanism.** `paddock-proc` owns a `Priority` enum `{ Interactive, User, Background, Idle }` and applies it in one place,
both to a child process at launch (`Command` extension: `.priority(p)`) and to the current thread
(`priority::apply_current`, the ADR 0020 helper, moved here so the executor and process launch share it):

| OS | Background / Idle (process) | Background / Idle (thread) | I/O |
|---|---|---|---|
| macOS | `posix_spawnattr` / `setpriority(PRIO_DARWIN_PROCESS, pid, PRIO_DARWIN_BG)` (the `taskpolicy -b` behaviour: CPU, I/O and GPU throttled) | `pthread_set_qos_class_self_np(QOS_CLASS_BACKGROUND / UTILITY)` | included in `PRIO_DARWIN_BG` |
| Linux | `nice 10` (Background) or `19` + `SCHED_IDLE` (Idle) in `pre_exec` | `setpriority(PRIO_PROCESS, tid, …)`, `SCHED_IDLE` for Idle | `ioprio_set(IOPRIO_CLASS_IDLE)` for Idle, best-effort 7 for Background |
| Windows | `IDLE_PRIORITY_CLASS` (Idle) or `BELOW_NORMAL_PRIORITY_CLASS` (Background) at `CreateProcess`, and `PROCESS_MODE_BACKGROUND_BEGIN` from inside the child (`SetPriorityClass(GetCurrentProcess(), …)`), which also lowers I/O and memory priority | `SetThreadPriority(THREAD_MODE_BACKGROUND_BEGIN)` | included in background mode |

A child that serves both kinds of work (an inference server, a store worker) takes the class per request where the
protocol allows it (ADR 0020 WP-9 for the store worker), otherwise runs at the class of its most common work and gets a
separate instance for the other. **Thread counts** come from one function, `paddock_proc::threads(kind, class)`: CPU work
`physical cores − 1` for User, `physical cores / 2` for Background, 1 for Idle; never logical cores (SMT siblings share
the core the user is typing on). A child's thread argument (`--threads`, pool sizes) is taken from it.

**Check.** Conformance **C39** (`conformance/src/priority.mjs`, `paddock-rules` once ADR 0022 phase 4 lands): every
`Command::new`/`std::process::Command` in non-test code of `crates/` and `apps/` goes through `paddock_proc::command` with
an explicit `.priority(...)`, or is on a reviewed allow-list with a reason; direct `setpriority`, `SetPriorityClass`,
`pthread_set_qos_class_self_np`, `*_PRIORITY_CLASS` outside `paddock-proc` fail. Tests in `paddock-proc` per OS: a child
launched as Background reports the class (`getpriority`/`proc_pidinfo` `PROC_PIDTASKINFO` policy, `GetPriorityClass`)
and its thread count, on each OS's CI runner.

**Instances today** (grep at `2d2fdaadf`):

| App | Site | What it does | Change |
|---|---|---|---|
| shared | `crates/paddock-proc/src/command.rs:9,63`; `sys.rs:44-47` | `BELOW_NORMAL` / nice 10 for every quiet command | becomes `Priority::Background`; callers state it |
| Debrief | `crates/debrief-ai-worker/src/main.rs:12-19`, `child.rs:193-194` | AI worker and llama-server: below-normal / nice 10 for chat and background alike | `User` for chat, `Background` for confirmed batch jobs (rule 46) |
| Debrief | `crates/debrief-ai-worker/src/server.rs:283-287` | `--threads` = all logical cores | `threads(Cpu, class)` |
| Debrief | `crates/debrief-ipc/src/process.rs:382-385`, `contain.rs:469` | sandboxed children: nice / below-normal | `paddock-proc` (PH1 moves these anyway) |
| Debrief | `crates/debrief-plugin-api/src/process.rs:15,52-53` | its own copy of the below-normal constant | deleted (PH1) |
| Debrief | `crates/debrief-core/src/threads.rs:116,126`; `crates/debrief-ai/src/models/priority.rs:17,29` | two thread helpers (QoS background vs utility) | `priority::apply_current` |
| Dyno | `crates/dyno-core/src/process.rs:75`; `dyno-platform/src/{unix_control,macos/control,linux/os_control,linux/sys,windows/control}.rs` | priority control *of other processes* (the user's tool) | out of scope (it is the product), allow-listed |
| Pitwall | `crates/pitwall-server/src/process.rs:42` | llama-server below-normal | `Priority` per request kind |
| Pitcrew, Spotter, Sighter, Pacenotes | – | no priority set on children (Spotter's renderer, Pitcrew's services) | C39 lists them; Background for Spotter's scheduled renders |

### 2. No polling without a wake-up

**Mechanism.** Waits are event-driven: channels, condvars, `Clock::wait(deadline, wake_token)`, file events
(`paddock-watch` `NotifySource`), child-exit waits (`paddock-proc` `wait_child(deadline, cancel)`), or a blocking request
that returns when something changed (a "status after generation N" call). A poll that cannot be avoided (a network peer,
an OS without events) is an ADR 0020 mode-2 check: a declared interval, backoff when nothing changes (×2 up to a declared
maximum), **demand** (paused without subscribers or a visible window) and **idle stop** (it ends when its purpose is met
or after a declared maximum). A store or OS call inside a loop body is work for the check, never for a `sleep` loop.

**Check.** ADR 0020 WP-12's lint, made binding and widened: **C40** flags `thread::sleep`, `sleep(` and `wait_timeout`
in non-test code outside `paddock-jobs::exec` and the allow-list (each entry with interval, backoff, demand and idle
stop); frontend `setInterval`/`timer(`/`interval(` outside the shared live feed likewise. A PR test per fixed instance
counts its work on a `ManualExecutor`/fake clock: **0 statements, 0 OS calls, 0 wake-ups across N idle turns** (the
budget D7 adopted: ≤ 1 wake-up/s and ≤ 0.5 % of a core per idle process, also measured nightly in `debrief-bench.yml`).

**Instances today.** Classified (survey): 44 periodic polls, 17 frontend polls, ~30 copies of a `try_wait`-sleep loop.
Raw grep of `thread::sleep(` in non-test paths (includes inline test modules; the C40 first run gives the exact list):
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

**Check.** **C41**: `thread::spawn`/`Builder::spawn` in non-test code outside `paddock-jobs` and the lease allow-list
(ADR 0020's mode-3 exemptions: pipe readers, lifelines, OS consumers) fails; every job kind registered with the jobs
registry declares its key function and its phases (a registry test enumerates kinds and asserts both); a PR test per
kind submits the same request twice and gets one id.

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
reads per tick and 0 machine snapshots; C40/C41's allow-list names the only callers of `machine_snapshot()`; nightly, the
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
| Dyno | `crates/dyno-platform/src/processes.rs:123`; Debrief `crates/debrief-monitor/src/source.rs:85` | whole table, by design (process lists) | allow-listed, `machine_snapshot` |
| Pitwall, Sighter, Pacenotes | – | none found | – |

## Rollout

1. **Paddock, now (with Debrief 1.2's wave 2):** `paddock-proc` `Priority`, `threads()`, per-pid and tree probes, one
   watcher for all children (Debrief 1.2 package P1 with 2.2); C39-C41 land in **warning** mode with their generated
   instance lists committed.
2. **Debrief 1.2:** packages P0-P4, NR1, BP1, J1 and B1 of the 1.2 plan (section "Performance packages"); PH1 moves its
   children onto `paddock-proc`, which takes rules 1 and 4 with it; C39-C41 become **errors for `apps/debrief`** when its
   lists are empty or reasoned.
3. **Other apps, each with its next release** (rule 1, one app at a time): Pitwall (llama-server priority per request,
   ADR 0020 WP-11), Pacenotes and Sighter (born compliant: the checks are errors for them from their first code), Pitcrew
   (`probe.rs`, ADR 0020 WP-10), Spotter (render watchdog and priority), Dyno (agent loops, ADR 0020 WP-5/6). Each turns
   the checks into errors for its folder when its lists are clean.
4. Lessons and rules: on acceptance, rule 13 gains "child processes included" and the four rules here become rules 47-50
   in `docs/rules.md` (with the reasons in `docs/lessons.md`).

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
| `PRIO_DARWIN_BG` throttles GPU work too (Metal inference for a confirmed batch job) | measured on the Mac runner when the first background inference job lands; the class is per request, so chat is unaffected |
| The tree probe misses grandchildren that re-parent (daemonising helpers) | Windows: the job object holds them; macOS and Linux: children of supervised processes are not allowed to daemonise (contain), tested |
| Lints flood with legitimate instances | warning mode first, generated allow-lists with reasons, errors per app only when its list is clean |

## Alternatives considered

- **Fix the measured instances only.** Rejected by rule 40: the same shapes exist in five apps.
- **One OS tier only (below-normal everywhere).** It is what Debrief does today and what the user found sluggish: it does
  not lower I/O, memory or GPU priority and leaves all cores busy.
- **Polling with longer intervals.** Halves the cost and keeps the bug class; the measured worst case (catalogue scans)
  grows with the data, not with the interval.
