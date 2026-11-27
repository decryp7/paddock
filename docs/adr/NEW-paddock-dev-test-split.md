# NEW-paddock-dev-test-split. paddock-dev's tests are split by what they read, and the crate is layered before it is split by area

- Status: **Accepted** 2026-11-27 by the user, relayed by the coordinator: D1 = (d) and (e) now, (a) the crate split deferred until
  after train y's measurements, revisited only if paddock-dev is still the bottleneck; D2 = the whole-repository target is keyed on
  the HEAD tree id; D3 = the layer check lands report-only (ER-41). Written for user decision 107 (2026-11-27: split tools/paddock-dev's tests into units the ADR 0116 test
  cache can skip). Revision 2 (this text) records the decisions, the work packages and the cost.
  The number is assigned when it lands (AGENTS.md, the `NEW-` rule).
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

## Decision

The program is (d), then (e); (a) is deferred (D1). (d) makes the cache's paddock-dev reuse sound; (e) removes the cycle that makes
any later split, by crate or by module, impossible. Whether (a) is worth its cost is decided after train y's measurements, and only
if paddock-dev is still the bottleneck then.

1. **(d) The whole-repository tests are their own test-cache target, keyed on the tree (D2).** The target is the tests whose names
   declare that they read the whole repository: the ADR 0121 classes (`test_cost::Class`, `graph_` and `tree_`) and the `live_` tests
   that read the real tree. It is defined by **one filterset owned by `src/test_cost.rs`** (ER-22: the class's one definition, beside
   `Class::filter`), not by a second test binary: 17 of the 89 tests are unit tests in `src/` that use private items, so a binary
   could not hold them without widening the API, and moving 72 tests between files would conflict with every open paddock-dev branch.
   nextest selects by filterset (`binary_id`, `test(/../)`), so a filter-defined target is reused and run exactly like a binary.
   The target id is `rust:tools/paddock-dev#whole`; its key is the ADR 0116 key over the **HEAD tree id** (`git rev-parse
   HEAD^{tree}`) instead of the node hash, with the same epoch, runner class and profile; the package target becomes "the package
   minus the whole-repository filter". Residual risk: a test that reads the whole tree under an unprefixed name stays in the package
   target; ADR 0121's `charge` catches the `cargo metadata` case at run time, the tree case only by cost, and the shadow mode and the
   nightly bypass are the net (ADR 0116 §6).
2. **(e) The layer check, report-only (D3).** `docs/paddock-dev-layers.toml` lists the layers and their modules; a test in
   `tools/paddock-dev/tests/it` reads every `crate::` path with `syn` (already a dependency) and reports each reference that points up a
   layer, with a positive control. It blocks after its first green run on the real tree (ER-41). One in, one out: it replaces no check;
   its class is the module cycle that blocks any split, and it is deleted if (a) lands, because cargo then refuses a crate cycle itself.
3. **(a) Deferred: the crate split, by the five layers of the sketch,** if it is revisited after train y and (e) reports no
   back-edge. The boundaries it would start from:

   | Crate | Modules (from §Measured's sketch) | Lines |
   |---|---|---|
   | `paddock-dev-base` | git_env, process_env, child_env, log_text, pathfind, cargo_target, input_hash, test_sleeper, wait_lock, job_outcome, repo_inputs, places, full_tree, nextest_log, test_cost, escape | about 7,200 |
   | `paddock-dev-host` | ci_host, ci_isolation, ci_setup_rust, ci_s3_guard, ci_tool, ci_build_jobs, ci_build_env, ci_rust_pin, ci_dyno_linux_units, local_sccache, disk, worktree, safe_remove, tool_cache, deps_cache, slots, heavy_cmd, heavy_install, health, wait, load, locks, locks_guard, release_isolation, hook, audit | about 41,900 |
   | `paddock-dev-model` | plan, plan_ci, plan_cmd, plan_gitea, plan_graph, plan_repo, plan_since, repo_model, gates, check, check_ids, host_tests, os_placement, test_features, generators, ipc_endpoints, run_scope, active_apps, lessons_driver | about 41,500 |
   | `paddock-dev-gates` | pre_push, ci_run | about 16,100 |
   | `paddock-dev-ci` | ci_runs, queue, pr, merge_check, test_cache | about 33,500 |
   | `paddock-dev` (binary) | main, lib dispatch, cli | about 1,100 |

   The boundaries are a starting point: WP-4 may move a module down a layer where that removes back-edges more cheaply than moving
   items, and the table in `docs/paddock-dev-layers.toml` is then the one source the split follows. Each crate gets its own `tests/`
   binary (one per crate, matklad's advice); the 30 `tests/it` files that start `./dev` call the crate's library where they test logic
   and stay in the binary crate where they test the command line. The whole-repository target of (d) stays one target across the crates
   (the same filterset, every package). (b) is not done.

What this removes: the unsound reuse of the whole-repository tests under the package key (1), and, once (e) blocks, the module cycle
(2). If (a) is done later, it removes the layer check (cargo then enforces the crate graph) and the single 140k-line compile unit.

### Interim measures being built (not this ADR's)

- **Decision 113, the interim paddock-dev reuse exclusion** (being built): until WP-1 and WP-2 land, the paddock-dev package is not
  reused by the test cache. It is removed when WP-2's target is live, as the last step of WP-2.
- **Decision 114, the OS scoping** (being built): which paddock-dev tests run on macOS and Windows (ER-52). It is independent of the
  split; if WP-5 is done, each crate's OS-scoped entries move with its modules (the `hostTests` names in `paddock.ci.json` are module paths,
  so they change with the move).

### Interaction with the Linux partitions and the job model

- **Partitions.** Partition the Linux paddock-dev leg by test (nextest `--partition slice:k/n` or `hash:k/n`), not by package: a
  package partition after (a) would put about half the time in one leg. The cache's filterset ANDs with the partition, as the crates leg
  does. The `whole` binary is the long pole of each partition (its ADR 0121 groups cap it at four tests at a time); with (d), a partition
  run whose area target is reused runs only its share of `whole`.
- **Job model (ADR 0123).** (d) and (e) change no job, leg or name. (a) adds packages to the paddock-dev workspace and leaves the job
  layout alone; if it is done, the legs keep running the workspace and the cache leaves packages out, so the model needs no new field.

## Decisions (taken 2026-11-27)

- **D1 = (d) and (e) now; (a) deferred** until after train y's measurements, revisited only if paddock-dev is still the bottleneck
  (the user). Cost of (d) and (e): 4.5 to 6.5 agent-days (§Work packages); (a) would add 4.5 to 6.
- **D2 = the HEAD tree id** keys the whole-repository target.
- **D3 = the layer check lands report-only** and blocks after its first green run.

## Work packages

| WP | What | Files | Size |
|---|---|---|---|
| WP-1 (d) | the whole-repository filterset in `test_cost` (graph, tree and live prefixes) with tests, failing first; the target `rust:tools/paddock-dev#whole` and the HEAD-tree key in `test_cache::key` | `src/test_cost.rs`, `src/test_cache/key.rs` | 0.5 d |
| WP-2 (d) | lookup, plan and record use the two targets (the package filterset becomes `package(=p) & not whole`); a positive control (a change outside the package invalidates `whole` and keeps the package target, a change inside invalidates both); removes decision 113's exclusion | `src/test_cache/{cmd,plan,mod}.rs`, ci.yml's paddock-dev record step if its arguments change | 1 to 1.5 d |
| WP-3 (e) | the layer table, the report-only check and its positive control | `docs/paddock-dev-layers.toml`, `tests/it/layers.rs` | 1 d |
| WP-4 (e) | the about 65 back-edges removed in batches (one refactor branch each, ER-56), until the check reports none; then it blocks | `src/` | 2 to 3 d, spread |
| WP-5 (a), deferred | the crate split in one freeze window of tools/paddock-dev: workspace members, `inputs.txt` and `build.rs`, the dev-tool cache inputs, the 321 path references in 85 files (registers, conformance, gates.toml globs, `hostTests`), test moves, per-crate nextest configuration, the Linux partitions by test; the layer check deleted | the paddock-dev workspace and the referencing files | 4.5 to 6 d |

**Total of WP-1 to WP-4: 4.5 to 6.5 agent-days** (with WP-5, 9 to 12). Order: WP-1 and WP-2 (WP-1 can land alone: it adds a definition
and a key nothing uses yet), then WP-3, then WP-4 batches beside feature work. WP-5, if revisited after train y, runs in a window agreed
with the train integrator (no other paddock-dev member in that train). WP-1 to WP-3 can run while the Linux partitions land; WP-2 rebases on them if they touch `test_cache/plan.rs`.

## Not checked

- Whether the planner sends a change outside paddock-dev's read set to the paddock-dev job (the soundness question of §Context): no
  `./dev plan` was run.
- The compile-time effect of (a): not built; the per-crate parallelism and sccache gain is common practice, not measured here.
- The layering's 65 items are from a path count, not a design; the real cut may differ.
- Wall time of each option: the replay sums per-test seconds; partitions and thread counts change wall time.
