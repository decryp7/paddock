# NEW-paddock-dev-test-split. paddock-dev's tests are split by what they read, and the crate is layered before it is split by area

- Status: **Proposed** 2026-11-27, for the user's decisions D1 to D3 below. Written for user decision 107 (2026-11-27: split
  tools/paddock-dev's tests into units the ADR 0116 test cache can skip). The number is assigned when it lands (AGENTS.md, the
  `NEW-` rule).
- Date: 2026-11-27
- Related: ADR [0116](0116-a-test-target-whose-inputs-did-not-change-reuses-its-pass.md) (the test-result cache: one target per Cargo
  package, `test_cache/key.rs` `Target`), ADR [0117](0117-the-repository-graph-is-the-one-model-and-every-plan-is-a-query.md) (the
  node input hash, `repo_model/hash.rs`; affected test targets by `binary_id`), ADR
  [0121](0121-whole-repository-tests-declare-a-cost-class-and-nextest-budgets-each-class-in-one-place.md) (the `graph_`, `tree_` and
  `live_` classes), ADR [0123](0123-the-ci-job-model-lives-in-one-pure-crate-and-every-reader-asks-it.md) (the job model), ER-20, ER-33,
  ER-37, ER-41, ER-44, ER-48, ER-52, rule 48.
- Evidence heads (ADR 0091), read 2026-11-27 and re-read before the commit: `origin/main` = `e60b56d27`, which is also
  `origin/train/2026-11-27-w` and `origin/train/2026-11-27-x`. `ci/paddock-dev-linux-partitions`, named in the brief, had **no ref on
  origin** when read; its design is not read here. Code was read in a worktree of `e60b56d27`; nothing was built or run except the
  log and history scripts described in §Measured.
- Shipped code: no claim here is about released code (`git tag --contains e60b56d27` is empty). Mechanisms relied on, by status
  (ADR 0096): on origin/main: the test cache in shadow mode (`docs/test-cache.toml`, `switch.green_runs = [9180]`), the
  `test_cache::plan` filterset by `package(=..)`, the ADR 0121 class prefixes and `test_cost`, `plan_ci::partition_linux_legs` (the
  crates job's package partitions), the ADR 0117 affected mode (`binary_id(=..)` filters). Held on a named branch: the paddock-dev Linux
  partitions (`ci/paddock-dev-linux-partitions`, not on origin). Prerequisites this ADR adds: a binary-level target in the test cache
  (WP-1) and a module layer check (WP-2).

## Measured

Rule 48: the results block is in [docs/benchmarks.md](../benchmarks.md), "paddock-dev test time by area and churn per train
(2026-11-27)". Sources: the nextest per-test lines of the paddock-dev jobs of runs 9212 (jobs 52161 Linux, 52148 macOS, 52162
Windows) and 9180 (51687, 51674, 51688), read through the Gitea API; history from `git log --first-parent
--diff-merges=first-parent origin/main`. No user data.

**The test step is most of the job.** 9212: Linux 302 s of 11.8 min (its `./dev` tool cache missed: 4.7 min of restore, build and
save, ADR 0115's subject), macOS 360 s of 8.1 min, Windows 282 s of 10.5 min. 9180: 295 s of 8.4, 329 s of 5.8, 458 s of 8.9 min.
macOS and Windows already run only their OS-scoped share (434 and 1,023 tests skipped of about 2,800, ER-52).

**Where the time is** (sum of per-test seconds, run 9212; the largest groups; the ADR 0121 class columns are across all areas):

| Area (module or `tests/it` file) | Linux s (%) | macOS s (%) | Windows s (%) |
|---|---|---|---|
| it/tree | 159 (18.5) | under 5 | under 5 |
| it/repo_graph_net | 85 (9.8) | 36 (4.5) | 103 (8.7) |
| it/pre_push + src/pre_push | 129 (15.0) | 154 (18.8) | under 10 |
| it/host_steps | 68 (7.9) | 70 (8.6) | under 4 |
| it/wrapper | 60 (7.0) | 82 (10.0) | 49 (4.1) |
| it/golden | 52 (6.0) | under 5 | under 5 |
| it/merge_check + src/merge_check | 27 (3.1) | 88 (10.7) | **545 (45.7)** |
| src/ci_runs + src/ci_run | 36 (4.2) | 86 (10.5) | 228 (19.1) |
| it/worktree | 19 (2.2) | 67 (8.2) | under 4 |
| src/local_sccache | under 2 | 63 (7.7) | 5 (0.4) |
| src/repo_model | 14 (1.6) | 15 (1.8) | 53 (4.4) |
| **whole-repository classes** (`graph_`, `tree_`, `live_`) | **480 (56)** | 143 (17) | 146 (12) |
| all tests | 861 | 817 | 1,191 |

Run 9180 gives the same shape (Linux whole-repository classes 58%, macOS 18%, Windows 9%). On Linux, 450 of the 480 whole-repository
seconds are in the `tests/it` binary; the longest single tests are the train replay (32.5 s) and the check engine's real-tree report
(16.6 s).

**The code is one cycle.** Of 66 top-level modules of `src/` (140,334 lines), **45 modules with 125,963 lines (90%) form one strongly
connected component** of `crate::` references (Tarjan over `crate::<module>` paths). Grouped into five candidate layers (base,
host/runner, model/plan/check, pre-push gates, CI/train tools) plus the binary, **about 65 distinct items point the wrong way**
(for example `tool_cache` -> `pre_push::heavy_tool`, `wait` -> `pre_push::exec::throttle_command_for`, `ci_tool` ->
`ci_host::phase::Held`, `places` -> `worktree::git`, `check` -> `pre_push`, `plan_ci` -> `ci_runs`). 30 of the 63 `tests/it` files
start the `./dev` binary, which links everything. 321 references to `tools/paddock-dev/src/...` paths sit in 85 files outside the
source (registers, conformance checks, workflows, gates.toml globs).

**Churn.** Every train of the last two weeks changed tools/paddock-dev (20 of 20). In the 15 trains of 2026-11-26 and 27, a train
changed on average 16 areas, and the low layers changed in most trains: a base module or a shared crate that every area uses
(`paddock-proc`, `paddock-testkit`, `paddock-env`, `paddock-cli`, the Cargo files) in 11 of 15.

**What each option would have run** (share of today's paddock-dev test seconds, replaying those changes against the layered split
above with dependents invalidated; the whole-repository classes keyed on the whole tree, as soundness requires, §Context):

| Unit replayed | Today | (b) test binaries by area | (a) crates, spawning tests left in the binary | (a) crates, spawning tests made library calls | (c) both |
|---|---|---|---|---|---|
| 15 trains and the direct commits after them, Linux | 100% | 98% | 97% | 95% | 93% |
| same, macOS | 100% | 96% | 94% | 91% | 87% |
| same, Windows | 100% | 95% | 98% | 96% | 91% |
| 17 members (2026-11-20 to 27), Linux | 82% | 78% | 76% | 69% | 66% |
| 17 members, macOS | 82% | 74% | 69% | 61% | 55% |
| 17 members, Windows | 82% | 73% | 77% | 74% | 65% |

Limits: the layering is a sketch, not a design; docs data files were mapped to the one area that reads them; a member is its merge
commit's diff to the train, not its trial run's diff to main; per-test seconds sum the threads, so wall time falls by less.

## Context

ADR 0116's target is a Cargo package (`test_cache/key.rs`, `Target::rust`; "the nextest binaries of one package are one target"), keyed
by the package node's read set (`repo_model/hash.rs`: the package folder, its path dependencies, the declared data files that reach it).
paddock-dev is one package, so any change to it reruns everything. The brief's premise is that splitting it by area lets the cache skip
the areas a train did not touch. The data above says that premise holds weakly:

1. **Trains are wide.** A train touches most areas and, in 11 of 15, a layer every area depends on. A split by area would have skipped
   2 to 9% of a train run's test time.
2. **The largest Linux cost cannot be cached by area.** The `graph_`, `tree_` and `live_` tests read the whole repository (ADR 0121:
   every tracked file, `cargo metadata` of every workspace, the conformance line). Their result depends on files outside any area, so a
   sound key for them is the whole tree. That is 56% of Linux test time, and it reruns on every change whatever the split.
3. **The code is one cycle,** so no split, by crate or by module, is possible before the cycle is broken.

Point 2 is also a soundness question for the cache as it is: the paddock-dev target's key does not cover what its whole-repository
tests read (nothing in `test_cache/` or `repo_model/hash.rs` treats the classes). A change outside the read set that the planner still
sends to the paddock-dev job would reuse a pass those tests might not give. Whether the planner sends such changes there was not
checked (§Not checked).

## Common practice (ER-37)

| Practice | Source | Fit here |
|---|---|---|
| A large Rust project is a flat workspace of many crates; splitting is cheap once the structure is flat | matklad, "Large Rust Workspaces" <https://matklad.github.io/2021/08/22/large-rust-workspaces.html>; rust-analyzer's `crates/` (about 40 crates, not fetched) | the end state for 140k lines; it needs an acyclic module graph first |
| One integration-test binary per crate, many modules inside: each binary is linked separately (a suite compiled 3x faster after merging) | matklad, "Delete Cargo Integration Tests" <https://matklad.github.io/2021/02/27/delete-cargo-integration-tests.html> | paddock-dev already does this (`tests/it`); option (b) goes against it, so split test binaries only where the split buys a different key |
| Test caches key a target on its declared input closure; a test that reads more declares it or is not cached | Bazel `data`/`external`, Nx `namedInputs` (ADR 0116 §Common practice) | the whole-repository classes need their own target with a whole-tree key |
| Spread tests across machines by test, not by package | nextest `--partition slice:m/n` and `hash:m/n` <https://nexte.st/docs/ci-features/partitioning/> | the Linux partitions; a package partition (`plan_ci::partition_linux_legs`) would be lopsided here (one layer holds 52% of Linux time) |
| Cargo has no test-result cache | cargo issue 10673 (ADR 0116) | the cache stays ours |

## Options

| | What | Train run skipped | Member run skipped | Compile and graph | Risk | Agent-days |
|---|---|---|---|---|---|---|
| **(a)** crates by area | 5 library crates plus the thin binary in the paddock-dev workspace; tests move with their area; 30 spawning test files rewritten to library calls where they test logic | 2 to 9% | 5 to 21 points (macOS most) | parallel crate builds and per-crate sccache hits (the common reason to split); 5 more link steps; a crate graph that must stay acyclic | **high**: breaks the 45-module cycle (about 65 items); rewrites 321 path references in 85 files; conflicts with every open paddock-dev branch (20 of 20 trains touch it), so it needs a freeze window | 7 to 9 (range 6 to 12) |
| **(b)** test binaries by area in one crate | `tests/it` split into area binaries; the cache gains a binary-level target | 2 to 5% | 4 to 9 points | more link steps (the 3x of matklad's measurement in reverse); no graph change | **soundness**: every binary links all of `src`, so its key must hold all of `src`; it is then reusable only for test-only changes | 1.5 to 2 |
| **(c)** both | | 7 to 13% | 16 to 27 points | as (a) | as (a) | 8.5 to 11 |
| **(d)** split by what tests read (recommended first) | the whole-repository classes move to their own binary (`tests/whole`), the cache keys it on the whole tree; the area tests keep the package key | none on a paddock-dev change (it is about soundness) | none | one more link step | low; makes the cache's paddock-dev reuse sound | 1 to 1.5 |
| **(e)** layer before splitting (recommended second) | a layer table and a check that every `crate::` path points down a layer, report-only first (ER-41); back-edges removed a batch at a time | none by itself | none | none | low; each batch is a refactor (ER-56) | 2 to 3 for the check and the first batches |

In job-minutes: with about 30 train runs and 17 member trial runs a week, (a) would save roughly 45 minutes a week of the single macOS
and Windows runners (macOS about 0.35 min a train run and 1.2 min a member run; Windows about 0.25 and 0.5), for 7 to 9 agent-days.
For comparison, the 4.7-minute `./dev` tool-cache miss of job 52161 alone is larger than any per-run saving above (ADR 0115).

## Decision (proposed)

1. **(d) now.** The whole-repository tests (`graph_`, `tree_`, `live_` names; 72 tests in `tests/it`, 17 in `src`) move to a second
   integration binary, `tests/whole/main.rs`. The test cache gains a binary-level target (`rust:tools/paddock-dev#whole`, nextest
   `binary_id(paddock-dev::whole)`), keyed on the HEAD tree id; the package target becomes "the package minus `whole`". The planner's
   reach for the paddock-dev job does not change. This is what ADR 0116 §2 asks of a key ("the target's source closure through declared
   edges"), applied to tests that read the whole tree.
2. **(e) next, report-only.** `docs/paddock-dev-layers.toml` lists the layers (base, host, model, gates, ci, binary), and a test in
   `tools/paddock-dev/tests/it` reads every `crate::` path with `syn` (already a dependency) and reports each back-edge. It blocks only
   after its first green run on the real tree. Back-edges are removed in small refactor batches beside feature work.
3. **(a) only after (e) shows no back-edge, and only if two more weeks of churn show a layer that stays quiet.** Then the split is a
   mechanical move in one freeze window. (b) is not done: its key is sound only for test-only changes.

What this removes: the unsound reuse of the whole-repository tests under the package key (1), and, once (e) blocks, the module cycle
that makes any finer test selection, by crate or by module, impossible (2). Nothing else is deleted; no register grows.

### Interaction with the Linux partitions and the job model

- **Partitions.** Partition the Linux paddock-dev leg by test (nextest `--partition slice:k/n` or `hash:k/n`), not by package: a
  package partition after (a) would put about half the time in one leg. The cache's filterset ANDs with the partition, as the crates leg
  does. The `whole` binary is the long pole of each partition (its ADR 0121 groups cap it at four tests at a time); with (d), a partition
  run whose area target is reused runs only its share of `whole`.
- **Job model (ADR 0123).** (d) and (e) change no job, leg or name. (a) adds packages to the paddock-dev workspace and leaves the job
  layout alone; if it is done, the legs keep running the workspace and the cache leaves packages out, so the model needs no new field.

## Decisions for the user

- **D1.** Is the area split of decision 107 changed into (d) plus (e), with (a) deferred until (e) is green and churn data support it
  (recommended), or is (a) done now (7 to 9 agent-days, about 6 beyond the recommended path, needs your yes, ER-37)?
- **D2.** The whole-repository binary's key: the HEAD tree id (recommended: sound, reruns on any change), or never cached (the same in
  practice, simpler)?
- **D3.** May the layer check land report-only as a new check (one in, one out: it replaces no existing check; its class is the cycle
  above, seen as the blocker of this decision)?

## Work packages

| WP | What | Files | Size |
|---|---|---|---|
| WP-1 | `tests/whole/main.rs`; move the class tests (names unchanged, so ADR 0121's overrides still match); nextest overrides by `binary_id` where they named `tests/it` paths | `tools/paddock-dev/tests/`, `.config/nextest.toml` | 0.5 d |
| WP-2 | binary-level target and its key in the test cache, the record step reading it, a positive control (a change outside the package invalidates `whole` and keeps the package target) | `src/test_cache/{key,cmd,plan,mod}.rs` | 0.5 to 1 d |
| WP-3 | layer table, the report-only check, its positive control | `docs/paddock-dev-layers.toml`, `tests/it/layers.rs` | 1 d |
| WP-4 | back-edge batches (about 65 items), each a refactor branch | `src/` | 1 to 2 d, spread |
| WP-5 (deferred, D1) | the crate split: workspace members, `inputs.txt`, `build.rs`, path references, test moves | the paddock-dev workspace and 85 referencing files | 5 to 7 d, in a freeze window |

Order: WP-1 and WP-2 together (one branch: the binary and its key must land as one), then WP-3, then WP-4. WP-1 to WP-3 can run while
the Linux partitions land; WP-2 rebases on them if they touch `test_cache/plan.rs`.

## Not checked

- Whether the planner sends a change outside paddock-dev's read set to the paddock-dev job (the soundness question of §Context): no
  `./dev plan` was run.
- The compile-time effect of (a): not built; the per-crate parallelism and sccache gain is common practice, not measured here.
- The layering's 65 items are from a path count, not a design; the real cut may differ.
- Wall time of each option: the replay sums per-test seconds; partitions and thread counts change wall time.
