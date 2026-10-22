# 14. Update deadline countdown and update-loop detection

- Status: accepted (2026-10-23)
- Related: ADR 0002 (runner auto-update); code: `pitcrew_core::update::watch`, `pitcrew_service::supervisor` (`watch_updates`,
  `auto_decision`), the Updates page and the runner cards

## Context

Runners that fall behind silently are a known failure: GitHub enforces a minimum runner version for self-hosted runners
(they must stay within 30 days of a release and stop taking jobs otherwise), and issue trackers are full of auto-updates
that loop (the same version downloaded again and again, `mv` missing, "peer reset connection").

**What is the rule for Gitea and Forgejo? There is none we can count down to.** Searched 2026-10-23: the documentation of
Gitea's `act_runner` and of Forgejo's runner publishes no minimum version and no time limit after which the server
refuses a runner; the servers keep speaking the older runner protocol and breaking changes come as ordinary release
notes (for example Forgejo runner 12 needs `git` installed, runner 8 validates workflow schemas, Gitea's `act_runner`
moved to Go 1.24 for building). GitHub's 30-day rule (and its enforcement dates, which GitHub itself paused and
extended in 2026) belongs to GitHub's service and does not apply here. Pitcrew therefore does **not** invent a server
deadline.

## Decision

**A countdown to the owner's own limit.** The update policy has `deadline_days` (default 30; 0 = off, per runner or in
the defaults): "remind me when a runner has been behind the latest release for this long". The clock starts when
Pitcrew first saw a release newer than the installed version (`behind_since` in `state.json`, kept while the runner stays
behind, cleared when it has caught up or is pinned; a pre-release does not start it). The snapshot carries a `Deadline`
(due date, whole days left, `ok` / `soon` within 7 days / `overdue`); the cards and the Updates page show "update due in
12 days" and "overdue by 3 days". A notification (`UpdateDeadline`) is sent once at `soon` and once at `overdue`.

**Loops.** From the update outcomes in the history, in the last 7 days and for the newest target version only:
3 failures or roll-backs (`failing`), 3 installs of the same version (`not sticking`: something puts the old binary
back) or 5 postponements (`postponed`: a job always runs). A loop carries a cause and a next step in words. While a loop
exists for the target, automatic updates to it stop (the decision becomes "available", not "apply") so the machine does
not go on downloading; the owner can still install it by hand, skip it or fix the cause. It is announced once per target
(`UpdateLoop`) and shown on the runner card and the Updates page.

## Consequences

- No false precision: for Gitea and Forgejo the deadline is the owner's choice, and the ADR records that nothing
  server-side enforces it. If a runner kind with a published rule is added (GitHub), its rule becomes that kind's
  default `deadline_days`.
- Loop detection reads history that already exists; nothing new to persist except the announced target.
