---
name: land-train
description: Use when landing an assembled train or integration branch onto main, from merging its members to the post-landing check. Paddock only.
disable-model-invocation: true
argument-hint: <train-branch> <member-branches...>
---

# Land a train (Paddock)

Arguments: `$ARGUMENTS` (the train branch first, then its members). Work in the integrator's locked worktree, never the main checkout. Rules: ER-8, ER-10, ER-12, ER-49; docs/rules.md 3 (i) to (k); docs/ci-schedules.md.

1. Merge each member into the train with `git merge --no-ff <member>`, one at a time. Before a member merges the train back, `./dev merge sync-member` lists the reverts to re-apply.
2. After each merge, `./dev merge check` diffs it against its parents for stale reverts and dropped hunks (exit 5 is a finding). Answer a finding in the merge message (`Hunks-dropped:`, `Tests-removed:`, `Reverted:`) or take the member out.
3. After each merge, `./dev merge check --gates` runs the fast member gate on the combined tree. A red member is pulled and sent back to its author; do not fix it on the train.
4. Number each `NEW-<slug>` rule: give it the next free number in docs/rules.md and replace every citation of `rule NEW-<slug>` across the tree (`git grep -n 'NEW-'`), as one train commit before the pre-push; C31 refuses a repeated number and warns on a placeholder left (docs/rules.md preamble, user, 2026-11-25).
5. Once, on the final combined tree: `./dev check --pre-push`. A "not measured" row is not a pass; fix it or say so.
6. Push the train once.
7. Dispatch `ci.yml` on the train with `./dev ci dispatch ci.yml --ref <train-branch> --max <duration>`, then wait with `./dev wait --ci-run <id> --max <duration>`.
8. On red, fix and run a failed-only round first: dispatch with the `since_run` input set to the failed run number (`./dev plan --since-run <id>` shows the plan). Check that every failed test really ran in that round: not filtered out, not skipped. Use "rerun failed jobs" only for a suspected flake (ER-12). After any fix commit on the train, rerun the member gate on the new head (`./dev check --pre-push --member-gate --base <the head before the fix>`) before the push (train c retro).
9. When every round is green, dispatch one full run on the final head: a plain dispatch with no `base` and no `since_run`. Nothing lands without it (ER-10).
10. Land: fast-forward main to the train head (`git merge --ff-only`, never a rebase or squash), in the integrator's worktree. The merge queue (`./dev queue tick`) does this step when it runs the landing.
11. If the run was green with FLAKY tests of code the change did not touch, record them: `./dev ci fix-items --workflow ci.yml --sha <the landed commit>` (ER-12).
12. Scope record: for a workflow whose scope producer has no first green run yet, `./dev ci scope-record --workflow <file> --run <id>` (docs/ci-scope-first-runs.toml).
13. Reuse check: the push to main must reuse the train's verdict. Read the push run: `./dev ci reuse` logs `reuse=true` and no job starts. If a new full run starts, stop it and report why the verdict was not reused (ER-49).

Report the train head, the run ids (failed rounds and the full run), pulled members, fix items and the reuse result.
