# Scheduled CI runs

Scheduled workflows run at night, local time UTC+8 (cron is UTC), staggered so they do not overlap on the three Linux
runner slots. Matrix jobs are capped with `strategy.max-parallel` so one Linux slot stays free for release and branch runs
(rule 50; trigger: Debrief's fuzz run at 10:23 local held all three slots for about 30 minutes).

**A train in CI skips the night (user, 2026-11-12).** Every scheduled workflow first asks `./dev ci schedule-skip` (the
`schedule-skip` action, or the `plan` action's step): while a train's `workflow_dispatch` run on `train/*` is queued or in
progress, the scheduled run ends early with one `::notice::skipped: train <branch> in CI ...` line, runs nothing and
writes no green marker; it runs again on the next schedule after the train lands. Such a run is not measured, never green
(ADR 0093, amendment 2026-11-12). When the Actions API cannot be read the run proceeds (fail open) and says so in its
summary. A run still listed as running after 24 h is named but not counted, so a lost runner cannot stop every schedule.

**Proxmox backup window.** Starts at 00:00 local (UTC+8; 16:00 UTC), stated by the user on 2026-11-10. The end is not
known; 04:00 local (20:00 UTC) is assumed until evidence says otherwise. The Windows CI VM crawled from 00:52 local on
2026-11-09/10 (memory note `windows-ci-proxmox-backup`). Rules:

- A workflow with a Windows leg neither starts in 00:00-04:00 local nor is likely to run into it. Because the end is
  only assumed, the Windows starts keep an hour of margin: nothing starts before 05:00. These times will be reset once the
  real end is read from the Proxmox task log.
- Linux-only and macOS-only workflows START between 22:00 and 23:59 local or after 04:00. debrief-fuzz runs until about
  02:30 local (estimate).
- Open point: whether the Linux runners are affected by the backup at all is unknown; the Linux-only runs that cross
  midnight (fuzz, perhaps mutants) assume they are not.

Typical durations are from the last five scheduled runs (Gitea API, 2026-11-06 to 11-10; the failed runs count, they
ran the same jobs).

| Workflow | Cron (UTC) | Local (UTC+8) | OS legs | Typical duration | Linux slots at most |
| --- | --- | --- | --- | --- | --- |
| debrief-mutants | `3 12 * * 6` | Saturday 20:03 | Linux | not measured (never ran on schedule) | 1 |
| debrief-bench | `3 14 * * *` | 22:03 | Linux | 5-13 min | 1 |
| canary | `23 14 * * *` | 22:23 | Linux | 2-5 min | 1 (`max-parallel: 1`) |
| debrief-corpus | `43 14 * * *` | 22:43 | Linux | 0-21 min | 1 |
| service-smoke | `17 15 * * *` | 23:17 | macOS VM | about 22 min | 0 |
| debrief-fuzz | `13 15 * * *` | 23:13 | Linux | 77-90 min with 6 shards at once; about 3 h at 2 at a time | 2 (`max-parallel: 2`), alone in its window |
| nightly | `3 21 * * *` | 05:03 | Linux, Windows, macOS | 25-123 min | uncapped for the rest (reusable ci.yml and many jobs); its five fuzz calls are chained in two chains, so at most 2 at once |
| debrief | `3 22 * * *` | 06:03 | Linux, Windows, macOS | 37-147 min | uncapped (parallel jobs, 4 e2e shards) |
| dev-tool | `33 21 * * *` | 05:33 | Linux, Windows, macOS | not measured | 1 (Linux leg) |

The Linux-only block (22:03-23:13) is serial so that fuzz, which takes two slots, runs after the small jobs have ended.
nightly and debrief cannot be capped by `max-parallel` (many unrelated jobs); only the stagger and the runner's capacity
limit them. Gitea counts runner slots per caller, so `max-parallel` inside the reusable `app-fuzz.yml` does not cap
five separate callers: nightly chains them with `needs:` instead.
