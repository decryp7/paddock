# 6. Supervision, jobs and housekeeping

- Status: accepted (2026-10-22)
- Related: ADR 0001-0003; code: `pitcrew_core::{controller, jobs, housekeeping, history, metrics, notify, session}`,
  `pitcrew_service::{supervisor, launch, launcher, cleanup, work, logfile}`

## Decision

### One supervisor thread, pure controllers

All runner state lives on one thread that receives messages (requests, output lines, worker results, log-on changes)
and ticks every second. Slow work (downloads, verification, `--version`, registration, folder scans and deletion)
runs on worker threads that post results back. There are no locks; one runner's failure is only a state of that
runner. Every decision is the core's: a `Controller` per runner (start, backoff, crash loop, stalls, stop / restart /
drain **between jobs**, holding for updates), the `JobTracker`, the `UpdateMachine` and the housekeeping planner.

### Jobs

"Task started" log lines give the job (repository, job, run, task; the runner masks secrets in its own log, so the
repository can read `***/repo`). The runner logs nothing when a job ends, so a job runs while the runner has child
processes (its console host does not count) **and for 20 s after the last one**: host-mode steps start fresh
shells, and a gap between two steps must not look like the end of the job. Stop, restart, drain, stall restarts,
session moves, updates and cleanups all wait for that.

### Stall rule

As in the PowerShell watchdog Pitcrew replaces (with the refinement of 2026-10-22): **at least 6 "failed to fetch
task" lines (timed out or failed otherwise) within 15 minutes while no job runs.** An idle runner logs nothing for
hours; silence is never a stall. Stall restarts do not grow the backoff.

### Logs

The service owns the runner's stdout/stderr pipes (no `cmd >>` redirection, which locked the file): every line goes
to the watchdog, a bounded in-memory tail (5000 lines / 4 MB) and `<dir>/logs/runner.log`, which Pitcrew rotates
itself between two lines (10 MB × 5 files by default) and to which it adds its own `<time> supervisor: …` lines.

### Sessions (Windows)

Interactive runners start in the console user's session (`WTSQueryUserToken` + `CreateProcessAsUserW`, with an
explicit list of inherited handles). With nobody logged on they wait (default) or, with
`logged_off = "service-account"`, run as LocalSystem until someone logs on and they are idle again. The graceful stop
is Ctrl+Break, sent by a helper (`pitcrew-service ctrl-break <pid>`) started in the runner's session. A runner left
running by a previous service instance (its pid is in `state.json`) is adopted and restarted between jobs to get its
output back; any other process running the same binary blocks the start with a message (e.g. the old scheduled task
still running).

### Housekeeping

Pitcrew cleans each runner's `work/` and `cache/` entries, leftover downloads (`*.download`, `*_new.exe`), old binary
names (`act_runner.exe`) and rotated logs beyond the rotation's `keep` — **only while the runner runs no job and no
update**, never entries changed in the last 10 minutes, and always keeping `<binary>.previous` for roll-back unless the
policy says otherwise. Triggers: the schedule (every 24 h), free disk below the minimum (10 GB), or the folder above its
cap (50 GB); a cleanup is not repeated within 10 minutes. The core plans the order (leftovers → old logs → aged work →
cache over its cap → under pressure the oldest work, then the oldest cache); the service deletes with verbatim long
paths, clears read-only attributes and retries, and reports what it could not delete ("a file is in use…", "access is
denied…") for the next run instead of failing. Every cleanup is a history entry and, when it freed something or had
problems, a notification; the Disk page shows a dry-run preview. Policies are per runner with defaults.

### History and metrics

Per runner, at most 4000 events (starts, exits with the reason, stalls, crash loops, jobs, updates, cleanups) in
`state.json`; the core computes summaries (uptime, restarts and jobs per local day, timeline spans and marks). Machine
metrics (CPU, memory, free disk) are sampled every 30 s into three tiers (6 h raw, 7 days of 5-minute averages,
90 days hourly; free disk aggregated as the minimum); runner folder sizes hourly. The service reports its own CPU and
memory so the dashboard can show what watching costs.

## Consequences

The service's integration tests drive the real supervisor with a fake runner (`examples/fake_runner.rs`) and a
controllable clock: exits and backoff, a stall with a real graceful stop, a stop that waits for a running job,
registration with a bad and a good token, settings validation and a manual cleanup — on Windows, Linux and macOS CI.
