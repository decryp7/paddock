# 33. Processes by identity, never by a bare pid

- Status: Accepted (2026-10-30, by the user) as binding rule 53. Pid reuse was the suspected cause of the Windows
  `ERR_ABORTED` failures; **the suspected cause was disproved by run 7158; the flake has another cause (WP-4)**. The rule
  stands on its own, on the real mistakes the sweep found in every shape below.
- Date: 2026-10-30
- Related: rule 53 (this decision), rule 40 / ER-21 (recurring bugs mean a missing design rule), ER-14 (tests touch only
  what they started), rules 42/43 (share first); ADR 0024 rule 4 (a watcher tick reads no whole process table;
  `perf/proc-per-pid`, squashed into this branch); the ADR 0030 draft on `fix/flaky-err-aborted-2` (unexplained browser
  aborts name their cause; kept out of this branch and handled with a local Windows reproduction); ADR 0023 (the same
  rule for ports)
- Evidence:
  - The Windows crate jobs of runs 7110, 7123, 7124, 7134, 7140, 7142 and 7145 failed in `paddock-chromium`'s
    `real_browser::*` with `net::ERR_ABORTED`.
  - In jobs 21399, 21598, 21908 and 21966, the test `a_browser_ended_by_force_leaves_nothing_behind` finished 0.04 to
    2 s before the failure, in the same test binary. The four real-browser tests run in parallel.
  - The ADR 0030 draft showed that ending the network service gives exactly the trace of these failures.
  - **Disproved as the cause.** CI run 7154 (this work together with ADR 0030's diagnostics) showed a Windows load
    aborted with no response event while the network service stayed the same process. CI run 7158, with every identity
    fix of this ADR in place and none of the ADR 0030 code, still failed on Windows with `ERR_ABORTED` in
    `a_new_tab_loads_a_page_at_once`. The suspected cause was disproved by run 7158; the flake has another cause (WP-4).
    The pid reuse below is still a real way to kill another program's process, which is what this ADR removes.
  - On macOS, `proc_pidinfo(PROC_PIDTBSDINFO)` refuses a process of another effective user. Measured by the review: a
    setuid `/usr/bin/top` child gives EPERM.

## Context

Before this change, `paddock_proc::kill_tree` worked in three steps:

1. It read the process table.
2. It ran `taskkill /T /F` on the root.
3. It killed every pid of the snapshot again, one by one, through sysinfo.

By step 3 those pids were free. Windows hands a freed pid to the next process it starts: a pid "is valid from the time
the process is created until the process has been terminated" (read 2026-10-30,
<https://learn.microsoft.com/en-us/windows/win32/procthread/process-handles-and-identifiers>). With four tests each
starting Chrome, the next owner is likely another test's browser helper. Killing its network service gives the
trace of the Windows failures: the request leaves, the service dies, the load is cancelled with `ERR_ABORTED
canceled=true`, and the server never sees a connection. That made it the suspected cause; the suspected cause was
disproved by run 7158, and the flake has another cause (WP-4; Evidence).

Whatever caused the flake, the same mistake has several shapes, and a sweep found them across the family:

1. **Snapshot, then kill.** A pid read from the table is killed later, and nobody asks who holds it now.
2. **Parent links without ages.** A tree is built from parent pids, which outlive their processes on Windows
   (`PROCESSENTRY32.th32ParentProcessID` is only "the identifier of the process that created this process", read
   2026-10-30, <https://learn.microsoft.com/en-us/windows/win32/api/tlhelp32/ns-tlhelp32-processentry32>). An orphan's
   parent pid can name a newer, unrelated process. Through it the orphan, an "explorer orphan" started at logon, joins a
   browser's tree, so its memory counts against the browser's cap and the cap's kill reaches it.
   `crates/paddock-proc/tests/it/orphan.rs` is that shape. `taskkill /T` walks the same links.
3. **Kill after reap.** `Child::try_wait` reaps a child that ended. A later `killpg(child.id())` signals whatever got the
   pid.
4. **A pid from a file.** A discovery or state file names a pid, which is trusted as long as some process has it.

**Pattern:** a process is ended, or counted as ours, by a pid read earlier, when that process may have ended in between.

**Invariant:** before anything acts on a process, it holds the process (a handle, a job, an unreaped child), or it
checks that the pid still belongs to the process it means (pid plus start time).

## Decision

1. **One identity type, in `paddock-proc`.**
   - `ProcessId` is a pid and a `StartTime`, in microseconds since the Unix epoch. The start time comes from the OS:
     - Windows: the creation time, `GetProcessTimes` on a handle.
     - macOS: `kp_proc.p_starttime` from `sysctl(CTL_KERN, KERN_PROC, KERN_PROC_PID, pid)`. This sysctl answers for
       every process, including setuid ones and zombies. `proc_pidinfo` refuses other effective users.
     - Linux: the boot time plus the start tick in `/proc/<pid>/stat`.
   - `ProcessId::of(pid)` reads the current holder of a pid. `ProcessId::of_recorded(pid, written_ms)` is for a pid
     read from a file whose process wrote it after it started; a holder that started later is someone else.
   - `Holder` and `Ending` say what a check or a kill found.

2. **Every kill checks identity, as tightly as the OS allows.** `kill_tree`, `end`, `ask` and `terminate` take a
   `ProcessId`.
   - **Windows:** the check and the action use one process handle. The handle keeps the process object, and so its pid,
     from being handed on: `TerminateProcess` for `end`, and a `SYNCHRONIZE` handle held across the `taskkill` close
     request for `ask`.
   - **Linux:** a pidfd is opened first. The start time is read, and `pidfd_send_signal(0)` confirms that the pidfd's
     process still runs. The signal then goes through the pidfd. This leaves no gap. Without pidfds (a kernel older
     than 5.3), Linux falls back to a check followed by a signal.
   - **macOS:** a check followed by a signal. A gap of microseconds remains; macOS has no pidfd.
   - A skipped kill (the pid was handed on) and a refused kill are logged. `kill_tree` returns what it found for each
     process.

3. **Trees follow parents by age, or by job membership.**
   - **Per-pid walks.** `perf/proc-per-pid` replaced the per-tick process table with per-pid reads:
     `proc_listchildpids` on macOS, `/proc/<pid>/task/*/children` on Linux, and a light Toolhelp snapshot on Windows.
     These walks take a `ProcessId`. `ProcessSource::started` gives every node's start time. A child counts only if it
     started no earlier than its parent: `>=`, because on Linux a child can share its parent's 10 ms tick. A parent's
     listed children count only if the parent still held its pid after the listing.
   - **Table walks.** `usage::descendants` and `tree_memory_with` apply the same rule to `Table` rows, which carry
     `started`. For a root that is gone, only the processes that started before the new holder of its pid count.
   - **Scans.** Where the children come from a scan taken earlier in the walk (Windows without a job), a child must also
     have started no later than the scan: a younger process got the scanned pid afterwards.
   - **Windows supervised children.** The job object's member list is the tree; it follows no parent pid.
     `Job::members` identifies each member through a handle that `IsProcessInJob` confirms. `Job::pids` and the
     memory sampler read the list.

4. **Supervised children are held, not looked up.**
   - **Spawn.** `Supervised` takes the child's identity at spawn, and the registry and the memory watcher hold
     `ProcessId`s.
   - **Kill on Windows.** The members are read first, then `TerminateJobObject` ends the job, then each member is ended
     by identity. The root is always treated as ours there, reaped or not: the `Child`'s handle pins its pid until it is
     dropped. Terminating a member that had already exited (Windows answers ACCESS_DENIED) counts as gone, not refused.
   - **The root.** It is trusted without a check only while `try_wait` says it has not been reaped. An unreaped child
     with no readable start time is still signalled through its group and killed through `Child`.
   - **The group (Unix).** It is signalled while the root is still itself, or while the root is gone and no process
     holds its pid. POSIX does not hand a pid on while a process group with that id exists, so such a group still
     holds only the root's helpers. When a newer process holds the pid, the group is left alone.

5. **Checks.**
   - **Clippy's `disallowed-methods`** (lint/clippy.toml, which C13 requires in every app's clippy.toml; also
     crates/clippy.toml and tools/paddock-dev): `libc::kill`, `libc::killpg`, nix's `kill`/`killpg`, and sysinfo's
     `Process::kill`/`kill_with`. Clippy resolves them by path, so a renamed import cannot hide one. The one
     implementation (`paddock-proc`'s `sys.rs`) and each known app site carry
     `#[expect(clippy::disallowed_methods, reason = "...")]`.
   - **Conformance check C53** covers what Clippy cannot see:
     - programs named in strings: `taskkill`, `Stop-Process`, `/bin/kill`, `kill`/`pkill` commands;
     - Win32 calls: `TerminateProcess`, `TerminateJobObject`;
     - a blanket `#![allow(clippy::disallowed_methods)]` (or one inside `#![cfg_attr(..)]`): an inner attribute turns
       the ban off for a whole crate or module. `chromium-driver` and `spotter-render` had one; it is narrowed to
       `#[expect]` on each `Command::new` site;
     - open follow-ups: any `ADR 0033 WP-<n>` in a comment or an `#[expect]` reason. Each needs a dated waiver in
       `paddock.waivers.toml`, so a known bare-pid kill or parent-pid walk fails CI when its waiver expires.
   - **Limits:**
     - a pid handed to another program some other way (a script file, a shell string built at run time) is not seen;
     - an item-level `#[allow(clippy::disallowed_methods)]` (for a `Command::new`) also hides a kill in the same item.
       It is allowed, because it is visible at the item, and a reviewer has to read it;
     - the existing file-wide allows in tests (Debrief, Pitcrew, Pitwall, tools/paddock-dev) are dated waivers;
     - parent-pid walks that do not kill are found by review and marked by hand. C53 does not find them.

### Residual risks

- **macOS:** the check-then-signal gap described in decision 2.
- **Linux without pidfds:** the same check-then-signal gap.
- **Pids from a file:** `started_by` allows 1 s on Linux, because the boot time is in whole seconds. Elsewhere it
  allows the record's millisecond. A pid handed on within that second, on Linux, is taken for the writer.
- **Clock steps:** a discovery file's `startedMs` is wall-clock time, and a start time is wall-clock based too (Linux
  through the boot time). If the clock steps back between a companion's start and the moment it writes its file, the
  companion looks as if it started after its record. It is then treated as stale: it is not ended, and a client may start
  a second companion. If the clock steps forward, a newer holder of the pid can pass as the writer. Both need a clock step
  within the companion's start-up; the fix would be a start time recorded in the file (a format change, not made).
- **Same tick:** a child may share its parent's start tick (`>=`). A stranger that started in the same tick as a
  handed-on parent pid would count. The creation time on Windows is kept in 100 ns units but advances with the system
  clock's update interval (commonly 0.5 to 15.6 ms, not measured here); macOS reports microseconds; Linux counts 10 ms
  ticks. So on every OS the window is a reuse within one clock tick of the parent's start.
- **Memory only:** a job member's memory is read by pid. A member that ends and whose pid is handed on between the list
  and the read is measured once wrongly, but never killed.

### Parallelism and cost (rule 51)

- **Per-tick paths** (the memory watcher) read only the tree's pids, as `perf/proc-per-pid` designed. They now read one
  start time per node and one more per parent after its listing, each a single syscall or one small `/proc` read.
- **The whole table** with start times (`snapshot()`) is read only by one-off work: ending a tree without a job, leak
  checks and `descendants_of`.
- **Measured** on the Mac mini (a release build of `cargo run --release -p paddock-proc --example snapshot_cost`, 50
  rounds, about 1,130 processes, load 17 to 23, 2026-10-30): a snapshot takes 13.6 to 14.7 ms, of which the start times
  take 0.53 to 1.77 ms. Recorded in `docs/benchmarks.md`. Windows is not measured. Its kill path uses the job's member
  list and reads no table, so a Windows snapshot matters only for `descendants_of` and leak checks.

## Alternatives

- **Only the job object (Windows) and the process group (Unix).** This closes the flake on Windows. It leaves the cap's
  walks, helpers that left the group, and the apps' pids from files.
- **Handles everywhere.** Exact, but macOS has no equivalent, and a handle held per process of a snapshot costs more than
  a start time. Handles are used where the OS has them (Windows handles, Linux pidfds).
- **Retry or wait in the tests.** Forbidden by ER-12, and it only moves the race.

## Consequences

- `paddock-proc`'s API breaks (pre-v1, rule 39), and every caller is updated in this change (Dyno, Spotter,
  `paddock-chromium`, `paddock-ipc`):
  - `kill_tree` now returns a `TreeEnding`;
  - `end`, `ask` and `terminate` take a `ProcessId`;
  - `descendants_of` and `processes_mentioning` return `ProcessId`s;
  - `usage::descendants` and `tree_memory_from` take a `ProcessId`;
  - `Table` rows are `Row { parent, rss, started }`, and `ProcEntry` has `started`;
  - `ProcessSource` has `started`.
- `paddock-ipc`'s `ClientConfig::is_alive` takes the `Discovery`. The default, `discovery::is_running`, treats a file
  whose pid was handed on as stale.
- A skipped kill leaves running a process that the old code would have ended. That process is not ours, which is the
  point. A helper whose start time cannot be read is no longer ended by the walk; on Windows its job still ends it.
- `paddock-proc` depends on `log`.
- `run_bounded` logs when it cannot read the child's start time: a timeout or the output cap then cannot end its tree.

## Migration

- **WP-1 to WP-3** (this branch):
  - the mechanism on top of the per-pid design;
  - the regression tests: a fake process table and the explorer-orphan tests, which were `should_panic` before;
  - the macOS sysctl, a setuid test, and a test that reaps a child and then ends its group;
  - the Windows job-member tree;
  - Clippy's `disallowed-methods` and C53, with the waivers;
  - rule 53.
- **WP-4:** find the flake's real cause, in a task of its own with a local Windows reproduction (the ADR 0030
  diagnostics and their two Windows tests go there). Runs 7154 and 7158 showed aborts with an unchanged network service
  and with every identity fix in place, so pid reuse is not it.
- **WP-5 Debrief:**
  - `chromium-driver` moves to `paddock-chromium` (X14);
  - `debrief-ipc` `kill_group`, the `debrief-ai-worker` reaper and the test kills (`debrief-collect`,
    `debrief-plugin-windows`, `health/commands/tests.rs`, `debrief-store-worker/tests/it/worker.rs`) go through
    `paddock_proc`;
  - the `debrief-monitor` `expand_children` walk follows parents by age.
- **WP-6 Pitcrew:**
  - `launch.rs` `kill_tree` and adopted runners (recorded with their start time);
  - `process.rs`, `process_watch` and `terminate.rs` through `ProcessId`;
  - `probe.rs` `alive` and `has_job_children`, and `pitcrew-core` `tree_members`, through
    `paddock_proc::descendants`.
- **WP-7 Pitwall:** `system.rs` through `paddock_proc`.
- **WP-8 Spotter:** the renderer is on `paddock_proc::Supervised` (`perf/proc-per-pid`), and its watchdog kills by
  identity (this branch). Its `launch.rs` watchdog follows on `fix/spotter-safe-kills`.
- **WP-9:** the `tools/paddock-dev` slot test, and Dyno's macOS parent display.

## Rollback

Revert the branch. The API change stays inside the monorepo. No file format changed: `paddock-ipc`'s discovery file
already carried `startedMs`.
