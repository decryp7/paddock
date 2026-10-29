# 13. Holds: the low-disk guard and pause windows

- Status: accepted (2026-10-23)
- Amended by [ADR 0022](0022-host-environment-and-shared-folders.md) (accepted 2026-10-29): the guard watches every volume a runner writes to, `SpaceCache` is keyed by volume, and two hold reasons (`SharedFolderUnavailable`, `SecretUnavailable`).
- Related: ADR 0006 (supervision and housekeeping); code: `pitcrew_core::{hold, disk_guard, pause}`,
  `pitcrew_service::{probe, supervisor::holds}`, Settings > Disk guard and Pause windows, the hold badge on runner cards

## Context

"No space left on device" is the most common way a self-hosted runner fails: the job dies half way. Cleanup (ADR 0006)
reacts after the fact. Jenkins' node monitors take a node offline below a free-disk threshold and bring it back after
recovery, but their own bug reports (JENKINS-37057, JENKINS-34712) show two traps: a monitor that fails on a full disk
and an offline flag that never clears. Owners also want runners that stay out of the way: no jobs at night, or only
while the machine is otherwise quiet.

## Decision

**One mechanism: a hold.** Pitcrew pauses a runner by *draining* it (finish the current job, take no more; the existing
`Drain`/`Resume`), for a *reason* (`HoldReason`: low disk, a pause window, a busy machine) and resumes it when no reason
is left. `hold::HoldTracker` is a pure state machine fed once per probe tick (5 s) with "a hold is wanted" and "the
runner is drained". It never touches a pause the user made: a runner already paused by the user is left paused; if the
user pauses, resumes, starts or stops it by hand the hold is off for good until the reason has gone away (so "Resume"
inside a pause window works, and the runner is not drained again at the next tick). Whether the drain is Pitcrew's is
saved per runner in `state.json` (`hold`), so a service restart does not strand a runner paused. Every hold and release
is a history event (`Held`/`Released`), a log line and a notification (`Held`, `Released`). A stopped runner or one in a
crash loop is not held.

**Low disk.** `[defaults.disk_guard]`: `enabled`, `drain_below_mb` (4 GB), `resume_above_mb` (6 GB; must be larger, the
hysteresis stops a disk hovering at one limit from flapping) and `reserve_mb` (1 GB; must be smaller than the drain
threshold). The reserve is the Jenkins lesson: space Pitcrew keeps for itself. Under it a cleanup is requested at once
(not on the schedule). Free space unknown changes nothing. A runner's disk is the disk of its folder.

**The disk is not polled often.** Reading disk space is not free (on macOS it is a file system call per volume).
`disk_guard::SpaceCache` answers from memory for at least 60 s between two reads of the same disk; every reader in the
service (the guard, scans, the machine sample) goes through `Probe::disk_space`. So reaction time to a falling disk is
about a minute, which is fine against jobs that run for minutes.

**Pause windows.** `[defaults.pause]`: `mode` = `off` | `schedule` | `when-idle`.
- `schedule`: `[[defaults.pause.windows]]` (`days`, `start`, `end`, local time, an end before the start spans midnight;
  the same type as update windows) hold the runners while open; the card shows "paused for N more minutes".
- `when-idle`: runners take jobs only when the machine is otherwise quiet. Busy means the load *outside* the runners
  (machine CPU minus the runners' share, from the metrics sample) is at least `busy_percent` (40) of all cores, and it
  stays busy for `quiet_minutes` (5) after the load drops. Unknown counts as idle, so a restart does not hold the
  runners.

Both go through the same tracker as the disk guard, so reasons combine and the runner resumes only when all are gone.

## Consequences

- The guard cannot interrupt a job; it prevents the next one. A single job that fills the disk is still the job's
  problem; cleanup between jobs and the reserve limit the damage.
- The settings are validated like the file on load; an invalid section is ignored with a message and the defaults apply.
- `when-idle` reads a 1-minute-bucket CPU figure; short bursts below the sample period do not count.
