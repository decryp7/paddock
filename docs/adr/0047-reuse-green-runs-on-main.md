# 47. Reuse green runs on main: a fast-forward landing reuses its branch's green runs (amends ADR 0042 §5 and D3)

- Status: **Draft (WIP, paused 2026-11-03 for Dyno 1.1.0)**; to become Proposed. Approved for drafting by the user
  (2026-11-03, item 9). An amendment to the accepted ADR 0042 (`8bf509f99` on `docs/redesign-adrs-2-4-6`); no new rule
  number expected (rule 50 and the proposed rule 61 get a sentence).
- Related: ADR 0042 §5 (CI once per tree; branch runs only; never `main`) and D3; ADR 0038 (the gate stamp, G26's input
  hash); rule 50; ER-15 / rule 9.

## Evidence

- The coordinator cancelled main runs 7428 and 7429 on `a18e18801` (now `origin/main`), which was already green in runs
  7426 and 7427 on `steward/land-adv3`, to free the runner for Spotter (coordinator report 2026-11-03; run numbers not yet
  re-read from Gitea).
- ADR 0042 §5 and D3 exclude every run on `main`.

## Decision (outline, to be written)

- A push to `main` whose head SHA (not only tree) already has successful runs of the same workflows, from a push to the
  landing branch, within ADR 0042's 24-hour bound, reuses them: the plan job marks the gate jobs skipped with "green in
  run <id> on <branch>", posts a commit status per workflow (context `<workflow>/reused`, target URL the original run),
  and writes a job summary line. Same SHA means same tree, workflows and history.
- Always run: tags, `*-release.yml`, nightly and scheduled runs, dispatches with inputs, and a main push whose SHA was not
  tested on a branch (a merge commit).
- One key with ADR 0038 and ADR 0042 §5 (rule 55).

## Still to do

Gitea API check (commit statuses, run lookup by SHA), work packages with agent-days (plan job lookup and status, test
against a fake Gitea API, docs), G.7 placement (CI kit is on the release path: outside release windows), decisions
(SHA vs tree hash; 24 h bound on main; who may force a full main run).
