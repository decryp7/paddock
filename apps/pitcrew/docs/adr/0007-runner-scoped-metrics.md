# 7. Runner-scoped metrics

- Status: accepted (2026-10-22)
- Related: ADR 0006; code: `pitcrew_core::{usage, metrics}`, `pitcrew_service::{probe, supervisor}`

## Context

The dashboard showed the CPU and memory of the whole machine. On a shared build machine that mostly measures other
software, and it does not say what the CI work costs. The owner asked for the dashboard to be about the runners.
Free disk stays machine-wide: it is the number that makes jobs fail, whoever filled the disk.

## Decision

**What is measured.** For each runner, the CPU and resident memory of its process **and everything it started** (the
job's shells, compilers, test runners, browsers). The service reads the OS process table on every probe tick (5 s)
through `sysinfo`, which is already a dependency and covers macOS (libproc), Linux (`/proc`) and Windows (Toolhelp
and process APIs); no per-OS adapter is needed.

**Which processes belong to a runner** (`usage::tree_members`, pure): the runner process, all its descendants by
parent pid, and, on macOS and Linux, every process in the runner's process group (Pitcrew launches runners as group
leaders, so a job process reparented to `init` when its parent exits is still counted). On Windows the parent-pid
tree is used; a Windows job object would also catch orphans, but Pitcrew does not put runners in one yet, so that is
a possible later refinement. A process that leaves the group (`setsid`) **and** loses its parent is not counted: it is
no longer the job's, and following it would be guessing. The tree is bounded (`MAX_TREE_PROCESSES` = 4096) and the
walk is cycle-safe.

**CPU.** Per mille of the **whole machine's** capacity (all logical cores = 1000), so the runners' share and the
machine's share add up in the same unit. `UsageTracker` takes the gain in cumulative CPU time of the tree's processes
since the previous tick and divides by `elapsed × cores`. A process seen for the first time counts all its CPU time
(it started within the interval); a process that exited between two ticks takes its last interval with it, a small
under-count we accept. The first reading after a runner starts sets the baseline only.

**Memory.** Resident set size, summed. Pages shared between processes (a browser's helpers) are counted once per
process, so the sum can exceed the physical use of a tree; resident size is what every OS reports cheaply and
comparably, and it is the number to compare with the machine's "memory in use".

**History.** The 30-second sample averages the probe ticks since the last sample and keeps the peak tick. Two series
sit next to each other:

- the machine series (`MachineSample`) keeps the machine's CPU, memory and free disk and gains `runners` — all runners
  summed per tick, then averaged — so a chart can show runners against the rest of the machine;
- one series per runner (`UsagePoint` in `RunnerState.usage`), three tiers like the machine's but shorter (1 h raw,
  7 days of 5 minutes, 30 days of hours) because up to 16 runners share the state file.

Both use the same generic tiered store (`TieredStore<P: Series>`), so retention, downsampling and the "clock went
back" rule exist once. New fields have serde defaults: older `state.json` files load with zeros and the
state version is unchanged (pre-v1, no migration).

**Protocol and UI.** `RunnerSnapshot.usage` (now), `Response::Metrics.usage` (a runner's series) and
`MachineSample.runners` are mirrored in `api.types.ts` and the app DTOs. The dashboard tiles show the runners' CPU and
memory, a muted line gives the machine's totals, the history shows CPU and memory as runners stacked on the rest of
the machine, the runner detail shows one runner with trends, and the tray tooltip shows the runners' totals.

## Consequences

- Runner numbers are comparable to machine numbers; "Mac 62 % CPU, runners 18 %" is now visible.
- A stopped runner records no points (a gap, not zeros); the machine series records zeros for the runners' share.
- Orphans that fully detach are not counted, and on Windows orphans reparented away from the tree are not counted.
- Reading the whole process table every 5 s is cheap (a few hundred rows; one `getpgid` per row on Unix).
