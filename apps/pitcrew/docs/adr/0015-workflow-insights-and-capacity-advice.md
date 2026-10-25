# 15. Workflow insights and capacity advice

- Status: accepted (2026-10-23)
- Related: ADR 0007 (metrics), `gitea_api`; code: `pitcrew_core::insights`, `app/src-tauri/src/ops.rs` (`fleet_insights`),
  `app/src/app/features/insights/`

## Context

Owners of a few runners want to know which jobs are slow, which are flaky, how long jobs wait for a runner and whether
to add capacity. GitLab's runner fleet dashboard answers this but needs ClickHouse and refreshes daily; Pitcrew can
answer it live and locally because it already sees every job finish.

## Decision

**Sources.** The jobs are the ones the service saw finish (the history, at most 1000 per runner, up to 90 days), which
carry the repository, the job name, start and end. The Gitea API adds what only the server knows: how the job ended and
how long it waited (`created_at` to `started_at`). The app reads `GET /api/v1/repos/{owner}/{repo}/actions/jobs/{id}`
with the owner's **read token from the OS credential store** (the service never sees it), exactly as the Activity page
does: read-only, cached in `job-outcomes.json` (versioned, bounded to 5000 jobs, rebuilt when damaged), and at most 25
new lookups per refresh, so a big history fills in over a few refreshes (the page says how many are pending). A masked
repository (`***`) or a server that answers 404 simply leaves the job without an outcome.

**Aggregation is the engine's.** `insights::compute` returns, over 1-90 local days: totals, per repository the success
share (jobs with a known outcome only), the share in the first and second half of the range, the average wait and a
result per day; the slowest jobs by average duration with the 95th percentile (grouped by repository and job name);
the flaky jobs (at least 4 runs with a known outcome, a failure rate between 10% and 90% and at least 2 flips between
consecutive outcomes, so a job that broke once and stayed broken is not flaky); the queue wait (average, 95th
percentile, longest); and each runner's busy share (the union of its job intervals over the range, so overlapping jobs
count once). The UI only formats.

**Advice** (`insights::advise`) turns the numbers into sentences with the next step, and only says something when the
numbers justify it:
- jobs wait (average at least 1 minute or 95th percentile at least 5 minutes) **and** the busiest runner was busy at least
  80%: add a runner or raise `runner.capacity` in its configuration;
- jobs wait **but** no runner is busy: not a capacity problem, check labels and that the runners were online;
- every runner busy under 20% and hardly any wait (and at least 10 jobs): capacity to spare, consider pause windows
  (ADR 0013);
- a repository's success share fell by 15 points or more between the halves (with at least 8 known outcomes).

## Consequences

- Busy time and flakiness are approximations from what Pitcrew saw: a runner that was off shows as idle, a job Pitcrew
  did not see (before it was installed) is absent. The page says "seen by Pitcrew".
- No new network surface: the same host, token and endpoint as before, bounded and cached.
- No Prometheus endpoint (dropped by the owner for v1).
