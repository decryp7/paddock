# 98. A branch is tested on the combined tree before approval, and tests read registries through their accessors

- Status: Proposed 2026-11-13. Drafted at the coordinator's request after the user approved starting this redesign on
  2026-11-13 (ER-21: one class, eleven instances below). Nothing here is built yet. ER-23 does not apply (no drawings).
- Date: 2026-11-13
- Related: ER-8 (checks that fail only on combined code run at merge time), ER-9 / rule 50 (h) (local checks and CI overlap),
  ER-21 / rule 40, ER-24 (machine load; one train's CI per runner class), ER-37 / rule 76 (common practice and cost), ER-41
  (a new check starts report-only), rule 50 (a) and (b) (one CI run per branch head; no re-dispatch to refresh against main),
  rule 55 (one implementation per mechanism); ADR 0038 (`./dev check --pre-push`), ADR 0089 (`./dev merge check`, "not
  measured" never passes), the design note [docs/plans/merge-queue.md](../plans/merge-queue.md) (slices 0 to 6).
- Number: 0098 was free on every `origin` branch on 2026-11-13 (highest file on any branch: `0097`; no branch has a
  `docs/adr/0098-*` or `docs/adr/01*` file; read with `git ls-tree` over every `refs/remotes/origin/*`).

```evidence-heads
origin/main f7bbe2b86
origin/train/2026-11-13-8 3a342cb42
```

## Context

A train merges several approved branches. Each branch was tested on its own (its pre-push gates and its one CI run, both
against the merge base with main). Some of them carry a test, a lint list or a registry entry that hard-codes text or file
layout owned by another branch's area. When two such branches meet, the combined tree fails, and nobody sees it until the
integrator builds the train. Each fix then lands on the train, by the integrator, hours after the authors finished.

### Instances (every row checked on 2026-11-13)

Train 8 (`train/2026-11-13-8`), the fixes the integrator made on the train after merging. Each failure is real: either run
8050 (ci.yml on the train at `68ad0d4f9`; `./dev ci triage 8050 --no-history`: 13 failing tests or jobs, 12 new against main
run 8045) or the parent tree of the fix, read with `git show`:

| Fix | What hard-coded what | Failure checked |
|---|---|---|
| `11c365fc4` | `release_isolation/tests.rs` (from `feat/lint-no-raw-canonicalize`) lists `pathfind.rs` in its folder; `feat/generator-registry` moved it to `src/pathfind.rs` | parent tree has only `src/pathfind.rs`; the test reads the old path (NotFound) |
| `be69a31f6` | `tools/heavy/inputs.txt` (from `fix/pre-push-heavy-slot`) is a hand list that must equal `cargo metadata`; another branch added `paddock-env` to heavy | parent: `Cargo.toml` names `paddock-env`, `inputs.txt` does not; run 8050 `inputs::heavy_inputs_txt_matches_cargo_metadata` failed on Linux and Windows |
| `0308201b4` | Pitwall's elevated-registration audit slices paddock-proc's `cleanup.rs` from `pub fn report(` to a doc line it knew; `fix/must-use-removals` put helpers between them | run 8050: `the_elevated_registration_touches_no_folder_the_user_controls` failed on macOS, Windows and Linux |
| `bddd2947c` | the generator registry's conformance entry (`docs/generators.toml`, from `feat/generator-registry`) greps ci.yml for the inline `--only` line; train 7 had moved it into `PADDOCK_CONFORMANCE_ARGS` | parent: the grep pattern matches nothing, `./dev gen check` reported NOT MEASURED (commit body); run 8050's train-gates job also failed, outside any test (its log not read for the cause) |
| `ecb6f33ad` | the `safe_remove` lint's allow list (train 7) met two new remove sites of `feat/ci-in-rust-wp5` | the fix adds both files to `ALLOWED` in `tools/paddock-dev/tests/it/safe_remove_lint.rs`; parent lacks them |
| `cd0c3aa14` | `feat/lint-no-raw-canonicalize` bans `Path::canonicalize`; `feat/generator-registry` and `fix/pre-push-base` arrived with raw calls | parent: `tests/it/generators.rs:10` and `worktree/ops.rs` call `.canonicalize()` |
| `6394bc365` | C71 (train 7) rejects fixed waits under 40 s; `fix/debrief-store-phase-logs` added five `recv_timeout(30 s)` | parent has the five calls |
| `b51ef0308` | C71 again; `feat/proc-spawn-detached-fix` shortened a wait to 15 s | parent has the 15 s wait |

Earlier trains, the same shape (a check or list on one side, new code on the other): train 4 (`train/2026-11-10-4`)
`46e2a3113` ("lint expectations match the merged tests"), `38b6b31de` (C53, scoped allows after the merge) and `602bc0140`
(C71, polls after the merge); train 7's `0b736ac0b` rewrote the C71 baseline after merges (a shrink of moved sites; I did not
find a failing run for it, so it is context, not an instance).

**A trial merge would have shown them.** The tree `git merge-tree --write-tree 12378b80c^1 12378b80c^2` (lint branch tip
`44881ad9c`, 2026-11-13 03:28, onto the train as it was; the integrator merged it at 07:49) already holds three of the
defects: the raw `canonicalize()` at `tests/it/generators.rs:10`, the test naming `"pathfind.rs"` in `release_isolation`
while the file is at `src/pathfind.rs`, and `heavy`'s `paddock-env` dependency with no line in `inputs.txt`. The merge
itself takes about 0.01 to 0.03 s (`git merge-tree` on four of train 8's merges, measured 2026-11-13).

### Two kinds inside the class

1. **A new rule meets new code** (`ecb6f33ad`, `cd0c3aa14`, `6394bc365`, `b51ef0308`, the train 4 rows). Nothing is wrong with
   either branch; only the combination fails. Only testing the combination before approval moves these earlier.
2. **A test or registry reads another area's text or layout** (`11c365fc4`, `0308201b4`, `bddd2947c`, and `be69a31f6`'s hand
   list). These also fail later, on main, whenever the other area changes, train or not. An accessor removes them.
   `bddd2947c` is the clearest: the accessor existed. `tools/paddock-dev/src/pre_push/ci_line.rs::conformance_args` is the
   one reader of the conformance line ("read from `.gitea/workflows/ci.yml` in this one place"), and it already handles
   both the inline line and `PADDOCK_CONFORMANCE_ARGS` (`from_env`). The registry entry wrote a second reader in `sh`.

**Pattern.** A branch is tested only against the merge base, so a semantic conflict with another approved branch is found
at train time. **Invariant.** Before a branch is approved, its gates and its affected tests have run on the tree it will
be merged into (the open train's head, or main when no train is open), and tests read declared data through the one
accessor of that data.

### What exists (read at the heads above)

- `./dev check --pre-push` (ADR 0038; `tools/paddock-dev/src/pre_push/mod.rs`): with `--refs-from-stdin` it checks the pushed
  commit in a locked throw-away worktree (`Detached::new`) and runs the fast gates CI's plan names there; `--base` (default
  `origin/main`) only chooses the diff. It runs no tests (`gates::NOT_RUN`: "tests (nextest, vitest, Playwright)").
- `./dev merge check` (`tools/paddock-dev/src/merge_check/mod.rs`): detectors over a merge's parents and result.
- ci.yml's `workflow_dispatch` takes `base` and `since_run`; with `base` set it runs the affected plan from that commit and
  the `train-gates` job (gate register, conformance with `$PADDOCK_CONFORMANCE_ARGS`, `cargo hakari generate --diff`,
  `cargo metadata` of every lockfile, publish dry run), whose `if:` is `inputs.base != ''` (`.gitea/workflows/ci.yml`).
- The merge queue's `./dev queue tick` (`tools/paddock-dev/src/queue/`, on main): the `Train` port's `build` merges pull
  requests `--no-ff` in its own clone with hooks off (`real.rs`, `core.hooksPath=/dev/null`), and the `Ci` port's `dispatch`
  goes through `./dev ci dispatch`, which waits while another run holds a single-machine runner and ends at `--max`. The
  design note says slices 1 to 3 are built and slices 4 to 6 (landing with rule 54, auto-tag, queue.yml) are not. Trains are
  still built by hand (train 8's merge commits are the integrator's). The trial merge needs only the built parts.

## Common practice (ER-37; sources fetched 2026-11-13)

| System | What a change is tested on before it merges | Source |
|---|---|---|
| GitHub merge queue | a temporary `gh-readonly-queue/<base>` branch with "the latest version of the `base_branch` as well as changes from pull requests ahead of it in the queue"; checks run on the `merge_group` event; a failing PR is removed and the rest rebuilt | [managing a merge queue](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue) |
| GitHub, without a queue | "Require branches to be up to date before merging": the branch must contain the base's tip; the page warns "More builds may be required" and names the queue as the way to get the same without authors refreshing | [about protected branches](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches) |
| GitLab | merged results pipelines "test a temporary merged commit that combines code from the source and target branches", on every MR pipeline; merge trains require them and add the MRs ahead | [merged results pipelines](https://docs.gitlab.com/ci/pipelines/merged_results_pipelines/), [merge trains](https://docs.gitlab.com/ci/pipelines/merge_trains/) |
| Zuul | before a job runs, "that change and all of the changes ahead of it in the pipeline queue will already be merged"; the gate (dependent) pipeline tests "every change ... exactly as it is going to be merged". That the check pipeline also merges onto the branch tip is my reading of the job-content page, which does not name the pipeline | [job content](https://zuul-ci.org/docs/zuul/latest/job-content.html), [gating](https://zuul-ci.org/docs/zuul/latest/gating.html) |
| bors-ng | "bors creates a merge commit, merging the main branch with all the pull requests that make up the batch"; archived 2024-04-04, recommends GitHub's queue | [repository](https://github.com/bors-ng/bors-ng) |
| Gitea (ours) | branch protection "Block merge if the pull request is outdated" (the GitHub up-to-date rule); no merged-results pipeline, no queue | [protected branches](https://docs.gitea.com/usage/access-control/protected-branches) |

All of them test the merge, not the branch alone, and they do it as the branch's ordinary pipeline, not as an extra one.
GitLab's merged results pipeline is the closest match: the one pipeline a change already gets runs on the merge commit. On
GitHub and GitLab it is a setting (0 build days). Gitea's "outdated" block does not fit here: trains land by a
fast-forward push, not by Gitea's merge button, and a branch made "up to date" with a train that later drops a member
carries that member's code.

## Decision

### 1. The branch's one CI run tests the trial merge (GitLab's merged results pipeline)

Before review asks for approval, the branch's one CI run (rule 50 (a)) is dispatched on a **trial merge**: the branch's
head merged `--no-ff` onto the **target**, the head of the open train (`train/<date>-<n>`) when there is one, else
origin/main. The run gets `base` = the target's sha, so the plan is exactly the branch's own change (the same size as a
run of the branch alone) and every test it reaches runs on the combined tree; `train-gates` runs because `base` is set.

- The merge is made the way the queue makes trains: the `Train::build` adapter with one member, in its own clone with
  hooks off. It is pushed to `trial/<branch>` (a scratch ref, overwritten by the next trial of that branch and deleted when
  the branch lands) and dispatched through `./dev ci dispatch` with `--max`.
- A conflict with the target is a finding, not a run: the author learns at once which files meet which branch.
- `./dev merge check` runs on the trial merge in process (it reads git only) before the dispatch.
- The run's verdict goes in the review: the reviewer sees red tests of the combined tree before approving. A trial that is
  red only in tests that also fail on the target is reported as such (`./dev ci triage --against <target run>`), not
  blamed on the branch.
- The train still runs its own full run before landing (ER-8). The trial does not replace it; it moves most failures
  from the train to the branch's author.

### 2. The local pre-push gates can run on the same trial merge

`./dev check --pre-push --onto <ref>`: the throw-away worktree checks out the merge of the pushed commit onto `<ref>`
instead of the commit alone; the change and the plan stay the branch's own (`--base` unchanged). Clippy (the canonicalize
ban), conformance (C53, C71) and, once the generator registry is on main, `./dev gen check` then see the combined tree
before the push. A row that fails only with `--onto` is labelled `TRIAL` so the author knows the cause is the combination.
Without `--onto` nothing changes.

### 3. Tests read declared data through its accessor (rule, next free number)

> A test, lint list or registry entry that needs data another part of the repository declares (a workflow's command line,
> a manifest's dependencies, a register's rows, where a file lives) reads it through the one accessor that owns that data
> (`ci_line::conformance_args`, `cargo metadata`, the register's parser, a constant the owning module exports), never by
> grepping or slicing that file's text. A source audit that must read another crate's code names the file through a
> path the owner exports and ends its span at an item boundary, not at a comment it remembers. A hand list that must equal
> generated data (heavy's `inputs.txt`) is generated, or its test prints the regenerate command (ER-40).

Where it is cheap it is checked: a report-only conformance check (the next free C id) flags registry `check` commands
in `docs/*.toml` and test files that run `grep`, `sed` or a regex over `.gitea/workflows/*.yml`. A survey on origin/main
(2026-11-13) finds 29 test files that name a workflow file (most in `conformance/test/`, which test checks of workflows and
may read them; the check takes an allow list with reasons) and 12 `include_str!` reads of shared crates' sources from apps
(source audits such as Pitwall's). Slicing by anchors is not linted (no cheap check); the rule and review cover it.

### What this does not build

No new queue, no new bot, no new workflow, no new label, no gate that blocks approval by itself. Slices 4 to 6 of the merge
queue (landing with rule 54, auto-tag, queue.yml) stay as planned and are not needed for this.

## Parallelism and cost (ER-20, ER-24)

- **Runners.** The trial run replaces the branch's run; it does not add one. Its plan is computed from `base` = the
  target, so it reaches the same projects as the branch alone (the branch's own diff), plus `train-gates` (one Linux job,
  already run on every base dispatch). It goes through `./dev ci dispatch`, so it waits while a train run or another trial
  holds the macOS or Windows runner, and only one train's CI uses each single-machine runner at a time. Rule 50 (b) stays:
  no re-dispatch only because the train moved; the train's own run catches what moved after the trial.
- **Machine.** The merge costs about 0.01 to 0.03 s (measured above). `--onto` uses the throw-away worktree pre-push
  already makes, so the local cost is the same as today's pre-push (CARGO_BUILD_JOBS=2, heavy's slot).
- **What it saves.** Train 8 needed eight fix commits by the integrator after the merges and run 8050 failed in 13 tests or
  jobs; each fix round on a train reruns the failed jobs of every merged branch (ER-10) while the train holds the runners.

## Work packages (agent-days; each check report-only until its first green run, ER-41)

| WP | What | Cost |
|---|---|---|
| WP-1 | `./dev ci dispatch ci.yml --trial <branch> [--onto <ref>]`: target = newest open train head or origin/main; `Train::build` with one member; merge check in process; conflict = finding; push `trial/<branch>`; dispatch with `base` = target sha; delete `trial/*` refs of landed branches in the landing step. Report-only: the result is shown to the reviewer; approval requires a trial run only after the first green one is recorded here by run id | 1 |
| WP-2 | `./dev check --pre-push --onto <ref>`: trial worktree, `TRIAL` rows, `./dev gen check` when the registry is on main; tests with a merge of two fake branches that only fail together | 0.75 |
| WP-3 | The rule (docs/rules.md, next free number, ER-n copy in engineering-rules) and the accessor fix: `./dev ci conformance-args` prints `ci_line::conformance_args`, the generator entry calls it; the report-only conformance check with its allow list; first run's count recorded | 0.5 |
| WP-4 | Docs: rule 50 (a) and (h) say the branch run is the trial run; the merge-queue note says slices 1 and 2 serve trials; the integrator brief template; a lessons entry for train 8 | 0.25 |

**Total 2.5 agent-days.** Common-practice baseline: 0 days on GitHub or GitLab (a setting); Gitea has no merged-results
pipeline, so about 1 day is the floor for any trial-merge run here. This design is 1.5 days beyond that floor, mostly the
local `--onto` (WP-2) and the check (WP-3); the cheaper options are in D1.

## Decisions for the user

- **D1. Scope.** (a) WP-1 to WP-4 as above, 2.5 days (recommended: WP-1 moves the test failures, WP-2 the lint and
  conformance ones that are cheaper to see before a push). (b) WP-1, WP-3 rule text and WP-4 only, about 1.5 days: the
  trial run in CI, no local `--onto`, no check. (c) No tool: the integrator's brief asks the author to merge the train head
  into a scratch branch and dispatch that by hand (0.25 day for the brief; it is GitHub's "up to date" rule done by hand,
  and is skipped under time pressure).
- **D2. Target when a train is open.** (a) The open train's head (recommended: train 8's defects were with branches
  already in the train). (b) Always origin/main (simpler, misses branch-against-train conflicts until the train).
- **D3. When the trial run becomes required for approval.** (a) After its first green trial run, recorded in WP-1 (ER-41;
  recommended). (b) Advisory only, never required.

## Consequences

- Most combination failures reach the branch's author before approval, in the branch's own run, with no extra runs.
- A trial can go stale when the train changes after it; the train's full run (ER-8) still catches that, so trains get
  fewer fix rounds, not none.
- `trial/*` refs appear on the server; they are scratch and cleaned when the branch lands.

Recurring: train-time combination failures (this ADR): train 8 `11c365fc4`, `be69a31f6`, `0308201b4`, `bddd2947c`,
`ecb6f33ad`, `cd0c3aa14`, `6394bc365`, `b51ef0308` (run 8050); train 4 `46e2a3113`, `38b6b31de`, `602bc0140`. Related, a
second reader of a datum that already had one accessor (`bddd2947c` against `ci_line::conformance_args`): rule 55's class.
