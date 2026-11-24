# Upstream issue for gitea/runner: a job-scoped process group on Unix (ADR 0112, D4)

Text to post at https://gitea.com/gitea/runner/issues. Written 2026-11-21 against v5.4.1 and v6.0.1.

---

**Title:** Host mode on macOS: processes a step leaves behind outlive the job (no job-scoped process group on Unix)

**What happens**

In host mode, a step that ends normally can leave processes running: a background script, a helper started with `&`. On Windows
the runner puts every step of a job into one job object and terminates it in `HostEnvironment.Remove`, so these end with the job.
On Linux and macOS the job's end relies on `KillTrackedProcesses`, which looks for `RUNNER_TRACKING_ID` in each process's
environment.

On macOS that marker cannot be read for programs Apple ships. `kern.procargs2` (and `ps eww`) returns no environment for
`/bin/bash`, `/bin/sh`, `/bin/sleep` and other platform binaries, even for the same user. An ad-hoc signed copy of the same binary
shows its environment. So a shell script a step leaves behind is never matched, and it keeps running after the job, sometimes for
days.

The per-step process group (`Setpgid` in `SysProcAttr`) would reach these processes, but it is signalled only when the step is
cancelled. `Group` on Unix is a no-op, with the comment that process groups already reclaim a step's tree. That is true on cancel,
not after a step that succeeded.

**How to reproduce**

1. A host-mode runner on macOS.
2. A job with a step `run: ( /bin/sleep 3600 & )` or `run: /bin/bash -c 'sleep 3600' &`, then any second step.
3. After the job ends, the `sleep` process is still running.

**Versions:** v5.4.1 and v6.0.1, macOS 27 (also expected on earlier macOS releases). The code paths are the same at both tags:
`internal/pkg/process/sysprocattr_unix.go`, `killer_unix.go`, `treekill.go`, `tracked.go`, `group_others.go`.

**Suggested shape**

Make `Group` on Unix job-scoped, the same idea as the Windows `Group`:

- `Assign` records the process group of each tracked step (its pgid, which equals the step's pid, plus the leader's start time).
- `Close`, called from `Remove`, signals each recorded group that still has members: SIGTERM, a short grace, then SIGKILL, the same
  sequence as `Killer.cancel`. A group whose leader start time no longer matches any member started after it is skipped, so a
  reused id is never signalled.
- `KillTrackedProcesses` stays as the net for processes that left their group (`setsid`).

A process that calls `setsid` and is not a platform binary is still caught by the tracking id; one that is both stays out of reach,
which seems acceptable without cgroups.

We can test a patch on a macOS host runner if that helps.
