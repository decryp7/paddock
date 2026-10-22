# 12. Tracking the processes a runner starts, and cleaning up leftovers

- Status: accepted (2026-10-22)
- Related: ADR 0006 (supervision), ADR 0007 (runner-scoped metrics, whose process tree this builds on); code:
  `pitcrew_core::{process_track, process_policy}`, `pitcrew_service::{process_watch, terminate, probe, supervisor::processes}`,
  `app/src/app/features/runners/` (Processes tab)

## Context

Jobs leave things behind: a Gradle or Kotlin daemon, an emulator, a dev server that never exited. They hold memory and
ports for days and make the next job behave differently. The owner wants to see which processes a runner started, which
job they belong to, which ones outlived their job, and to have the machine tidy itself, without ever killing a running
job's work or anything that is not the runner's.

## Decision

**The list.** Every probe tick (5 s) the service takes the runner's process tree from the process table
(`usage::tree_members`: the runner, its descendants, and its process group on macOS/Linux; threads are already filtered
out) and describes at most 512 of them (`Probe::process_metas`: name, executable, command line, start time, memory and a
per-process CPU share from the same tracker as ADR 0007). `Request::Processes` returns up to 300 rows (leftovers first,
then the runner, then by memory), each with pid, parent, name, CPU, memory, start time, the job it started in and, for
leftovers, what will happen. The command line is cut and passed through `redact` (the logs' redaction), so tokens never
reach the UI. Reading the list has no side effects.

**Which job.** A process belongs to the job that was running when it started: job spans come from the history's finished
jobs (the newest 256) and the running one, with 10 s of slack at both ends (the service notices jobs on a tick).

**Leftover.** A process, other than the runner, that belongs to a *finished* job and is still alive. A process of a
running job is not a leftover, and one that started outside any job is listed but never cleaned automatically (nothing
says it is the job's). Peak CPU, peak memory and the number of survivors are stored with each finished job
(`FinishedJob.peak_cpu_permille/peak_mem_bytes/leftovers`) and shown in Activity.

**Automatic cleanup is on by default** (user decision, 2026-10-22) and conservative:

- only while the runner is **idle** (no job running, runner alive);
- only processes **from that runner's tree**, checked against the table read at the moment of the decision;
- a **keep time** after the job ended: a number and a unit (minutes, hours, days; 1 minute to 30 days), default **10
  minutes**;
- **rules** match a process by name or path, or by command line, with `*` and `?` (case-insensitive; without a wildcard the
  name or full path must be equal), each with its own keep time or `never`; first match wins; a runner's own rules are tried
  before the defaults'. Shipped defaults: `*GradleDaemon*` 3 h, `*KotlinCompileDaemon*` 3 h, `qemu-system*` 30 min,
  `*/emulator/emulator*` 30 min, `adb` never (documented in `process_policy.rs` and the user guide);
- configuration `[defaults.processes]` (`cleanup`, `keep`, `stop_timeout_secs`, `rules`) and per-runner `[runners.processes]`
  (`cleanup`, `keep`, `rules`); it can be turned off globally or per runner; with it off, leftovers are still listed (without a countdown);
- **graceful then forced**: SIGTERM to that one pid (never the group: a leftover is usually still in the runner's group,
  and a group signal would stop the runner); on Windows `taskkill` (WM_CLOSE). If it is still alive after the stop timeout
  (15 s) it is ended (SIGKILL / `taskkill /F`); a process that survives even that is given up on and reported;
- every stop is logged in the runner's log and recorded in the history as `ProcessStopped` (name, pid, age, the rule used,
  who decided, whether it had to be ended), shown in Activity;
- "Keep longer" adds time to one leftover (kept in memory per pid and start time, capped at 30 days in total; a service
  restart forgets it, and the leftover then follows its rule again).

**Stopping by hand** (`Request::StopProcess`) uses the same machinery and the same guard (`check_stop_target`), with a
confirmation in the UI: the process must be in the runner's tree now, with the start time the app saw (a reused pid is
refused), is not the Pitcrew service, not pid 0 or 1, and is **not the runner itself unless the caller says so
explicitly** (`allow_runner`); the supervisor then treats the exit like any other runner exit.

**Platforms.** macOS and Linux for real (signals via `nix`; verified in a Linux container with the fake runner starting
children that outlive the job). Windows compiles (`taskkill`) and is exercised only on the Windows CI runner; there the tree
is the parent-pid tree (ADR 0007), so orphans that lost their parent are not found.

## Consequences

- A new default behaviour stops processes without asking, within the limits above; users who keep daemons longer add a
  rule or turn it off, and the Processes tab always shows what will happen and when.
- Command-line matching can match more than intended (`*gradle*`); the log line names the rule that applied.
- A daemon that detaches (`setsid`) and loses its parent leaves the tree and is no longer tracked (ADR 0007).
- Job attribution is by start time, so a process started by a step of a later job that outlives it belongs to that later job.
