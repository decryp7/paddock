# 113. A run declares its scope as data; one reader decides "complete"

- Status: Accepted 2026-11-21 (the user, through the coordinator). ER-21, ER-33, ER-15, ER-41, ER-49.
- Date: 2026-11-21
- Related: ER-15 (a tag counts only a complete CI run), ER-33 (a check counts only if it can see the thing), ER-37 (measure
  against common practice), ER-41 (a new gate starts report-only), ER-49 (test a commit once); ADR 0089 (typed outcomes: not
  measured never passes); the D4 reuse (`./dev ci reuse`, `docs/ci-schedules.md`).
- Number: 0113. On 2026-11-21 the highest ADR on `origin/main` was 0108 and 0112 was taken on a branch; no `origin` branch held 0113.
- Evidence heads (ADR 0091), read 2026-11-21: `origin/feat/ci-d4-and-package-filter` = `8966419f1`.

## Context: the class

A run is "complete" when it tested what a push to main tests. Three shell lines said so, one each in ci.yml, debrief.yml and the
shared plan action (`echo "plan verdict: complete=$complete"`), each with its own condition. Each was wrong in a new dimension:

| When | What was counted as complete | Source |
|---|---|---|
| 2026-11-12 | a schedule-skipped run (every job skipped) and a `since_run` rerun, both green | the verdict-reader review of that day (ER-15's "Why"); the `complete=` line and `./dev ci verdict` were the fix |
| 2026-11-21 | a full train dispatch **without WebKit**: the D4 reuse then skipped the main push run for it, and `tag.yml` would read a green verdict that never ran WebKit | review of 6165faba0, P1-1; fixed in `a8e13b014` by adding a term to the ci.yml line |
| 2026-11-21 | a dispatch with `app=X` (one app only) kept an exemption in that term | re-check of `a8e13b014`, P2-1; fixed in `8966419f1` |

Every fix added one more term to a shell condition. The next dimension (an OS leg, a job, an engine) needs another, and nothing
checks that the terms add up to "what a push to main does". Seen three times in two days: this is the class (ER-21).

## Decision

1. **The plan writes its scope as data.** The planner prints one line, `plan scope: {json}`, once per plan job:

   ```json
   {"v":1,"workflow":"ci.yml","rerun":false,
    "ran": {"jobs":[...],"engines":["chromium","webkit"],"apps":["dyno@ubuntu-latest",...],"whole":true},
    "need":{"jobs":[...],"engines":[...],"apps":[...],"whole":true}}
   ```

   `ran` is what this run's own plan switched on: the project jobs (`crates`, `ui`, `e2e`, `e2e-webkit`...), the `crate` and
   `paddock-dev` jobs on each runner, the app legs (`app@os`, with the e2e leg), the browser engines of its tests, and whether every
   test ran whole (no package filter). `need` is the same planner's answer for a push to main on the same tree, with every engine on
   and every test whole. `rerun` is set for a rerun of failures.
2. **One reader compares, and checks that the run ran what it declared.** `./dev ci verdict` (and so `ci reuse`, which asks it)
   calls `run_scope::read` and counts a run only when `Declared::complete` is true (not a rerun, `need` not empty, `ran` covers `need`
   in every dimension, equal or wider) **and** the jobs it declared ran: each declared job and app leg is mapped to its job names
   (`run_scope::PROJECT_JOBS`, checked against the real ci.yml by a test) and must have been `success`. A declared job that was
   skipped or cancelled makes the run partial (dropping the planner's `webkit` output skips `e2e-webkit`: not green); one with no job
   of its name is not measured; a failed one is the run's own red. This is what GitHub's required status checks do: a required check
   that was skipped blocks. A line of another workflow, two lines, an unreadable line or another version is *not measured*, never a
   pass (ER-33). The reason a run is partial is a sentence: "it did not cover what a push to main does: engines webkit; apps
   dyno@ubuntu-latest:e2e-webkit".
3. **What "complete" means, plainly.**
   - **ci.yml, a push to main:** `need` is the push's *own* plan (the diff from the last green main). Complete then means "the run did
     everything its own plan chose, with every engine, whole tests, every job it declared ran and succeeded". It does not prove the push
     covered the whole tree; that trust (the affected plan after a green base) is the one a push to main always had.
   - **ci.yml, every other run** (a dispatch with or without a base, with `app`, a tag's gates, the nightly): `need` is the whole tree.
     A full dispatch with WebKit is complete; one without, with `app`, with a base (narrowed), a reused or a rerun run is not.
   - **App workflows** (debrief.yml, the shared plan action for the four release workflows): `full` and `run` still decide, as the
     workflows' own conditions say; the planner does not compute their scope. `need` is a constant (one `gates` job, the app, the
     app's full engines) and `ran` is that when `run` and `full` are true and the run is not a rerun, and empty otherwise. What
     changed is that the comparison, the rerun flag and the empty-scope rule are in one Rust function, not three shell lines, and the
     reader no longer takes the workflow's word for "complete". Their `gates` job is not one job, so "ran means ran" does not check it.
   - An **empty `need`** proves nothing: such a run is partial, and the refusal names the last fully tested commit on main, as it does
     for any partial-only commit (tested in `verdict.rs`).
4. **No workflow decides "complete" in the end.** ci.yml's `./dev plan --ci` prints its line (a reused run, `PLAN_REUSE`, declares it
   ran nothing and switches every job off; a `since_run` rerun declares `rerun`). debrief.yml and the plan action call
   `./dev ci scope --workflow F --app A --full B --run B --since-run S` after they decided `run`. The `e2e-webkit` job and the declared
   engines read the one `webkit` output of the planner, not two expressions.
5. **Report-only first (ER-41).** The reader does not trust the new line before a real run has printed it. Each producer (`ci.yml`,
   `debrief.yml`, `plan-action` for the release workflows) keeps printing the old `plan verdict: complete=` line, and `./dev ci
   verdict` decides on it, until the producer's first green scope run is recorded in `docs/ci-scope-first-runs.toml`. Meanwhile the
   reader prints the scope view as a `::notice::` ("scope view (report-only; ...): run N declares a complete scope" or "is partial:
   why"), which `tag.yml` shows. `./dev ci scope-record --workflow F --run ID` records the run after checking it (finished, green,
   its workflow, a complete declared scope, its declared jobs ran); that one step switches the producer, and the old block of that
   producer is deleted in the same change (a conformance test fails while a recorded producer still prints it, and while an
   unrecorded one does not). A missing or unreadable register is report-only, never a switch.
6. **Conformance keeps it wired.** A test per producer: the call exists, comes after `run` is decided, names its own workflow and app,
   passes the run and full flags (and the rerun input), the release callers' `full:` equals their `gates` job's condition, and the old
   line is present exactly while its producer is unrecorded and correct while it is. Positive controls remove each part. The semantics
   are Rust tests with a positive control per dimension.

## Common practice (ER-37)

- **GitHub required status checks and merge queues** do not let a workflow say "I am sufficient": the repository lists the checks a
  change needs (branch protection, rulesets), and a merge is allowed only when every required check ran and passed; a skipped required
  check blocks (teams add a final "all required" job for that reason). GitHub merge queues re-run the required checks on the
  merge-group commit, so the requirement is data outside the run. Sources: GitHub Docs, "About protected branches" (required status
  checks) and "Managing a merge queue".
- **GitLab merge trains** and **bors** do the same: a declared list of required jobs, compared by the merger, not by the pipeline.
- This ADR is that shape with the smallest list that fits: `need` is computed by the planner from the tree (no hand-kept list), and
  the comparison is one function in the reader. Cheaper options and why not: keep adding terms to the shell line (the cost we are
  paying, three times); a hand-kept list of required jobs in the repository (drifts from the matrix, the class of ER-22).

## Cost (ER-37)

About 0.5 agent-day for the declaration and the comparison, plus about 0.5 for "ran means ran", the report-only phase and its recorder
that the review asked for: the module (`run_scope.rs`), three call sites, two new commands (`ci scope`, `ci scope-record`), the reader
change, the register, conformance tests. Common practice (a required-checks list) costs the same order, and the planner already holds
the data.

## Consequences

- The scope line is added beside `plan verdict: complete=` first; `plan mode: full=` stays for the rerun planner (a different
  question). Each producer's old line goes when it is recorded, which is the exit of this ADR's report-only phase.
- A new dimension of a run (a runner, an engine, a job) enters `scope_of` once and both `ran` and `need` carry it; a workflow cannot
  forget it. A declared job that the workflow's `if` skips for a reason other than the plan is now seen by "ran means ran", for the jobs
  `PROJECT_JOBS` maps; jobs whose `if` reads more than the plan (`commits`, `train-gates`, `audit`) are not mapped.
- Runs from before this change have no scope line. Until a producer is recorded they still decide on the old line, so a tag on an
  older green commit keeps working; after it is recorded such a run is partial and a tag needs a run made after.
- The first real run that printed each line is recorded in `docs/ci-scope-first-runs.toml` (ER-41); none is yet.

Recurring: the class is "a completeness verdict that does not carry a run's full scope". Earlier instances are in the table above.
