# 95. Tests get a writable repository copy through one cheap helper, and pay whole-repository setup once per run

- Status: Proposed 2026-11-10, revision 2. Drafted at the user's request of 2026-11-10 (ER-21: one class, nine red runs
  over three causes in the golden and graph tests, plus one slow CI step, each fixed one cause at a time). Includes the
  coordinator's input from the review of `origin/fix/golden-live-windows-timeout`. Revision 2 answers the independent
  review of revision 1 (`55286a33f`, "changes": P1-1, P1-2 and the P2s; see "Review answers"). Amends ADR 0078
  (`docs/adr/amendments/0078-2026-11-10.md`). Nothing here is built yet. ER-23 does not apply (no drawings).
- Date: 2026-11-10
- Related: ER-12, ER-14, ER-20 / rule 51, ER-21 / rule 40, ER-22 / rules 42 and 55, ER-27, ER-33 / rule 71; ADR 0025
  (kept clones), ADR 0071 §6 (nextest setup scripts not used), ADR 0078 (accepted: `paddock_testkit::scratch`, one run
  root, its leak check), ADR 0080 (CI speed), ADR 0088 (ratchets), ADR 0094 (drafted with this one); the RT-5 check host
  (`./dev check`, `origin/feat/rt5-check-host`, not on main).
- Number: 0095 was free on every `origin` branch on 2026-11-10 (see ADR 0094's Number line; the same listing).
- Prerequisites: ADR 0078's run root, its `.lock` and its leak check (0078 WP-1 and WP-3; `run_root()` and `ScratchDir`
  exist on no `origin` branch today, `git grep` over every branch's `crates/paddock-testkit`); the RT-5 host for the
  check (§5); the golden fix reaching main (`61b82cd32` is in `origin/train/2026-11-09-3` `c151422a4`, not in
  `origin/main` `b51c72509`).
- Evidence: read on 2026-11-10 with `git show`, `git grep`, `git log` and `git merge-base --is-ancestor` after
  `git fetch`; paths without a branch are from `origin/train/2026-11-09-3` (revision 1 read `67a633ef3`, revision 2
  `c151422a4`, which carries the golden fix). Commit-message numbers are quoted, not re-measured (no build or CI run for
  this ADR). No user data was read. The heads, re-read right before the commit:

```evidence-heads
read: 2026-11-10
origin/train/2026-11-09-3 c151422a4
origin/main b51c72509
origin/fix/golden-live-windows-timeout 61b82cd32
origin/feat/rt5-check-host ec8530a95
origin/docs/adr-windows-gate-ratchets db7782e01
```

## Context: the incidents

The golden live tests in `tools/paddock-dev/tests/it/golden.rs` compare `./dev`'s planner with `scripts/affected.mjs`
on the real repository; the contract, tree and inputs tests read the real repository's graph. Each pays a large fixed
setup, and each new cause took them past the budget (golden: the waiver `slow-timeout = { period = "60s",
terminate-after = 4 }`, 240 s; the others 60 s; `tools/paddock-dev/.config/nextest.toml`):

| # | Runs | Cause | Fixed by |
|---|---|---|---|
| 1 | red: 7819 (job 33238) and four more since 7668; green but at 195 s of 240 s: 7810 (job 33008) | the oracle ran `affected.mjs`, and the Rust side read the graph, once per sample: 36 samples × 23 workspaces = 828 `cargo metadata` runs | `b804ac78a` (2026-11-09): one node process and one Rust graph read for all samples, with a bound that fails when cargo runs more than once per workspace (`golden.rs:50-55`). Mac under load: 52 s → 8 s (its message) |
| 2 | red: 7787 (macOS, six tests over 60 s) and 7788 (macOS, two more) | graph-reading tests each ran `cargo metadata` over every workspace at once and queued on cargo's package-cache lock | `54f8ab5ce` (contract tests read the app graph once) and `60f2d7249` (test group `repo-graph`, `max-threads = 2`) |
| 3 | red: 7879, 7893; green but at 183 s and 236 s: 7881 (host to itself) | `copy_checked_out_files` copied about 11,000 files (550 MB), hashed and compressed them with `git add -f -A` (780 MB of objects) and rewrote them with `reset --hard`; both edit tests at once | `61212a3fb` and `61b82cd32`, now in `origin/train/2026-11-09-3` `c151422a4`: alternates plus index reuse, `checkout-index --all`, `gc.auto 0`, `count-objects -v`, phase output in the two narrowing tests. Mac: copy 38 s → 3.9 s; the two tests 35/39 s → 7.4/21 s (its message). Windows after the fix: not measured yet |
| 4 | 7808 (a slow step, not a test) | `conformance/ci/kept-clone.sh` ran `reset --hard` before its sparse patterns, writing the whole tree: 50 to 110 s on the Windows CI PC | `5544158b2`: sparse patterns first (`kept-clone.sh:166-174`), with a test (`conformance/test/kept-clone.test.mjs:177`) |

Nine red runs over three causes in tests, plus one slow CI step. A related fixed-cost case outside this class: the
wrapper hash tests (red 7725, 7812, 7819) start 34 programs per hash (`b804ac78a`'s message); they were split, not shared.

**Review of the fix** (`61b82cd32`). `61212a3fb`'s mechanism test counted loose objects only, which races git's detached
auto-gc: the old code made 14,382 objects, above the default `gc.auto` threshold of 6,700 (git-config, `gc.auto`,
https://git-scm.com/docs/git-config, read from the documentation, not re-fetched today). `61b82cd32` sets `gc.auto 0`,
counts loose plus packed objects with `count-objects -v` (it still fails with the old helper swapped in, per its
message), and makes the two narrowing tests print `copy`, `read_graph` and per-edit durations.

**Still open.** The copy writes every checked-out file once (`checkout-index --all`), and file creation is what is slow
on the Windows host. It lives in one test file, so the next test that needs a copy will write its own. The other live
tests (`live_plan_...`, `live_range_plans_...`, `live_ci_jobs_table_...`) print no phases. Graph reads are once per test
process at best, and nextest runs a process per test (https://nexte.st/docs/design/why-process-per-test/, read, not
re-fetched; CI runs nextest `--profile ci`), so a per-binary `OnceLock` shares nothing in CI.

**Other sites** (`git grep`, revision 2). Rust tests that name the real root use `repo_root()`, `CARGO_MANIFEST_DIR`
with `.join("../..")` (`ipc_endpoints.rs:15`) or `CARGO_MANIFEST_DIR` with `.ancestors().nth(2)` (`golden.rs:47`,
`tree.rs:15`, `dev_tool_workflow.rs:11`, `wrapper.rs:8`, `contracts.rs:12`). Of these files only `golden.rs` writes a
copy of the real tree; `ipc_endpoints.rs` and `wrapper.rs` run git writes on fixture repositories. `git add -A|--all|.`
in tests: 14 sites, all on small fixtures except the old copy. `.mjs` tests: 12 name the real root (`PADDOCK`,
`conformance/test/helpers.mjs:66`); 3 of them run git writes (`ci-actions`, `first-release-notes`, `kept-clone`), all on
fixture repositories; none copies the real tree. CI: `.gitea/actions/stable-workspace/action.yml:98`'s Windows fallback
`clone_checkout` makes a non-sparse clone and checks it out whole the first time (later runs write only the diff); used
by Debrief's `tauri-smoke` and `bundle` matrices (`debrief.yml:639`, `:900`); not measured.

## Pattern

**A test or CI step pays a cost proportional to the whole repository (write every file, hash every file, read every
workspace's graph) once per test or per sample, and the budget hides it until the slowest host is shared.** Each
instance was found by a timeout and fixed by hand in its own file.

**Invariant.** A test that needs a writable copy of the real repository gets it from one helper; whole-repository
reads that do not change during a run are made once per run, with the code under test still parsing them; every such
test prints its phases with durations; and a check refuses a whole-tree write of the real repository that does not go
through the helper.

## Decision

1. **A test-support crate `paddock-testkit-repo`** (D1), a dev-dependency only. Not a feature of `paddock-testkit`:
   `paddock-proc` and `paddock-slots` dev-depend on the test kit (`crates/paddock-proc/Cargo.toml:25`,
   `crates/paddock-slots/Cargo.toml:15`), so the kit cannot depend on them. It depends on `paddock-testkit` (ADR 0078's
   `ScratchDir`) and `paddock-proc`:
   - `RepoCopy::of(src).build_in(&scratch) -> RepoCopy { root, phases }`: `61b82cd32`'s mechanism (alternates, index
     entries from `ls-files --stage` filtered to regular files on disk, `checkout-index`, edited files hashed, LF
     checkout, `commit --no-verify`, `gc.auto 0`). Git runs through `paddock_proc` with a timeout; each step is a phase
     (`copy.index`, `copy.checkout`, `copy.edited`, `copy.commit`) with duration, file count and exit status (ER-27).
   - Its mechanism test counts the copy's own objects with `count-objects -v` against folders + 1 + edited files, with a
     planted full `add -A` copy as the positive control that must fail (ER-33).
2. **Once per run, across processes** (D2): `repo::once(key, build)` makes a read-only artifact under a reserved
   `<run root>/.cache/` (the 0078 amendment). `<key>` is the source's HEAD tree, index checksum and the content hash of
   edited files.
   - Locking: a separate `<key>.lock` file, never the artifact (on Windows `LockFileEx` blocks reads of the locked
     range), locked with `std::fs::File::lock`/`try_lock` (stable since 1.89.0: `#[stable(feature = "file_lock", since =
     "1.89.0")]` in the toolchain's `std/src/fs.rs:858`; `paddock-slots` is a non-blocking pool, the wrong shape). The
     first test builds into `<key>.tmp-<pid>`, renames it to `<key>` and releases; a waiter polls `try_lock` through
     `paddock_testkit::wait` with a bound, logs its wait as a phase, and on the bound fails with a named error. A
     waiter's budget is its own test's: D7's measured budgets include one build plus one wait.
   - Artifacts: the raw `cargo metadata --no-deps` JSON of every workspace, served through `paddock-repo-graph`'s
     existing `CargoMetadata` port (`crates/paddock-repo-graph/src/cargo.rs:6`), so the Rust reader under test still
     parses it; only the process runs once. A writable copy is never shared: each test that edits builds its own.
   - Which tests keep the real read: `paddock-repo-graph`'s adapter tests on fixtures (the runner at `cargo.rs:83`), the
     cache's builder (one real run per workspace per run), and the oracle `affected.mjs` (it is the other implementation
     and must read for itself; `b804ac78a` already makes that once per test). The contract, tree, inputs and golden Rust
     sides read the cache; the `repo-graph` test group goes when they do.
3. **Phases in every live test** (D4): the two narrowing tests already print `copy`, `read_graph` and per-edit
   (`61b82cd32`); every other live golden test prints `read_graph`, `oracle` and per-sample durations (the
   `success-output = "final"` override shows them in the job log).
4. **No smaller copies for comparison tests** (D3). A golden test compares two implementations on the same input; a
   copy with fewer paths makes both agree while comparing less, and "the golden cases still pass" proves nothing
   (ER-33). Copies stay whole. A later ADR may add cones only with an assertion that the cone copy's workspace and
   project set equals the full tree's (from the cached graph).
5. **The check** (D5): `./dev check whole-tree` in the RT-5 host, matching calls, not words in a file:
   - **Tests** (Rust by a syn pass, `.mjs`/`.ts` by their parser): a git call whose subcommand writes the whole tree
     (`add -A|--all|.`, `reset --hard`, `checkout -f` or `switch`/`restore` without paths, `checkout-index --all`,
     `read-tree -u`, `clone` without `--no-checkout` or `--sparse`, `worktree add`) or a recursive copy (`cpSync` with
     `recursive`, `cp -r`, `rsync`, a recursive copy function), whose directory or source argument, following `let`
     bindings in the function and one call into a same-file function's parameter, is a real-root expression
     (`repo_root()`, `CARGO_MANIFEST_DIR` with `..` or `.ancestors().nth(n)`, `PADDOCK`, `--show-toplevel`). It is an
     error unless the call is inside `paddock-testkit-repo`. Fixture repositories in temp folders are not flagged.
   - **CI** (`.gitea/`, `conformance/ci/`, `apps/*/scripts/ci/`): `actions/checkout` without `sparse-checkout` on a host
     leg, `git clone` without `--no-checkout`, `--sparse` or `--filter`, and `reset --hard` or `checkout -f` into a
     clone created in the same script before its sparse setup. A `checkout -f` in an existing worktree writes only the
     diff and is not flagged. A marked line (`# whole-tree: <why>`) passes; `stable-workspace` is measured (WP-0) and
     either marked or moved to kept-clone.
   - Positive controls: one planted site of each kind must be reported, and a planted fixture-repo `add -A` must not.
6. **kept-clone keeps its own code** (D6): it fetches a commit from the remote into a slot's clone, not a copy of a
   local checkout; the CI half of §5 and its run-7808 test cover it.
7. **Budgets shrink after measurement** (D7): after WP-3, at least five Windows host runs **with another job on the
   host** (the condition of runs 7879 and 7893) are read; the golden waiver drops to the worst of them plus 50 %, or is
   removed if the default 60 s holds (ER-12).

## Parallelism and cost (rule 51)

Cheaper first: no hashing (done in the fix), no rewrite, graph processes once per run instead of once per test (§2).
Parallel second: tests stay parallel; the once-lock serialises only the first build of a shared artifact, and cargo's
package-cache lock is taken once per workspace per run instead of once per test.

## Alternatives

- **Raise the budget.** ER-12.
- **`OnceLock` per test binary.** No effect under nextest's process per test; kept for in-process state only.
- **A nextest setup script** that builds the cache before the tests. The right shape (once per run, before the tests
  that need it), but the feature is experimental (`experimental = ["setup-scripts"]`) and ADR 0071 §6 decided against it
  on 2026-11-09 (<https://nexte.st/docs/configuration/setup-scripts/>, read there); it also does not run under
  `cargo test`, so the lock-based once is needed anyway. Revisit when stable: the script would then only warm the same
  cache.
- **Merge the live tests into one test.** One red result hides the others, and the budget grows with every case.
- **Smaller (cone) copies.** See §4.
- **`git worktree add` from the source.** Writes a worktree entry into the real repository (ER-14) and still writes
  every file. **`git clone --shared`** fails in CI's depth-1, blob-less, sparse clone (`golden.rs:602`). **Hard links or
  reflinks**: an edit through a hard link edits the source; reflinks are not on NTFS.

## Consequences

- One helper to fix when the next copy problem appears; a phase line per step in every live test; a check that sends
  new whole-tree writes to the helper.
- A new test-support crate (a dev-dependency only).
- The run root gains a reserved `.cache/` (the 0078 amendment).

## Rollback

Tests call their own copy code and run `cargo metadata` themselves again; the check leaves the RT-5 registry; the
amendment is withdrawn, after which a `.cache/` left in a run root is reported by 0078's leak check like any leftover
and removed with the root by the exclusive-lock holder.

## Review answers (revision 2)

- **P1-1 ADR 0078 conflict.** The leak check would report the cache in every job, and the run root is not built. The
  run root is now a named prerequisite; `<run root>/.cache/` is reserved, exempt from the leak check and removed only
  by the exclusive-lock holder (0078's sweep), recorded as `docs/adr/amendments/0078-2026-11-10.md`. Rollback fixed.
- **P1-2 Cones.** Dropped (§4, D3): a comparison test on less input compares less, and passing golden cases prove
  nothing; a later cone needs a set-equality assertion.
- **P2s.** Incidents: 7810 green at 195 s, 7819 red, 7787/7788 with `54f8ab5ce`, the count restated. `repo::once`: temp
  name then rename, a separate `.lock`, `File::lock` instead of `paddock-slots`, waiter budgets inside D7. A separate
  crate instead of a kit feature (the dev-dependency cycle). Which tests keep the real read (§2). The check matches
  calls, adds the `ancestors().nth(2)` form, clone flags, `switch`/`restore`, `cp -r`/`rsync`, `actions/checkout`
  without sparse, and exempts `checkout -f` in an existing worktree. nextest setup scripts evaluated. D7 measures
  several shared-host runs. Prerequisites stated. Estimate +1.5 days. `.mjs` sites counted.

## Decisions for the user

| # | Decision | Options | Recommended |
|---|---|---|---|
| D1 | Where the helper lives | (a) a test-support crate `paddock-testkit-repo` on `paddock-testkit` and `paddock-proc`; (b) a `git` feature of `paddock-testkit` (a dependency cycle today); (c) private to `paddock-dev`'s tests | **(a)** |
| D2 | Shared setup | (a) a cross-process once per run in `<run root>/.cache/` (needs the 0078 amendment), raw `cargo metadata` through the `CargoMetadata` port, a writable copy per test; (b) `OnceLock` per binary; (c) a nextest setup script | **(a)**: the only one that shares under nextest today |
| D3 | Cone copies | (a) none in this ADR; (b) cones with a workspace-and-project set-equality assertion | **(a)** |
| D4 | Phase output | (a) every live test and the helper; (b) only on failure | **(a)** (ER-27) |
| D5 | The check | (a) `./dev check whole-tree`, call-matching over tests and CI with positive controls; (b) review only | **(a)** |
| D6 | kept-clone | (a) keeps its own code under the CI half of the check; `stable-workspace` measured and marked or moved; (b) a shared library for both | **(a)** |
| D7 | Budgets | (a) shrink to the worst of five or more shared-host Windows runs plus 50 %, or remove; drop `repo-graph` once its tests read the cache; (b) keep both | **(a)** |
| D8 | ADR 0078 amendment | (a) reserve `<run root>/.cache/` as stated; (b) keep the cache outside the run root, with its own sweep and leak rules | **(a)**: one root, one sweep, one leak check |
| D9 | Tag gating | (a) none: test and CI code only | **(a)** |

## Work packages (raw agent-days, one agent)

Prerequisites outside this ADR: ADR 0078 WP-1 (run root, `.lock`, `ScratchDir`) and WP-3 (leak check), and the RT-5 host.

| WP | Content | Days |
|---|---|---|
| WP-0 | Measure: phase durations of the two narrowing tests on the Windows host once the fix is on main; `stable-workspace`'s `clone_checkout` duration from Debrief job logs | 0.5 |
| WP-1 | `paddock-testkit-repo` (new crate per the new-crate skill), `RepoCopy` with phases through `paddock_proc`, the `count-objects` mechanism test with its planted control, tests on Linux, macOS and Windows | 1.75 |
| WP-2 | `repo::once` (key, `.lock`, temp then rename, bounded wait), the recorded `CargoMetadata` adapter; the 0078 leak-check exemption for `.cache/` with its own positive control (a leftover outside `.cache/` is still reported) | 1.25 |
| WP-3 | `golden.rs`, contracts, tree and inputs tests onto the helper and the cache; phases in every live test; drop `repo-graph`; read five or more shared-host Windows runs and shrink the waiver (D7) | 1 |
| WP-4 | `./dev check whole-tree` (syn and JS call matching with one level of same-file calls, the CI half), positive and negative controls | 1.5 |
| WP-5 | Lesson, rule text (proposed: "a test that needs a copy of the real repository takes it from `paddock-testkit-repo`"), ADR index | 0.25 |

**6.25 days**; first cut (WP-0, WP-1, WP-2 and the golden part of WP-3) 4 days, after the prerequisites. **Gates no tag.**
