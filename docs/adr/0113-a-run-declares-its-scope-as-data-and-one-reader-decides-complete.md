# 113. A run declares its scope as data; one reader decides "complete"

- Status: Accepted 2026-11-21 (the user, through the coordinator). ER-21, ER-33, ER-15, ER-49.
- Date: 2026-11-21
- Related: ER-15 (a tag counts only a complete CI run), ER-33 (a check counts only if it can see the thing), ER-37 (measure
  against common practice), ER-49 (test a commit once); ADR 0089 (typed outcomes: not measured never passes); the D4 reuse
  (`./dev ci reuse`, `docs/ci-schedules.md`).
- Number: 0113. On 2026-11-21 the highest ADR on `origin/main` was 0108 and 0112 was taken on a branch; no `origin` branch held 0113.
- Evidence heads (ADR 0091), read 2026-11-21: `origin/feat/ci-d4-and-package-filter` = `8966419f1`.

## Context: the class

A run is "complete" when it tested what a push to main tests. Three shell lines said so, one each in ci.yml, debrief.yml and the
shared plan action (`echo "plan verdict: complete=$complete"`), each with its own condition. Each was wrong once, in a new dimension:

| When | What was counted as complete | Source |
|---|---|---|
| 2026-11-12 | a schedule-skipped run (every job skipped) and a `since_run` rerun, both green | `plan mode: full=` logged before the decision; the `complete=` line and `./dev ci verdict` were the fix |
| run 8335 | a run whose scope had a hole the line did not look at | docs/lessons.md, ER-49 note |
| 2026-11-21 | a full train dispatch **without WebKit**: the D4 reuse then skipped the main push run for it, and `tag.yml` would read a green verdict that never ran WebKit | review of 6165faba0, P1-1; fixed in `a8e13b014` by adding a term to the ci.yml line |
| 2026-11-21 | a dispatch with `app=X` (one app only) kept the exemption, caught by the re-check | re-check of `a8e13b014`, P2-1; fixed in `8966419f1` |

Every fix added one more term to a shell condition. The next dimension (an OS leg, a job, an engine) will need another, and nothing
checks that the terms add up to "what a push to main does". Seen four times: this is the class (ER-21).

## Decision

1. **The plan writes its scope as data.** The planner prints one line, `plan scope: {json}`, once per plan job:

   ```json
   {"v":1,"workflow":"ci.yml","rerun":false,
    "ran": {"jobs":[...],"engines":["chromium","webkit"],"apps":["dyno@ubuntu-latest",...],"whole":true},
    "need":{"jobs":[...],"engines":[...],"apps":[...],"whole":true}}
   ```

   `ran` is what this run's own plan switched on: the project jobs (`crates`, `ui`, ...), the `crate` and `paddock-dev` jobs on each
   runner, the app legs (`app@os`, with the e2e leg), the browser engines of its tests, and whether every test ran whole (no
   package filter). `need` is the same planner's answer for a push to main on the same tree: its own plan for a push, the whole
   tree for any other run, with every engine on and every test whole. `rerun` is set for a rerun of failures.
2. **One reader compares.** `./dev ci verdict` (and so `ci reuse`, which asks it) calls `run_scope::read` and counts a run only when
   `Declared::complete` is true: not a rerun, `need` not empty, and `ran` covers `need` in every dimension (equal or wider). A line of
   another workflow, two lines, an unreadable line or another version is *not measured*, never a pass (ER-33). The reason a run is
   partial is the sentence "it did not cover what a push to main does: engines webkit; apps dyno@ubuntu-latest:e2e-webkit".
3. **No workflow writes "complete".** ci.yml's `./dev plan --ci` prints its line (a reused run, `PLAN_REUSE`, declares that it ran
   nothing and switches every job off; a `since_run` rerun declares `rerun`). The app workflows (debrief.yml, the shared plan action
   for the four release workflows) call `./dev ci scope --workflow F --app A --full B --run B --since-run S` after they decided `run`.
   The `e2e-webkit` job and the declared engines read the one `webkit` output of the planner, not two expressions.
4. **Conformance keeps it wired.** A test per producer: the call exists, comes after `run` is decided, names its own workflow and app,
   passes the run and full flags (and the rerun input), and no hand-written verdict line is left. A positive control removes each
   part and shows it red. The semantics are Rust tests with a positive control per dimension (a run missing one engine, one app, one
   runner, whole tests, or a rerun is not complete).

## Common practice (ER-37)

- **GitHub required status checks and merge queues** do not let a workflow say "I am sufficient": the repository lists the checks a
  change needs (branch protection, rulesets), and a merge is allowed only when every required check ran and passed. A skipped
  required check blocks (a skipped job reports success unless a rule says otherwise, which is why teams add a final "all required"
  job). GitHub merge queues re-run the required checks on the merge-group commit, so the requirement is data outside the run. Sources:
  GitHub Docs, "About protected branches" (required status checks) and "Managing a merge queue".
- **GitLab merge trains** and **bors** do the same: a declared list of required jobs, compared by the merger, not by the pipeline.
- This ADR is that shape with the smallest list that fits: `need` is computed by the planner from the tree (no hand-kept list), and
  the comparison is one function in the reader. Cheaper options and why not: keep adding terms to the shell line (the cost we are
  paying, four times); a hand-kept list of required jobs in the repository (drifts from the matrix, the class of ER-22).

## Cost (ER-37)

About 0.5 agent-day: the module (`run_scope.rs`, about 300 lines with tests), three call sites, one new command, the reader change,
conformance tests. Common practice (a required-checks list) costs the same order, and the planner already holds the data.

## Consequences

- The scope line replaces `plan verdict: complete=` (removed from `plan_since`). `plan mode: full=` stays: the rerun planner reads
  it to tell whether an earlier run was full, which is a different question.
- A new dimension of a run (a runner, an engine, a job) enters `scope_of` once and both `ran` and `need` carry it; a workflow cannot
  forget it. A job that the plan switches on but whose `if` skips it is not seen: the `if`s read the plan's outputs, and the
  `e2e-webkit` one now reads the same `webkit` value the scope uses.
- Runs from before this change have no scope line and are partial for the reader: a tag needs a run on a commit made after it.
- A first real run that printed the line is recorded where each producer is defined (ER-41); not yet.

Recurring: the class is "a completeness verdict that does not carry a run's full scope". Earlier instances are in the table above.
