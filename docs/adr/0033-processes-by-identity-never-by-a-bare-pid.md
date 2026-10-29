# 33. Processes by identity, never by a bare pid

- Status: Proposed (2026-10-30). The shared crates are migrated on `fix/proc-identity-kills`, ahead of acceptance, as the
  candidate fix for the Windows `ERR_ABORTED` flake; the apps follow after acceptance (WP-5 to WP-9)
- Date: 2026-10-30
- Related: rule 40 / ER-21 (recurring bugs mean a missing design rule), ER-14 (tests touch only what they started), rules
  42/43 (share first); ADR 0030 (unexplained browser aborts name their cause; merged into this branch so one Windows run
  shows both); ADR 0023 (the same shape for ports: a resource freed and handed on before it is used)
- Evidence: Windows crate jobs of runs 7110, 7123, 7124, 7134, 7140, 7142 and 7145 failed in `paddock-chromium`'s
  `real_browser::*` with `net::ERR_ABORTED`. In jobs 21399, 21598, 21908 and 21966 the test
  `a_browser_ended_by_force_leaves_nothing_behind` finished 0.04 to 2 s before the failure, in the same test binary
  (the four real-browser tests run in parallel). ADR 0030 showed that ending the network service gives exactly the trace
  of these failures. Not reproduced on Windows yet: the first Windows run of this branch confirms or refutes the cause
  (with ADR 0030's message, a failure says whether the network service restarted).

## Context

`paddock_proc::kill_tree` (before this change) read the process table, ran `taskkill /T /F` on the root, and then killed
every pid of the snapshot one by one through sysinfo. `taskkill` had already ended those processes, so by then their pids
were free, and Windows hands a freed pid to the next process it starts (Microsoft: a pid "is valid from the time the process
is created until the process has been terminated", read 2026-10-30,
<https://learn.microsoft.com/en-us/windows/win32/procthread/process-handles-and-identifiers>). With four tests each
starting Chrome (a browser, a network service, a GPU process, renderers), the next process to get such a pid is likely
another test's browser helper. Killing it gives the trace of ADR 0030: the request leaves, the service dies, the load is
cancelled with `ERR_ABORTED canceled=true`, and the server never sees a connection.

The same mistake has several shapes, and the sweep found them across the family:

1. **Snapshot, then kill.** A pid read from the table is killed later without asking who holds it now.
2. **Parent links without ages.** A tree is built from parent pids, which outlive their processes on Windows
   (`PROCESSENTRY32.th32ParentProcessID` is only "the identifier of the process that created this process", read
   2026-10-30, <https://learn.microsoft.com/en-us/windows/win32/api/tlhelp32/ns-tlhelp32-processentry32>): an orphan's
   parent pid can name a newer, unrelated process, whose "children" are then strangers. `taskkill /T` walks the same
   links.
3. **Kill after reap.** `Child::try_wait` reaps a child that ended. A later `killpg(child.id())` signals whatever got the pid.
4. **A pid from a file.** A discovery or state file names a pid, which is trusted as long as some process has it.

**Pattern:** a process is ended (or counted as ours) by a pid that was read earlier, whenever that process may have ended
in between. **Invariant:** before anything acts on a process, it checks that the pid still belongs to the process it
means. That process is identified by its pid plus its start time, or held through a handle or job object.

## Instances (sweep of 2026-10-30)

Shared code, fixed on this branch:

| Where | Shape | State |
|---|---|---|
| `paddock-proc` `tree.rs` `kill_tree` | 1, 2 (`taskkill /T`, sysinfo kill of snapshot pids) | broken; fixed |
| `paddock-proc` `usage.rs` `descendants` (also memory caps, `tree_memory`) | 2 | at risk (a stranger's memory counted and its tree killed over the cap); fixed |
| `paddock-proc` `supervised.rs` `Supervised::kill`, `Registry::end_all`, the memory watcher | 3 (`kill_tree(c.id())` after `is_running()` reaped) | broken; fixed |
| `paddock-proc` `run.rs` `run_bounded` timeout and cap | 3 (the waiter thread reaps, then `kill_tree(pid)`) | broken; fixed |
| `paddock-proc` `tree.rs` `ask`, `end`, `terminate`, `processes_mentioning` | 1 (bare `u32` API) | at risk; fixed (they take or return `ProcessId`) |
| `paddock-chromium` `launch.rs` `end_leftovers` | 1 | at risk; fixed |
| `paddock-chromium` `tests/it/real_browser.rs` network-service test | 1 | at risk; fixed |
| `paddock-ipc` `link/client.rs` `replace` | 4 (`terminate(d.pid)` from the discovery file) | broken; fixed (`ProcessId::of_recorded`) |
| `dyno-platform` `process.rs` `TreeGuard` | adopts the new `kill_tree` signature | fine before; migrated |
| `paddock-service` `native.rs`, `paddock-proc` `job.rs` | handle or job we own | fine; waiver comment |

Apps and tools, listed with a dated waiver in `paddock.waivers.toml` (C53 below, until 2026-12-31):

| Where | Shape | State | WP |
|---|---|---|---|
| Debrief `chromium-driver` `process.rs` `Supervised::kill`, `launch.rs` watchdog | 3, 2 (`taskkill /T`) | broken | WP-5 |
| Debrief `debrief-ipc` `process.rs` `OsChild::kill_group` | 3 | broken | WP-5 |
| Debrief `debrief-ai-worker` `child.rs` exit reaper | 3 | broken | WP-5 |
| Debrief `debrief-collect` `runner.rs`, `debrief-plugin-windows` `command.rs` (tests) | 1 | at risk | WP-5 |
| Debrief `debrief-monitor` `source.rs` `expand_children` | 2 (display only) | at risk | WP-5 |
| Pitcrew `launch.rs` `kill_tree` (`taskkill /T`, killpg; adopted runners from the state file) | 2, 4 | broken | WP-6 |
| Pitcrew `process_watch.rs` `begin` with `terminate.rs` | 1 | broken | WP-6 |
| Pitcrew `probe.rs` `has_job_children`, `pitcrew-core` `usage.rs` `tree_members`, `process.rs` `tree_members` (test) | 2 | at risk | WP-6 |
| Pitcrew `probe.rs` `alive` (no start time counts as a match) | 4 | at risk | WP-6 |
| Pitwall `pitwall-server` `system.rs` `terminate` | sysinfo kill of an unreaped child | fine today | WP-7 |
| Spotter `spotter-render` `process.rs` (a copy of the old `kill_tree`) | 1, 2, 3 | broken | WP-8 |
| `tools/paddock-dev` `tests/it/slots.rs` | 4 (test) | at risk | WP-9 |
| Dyno `macos/procinfo.rs` parent for the tree view | 2 (display only) | at risk | WP-9 |

Fine as they are: `paddock-startup` and Debrief's lifecycle markers and agent lock already compare start times; Debrief
`contain.rs` and `elevation.rs`, Pitcrew `win_session.rs`, and Pitwall `service/windows.rs` end processes through handles
they own (each now carries a waiver comment that says so).

## Decision

1. **One identity type, in `paddock-proc`.** `ProcessId` holds a pid and a `StartTime` (microseconds since the Unix epoch,
   as the OS reports it: the creation time from `GetProcessTimes` on Windows, `pbi_start_tv*` on macOS, and the boot time
   plus the start tick of `/proc/<pid>/stat` on Linux). Two processes that had the same pid one after the other are
   different values. `ProcessId::of(pid)` reads the current holder, for a child we have not reaped or right after a fresh
   look at the table. `ProcessId::of_recorded(pid, written_ms)` is for a pid from a file whose process wrote it after it
   started: a holder that started later is someone else. `Holder` and `Ending` say what was found.
2. **Every kill checks identity.** `kill_tree`, `end`, `ask` and `terminate` take a `ProcessId`.
   `processes_mentioning` and `descendants_of` return them. Each process is ended only if its pid's holder still has
   the recorded start time. On Windows the check and `TerminateProcess` use one handle, which keeps the process object,
   and so its pid, from being handed on in between. On Unix it is a check and then a signal: there is a gap of
   microseconds, against the gap of seconds before. A skipped kill logs a warning that names both start times.
3. **Trees follow parents by age.** `usage::descendants` counts a process as a child only if it started no earlier than its
   parent. For a root that is gone, it counts only the processes that started before the new holder of the root's pid.
   A process with an unknown start time is not counted. Memory caps use the same walk.
4. **Windows ends supervised trees through the job object.** `Supervised::kill` calls `TerminateJobObject` on the child's
   kill-on-close job before anything else, and no Paddock code runs `taskkill /T`. The identity walk stays as the backup for
   a helper outside the job. `ask` sends a close request to the one process (`taskkill` without `/T`), as its doc always said.
5. **A process we have not reaped is ours.** `Supervised` takes the child's identity at spawn. It ends the group, and the
   root without the check, only while `try_wait` says that the child has not been reaped. After reaping it, the root is
   checked like any other process. The root's process group (Unix) is signalled while the root is still itself, or when
   it is gone and no process holds its pid: POSIX does not hand a pid on while a process group with that id exists, so
   such a group still holds only the root's helpers. When a newer process holds the pid, the group is left alone.
6. **Check C53** over every Rust file in the repository: `libc::kill`, nix `signal::kill`, `killpg`, `TerminateProcess`,
   `TerminateJobObject`, `taskkill`, a `kill`/`pkill` command, and sysinfo's `kill`/`kill_with` on a process looked up
   by pid are errors outside `crates/paddock-proc/src/sys.rs`. A site that ends a process through a handle or job it owns
   carries `// conformance: pid-kill — <reason>`. The apps' current sites are dated waivers (WP-5 to WP-9). The check
   enumerates every site, so a new one fails CI.

### Parallelism and cost (rule 51)

The snapshot now reads one start time per process. On Windows that is an `OpenProcess`, a `GetProcessTimes` and a
`CloseHandle`. On macOS it is one `proc_pidinfo`, and on Linux one read of `/proc/<pid>/stat`. A snapshot is taken by
each tree kill and by each memory-cap sample (every 200 ms for each capped child). Measured on the Mac mini (macOS, a
release build of `cargo run --release -p paddock-proc --example snapshot_cost`, 50 rounds, about 1,130 processes, load
17 to 23, 2026-10-30): a whole snapshot takes 13.6 to 14.7 ms, and the start times account for 0.53 to 1.77 ms of that
(4 to 12%). This is an indication taken under load (docs/benchmarks.md). Windows is not measured yet. If it shows a cost
that matters, the cheaper operation is to read start times lazily, only for the processes the walk visits (a handful
per tree), behind the same `Table` type. The work is not parallelised: one snapshot is one pass over the table, and the
memory sampler already runs one thread per capped child.

## Alternatives

- **Only the job object (Windows) and the process group (Unix).** This closes the flake on Windows, but leaves helpers
  that left the group, the walk used by memory caps, and the apps' file-trusted pids as they were.
- **Linux `pidfd` and Windows handles everywhere.** Exact on those systems, but macOS has neither. A handle kept for every
  process in a snapshot costs more than a start time. The Windows kill uses a handle where it matters (decision 2).
- **Retry or wait in the tests.** This is forbidden (ER-12), and it only moves the race.

## Consequences

- `paddock-proc`'s API breaks (pre-v1, rule 39): `kill_tree`, `end`, `ask`, `terminate`, `descendants_of`,
  `processes_mentioning`, `usage::descendants` and `Table` rows (`Row` with `started`). The callers in the shared crates
  and Dyno are updated in the same change.
- A kill that is skipped leaves a process running that the old code would have ended. That process is not ours, which is
  the point. A helper whose start time cannot be read (a protected process on Windows) is no longer ended by the walk. Its
  job object still ends it.
- `paddock-proc` depends on `log`.

## Migration

- **WP-1** (this branch): `paddock-proc` mechanism, the regression test with a fake process table, Windows job termination,
  and the start time on three OS. About 1 day.
- **WP-2** (this branch): `paddock-chromium`, `paddock-ipc`, Dyno's `TreeGuard`. 0.5 day.
- **WP-3** (this branch): C53, its tests, the waivers. 0.5 day.
- **WP-4**: one Windows CI run of this branch. A failure with "network service restarted" while no browser was ended
  nearby means this was not the cause, and ADR 0030's reload decision comes back.
- **WP-5** Debrief (`chromium-driver` moves to `paddock-chromium`, X14; `debrief-ipc`, `debrief-ai-worker` and the test
  kills go through `paddock_proc`). About 1 day.
- **WP-6** Pitcrew (`launch.rs`, `process.rs`, `process_watch`, `probe.rs` through `ProcessId` and
  `paddock_proc::descendants`; adopted runners recorded with their start time). About 1.5 days.
- **WP-7** Pitwall `system.rs` through `paddock_proc`. 0.25 day.
- **WP-8** Spotter `spotter-render` onto `paddock_proc::Supervised` (or `paddock-chromium`, X14). 0.5 day.
- **WP-9** `tools/paddock-dev` slot test and Dyno's macOS parent display. 0.25 day.

## Rollback

Revert the branch. The API change is internal to the monorepo, and no file format changed: `paddock-ipc`'s discovery
file already carried `startedMs`.
