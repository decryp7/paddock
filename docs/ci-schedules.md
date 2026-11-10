# Scheduled CI runs

Scheduled workflows run at night, local time UTC+8 (cron is UTC), staggered so they do not overlap on the three Linux
runner slots. Matrix jobs are capped with `strategy.max-parallel` so one Linux slot stays free for release and branch runs
(rule 50; trigger: Debrief's fuzz run at 10:23 local held all three slots for about 30 minutes).

**Proxmox backup window.** Starts at 00:00 local (UTC+8; 16:00 UTC), stated by the user on 2026-11-10. The end is not
known; 04:00 local (20:00 UTC) is assumed until evidence says otherwise. The Windows CI VM crawled from 00:52 local on
2026-11-09/10 (memory note `windows-ci-proxmox-backup`). Rules:

- A workflow with a Windows leg neither starts in 00:00-04:00 local nor is likely to run into it. It starts after 04:00.
- Linux-only and macOS-only workflows run 22:00-23:59 local or after 04:00, and finish before about 08:00 local.
- Re-check the table when the real end of the window is known.

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
| nightly | `3 20 * * *` | 04:03 | Linux, Windows, macOS | 25-123 min | uncapped (reusable ci.yml; its fuzz shards use `max-parallel: 2`) |
| debrief | `3 21 * * *` | 05:03 | Linux, Windows, macOS | 37-147 min | uncapped (parallel jobs, 4 e2e shards) |
| dev-tool | `13 21 * * *` | 05:13 | Linux, Windows, macOS | not measured | 1 (Linux leg) |

The Linux-only block (22:03-23:13) is serial so that fuzz, which takes two slots, runs after the small jobs have ended.
nightly and debrief cannot be capped by `max-parallel` (many unrelated jobs); only the stagger and the runner's capacity
limit them.
