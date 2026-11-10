# Scheduled CI runs

Scheduled workflows run at night, local time UTC+8 (cron is UTC), outside the Proxmox backup window and staggered so
they do not overlap on the three Linux runner slots. Matrix jobs are capped with `strategy.max-parallel` so one Linux
slot stays free for release and branch runs (rule 50; trigger: Debrief's fuzz run at 10:23 local held all three slots).

The backup window is not recorded anywhere in the repo. Assumed 01:00-04:00 local (17:00-20:00 UTC); the Windows CI VM
was observed slow from 00:52 local on 2026-11-10. Replace this with the real window once known.

| Workflow | Cron (UTC) | Local (UTC+8) | Linux slots at most |
| --- | --- | --- | --- |
| nightly | `3 14 * * *` | 22:03 | uncapped (reusable ci.yml and many jobs) |
| debrief-bench | `7 15 * * *` | 23:07 | 1 |
| service-smoke | `17 15 * * *` | 23:17 | 0 (macOS VM) |
| canary | `37 15 * * *` | 23:37 | 1 (`max-parallel: 1`) |
| debrief-corpus | `37 16 * * *` | 00:37 | 1 |
| debrief-mutants | `37 16 * * 6` | Sunday 00:37 | 1 |
| debrief | `33 20 * * *` | 04:33 | uncapped (parallel jobs, e2e 4 shards) |
| dev-tool | `43 20 * * *` | 04:43 | 1 (Linux leg) |
| debrief-fuzz | `47 21 * * *` | 05:47 | 2 (`max-parallel: 2`) |
