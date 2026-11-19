# 8. Storage: SQLite in the agent, raw samples for a day, rollups for months

- Status: accepted (2026-10-23)
- Related: ADR 0002, `dyno_core::history`, `dyno_agent::store`

## Context

The agent writes small records every few seconds for months, must survive crashes and power loss, must stay tiny in
memory and CPU, and queries ranges for charts. DuckDB (Debrief's store) is built for large analytical scans; Dyno's
load is frequent small writes and range reads. The user asked for the choice to be made and recorded.

## Decision

1. **SQLite via `rusqlite` with the bundled library**, WAL journal, `synchronous = NORMAL`, owned by the agent (single
   writer). Crash-safe, a few hundred KB of RAM, no service, one file in the user's data directory.
2. **Tiers** (`dyno_core::history`): raw points every few seconds kept `raw_hours` (default 24 h), then 1-minute,
   15-minute and hourly rollups holding **min/avg/max** (a short spike stays visible at every zoom level) kept for
   days, weeks and months (defaults 14 d / 90 d / 2 y, configurable, bounded). Rollups are computed incrementally when
   a bucket closes; pruning runs hourly in small transactions. A range query picks the finest tier that covers it and
   returns at most a few thousand points.
3. **Tables**: `points(tier, at, data)`, per-app minutes, disk free space, timeline `events`, `activity` with undo
   records, `alerts`, `snapshots`, `boots`, app start/crash events, daily scores, and — for later features — issues,
   daily stats, the process baseline (first-seen programs) and sleep/wake records. Values are small JSON documents
   (debuggable, forward-compatible through serde defaults); indexed columns are only those queried.
4. **Versioned schema.** `meta.schema` holds the version; newer-than-known is refused with a message (not downgraded),
   older is migrated forward by numbered steps in one transaction.
5. **A damaged or foreign file is moved aside** (`dyno.damaged-<time>`) and a fresh database created; the agent
   reports it as an alert so the user knows history restarted. When the data folder is not writable the agent falls
   back to an in-memory store and says so.
6. **Privacy and size.** Local only, never uploaded; no file contents, command lines are stored only for events the
   user can see in the UI, and the whole database can be cleared or exported (CSV/JSON; Parquet for "Open in Debrief").
   The database size is capped; when over the cap the oldest rollup rows go first.
7. **DuckDB stays Debrief's.** Deep analysis of an incident exports a time window and opens it in Debrief.

## Consequences

- One writer, many readers: the app never opens the database; it asks the agent.
- Retention settings change what is kept at the next prune; lowering them deletes older rows.

## Store writers (amendment, 2026-11-19)

Added during the ADR 0108 WP-1 reviews (family ADR 0108, Bulk APIs). The same defect showed up four times on one branch: the
history writer, the alarm engine, the alert cooldowns and an alarm write that failed while its batch committed all moved
ahead of a store that had not stored them. One rule now covers every writer: **commit first, then keep and announce.**

1. **Writes go in a batch.** A sampler tick is one transaction (`Store::batch`), and so are an alarm request, a prune and
   a quarter hour's disks. A writer that must be all or none (`put_app_minutes`, `put_changes`, `put_issues`, the baseline,
   an alarm change, an alert's rows) is a savepoint (`Store::atomic`), inside a batch or alone.
2. **A failed write fails its batch.** When a savepoint inside an open batch fails, the batch's `commit()` rolls everything
   back and returns `StoreError::BatchFailed`, so a caller never commits half its work. A batch dropped while the thread
   panics is rolled back too.
3. **In-memory state follows the commit.** What a writer changes in memory is kept only when the commit succeeds, and is put
   back otherwise. The sampler snapshots its tick state first (`tick_state::TickState`: the history writer, the alert and
   custom-rule evaluators, the recorder's last capture, and the alarm engine only when the tick has alarm work). An
   `AlarmCentre` request, the unusual-program look and the recorder's capture each restore their own state.
4. **Nothing is announced before the commit.** The tick collects its alarm changes and alerts and sends them after the
   commit; a request returns its changes to push only after its commit. When the commit fails nothing is announced, and the
   next tick or request does the work again.

Checked by `dyno-agent/src/store_contract.rs`. It runs each writer and request path (the sampler tick, the alarm requests,
the history writer, `put_changes`, `put_issues` and the baseline writes) with every table refusing writes, and asserts that
no row changed, nothing was announced and nothing was kept in memory. Its positive control is a writer shaped like the old
bug (keep the change, swallow the error), which the harness must fail. Removing the alarm restore also turns it red (checked
by hand, 2026-11-19). The unusual look and the recorder have tests of their own (`unusual::budget_tests`, `recorder::tests`).
