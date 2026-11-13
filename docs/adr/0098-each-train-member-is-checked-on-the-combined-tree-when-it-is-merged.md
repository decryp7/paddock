# 98. Each train member is checked on the combined tree when it is merged, and tests read registries through their accessors

- Status: **Accepted 2026-11-13 by the user** (decision relayed by the coordinator the same day): D1 (b) the member gate at
  each merge, per-member blame, and the reader rule with its report-only check; D2 (b) also a trial onto the open train
  before approval; D3 (a) members are sent back only after the gate's first green run. WP-1 to WP-4, about 2.5 agent-days.
  History: Proposed 2026-11-13. Drafted at the coordinator's request after the user approved starting this redesign on
  2026-11-13 (ER-21: one class, eight checked instances and three earlier ones, below). Nothing here is built yet. ER-23
  does not apply (no drawings). Revision 2 (2026-11-13) answers the one review round: the mechanism moved from "a trial
  before approval" to "a gate on each member at merge time", the instance timing is counted, and the conflicted trial
  merge is stated (see "Review answers").
- Date: 2026-11-13
- Related: ER-8 (checks that fail only on combined code run at merge time), ER-9 / rule 50 (h), ER-10 (failed-only fix
  rounds), ER-21 / rule 40, ER-24 (machine load; one train's CI per runner class), ER-37 / rule 76, ER-41 (a new check starts
  report-only), rule 48 (benchmarks), rule 50 (a), rule 55; ADR 0038 (`./dev check --pre-push`), ADR 0089
  (`./dev merge check`), [docs/plans/merge-queue.md](../plans/merge-queue.md) (slices 0 to 6).
- Number: 0098 was free on every `origin` branch on 2026-11-13 (highest file on any branch: `0097`; read with `git ls-tree`
  over every `refs/remotes/origin/*`).
- Measurements: [docs/benchmarks.md, "Trial merges and the train-gates job"](../benchmarks.md#trial-merges-and-the-train-gates-job-2026-11-13-adr-0098).

```evidence-heads
origin/main f7bbe2b86
origin/train/2026-11-13-8 3a342cb42
origin/train/2026-11-12-7 f7bbe2b86
```

## Context

A train merges several approved branches. Each branch was tested on its own, against its merge base with main. Some of
them carry a test, a lint list or a registry entry that hard-codes text or file layout owned by another branch's area, or a
new rule that another branch's new code breaks. When the two meet, the combined tree fails; the integrator finds it when
the whole train runs, hours after the authors finished, and fixes it on the train.

### Instances (every row checked on 2026-11-13)

Train 8 (`train/2026-11-13-8`), the fixes the integrator made on the train. Each failure is real: run 8050 (ci.yml on the
train at `68ad0d4f9`; `./dev ci triage 8050 --no-history`: 13 failing tests or jobs, 12 new against main run 8045) or the
parent tree of the fix, read with `git show`.

| Fix | What hard-coded what | Failure checked |
|---|---|---|
| `11c365fc4` | `release_isolation/tests.rs` (from `feat/lint-no-raw-canonicalize`) lists `pathfind.rs` in its folder; `feat/generator-registry` moved it to `src/pathfind.rs` | parent tree has only `src/pathfind.rs`; the test reads the old path (NotFound) |
| `be69a31f6` | `tools/heavy/inputs.txt` (from `fix/pre-push-heavy-slot`) is a hand list that must equal `cargo metadata`; `feat/lint-no-raw-canonicalize` added `paddock-env` to heavy | parent: `Cargo.toml` names `paddock-env`, `inputs.txt` does not; run 8050: `inputs::heavy_inputs_txt_matches_cargo_metadata` failed on Linux and Windows |
| `0308201b4` | Pitwall's elevated-registration audit slices paddock-proc's `cleanup.rs` from `pub fn report(` to a doc line it knew; `fix/must-use-removals` put helpers between them | run 8050: `the_elevated_registration_touches_no_folder_the_user_controls` failed on macOS, Windows and Linux |
| `bddd2947c` | the generator registry's conformance entry (`docs/generators.toml`, `feat/generator-registry`) greps ci.yml for the inline `--only` line; train 7 had moved it into `PADDOCK_CONFORMANCE_ARGS` | parent: the grep matches nothing and `./dev gen check` reported NOT MEASURED (commit body); run 8050's train-gates job also failed, outside any test (its log not read for the cause) |
| `ecb6f33ad` | the `safe_remove` lint's allow list (train 7) met two new remove sites of `feat/ci-in-rust-wp5` | parent lacks both files in `ALLOWED` of `tools/paddock-dev/tests/it/safe_remove_lint.rs` |
| `cd0c3aa14` | `feat/lint-no-raw-canonicalize` bans `Path::canonicalize`; `feat/generator-registry` and `fix/pre-push-base` arrived with raw calls | parent: `tests/it/generators.rs:10` and `worktree/ops.rs` call `.canonicalize()` |
| `6394bc365` | C71 (train 7, `da9ef5209`) rejects fixed waits under 40 s; `fix/debrief-store-phase-logs` added five `recv_timeout(30 s)` | parent has the five calls |
| `b51ef0308` | C71 again; `feat/proc-spawn-detached-fix` shortened a wait to 15 s | parent has the 15 s wait |

Earlier, the same shape: train 4 (`train/2026-11-10-4`) `46e2a3113` ("lint expectations match the merged tests"),
`38b6b31de` (C53, scoped allows) and `602bc0140` (C71, polls). Their commits have no body, so I did not establish which
branches met or when; they are not counted below. Train 7's `0b736ac0b` rewrote the C71 baseline after merges; I found no
failing run for it, so it is context, not an instance.

### Where was the other side when the branch was finished?

Approval times are not recorded, so the branch's tip commit time stands in for "finished"; approval came after it, and a
train only grows until it lands. Times are committer dates (+08:00) from `git log`. Train 8's first merge is `11e6be4b1` at
11-13 06:35; train 7 was open until main moved to `f7bbe2b86` (pushed 11-13 10:11, origin/main reflog), and none of the train
7 commits below was on main before that (`git merge-base --is-ancestor` against main's earlier heads `77bc33215` and
`b519fd27a`).

| Kind | Instances | Count |
|---|---|---|
| The other side was already on the **open train** (train 7, not main) when the branch's tip was made | `bddd2947c` (`a382865d4`, 11-12 14:10, against `feat/generator-registry` tip 11-13 00:34), `ecb6f33ad` (safe_remove lint `53e3b5120`, merged into train 7 11-12 16:18, against wp5-r1 tip 04:07), `6394bc365` and `b51ef0308` (C71 `da9ef5209`, 11-12 07:59, against tips 00:30 and 01:58) | 4 |
| **Siblings**: both sides in review at the same time; neither was on any train when either tip was made | `11c365fc4`, `be69a31f6`, `cd0c3aa14` (`feat/lint-no-raw-canonicalize` tip 03:28 against `feat/generator-registry` 00:34, `fix/pre-push-heavy-slot` 02:57, `fix/pre-push-base` 00:54; all merged into train 8 between 06:42 and 07:49) | 3 |
| The other side was **on main** already (`e20ae50c3`, 11-08): the branch alone breaks it | `0308201b4` (`pitwall-server` depends on `paddock-proc`, its `Cargo.toml:41`, so the branch's own affected plan reaches the test; a branch run with tests would have failed) | 1 |

A trial before approval onto the open train would have caught at most the first four; a trial onto main, none. Only a check
at merge time sees the three sibling cases. (Whether a sibling was approved before or after its partner reached train 8 is
unknown, because approval times are not recorded.)

**The conflicted trial.** `git merge-tree --write-tree 12378b80c^1 12378b80c^2` (`feat/lint-no-raw-canonicalize` onto train
8 as it was) exits 1: content conflicts in `tools/heavy/Cargo.toml` (both sides changed the dependency block; the
`paddock-env` line is inside the conflict) and `tools/paddock-dev/src/ci_host/build_folder.rs`. A conflicted tree does not
build, so no test can run on it; the integrator resolved both by hand at 07:49. Of the cleanly merged files, two already
hold defects: the raw `canonicalize()` at `tests/it/generators.rs:10` and the test naming `"pathfind.rs"` in
`release_isolation` while the file is at `src/pathfind.rs`. The `inputs.txt` defect appears only once the `Cargo.toml`
conflict is resolved with both sides kept. In the queue, `Train::build` drops a member on any conflict and names the files
(`queue/real.rs`: `merge --no-ff`, then `diff --name-only --diff-filter=U`), so a gate on the merged tree runs only on
members that merged cleanly, or after a person resolves the conflict, as the integrator does today.

### Two kinds inside the class

1. **A new rule meets new code** (`ecb6f33ad`, `cd0c3aa14`, `6394bc365`, `b51ef0308`). Neither branch is wrong; only the
   combination fails. Only checking the combination moves these earlier.
2. **A test or registry reads another area's text or layout** (`11c365fc4`, `0308201b4`, `bddd2947c`, and `be69a31f6`'s hand
   list). These fail whenever the other area changes, train or not. An accessor removes most of them; slicing source text by
   anchors (`0308201b4`) has no cheap lint and stays unchecked (below). `bddd2947c` is the clearest: the accessor existed.
   `tools/paddock-dev/src/pre_push/ci_line.rs::conformance_args` is the one reader of the conformance line and already
   handles both the inline line and `PADDOCK_CONFORMANCE_ARGS` (`from_env`); the registry entry wrote a second reader in `sh`.

**Pattern.** Members are checked one by one against main and then only together as a whole train, so the member that breaks
the combination is found late and fixed by the integrator. **Invariant.** When a member is merged into a train, the gates its
change reaches run on the combined tree (the merge commit, diffed from the train before it), and a member that fails them is
named before the next one is merged; tests read declared data through the one accessor of that data.

### What exists (read at the heads above)

- `./dev check --pre-push` (ADR 0038; `tools/paddock-dev/src/pre_push/mod.rs`): with `--refs-from-stdin` it checks a given
  commit in a locked throw-away worktree (`Detached::new`) and runs the fast gates CI's plan names for the diff from `--base`
  (fmt, host and Windows Clippy of changed workspaces, conformance with ci.yml's arguments, commit-lint, lockfiles). It runs
  no tests (`gates::NOT_RUN`: "tests (nextest, vitest, Playwright)"). Measured: about 100 s of wall time for a shared-crate
  change with warm build folders (docs/benchmarks.md, "`./dev check --pre-push` run times").
- `./dev merge check` (`tools/paddock-dev/src/merge_check/mod.rs`): detectors over a merge's parents and result. ER-8 already
  asks for the combined-only checks at each merge into a train.
- ci.yml's `workflow_dispatch` with `base` runs the affected plan from that commit and the `train-gates` job (gate register,
  conformance with `$PADDOCK_CONFORMANCE_ARGS`, `cargo hakari generate --diff`, `cargo metadata` of every lockfile, publish
  dry run; `if: inputs.base != ''`).
- The merge queue (`tools/paddock-dev/src/queue/`, on main). Per merge-queue.md line 3, slices 1 to 3 are built except
  slice 2's Windows leg; slices 4 to 6 are built only in part (landing without rule 54; no rule 54 dispatch, no auto-tag, no
  queue.yml). `Train::build(main, prs: &[Pull], marker)` merges ledger pull requests (`Pull`: number, branch, head) `--no-ff`
  in its own clone with hooks off, runs the merge check on each merge, drops a conflict or a finding at once, and then adds
  a marker commit `chore(queue): train round <n>` with `Queue-Prs:` trailers (`real.rs:556`). The tick runs **no code of any
  pull request** (merge-queue.md step 3: gates are CI jobs, run without the queue's token), so a local gate on each member
  cannot run inside the tick; on a red train run the tick blames members through `Train::affected` (the planner's projects of
  each member) and `ci triage` (new failures only), drops every suspect and retries the rest (Q3). Trains are still built by
  hand today (train 8's merge commits are the integrator's).

## Common practice (ER-37; sources fetched 2026-11-13)

| System | What a change is tested on before it merges | Source |
|---|---|---|
| GitHub merge queue | each entry on a temporary `gh-readonly-queue/<base>` branch with "the latest version of the `base_branch` as well as changes from pull requests ahead of it in the queue"; checks run on the `merge_group` event; a failing entry is removed and the rest rebuilt; "build concurrency" 1 to 100 limits parallel runs | [managing a merge queue](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue) |
| GitHub, without a queue | "Require branches to be up to date before merging"; "More builds may be required"; the queue "provides the same benefits" without authors refreshing branches | [about protected branches](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches) |
| GitLab merge trains | each MR's pipeline runs on "the changes of both merge requests combined with the target branch" (the MRs ahead); up to 20 in parallel; a failed MR is removed and the pipelines behind it restart; requires merged results pipelines | [merge trains](https://docs.gitlab.com/ci/pipelines/merge_trains/), [merged results pipelines](https://docs.gitlab.com/ci/pipelines/merged_results_pipelines/) |
| Zuul | before a job runs, "that change and all of the changes ahead of it in the pipeline queue will already be merged"; the gate pipeline tests "every change ... exactly as it is going to be merged"; on a failure, changes behind are re-tested without it | [job content](https://zuul-ci.org/docs/zuul/latest/job-content.html), [gating](https://zuul-ci.org/docs/zuul/latest/gating.html) |
| bors-ng | "creates a merge commit, merging the main branch with all the pull requests that make up the batch"; bisects a failed batch; archived 2024-04-04, recommends GitHub's queue | [repository](https://github.com/bors-ng/bors-ng) |
| Gitea (ours) | "Block merge if the pull request is outdated" only; no queue, no merged-results pipeline | [protected branches](https://docs.gitea.com/usage/access-control/protected-branches) |

All of them test each change on top of the changes ahead of it, at queue (merge) time, and drop the one that fails. They
differ in how many runs they spend: GitHub and GitLab run one pipeline per entry (in parallel), bors one per batch with
bisection. On those platforms it is a setting (0 build days). Paddock's queue design already chose the batch shape for the
expensive part: one CI run per train, with blame by the affected plan instead of bisection, because the runners are shared
(merge-queue.md, Q2 and Q3).

## Decision

### 1. A member gate at each merge into a train (the cheap half of GitHub's per-entry check)

When the integrator merges a member into a train (by hand today), the merge commit gets the fast gates with `--base` = the
train before the merge (`<merge>^1`): `./dev check --pre-push` checks that commit in its throw-away worktree, so the plan is
the member's own change and the gates see the combined tree. That covers Clippy (the canonicalize ban), conformance (C53,
C71), commit-lint, lockfiles and, once the generator registry is on main, `./dev gen check`. A member that fails is not fixed
on the train: the integrator takes it out again (resets the train branch in their own locked worktree to `<merge>^1` before
anything else is merged) and sends it back to its author with the failing rows, as the queue drops a member. A conflict is
the same: the member goes back with the conflicting files, unless the integrator chooses to resolve a trivial one.

In the queue the tick runs no pull-request code, so there the member gate is the CI side that already exists: one run per
train (`train-gates` plus the affected plan) and blame per member through `Train::affected` and `ci triage`. Nothing new is
built for it; hand-built trains get the same blame through one read-only command (WP-1). Neither form uses `Train::build`
for a one-member trial: that would need a ledger `Pull` and would write a `chore(queue): train round` marker, which a hand
train does not have.

Tests stay once per train in CI (ER-8, the train's dispatch), not once per member: a per-member CI run is what GitHub and
GitLab do, but here it costs the runners each member's plan again (D1 (c)).

### 2. Tests read declared data through its accessor (rule, next free number)

> A test, lint list or registry entry that needs data another part of the repository declares (a workflow's command line,
> a manifest's dependencies, a register's rows, where a file lives) reads it through the one accessor that owns that data
> (`ci_line::conformance_args`, `cargo metadata`, the register's parser, a constant the owning module exports), never by
> grepping or slicing that file's text. A source audit that must read another crate's code names the file through a path
> the owner exports and ends its span at an item boundary, not at a comment it remembers. A hand list that must equal
> generated data (heavy's `inputs.txt`) is generated, or its test prints the regenerate command (ER-40).

Checked where cheap: a report-only conformance check (C121, the id assigned centrally on 2026-11-13) flags registry `check` commands in `docs/*.toml`
and test files that run `grep`, `sed` or a regex over `.gitea/workflows/*.yml`. A survey on origin/main (2026-11-13) finds 29
test files that name a workflow file (most in `conformance/test/`, which test checks of workflows and may read them; the
check takes an allow list with reasons) and 12 `include_str!` reads of shared crates' sources from apps (source audits such as
Pitwall's). **Kind 2 is only partly checked:** slicing another file's text by anchors (`0308201b4`) and hard-coded paths into
another crate (`11c365fc4`) are not linted (no cheap, low-noise check); the rule, review and the train's test run cover them.

### What this does not build

No new queue, bot, workflow or label; no per-member CI run. The pre-approval trial (D2 (b), WP-4) adds one scratch ref
namespace, `trial/*`. The queue's slices 4 to
6 stay as planned.

## Parallelism and cost (ER-20, ER-24)

- **Runners: no extra runs.** The train still gets one CI run per round (rule 50 (a), ER-10 fix rounds with `since_run`),
  dispatched through `./dev ci dispatch`, so it waits while another train holds the macOS or Windows runner. The Linux
  `train-gates` job it already contains took 288 s in run 8053 (train 8 at `3a342cb42`, green; the run took 13 min 20 s in
  all). A member the member gate takes out never reaches that run, so the run carries fewer fix rounds.
- **Machine.** The member gate runs locally in the integrator's worktree: about 100 s of wall time per member that changes a
  shared crate (the recorded pre-push measurement), less for a docs or single-app member; conformance alone took 20.8 s.
  Train 8 had 18 member merges, so up to about 30 minutes of the integrator's local time per train, at `CARGO_BUILD_JOBS=2`
  through heavy's slot and within pre-push's 15-minute budget, one member at a time (merges are sequential). The merge itself
  costs 0.01 to 0.03 s.
- **Against what it saves.** Train 8: eight fix commits by the integrator after the merges, and run 8050 red in 13 tests or
  jobs (each red round reruns its failed jobs while the train holds the runners). Four of the eight (`cd0c3aa14`,
  `6394bc365`, `b51ef0308`, `bddd2947c`) are static findings the member gate sees at the merge; the four test failures still
  show in the train's run, now blamed to a member and sent back instead of fixed on the train.

## Work packages (agent-days; each check report-only until its first green run, ER-41)

| WP | What | Cost |
|---|---|---|
| WP-1 | `./dev merge check --gates`: after the detectors, run `./dev check --pre-push` on the merge commit with `--base <merge>^1` and add its rows (report-only: printed, exit unchanged, until its first green run on a real train is recorded here by train head); `./dev ci triage <run> --members <train-base>`: the read-only blame the queue already computes (`Train::affected` per first-parent merge plus new failures), for hand-built trains. Tests with two fake members that only fail together | 0.75 |
| WP-2 | The rule (docs/rules.md, next free number, and the ER copy) and the accessor fix: `./dev ci conformance-args` prints `ci_line::conformance_args`, the generator entry calls it; the report-only conformance check with its allow list, its first count recorded | 0.5 |
| WP-3 | Docs: the integrator's train checklist (member gate, take out and send back), ER-8's text names the member gate, a lessons entry for train 8; rule 50 (a) says the branch run is the WP-4 trial run | 0.25 |
| WP-4 | The trial before approval (D2 (b)): `./dev ci dispatch ci.yml --trial <branch>` merges the branch head `--no-ff` onto the open train's head (origin/main when no train is open) in its own clone with hooks off (the queue's `real.rs` git setup, not `Train::build`, which needs a ledger `Pull` and writes a marker), runs the merge check in process, reports a conflict with its files instead of dispatching, pushes `trial/<branch>` and dispatches with `base` = the target's sha through the runner-busy wait. It replaces the branch's one CI run (rule 50 (a)), so it adds no run; `trial/*` refs of landed branches are deleted at landing. Report-only for the reviewer until its first green trial run is recorded here | 1 |

**Accepted scope: WP-1 to WP-4, 2.5 agent-days.** The cost of the options as compared before the decision: Baseline: 0 days on GitHub or GitLab (a setting). On Gitea the cheapest form of the merge-time
check is WP-1 plus WP-3, about 1 day. WP-2 (0.5 day) goes beyond that floor; it is offered separately because three of the
eight instances are kind 2 and would also break on main without a train (they fail whenever the other area changes), and
because `bddd2947c` was a second reader of data that already had an accessor, rule 55's class.

## Decisions

Decided by the user on 2026-11-13: **D1 (b), D2 (b), D3 (a)**. The options as they were put:

- **D1. Scope.** (a) WP-1 and WP-3: the member gate at each merge and the blame command, about 1 day (recommended: the
  cheapest option that covers the merge-time case; it sees 4 of 8 at the merge and blames the rest in the train's run).
  (b) (a) plus WP-2, the accessor rule and its report-only check, 1.5 days (if the user wants kind 2 addressed now).
  (c) GitHub's full per-entry form: one CI run per member on top of the ones ahead. It sees all eight before the next
  member merges, but at run 8053's size (13 min 20 s, 288 s of it the Linux train-gates job) train 8's 18 members would take
  about 4 hours of runner time per train instead of one run, plus about 1 day to build the per-member dispatch. Not
  recommended (ER-24).
- **D2. A trial before approval as well** (revision 1's mechanism). (a) No (recommended): it would have caught at most the
  four instances whose other side sat on the open train, and none of the three sibling cases. (b) Yes, about 1 day more: the
  branch's one CI run is dispatched on its merge with the open train head.
- **D3. When the member gate starts to send members back.** (a) After its first green run on a real train, recorded in WP-1
  (ER-41; recommended). (b) Advisory only.

## Consequences

- Static combination failures are found at the merge that causes them and go back to the member's author; the train's run
  carries the test failures, blamed per member.
- The integrator spends up to about 30 minutes more local time per large train; the runners spend nothing more.
- Kind 2 recurs less once the rule lands; anchor slicing stays a review item.

## Review answers (revision 2)

- P1-1 (timing): counted above; revision 1's claim that a pre-approval trial would have shown the defects was wrong for the
  three sibling cases. The mechanism is now the merge-time member gate; the pre-approval trial is D2 (b).
- P1-2 (conflict): `12378b80c`'s trial merge conflicts in `tools/heavy/Cargo.toml` and `ci_host/build_folder.rs`; it holds two
  defects in cleanly merged files and the third only after the conflict is resolved; a conflicted member goes back (above).
- Facts: `Train::build` takes `Pull` values and writes the `chore(queue): train round` marker, so no one-member trial uses it.
  Slice 2's Windows leg is not built.
- ER-37: the recommended option is about 1 day; WP-2 is offered separately; run 8053's `train-gates` 288 s is in the cost.

Recurring: train-time combination failures (this ADR): train 8 `11c365fc4`, `be69a31f6`, `0308201b4`, `bddd2947c`,
`ecb6f33ad`, `cd0c3aa14`, `6394bc365`, `b51ef0308` (run 8050); earlier, not classified: train 4 `46e2a3113`, `38b6b31de`,
`602bc0140`. Related: a second reader of data that already had one accessor (`bddd2947c` against
`ci_line::conformance_args`), rule 55's class.
