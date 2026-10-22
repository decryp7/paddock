# 13. Per-app usage over time, and limits that explain themselves

Status: accepted. The number is provisional; it is allocated again when this branch merges (see `docs/suite-lessons.md`).

## Context

The dashboard showed only today's counters per app, kept in memory, so a restart cleared them and with them the daily token
budget. The roadmap approved usage over time per app and an app detail view. Limits were requests per minute, tokens per
day and concurrency.

## Decision

- `pitwall_core::usage::Usage` is a pure store of counters per app in **hourly buckets (kept 7 days)** and **daily buckets
  (kept 90 days)**, at most 256 apps per bucket (the rest count as `other`). Series queries return every bucket of the range
  (24 h and 7 d hourly, 30 d daily) so the UI needs no gap handling, and at most eight apps, the rest folded into `other`.
- The server records every finished request (also cancelled ones and embeddings) and persists the store to `usage.json`
  every 30 seconds with the existing atomic-write-and-backup helper. Loading tries the file, then its backup, then starts
  empty; a file of another version is treated like damage. Budgets therefore survive a restart.
- **Peak memory** is attributed, not measured: the footprint (RAM plus video memory) of the model that served the request,
  the largest per bucket. It covers models an app's requests loaded or kept hot, needs no per-process accounting, and is
  stated as such in the UI and the user guide.
- Limits grow by requests per day, tokens per month and a **memory share** (a percentage of the machine's RAM that the
  serving model may occupy, checked once the model is resident). All are optional, refused with a typed, explained error, and
  evaluated by pure functions (`check_budget`, `check_memory`, `statuses`); the UI shows the engine's statuses and never
  recomputes them. At 80 % the server raises one `limitWarning` event per app, limit and period.

## Consequences

- No new outbound connection and no content is stored: counters and model sizes only.
- Hourly resolution inside 90 days is not kept (daily only); a longer view needs another tier, added the way `metrics.rs` did.
- The memory share is checked after loading, so a refused request may have loaded a model that the idle rules release.
