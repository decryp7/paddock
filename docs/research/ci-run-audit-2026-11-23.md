# Why the monorepo's CI runs everything: the planner and the graph (2026-11-23)

- Asked by the user, 2026-11-23: "please optimize CI runs. runs only those necessary. It keep running all steps over and over
  again which does not make sense", then: "when using a monorepo, it tends to build everything; if an app is in a repo by
  itself, this problem will not happen. We need to find out how to fix this for monorepo", and "focus 100% on the root cause,
  the graph". This note is the input for the graph ADR (`docs/adr-repo-graph-model`, being drafted).
- Scope (narrowed by the coordinator): where the planner's rules mark more than the dependency graph would, with trains 19 and 20
  as examples; what `crates/paddock-repo-graph` covers and misses; how Nx, Bazel, Turborepo, Pants and moon model it. Job overhead,
  job consolidation and reruns across rounds are left out on purpose; the measurements below only size the problem.
- Not duplicated here (other branches): (a) `fix/plan-actions-readme-not-global` (`.gitea/actions/README.md` on the global list),
  (b) `feat/infra-only-smoke` (CI-plumbing-only changes run a smoke set), (c) `docs/adr-test-result-cache` (test results cached by
  input hash), and `feat/os-scoped-tests` (`c61339b9f`, built, not landed: OS-independent tests on Linux only).
- Evidence heads (ADR 0091), read 2026-11-23 and re-read before the commit: `origin/main` = `b3c254a2d`,
  `origin/train/2026-11-22-20` = `803aed12d` (this branch's base), `origin/fix/run-scope-reuse-p2` = `bab7a4a97` (ADR 0113),
  `origin/feat/os-scoped-tests` = `c61339b9f`, `origin/ci/staged-fast-then-long` = `13d02a1ca`. Nothing here claims shipped code:
  no tag contains train 19's base `67ac6d6ec` (`git tag --contains` is empty).
- Method. Planner answers: `./dev plan <base> <head> --explain` run read-only in this worktree for train 19
  (`67ac6d6ec..b3c254a2d`), train 20 (`b3c254a2d..803aed12d`) and train 20's round-to-round diffs (`0ae8b4794..fe097b7a4`,
  `fe097b7a4..803aed12d`); every `why` entry grouped by cause. Code: `tools/paddock-dev/src/plan.rs`, `plan_ci.rs`,
  `plan_gitea.rs` and `crates/paddock-repo-graph/src/graph.rs` at `803aed12d`. Run cost: Gitea API job and step times and the job
  logs (each cut at its `::group::` markers) of the runs named below. Numbers are in
  [benchmarks.md](../benchmarks.md#ci-rounds-of-trains-19-and-20-by-runner-class-2026-11-23-docsci-run-audit).

## 1. The answer in short

A split repository gets "only the app that changed" for free: its CI only sees its own files. Our monorepo has a real dependency
graph (`paddock-repo-graph`: Cargo packages and npm workspaces), and for source files it is precise enough. But six rules around
it, plus one choice of base, override it, and in both measured trains they turned a narrow change into "every project, every OS":

1. **Train landing runs have no base, so they plan everything by definition.** A train is dispatched without `base`
   (`plan_ci::choose_base`: "a manual run without a base: everything"), because ADR 0113 counts only a whole-tree run with WebKit as
   *complete*, which the push to main then reuses (ER-49). The graph is not consulted at all on the run that decides the landing.
   Runs 8610, 8635 and 8647 all printed `base: a manual run without a base: everything`.
2. **Whole-file globals** (`GLOBAL` in `plan.rs`): `.gitattributes` marked all 18 projects in train 19 for a comment and a
   `merge=` driver on `docs/lessons.md`, which no build or test reads.
3. **Unknown root files mark everything**: `spikes/dialog-hidden/Cargo.lock` (a url bump in a spike nobody builds) marked all 18.
4. **CI plumbing propagates to every job that uses it, as "all" kinds**: a shared action or a `conformance/ci/*.sh` script used by
   every job marks every project, with Rust and UI halves, on every OS (16 of 18 projects in both trains). (a) and (b) address
   parts of this; the rule shape stays.
5. **A mention is treated as a use**: the plan job's "C100 stays wired" grep names `.gitea/workflows/nightly.yml`, so the planner
   sees nightly.yml as "used by ci.yml, job plan", and a plan-job input marks everything (all 18 in train 20, which only added a
   step to nightly.yml).
6. **Any `conformance/` file marks every app's whole gates** (kind `all`: Rust and UI, three OS), because every app's gates run
   `paddock-conformance`. A new conformance check (C156) in train 20 re-ran every app's Rust tests on macOS and Windows.
7. **The unit of execution is the app or workspace, not the package or test.** The graph knows the changed packages; the run
   still tests the whole workspace on three OS on every full and landing run (the package filter is used only on member runs),
   and an e2e suite runs whole for any change that reaches its app.

A train round costs about 700 to 730 job-minutes (train 19: 731; train 20 round 1: 704), of which about 410 to 425 on the six
Linux lanes, 100 to 125 on the one Mac and 180 to 195 on the one Windows runner. Both trains planned "everything" by every rule
above, so the planner's precision decided nothing.

## 2. How large monorepos avoid building everything

### 2.1 What the comparable tools do

| Tool | Graph unit | What decides "affected" | How CI config and shared files are kept from marking everything | Result cache |
|---|---|---|---|---|
| **Bazel / Buck2** (Google, Meta) | target (a library, a binary, one test target) | the changed files' owning targets and their reverse dependencies (`rdeps`); Chromium's CQ runs `gn analyze` on the changed files to get the affected targets per builder | inputs are declared per target and actions run hermetically, so a file no target declares reaches nothing; CI config is not a build input at all | remote action cache; passing test results are cached by input digest and shown as `(cached)` (`--cache_test_results=auto`) |
| **Nx** | project, and since the task-based model a *task* (`project:target`) | "Nx selects a task when your change touches one of its inputs" (Nx 24; project-level selection only with `affected.legacy`) | `namedInputs` per task (`default`, `production`, `sharedGlobals`), negated globs exclude config and test files; a lockfile change affects every task by default, narrowed with `projectsAffectedByDependencyUpdates: "auto"` to projects whose resolved dependencies changed; the old global `implicitDependencies` are deprecated in favour of inputs | local and remote (Nx Cloud) task cache |
| **Turborepo** | package, optionally task | `--affected` (package level), or with `affectedUsingTaskInputs` only tasks whose `inputs` match the changed files | `globalDependencies` are explicit and every match busts every task, so the docs keep the list short; root `turbo.json`, `package.json` and the lockfile still select everything | local and remote task cache |
| **Pants** | target (file-level for Python, Go, JVM) | `--changed-since=<ref> --changed-dependents=transitive` | dependency inference from imports; config files are only inputs of the targets that declare them | local and remote cache, test results included |
| **moon** | project and task | `--affected` from the project and task graph; task `inputs` decide | explicit task inputs; global config, lockfile and toolchain changes affect every project | hashed task cache |
| **GitHub / GitLab path filters** (what a split repo gets) | workflow or job | `on.push.paths`, `rules:changes` | each repository only sees its own files; CI config is per repo | none built in |

Notes and caveats. A GitHub workflow skipped by a path filter leaves a required check pending and blocks the merge; a job skipped
by a condition reports success. GitHub's advice is to avoid requiring workflows that can be skipped, so monorepos on GitHub decide
inside a job (a "changes" job), which is what our `plan` job is. GitLab's `rules:changes:compare_to` in merged-results pipelines can
fire on changes of the target branch, because the temporary merged commit is the comparison base. GitLab merge trains and bors run
the pipeline on the merged result, and Rust's bors batches PRs into rollups "to reduce CI costs and merge delays"; neither changes
*what* runs, they change how often. Playwright 1.46+ has `--only-changed=<ref>`, which follows a test file's imports, but not an
app source change behind a page.

### 2.2 Our planner against them

| Dimension | Bazel / Pants | Nx / Turborepo / moon | Paddock (`./dev plan` at `803aed12d`) |
|---|---|---|---|
| Graph unit | target, file | task (project + target) | Cargo **package** folder and npm **workspace** folder (`paddock-repo-graph` `Graph::node_of`, `dependents`); then collapsed to a **project** (an app, `crates`, `ui`, ...) with a kind (`rust`, `ui`) |
| Execution unit | target, test target | task | the whole workspace per OS (`app <app> (os)`, `crate (os)`, `paddock-dev (os)`), the whole e2e suite per app and engine; nextest package filter only on member runs (`crates_filter`, `plan_ci::member_run`) |
| Global inputs | none needed (declared inputs) | explicit, short, per task where possible | `GLOBAL` regexes (toolchain, root `package.json`, `.npmrc`, `.nvmrc`, `.gitattributes`, `tsconfig/`) whole-file, plus "unknown root file", plus a changed `ci.yml` header or plan job |
| CI config | not a build input | an input only of the tasks that list it | a line-based job split of each workflow (`plan_gitea`): a changed shared action or script marks every job that uses it, and its projects with kind `all` |
| Lockfile | per external dependency target | all tasks, or only projects whose resolved deps changed (`auto`) | `Cargo.lock`: the workspace's packages whose resolved dependencies changed (lock diff, precise); `package-lock.json`: `lock_diff` per workspace (precise) |
| Test level | per test target | per task; test files via inputs | none: no test-file to import map; `--since-run` reruns failed tests only |
| Result cache | yes, remote, tests included | yes, remote | compile cache (sccache) only; no task or test cache ((c) proposes one) |
| Base of a landing run | the merge-group commit's diff, cached results reused | `--base` = target branch | **none**: a train dispatch has no base and plans everything |

### 2.3 Where `paddock-repo-graph` is precise enough, where the rules override it, where it needs a finer level

- **Precise enough.** `cargo_graph` (from `cargo metadata`) and `npm_graph` give package-level nodes and edges; `dependents` is the
  reverse closure; `lock_diff` names the npm workspaces whose resolved packages differ; the Cargo lock diff names the packages
  whose resolved dependencies changed. For source edits this is the Pants/Nx level. Example: in train 20's second round
  (`0ae8b4794..fe097b7a4`, Debrief e2e, the e2e helper and a conformance check) the graph part marked no crate at all; the
  `crates` project stayed off.
- **Overridden by the rules around it** (all in `tools/paddock-dev/src/plan.rs` and `plan_gitea.rs`, not in the crate):
  `GLOBAL` and "unknown root file" (`mark_all`), the shared-action and script chains (`gitea_targets` and `kind(..., "all")`), the
  plan-job rule (`CI_JOBS`: `("plan", &["everything"])`), the conformance rule (`CONFORMANCE_CODE` and the `conformance/` loop,
  each `kind(..., "all")`), the npm rule that a dependent ending in `src-tauri` gets kind `all`, and the base choice in
  `plan_ci::choose_base`. Section 3 lists each with its example.
- **Needs a finer level.** (1) Execution per package and per OS: the graph's package set is thrown away at the project step on
  full and landing runs. (2) Test files to imports: e2e specs and Vitest specs have no node of their own, so any change reaching
  an app runs its whole e2e suite in both engines (Debrief: 703 Chromium and about 692 WebKit tests per run, 4 shards). (3) Data
  files read by tests (goldens, fixtures, conformance inputs) are attached by hand-written rules (`C99_INPUT`, `C100_INPUTS`,
  `E2E_GUARD_INPUTS`), the class Bazel solves with declared `data` deps and Chromium's `gn analyze` documents as its known gap.

## 3. Gap list: where the planner marks more than the graph would

Each row: the rule, its code, the train example from `--explain`, what the graph alone would mark, and the fix shape. "18" means
all projects (9 apps and 9 non-app projects).

| # | Rule (code at `803aed12d`) | Example | Marked | Graph alone would mark | Fix shape (common practice) |
|---|---|---|---|---|---|
| G1 | No base on a train dispatch: `plan_ci::choose_base`, `workflow_dispatch` with empty `base` → everything; ADR 0113 `need` = whole tree for every dispatch | runs 8610, 8635, 8647: `base: a manual run without a base: everything` | 18, all OS, WebKit on | the train's diff against main (the same plan the push to main computes from the last green main) | dispatch the train with `base` = main's tested head; ADR 0113's `need` for that run = the push's own plan (the shape ADR 0113 already uses for a push to main). GitHub merge queue, GitLab merge trains and Nx `--base` test the merged result's diff. |
| G2 | `GLOBAL` whole-file: `^\.gitattributes$` | train 19: `/docs/lessons.md merge=union` → `merge=lessons` plus a comment | 18 | conformance (it reads attributes), nothing else | diff the attribute lines: only `text`, `eol`, `binary`, `filter`, `diff` changes on paths a build reads mark their owners; `merge=` and comments mark nothing. Nx/Turborepo keep globals short and explicit. |
| G3 | Unknown root file → `mark_all` (`plan.rs` "unknown root file") | train 19: `spikes/dialog-hidden/Cargo.lock` | 18 | nothing (no job builds `spikes/`) | declare `spikes/` (and every root folder) with an owner or as not built; an undeclared path fails the plan's own test instead of planning everything. Bazel: an undeclared file reaches nothing. |
| G4 | Plan-job rule: `CI_JOBS` `("plan", &["everything"])` plus "a file named in a job is used by it" (`plan_gitea`) | train 20: one added step in `nightly.yml`; the plan job's "C100 stays wired" grep names nightly.yml → "`.gitea/workflows/nightly.yml` used by ci.yml, job plan" | 18 | nightly.yml's own users: nightly (not run on a train), conformance (C100) | treat a path inside a shell string as data, not as a use; move the "C100 stays wired" grep out of the plan job into the conformance job; mark only the jobs whose `uses:`/script imports change. |
| G5 | `ci.yml` header or plan job changed → global (`global .gitea/workflows/ci.yml`) | train 19: `ci.yml` +37 lines (plan step and env); fe097b7a4..803aed12d: the revert of a member that had edited ci.yml | 18 | the jobs whose text changed, and the plan job's outputs only if their computation changed | split the plan job's inputs: env and step text that feed `./dev plan --ci` mark paddock-dev's planner tests and the plan's golden; a changed output name marks the jobs that read it. |
| G6 | Shared CI plumbing chains: a changed shared action or `conformance/ci/*.sh` marks every job that uses it (`gitea_targets`), with `kind(..., "all")` for app legs | train 19: `setup-rust`, `app-gates`, `dev-tool-cache.sh`, `kit.sh` (16 projects each); train 20: `setup-rust`, `build-jobs`, `dev-tool-cache`, `node-caches`, `cache-report.sh` (13 to 16 projects) | 16 of 18, Rust and UI, 3 OS | the jobs that run the changed step, and only one smoke per OS for a setup-only change | (b) gives setup-only changes a smoke set; the general rule: a plumbing change marks the *steps* that changed (setup vs test), not the tests behind them. Nx/Turborepo: CI files are not task inputs; Bazel: not build inputs. |
| G7 | `.gitea/actions/README.md` on the global list | both trains | 18 | nothing (prose) | (a), in flight. |
| G8 | Any `conformance/` file (not `test/`, `fixtures/`) → every app, kind `all`; `CONFORMANCE_CODE` → every app with conformance checks | train 20 round 2: `conformance/src/checks/C156-axe-scan-held-still.mjs`, `index.mjs`, `paddock.ci.json`, `reserved/C156-*` | 9 apps, Rust and UI, 3 OS | the conformance job, and each app's conformance step (Linux, static) | a conformance change marks a `conformance` step of each app leg (or the one conformance job), not the app's tests; a reservation file (`conformance/reserved/*`) marks nothing. |
| G9 | npm dependent ending in `src-tauri` → kind `all` (`plan.rs`, the npm loop) | train 20: `ui/projects/paddock-ui/testing/src/until.ts` → `npm apps/shell-demo/src-tauri`, `npm apps/workbench-demo/src-tauri` | 2 demo apps, Rust and UI | the demos' UI half | kind `ui` for an npm edge; a Rust half only from a Cargo edge or the Tauri config. |
| G10 | `E2E_GUARD_INPUTS` (`paddock-e2e/src/env.ts`, `build-guard.ts`) → the UI half of every app | train 20: `e2e guard ui/projects/paddock-e2e/src/env.ts` | 9 apps' UI and e2e | the same (every app's Playwright config imports it) | correct as a rule; finer only with a test-file import map (G12). |
| G11 | Execution per workspace, not per package: the package filter (`crates_filter`, `paddock_dev_filter`) only on member runs (`plan_ci::member_run`) | train 19: `crates/paddock-watch` and `crates/paddock-proc` changed → the whole `crate (os)` legs ran (run 8610) | every package of the workspace on 3 OS | `dependents` of `paddock-watch` and `paddock-proc` | run the reached packages (`-p`) on every run kind, as ADR 0080 D5 chose ("package level"); full workspace only on nightly and release gates. With feat/os-scoped-tests the macOS and Windows legs already narrow to `hostTests`. |
| G12 | No test-level graph: e2e and Vitest suites run whole; no test file to import map | train 20: `apps/debrief/app/e2e/a11y.e2e.ts`, `app.ts`, `axe.ts`, `pipeline.e2e.ts` → all of Debrief's 4 shards, both engines (run 8636) | ~1,400 Debrief e2e tests | specs importing the changed helpers, plus every spec when app source changed | test-file nodes from imports (what Playwright `--only-changed` and Pants' inference do) for test-only changes; whole suite when app source changed. |
| G13 | Docs inputs: `docs/lessons.md` → `conformance`, `tools` | both trains | 2 | the same if a check reads it (C145 lessons check) | correct as declared; listed only to show docs are already narrowed. |
| G14 | The base of a push to main = the last green **push** run (`choose_base`), so a red push widens the next push's plan back to that run; after G1 is fixed the train run must count as the main base | run 8626 (push of b3c254a2d after red 8610): reuse refused, a full rerun | 18 | the push's diff from main's tested head | let the base reader accept the train's complete run on the same tree (ADR 0113's verdict reader is the one place). |

What the graph alone would have marked, for the two trains:

- **Train 19** (`67ac6d6ec..b3c254a2d`): the url 2.5.9 bump (RUSTSEC-2026-0592) touched every app's and every tool's `Cargo.lock`,
  so the graph alone marks every Rust half anyway; the UI halves, `ui`, `icons`, `eslint` and WebKit were reached only through
  G1, G2, G3, G5 and G6. Train 19 is a fair "almost everything" train.
- **Train 20** (`b3c254a2d..803aed12d`): Debrief's UI and e2e, the e2e helper (`paddock-e2e`, `paddock-ui/testing`), conformance
  C154 and C156, paddock-dev, `tools/heavy`, CI plumbing. The graph and the declared rules mark Debrief (UI and e2e), the UI and
  e2e halves of the apps (G10), `ui`, conformance, paddock-dev and tools. G1, G4 to G8 and G9 added: `crates` (the `crates` job and
  three `crate (os)` legs), `icons`, `eslint`, and the Rust halves of every app on Linux, macOS and Windows. Measured on round 1
  (runs 8635 to 8640), those jobs cost about 22 job-min of crate legs, 10 of eslint, icons and audit, and 43 of app Rust legs on
  macOS and Windows; the Rust parts of the Linux app legs are not split out (**not measured**).

## 4. Monorepo or split: why fixing the selection beats splitting

ADR 0014 (accepted 2026-10-24) moved the apps into one repository so that a shared change and its adoption land in one commit
(AGENTS.md: "A shared change and its adoption by the apps land in one change; there is no publish-and-consume step"). Before it,
shared code reached an app by a tagged release (`publish.yml` to the Gitea registry), the release scripts existed in five copies
(X9), and a `canary.yml` tested consumers after the fact.

A split would get path filtering for free, but only for changes inside one app. For the changes that dominate our trains it would
not save runner time:

- A shared crate or `@decryp7/paddock-ui` change becomes publish, then a version bump PR in each of nine app repositories, then
  nine full CI runs. Train 19's url bump would be ten repositories with ten full runs, the same work plus the publish and bump
  steps.
- Version skew comes back: apps on different shared versions, the breaking-change rule (rule 39/42/43, "update every app in the
  same change") becomes a coordination problem, and a regression in shared code is found per app, later.
- CI plumbing (actions, `conformance/`, `./dev`) would be copied per repository or published as its own package; a change to it
  re-runs every repository anyway (the G6 class does not go away, it multiplies).
- Common practice for a family of apps sharing libraries (Google, Meta, Nx and Turborepo users) is one repository with a precise
  graph and a result cache, not a split.

So the gap is not the monorepo; it is that our selection stops at the project level and lets seven rules override the graph.

## 5. Recommended fixes for the graph ADR, ranked

Minutes are per train round, from the round-1 costs of train 20 (704 job-min) unless named; agent-days are raw estimates.

| Rank | Fix | Gaps | Saves | Agent-days | Common-practice option | Covered elsewhere |
|---|---|---|---|---|---|---|
| 1 | Train landing run planned from main's tested head (`base` = main), ADR 0113 `need` = the push's own plan for that run, the push to main reuses it | G1, G14 | everything outside the train's diff: on train 20 at least the ~75 job-min of section 3 plus every gap below becoming effective (none of G2 to G11 matters while G1 plans everything) | 1.5 to 2 | the same: merge queues and merge trains test the merged result with the same required checks | none |
| 2 | Plumbing marks steps, not tests; mentions are not uses; plan-job inputs split | G4, G5, G6 | train 20: the 16-to-18-project marks; with (b) a setup-only change costs a smoke set | 1.5 (on top of (b)) | Nx/Turborepo: CI files are not task inputs | (a) G7, (b) part of G6 |
| 3 | Conformance changes mark a conformance step, not app tests; npm edges keep kind `ui` | G8, G9 | train 20 round 2: the app Rust legs on 3 OS (about 43 job-min on macOS and Windows in round 1) | 1 | Nx named inputs per task | none |
| 4 | Attribute-aware `.gitattributes`, declared root folders (no "unknown root file") | G2, G3 | train 19: two of its four everything-reasons | 0.5 | Turborepo/Nx: short explicit globals | none |
| 5 | Package-level execution on every run kind (`-p` from `dependents`), whole workspace on nightly and release gates | G11 | the unreached packages of every workspace; not measured per package | 1.5 | Bazel `rdeps`, Pants `--changed-dependents`, Nx tasks; ADR 0080 D5 already chose package level | partly feat/os-scoped-tests (host legs only) |
| 6 | Test-file nodes for e2e and Vitest specs from imports | G12 | Debrief's 4 shards (about 102 job-min per round) on test-only changes | 2 | Playwright `--only-changed`, Pants inference | (c) caches results; this selects |
| 7 | Task and test result cache by input hash | all, across rounds | not measured here | see (c) | Bazel, Nx Cloud, Turborepo remote cache | (c) |

Common-practice baseline (ER-37): ranks 1 to 4 are what Nx, Turborepo, merge queues and Bazel users get by default, about 4.5 to
5 agent-days together; ranks 5 and 6 go towards Bazel/Pants granularity (3.5 days) and are the user's choice; rank 7 is (c).

## 6. Decisions for the user

- **D1. What the landing run tests.** (a) The train's diff from main's tested head, with the push to main reusing it
  (*recommended*, the merge-queue practice); nightly and release gates stay whole-tree and every OS. (b) Keep the whole tree on
  every landing run (today). Common practice: (a).
- **D2. Granularity target of the graph ADR.** (a) Package level for execution plus explicit, short globals (ranks 1 to 5;
  *recommended*, Nx/Turborepo practice, about 6 agent-days). (b) Add test-file nodes (rank 6, about 2 more days). Common practice:
  (a); (b) is Pants/Bazel level.
- **D3. Monorepo or split.** Keep the monorepo and fix the selection (*recommended*, and the common practice for libraries shared
  by several apps); splitting brings back publish-and-consume, version skew and copied CI plumbing (section 4).

## Sources

- Nx, "Run only tasks affected by a change": https://nx.dev/docs/features/ci-features/affected (task-based selection, lockfile and
  `projectsAffectedByDependencyUpdates`, read 2026-11-23)
- Nx, "Inputs and named inputs": https://nx.dev/reference/inputs; "Configure inputs": https://nx.dev/docs/guides/tasks--caching/configure-inputs;
  deprecated global implicit dependencies: https://nx.dev/docs/reference/deprecated/global-implicit-dependencies
- Turborepo configuration (`globalDependencies`, `inputs`, `--affected`, `affectedUsingTaskInputs`):
  https://turborepo.dev/docs/reference/configuration; constructing CI: https://v2-7-6.turborepo.dev/docs/crafting-your-repository/constructing-ci
- Pants `changed` subsystem: https://www.pantsbuild.org/dev/reference/subsystems/changed
- moon affected and cache: https://moonrepo.dev/docs/run-task, https://moonrepo.dev/docs/concepts/cache
- Bazel test caching (`--cache_test_results`, `(cached)`): secondary sources only, not verified in the official reference in this
  session: https://bazel.virtuslab.com/book/1~2~1/, https://blog.engflow.com/2023/09/18/bazel-testing-tips; official reference
  to check: https://bazel.build/reference/command-line-reference
- Chromium CQ analyze step: https://www.chromium.org/developers/testing/commit-queue/chromium_trybot-json and
  https://gn.googlesource.com/gn/+/main/src/gn/command_analyze.cc (older pages; current CQ use not verified)
- GitHub, troubleshooting required status checks (skipped workflows stay pending; skipped jobs report success):
  https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/troubleshooting-required-status-checks
- GitLab merge trains and merged-results pipelines: https://docs.gitlab.com/ci/pipelines/merge_trains/,
  https://docs.gitlab.com/ci/pipelines/merged_results_pipelines/
- Rust bors and rollups: https://rustc-dev-guide.rust-lang.org/tests/ci.html, https://forge.rust-lang.org/release/rollups.html
- Playwright `--only-changed` (1.46): https://playwright.dev/docs/test-cli
