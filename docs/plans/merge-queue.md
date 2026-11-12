# Merge queue on the branch ledger (design note)

Status: design note, 2026-11-12. No code yet besides the ledger (slice 0). Owner of `.gitea/` for this work: the integrator.
Inputs: the user's decisions of 2026-11-12 (below), the tools research of 2026-11-12
(`dev-targets/review-scratch/tools-research-2026-11-12.md`, candidates 3 to 5), rule 50, rule 54, ER-8, ER-10, ER-31,
ER-37, ADR [0080](../adr/0080-ci-at-the-speed-of-the-critical-path.md) and [docs/ci-schedules.md](../ci-schedules.md).

## Decided by the user (2026-11-12)

1. The branch ledger is Gitea draft pull requests with labels (`.gitea/labels.toml`, `./dev pr`). Built: slice 0.
2. Trains become a full merge queue that drops failing branches automatically, like GitHub merge queues and GitLab merge
   trains. Built after the ledger.
3. Owed Windows tests (`owes/windows-tests`) run in the queue's Windows leg.
4. Plan status is derived from the ledger, not kept by hand: `./dev pr plan-status` (built in slice 0) is the queue's
   landing step.
5. Release tags `*-v*` are protected and pushed by a release bot account; the queue's tag step uses the bot's token. The
   user creates the bot account.

## The ledger (slice 0, on `feat/dev-pr-ledger`)

- Every branch gets a draft pull request against main (`./dev pr open <branch>`): `WIP:` title (Gitea's draft prefix), the
  body lists the commits, label `state/built`. Opening it starts no CI run: ci.yml, debrief.yml and debrief-mutants.yml lost
  their `pull_request` trigger (rule 50: runs only on dispatch, a train, main, tags and schedules).
- State labels are one exclusive scope: `state/built` -> `state/in-review` -> `state/fixes` | `state/approved` ->
  `state/in-train` -> `state/landed`. Debts: `owes/windows-tests`, `owes/vm-rows`, `owes/rule54-bundle`. Also `security`,
  `app/<app>` (one per app folder), `review-round/1|2` (exclusive).
- `./dev pr label <branch> +x -y`, `./dev pr list [--state approved]` (branch, head, labels, newest CI run per workflow),
  `./dev pr sync` (labels from the TOML; never deletes), `./dev pr plan-status [--write]`.

## How the queue works

One queue for the repository, one train at a time. Runners are the scarce resource (rule 50; 35 min of every runner per
full train run, lessons:1751), so the queue does not run GitLab's or Zuul's parallel speculative pipelines; it runs one
batch, and only what failed is rerun.

1. **Take.** The `state/approved` pull requests, oldest approval first, up to the batch size (decision Q2). A PR that is
   not mergeable with main, is a draft of another base, or has no green review round is skipped with a comment. Each taken
   PR moves to `state/in-train`.
2. **Build the train.** A fresh branch `train/<date>-<n>` from origin/main, in the integrator's locked worktree. Each PR is
   merged with `--no-ff` in order, then `./dev merge check` on the merge (ADR 0089) and the workspace-hack diff; a conflict
   or a merge-check finding drops that PR at once (`state/fixes`, comment with the conflicting files or the finding) and the
   train goes on without it. Lockfiles are regenerated, never hand-merged.
3. **Local fast gates (ER-9).** fmt, clippy (host and xwin), lint and type-check of the touched files, the conformance line,
   `./dev gates check`, commit-lint over `origin/main..train`. A failure here is mapped to its PR (step 5) and dropped
   before any runner is used.
4. **Dispatch (rule 50h, ER-10).** `ci.yml` (and each app workflow the plan reaches) by `workflow_dispatch` with `base` =
   origin/main: the affected plan of the whole train. The Windows leg runs every crate and app that an `owes/windows-tests`
   PR touches, in one leg (needs a new dispatch input, slice 2). Slower local checks (Linux container, tests under load)
   run beside it; a local failure cancels the run.
5. **On red: find the culprit.** `./dev plan --since-run <run>` lists the failed jobs and tests. Each failed test belongs
   to a project (crate, app, tool); the suspects are the train's PRs whose affected set (`./dev plan origin/main <pr-head>`)
   contains that project. One suspect: it is dropped. Several: decision Q3 (bisect the suspects, or drop them all). A failure
   that also fails on main (the triage of tools-research candidate 2) is not blamed on any PR; the train stops and the
   coordinator decides. A dropped PR gets `state/fixes` and a comment naming the failed tests, the run and the job logs.
6. **Retry the rest.** The train is rebuilt without the dropped PRs and only what failed is rerun (`since_run`, ER-10 fix
   rounds). A suspected flake gets at most one rerun of the failed jobs on the same commit, never to turn red green (ER-12).
7. **Final run (ER-8).** When every round is green, one full affected run against main on the final train head. Nothing
   lands without it.
8. **Rule 54.** If any PR in the train touches an app's release path (`owes/rule54-bundle`, or the plan says so), the queue
   dispatches `<app>-release.yml` with gates on and the installers built, on the final head, before landing (rule 54(c):
   gates skipped only where a green ci.yml run on the same commit covered them).
9. **Land.** `git push origin <train-head>:main` as a fast-forward only (never a force, never a rebase), then check that
   origin/main equals the train head. Decision Q1: automatic, or after the coordinator's approval. Gitea marks each PR merged
   when its head reaches main; the queue sets `state/landed`, clears `owes/windows-tests` for the PRs the Windows leg covered,
   and runs `./dev pr plan-status --write` on main's plan files (a docs-only commit, rule 50d) so no plan row says "open"
   for landed work.
10. **Releases (ER-31).** The queue never tags as part of a landing. `./dev queue release <app>` (slice 5) tags the app's
    tested commit, using the release bot's token on protected `*-v*` tags (decision 5); ER-31(a) still lets an app tag on
    its release branch's tested commit when the failures are in code it does not ship, and that branch then goes through the
    queue like any other.

**One train's CI at a time per shared runner class.** Before any dispatch (step 4, a fix round, the final run, a rule 54
run) the queue lists the in-progress and queued paddock runs and the runner classes their jobs use; while another run holds a
class this dispatch needs (the macOS runner `d7-mac-mini` above all, which is one host), it waits for that run with
`./dev wait --ci-run <id> --max <duration>` instead of dispatching beside it. This removes a recurring class (ER-21),
**overlapping CI runs on one shared runner**: runs 7960 and 7961, and 7969 to 7971 with 7967, overlapped on d7-mac-mini, and
heavy Debrief tests then hit their fixed 90 s budgets. Hand dispatches made that overlap possible; the queue is the one
dispatcher of train runs, so it can hold the rule (coordinator, 2026-11-12).

**The Windows backup window (00:00 to 04:00 local).** The queue does not dispatch a run with a Windows leg that would start
in, or is likely to run into, 00:00-04:00 local (docs/ci-schedules.md). In that window it either holds the train, or runs
the Linux and macOS legs and keeps `owes/windows-tests` on the PRs, which then cannot land until a later Windows leg runs
(decision Q4).

## Common practice (ER-37)

| System | Batches | On a failure | Lands | Source (fetched 2026-11-12) |
|---|---|---|---|---|
| GitHub merge queue | temporary `merge_group` branch per entry, with the entries ahead; build concurrency 1-100; min and max group size, wait time | the failing PR is removed and the branches behind are rebuilt; no bisection is described | automatic once the required checks pass | [docs](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue) |
| GitLab merge trains | each MR's pipeline includes the MRs ahead; up to 20 parallel pipelines (minimum 1, sequential) | the failing MR is removed; the pipelines behind it restart without it | automatic; "merge immediately" exists for emergencies | [docs](https://docs.gitlab.com/ci/pipelines/merge_trains/) |
| bors-ng | one batch on `staging`, one collecting | bisects the batch until it isolates the failing PR | fast-forwards master to the tested commit | [repo](https://github.com/bors-ng/bors-ng) (archived 2024-04-04; recommends GitHub's queue) |
| Mergify | `batch_size`, `max_parallel_checks` for speculative batches | splits the failed batch to find the culprit; `batch_max_failure_resolution_attempts: 0` drops the whole batch | automatic; fast-forward in serial mode | [docs](https://docs.mergify.com/merge-queue/batches/) |
| Zuul (OpenStack) | dependent pipeline: each change tested with all ahead of it; window starts at 20, halves on a failure | the failing change is removed; changes behind are re-tested from the new tip | automatic, in queue order | [docs](https://zuul-ci.org/docs/zuul/latest/gating.html) |
| Gitea 28.3 (ours) | none: no merge queue; "merge when checks succeed" exists since 1.17 (secondary sources, not verified here) | - | - | tools research, appendix "Gitea" |

What this design does the same: tests the combination before main gets it (all five), removes the failing entry and
retries the rest (GitHub, GitLab, Zuul), main gets exactly the tested commit by fast-forward (bors, Mergify serial mode).
What it does differently and why: one train at a time instead of parallel speculative pipelines, because the runners are
shared and scarce (rule 50; a full train run costs about 35 min of every runner); culprits are first narrowed by the
affected plan, which none of the five can do, so most failures need no bisection run at all.

## Slices and cost (agent-days)

| Slice | What | Cost |
|---|---|---|
| 0 | Ledger: labels, `./dev pr open/label/list/sync/plan-status`, no `pull_request` triggers | 1.5 (this branch) |
| 1 | `./dev queue build`: take approved PRs, build the train branch with merge check and hakari diff, fast gates, labels | 1.5 |
| 2 | `./dev queue run`: dispatch with `base` and `since_run`, a Windows-leg dispatch input for owing crates and apps, wait with `./dev wait --ci-run`, the one-run-per-runner-class guard, the backup-window guard | 2 |
| 3 | Culprit and retry: failed tests to projects to suspect PRs, drop with `state/fixes` and a comment, rebuild, failed-only rerun, the final full run | 1.5 (+1 for bisection, Q3) |
| 4 | Landing: rule 54 dispatch, fast-forward with the remote-head check, `state/landed`, `plan-status --write` commit | 1 |
| 5 | Release step: `./dev queue release <app>` with the bot token on protected tags (after the user creates the bot) | 0.5 |

Full queue: 7.5 to 8.5 agent-days after slice 0. Cheaper options, always on offer:

- **Thin `./dev train`** (tools research D3a, 2 d): slices 1 and 4 only; the integrator still reads failures and drops PRs
  by hand with `./dev pr label`. Keeps most of the speed-up of the ledger.
- **Scripted checklist** (0.5 d): the integrator's steps as a script that prints the next command; no automation of drops.
- **Gitea's "merge when checks succeed"** (0 d to try): merges one PR when its own checks pass, but tests no combination,
  so it gives up what trains are for; not recommended.

## Decisions for the user

- **Q1. Fast-forward of main.** (a) Automatic after the final full run, the merge check and rule 54 are green
  (recommended: what all five systems do; the gates, not a person, decide; the coordinator can hold the queue with a
  `queue/hold` label or `./dev queue hold`). (b) The coordinator approves each fast-forward (one round trip per train, as
  today).
- **Q2. Batch size.** (a) Up to 6 PRs per train (recommended: the five trains so far took 5 to 24 run-hours each, tools
  research appendix; a small batch keeps a culprit search short). (b) Up to 10. (c) 1 (no batching: every PR its own run; most runner time).
- **Q3. Several suspects for one failure.** (a) Drop all suspects and retry the rest; each dropped author gets the failed
  tests (recommended: cheapest in runner time, and the affected plan usually leaves one suspect). (b) Bisect the suspects
  with failed-only reruns, as bors and Mergify do (+1 d to build, one more run per halving).
- **Q4. Owed Windows tests during the backup window.** (a) Hold the train until 04:00 (recommended when anything owes
  Windows tests). (b) Run Linux and macOS now and land only the PRs that owe nothing.
- **Decided (user, 2026-11-12):** protected `*-v*` tags with a release bot account; the queue's tag step uses the bot's
  token; the user creates the bot account.
