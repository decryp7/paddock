# 0117. The repository graph is the one model of the repository, and every plan is a query on it

- Status: Proposed 2026-11-23. The direction is the user's (2026-11-23, through the coordinator: "fix the graph first, and hold all
  trains until it lands"); the decisions below (D1 to D10) are open.
- Date: 2026-11-23
- Related: ER-20 (bulk work, spawn budgets), ER-21 (a recurring class gets a redesign), ER-22 (share first; a value another
  module decides is read from it), ER-15 and ER-49 (a smoke or partial run is never complete; test a commit once), ER-37 (common
  practice and cost), ER-40 (generated files), ER-42 (read declared data through its accessor), ER-46 (lists only shrink); ADR 0022
  (paddock-dev, `paddock-repo-graph`), ADR 0038 (CI commands as data in `paddock.ci.json`), ADR 0060 (the CI model of `./dev gates`),
  ADR 0080 (CI speed), ADR 0113 and its amendment of 2026-11-23 (the declared run scope; plumbing-only and test-only runs),
  the 0116 drafts (`a test target whose inputs did not change reuses its pass`, worktree `adr-test-result-cache`; `a merge
  regenerates what it made stale`, local `feat/merge-regenerates`).
- Supersedes as a mechanism (not as behaviour): the planner rule tables of `tools/paddock-dev/src/plan.rs` and `plan_gitea.rs`,
  the ADR 0113 amendment's planner-rule design (its behaviours stay and become queries, §6), the hand lists `hostTests` (C155) and
  `docs/generators.toml` `paths`.
- Number: 0117. Searched 2026-11-23: `git ls-tree docs/adr/` over every `origin` branch and every local branch, and `docs/adr/` of
  every worktree under the worktree root. Highest found: 0115 on `origin`; 0116 twice, unpushed (`feat/merge-regenerates` and the
  `adr-test-result-cache` worktree, a clash between those two, not this one); no 0117.
- Evidence heads (ADR 0091), read 2026-11-23 and again before commit: `origin/main` = `b3c254a2d`, `origin/train/2026-11-22-20` =
  `803aed12d` (this branch's base), `origin/fix/run-scope-reuse-p2` = `bab7a4a97` (ADR 0113), `origin/feat/os-scoped-tests` =
  `c61339b9f` (`hostTests`, C155), `origin/feat/infra-only-smoke` = `29864b700` (the ADR 0113 amendment, docs only; its planner work
  stopped unbuilt).

## Context: the class

`paddock-repo-graph` (1,235 lines) knows Cargo and npm projects and their edges (path references in Tauri `frontendDist`,
`tsconfig`, `angular.json`, ESLint config). Everything else the planner knows is rules over file paths and text in about 6,300 lines
(`plan.rs` 1,992, `plan_since.rs` 1,812, `plan_ci.rs` 1,342, `plan_cmd.rs` 630, `plan_gitea.rs` 390, `plan_repo.rs` 160): a
`GLOBAL` regex list (`plan.rs:40`: `rust-toolchain`, the root `package.json`, `.npmrc`, `.nvmrc`, `.gitattributes`, `tsconfig/`),
about twenty more path regexes each with its own marks (`C99_INPUT`, `C100_INPUTS`, `PER_MACHINE_INPUT`, `NOTICES_INPUTS`,
`DOC_INPUTS`, `OWNERS`...), a line-based ci.yml splitter (`plan_gitea::workflow_jobs`) and a hand table of what each ci.yml job runs
for (`plan_gitea::CI_JOBS`). A second, better CI model already exists for another purpose: `gates/ci.rs` (ADR 0060, 1,708 lines,
`yaml-rust2`, workflows, jobs, steps, local actions, matrices, conditions). Two more hand lists follow the code by hand:
`hostTests` (which packages and modules run on Windows and macOS, checked by C155) and `docs/generators.toml` `paths`.

Each rule marks a project *whole*, with no notion of *how* a file reaches it, so a planner question that needs the kind of reach
(plumbing, test, release step, code) or a finer node (a job, a test file, an OS) gets a new rule, and the rules over-mark:

| Instance | What it did | Checked |
|---|---|---|
| A README in an action folder is a global | `.gitea/actions/README.md` is an "action nobody uses", so `gitea_targets` answers `None` and the planner marks everything | `./dev plan 298cc726b^ 298cc726b` (train 20's cache-step commit): `"all": true` |
| A shared action marks every job that uses it, whole | the same commit (four setup and cache actions): every app leg, crates coverage, three crate legs, UI specs, e2e shards, paddock-dev | the same plan's `why` (`build-jobs/action.yml used by ci.yml, job app`) |
| Installers rebuild on a cache step | the same plan marks `app:dyno` with `build-jobs ... used by dyno-release.yml`; the plan action asks `./dev plan --ci --app dyno`, which answers `true`, so `dyno-release.yml` builds installers | `.gitea/actions/plan/action.yml` (`run=$(... plan --ci --app "$APP")`), `plan.rs` `affected` |
| A test-only change runs the whole suite | `apps/dyno/app/src/app/x.spec.ts` marks `app:dyno` (kind `ui`): every UI and e2e command | `plan.rs` test `conformance_is_in_every_plan`, `owners()` |
| Foundation changes run app jobs | a change in `conformance/src` or `conformance/bin` marks every app ("its checks run paddock-conformance") | `plan.rs` `CONFORMANCE_CODE` and the `conformance/` loop |
| OS placement is a hand list | `hostTests` in each `paddock.ci.json` (about 75 paddock-dev module prefixes, `only.macos-latest` about 22), C155 compares it with OS cfgs and OS-API sites; train 20's `tests/it/lessons_merge.rs` (`#![cfg(unix)]`) already misses it | `origin/feat/os-scoped-tests` `c61339b9f`; the coordinator's report from the paused merge |
| Generator inputs are a hand list | `docs/generators.toml` `cargo-lock-crates` lists `crates/**/Cargo.toml` by hand; `Registry::due` matches a diff against it | `docs/generators.toml:40` on this base |

The ADR 0113 amendment of 2026-11-23 started to fix four of these as four more planner rules. The user stopped that the same day:
the class is "a planner fact that is a rule over paths instead of a node or an edge of the graph", and every new rule is another
instance. Seen seven times above (ER-21).

## Decision

### 1. One model, one crate

`paddock-repo-graph` becomes the model of the repository: typed nodes, typed edges and declared inputs, built once per run from
the tracked files and a few bulk reads, and queried by everything that plans: `./dev plan` (all modes), the run scope (ADR 0113),
the test-result cache (0116 draft), generators (`due`, stale after a merge), C155 and the gates model. The planner keeps only the
CI-specific *output* (matrix legs, job outputs, summaries); it decides nothing a query can answer. The crate stays pure (ports for
files, git history and cargo metadata, as today); extractors that read a format live in it as modules (`imports` for TS/JS already
drafted, `cargo`, `npm`, `ci` moved from `gates/ci.rs`, `os`).

### 2. Node kinds

| Kind | Id | Built from |
|---|---|---|
| `project` | `crate:<dir>`, `npm:<dir>`, `app:<name>`, root projects (`ui`, `crates`, `conformance`, `tools`, `icons`, `eslint`, `paddock-dev`) | Cargo metadata, package.json workspaces, `paddock.ci.json` |
| `workflow`, `job`, `step` | `wf:ci.yml`, `job:ci.yml#crates`, `step:ci.yml#crates/3` | the ADR 0060 reader (`gates/ci.rs`), moved into the crate |
| `action` | `action:setup-rust` | `.gitea/actions/*/action.yml`, same reader |
| `script` | `script:conformance/ci/cache-report.sh` | `conformance/{ci,release,src/util}`, `scripts/` |
| `test-file` | `test:apps/dyno/app/e2e/flows.spec.ts` | the project's declared test globs (§3) |
| `test-target` | `nextest:<pkg>::<bin>`, `vitest:<project>`, `playwright:<project>/<engine>` | Cargo targets, the contract's runner declarations |
| `generator` | `gen:cargo-lock-crates` | `docs/generators.toml` (command, outputs; inputs become edges, §4) |
| `doc` | `doc:<path>` | everything matched by the docs input (§3) that no other node owns |

Every tracked file belongs to exactly one *owner* node (the deepest that claims it); a file no node claims is an error in the
graph's own check, not a silent global.

### 3. Declared inputs, replacing `GLOBAL` and the path regexes

One file, `docs/repo-graph.toml` (schema documented in the file, read by one accessor, ER-42), merged over defaults built into the
crate, in the shape of Nx `namedInputs` and Turborepo `globalDependencies`:

```toml
[inputs.rust-toolchain]            # a named input: a file set and the node kinds it reaches
files = ["rust-toolchain.toml", ".cargo/**", "lint/**"]
reaches = ["kind:project/cargo", "job:ci.yml#crate", "job:ci.yml#paddock-dev"]
reach = "code"                      # code | plumbing | test | release | docs (§5)

[inputs.npm-root]
files = ["package.json", ".npmrc", ".nvmrc", "tsconfig/**"]
reaches = ["kind:project/npm"]
reach = "code"

[inputs.git-attributes]
files = [".gitattributes"]
reaches = ["kind:generator", "conformance"]   # it changes line ends, not builds: see D4
reach = "code"
```

A project contract (`paddock.ci.json`) declares what only the project knows: `tests` (test-file globs per runner), `smoke` (the
smoke set per phase and a nextest filterset), and OS overrides with a reason (§7). The current regex tables move into this file
one by one, each with the golden cases that cover it (§8).

### 4. Edges

| Edge | From → to | Extractor |
|---|---|---|
| `depends` | crate → crate, package → package | Cargo metadata, npm workspaces (today) |
| `path-ref` | project → project | `frontendDist`, `tsconfig`, `angular.json`, ESLint config (today) |
| `uses` | workflow/job/step/action → action, workflow → reusable workflow | the CI reader |
| `runs` | step → script, step → project phase (`./dev ci run <p> <phase>`), step → test-target | the CI reader plus the contract |
| `imports` | script → script, test-file → module (relative TS/JS imports, transitively), test-file → package | `imports` extractor (the scanner drafted on `feat/infra-only-smoke`, uncommitted, with tests) |
| `tests` | test-target → test-file → project | Cargo targets (`tests/*.rs` is a binary, `tests/<dir>/**` a helper of every binary of the crate), contract runner globs |
| `input` / `output` | generator → files or nodes | `docs/generators.toml` `outputs`, inputs as references to nodes (`kind:project/cargo` manifests and their closure, `paddock.ci.json`, `wf:ci.yml`, `action:*`, the Rust sources of TS bindings, `scripts/notices.sh`) instead of hand globs |
| `os` | node → OS set | §7 |

### 5. Queries

All bulk: each takes the whole changed-file set and answers per node (ER-20, PRINCIPLES "Bulk APIs").

- **`reach(changed) -> {node: Reach}`.** The owners of the changed files, then the reverse closure over the edges. `Reach` is the
  strongest *kind* over the paths that arrive at the node: `code` > `release` > `test` > `plumbing` > `docs`. A path's kind is the
  kind of its first edge out of the changed file's owner: a file owned by an `action` or `script` node reached only through
  `uses`/`runs` is `plumbing`; a file owned by a release action, a release workflow or `conformance/release` is `release`; a file owned
  by a `test-file` node is `test`; source, manifests and config are `code`.
- **`selection(reach) -> {job or leg: Full | Smoke | Affected(test-targets) | Installers | Skip}`.** `code` → full for that project;
  `test` → the reached `test-target`s with their files (nextest `binary_id(...)`, vitest `--include`, Playwright file arguments);
  `plumbing` → the project's declared smoke set; `release` → installers (rule 54); `docs` → nothing beyond the docs checks. A
  project without a `smoke` or `tests` declaration falls back to full (never less than declared).
- **`input_hash(node)`** — the content hash of the node's own files plus its declared inputs plus its dependencies' hashes, for
  the test-result cache (0116 draft) and `ER-49` reuse.
- **`select_os(node) -> OS set`** — §7.
- **`due(generators, changed)`** and **`stale_after_merge(generators, a, b)`** — the generators whose input closure the change reaches.

### 6. The approved behaviours become queries (golden differences, §8)

| Behaviour (user, 2026-11-23) | As a query |
|---|---|
| A README or other doc in `.gitea/` is not global | a `.md` under `.gitea/` is owned by a `doc` node (`reach = docs`); only C100 and conformance read it |
| Plumbing-only → smoke | `reach = plumbing` for a project → its declared smoke set |
| Test-only → affected tests | `reach = test` → `Affected(test-targets)` through `imports`/`tests` edges; a helper no spec reaches → full (cannot prove) |
| Rule 54: installers only on real release changes | `Installers` only for `reach ∈ {code, release}` of an app; plumbing never; tags always (`all`) |
| Foundation changes plan no app jobs | `tools/`, `conformance/` (not the plumbing scripts), docs own no edge into an app: the `conformance → every app` rule goes |

The **declared run scope** (ADR 0113) is produced from `selection`: `ran` gains `smoke` and `affected` (the jobs and legs that ran
less than full) and the line becomes `"v":2`; `Declared::complete` refuses a run with either ("it ran a smoke set ... / only the
affected tests ... not the full suite"), and an older reader refuses v2 as not measured (ER-15, ER-33, ER-49). Until ci.yml is
recorded (ADR 0113 §5) the old `plan verdict: complete=` line also says false when any job ran less than full.

### 7. OS placement is derived, with exceptions kept by hand

The C155 detectors become edge extractors: an OS `cfg`, a use of an OS-API crate (`windows_sys`, `libc`, `objc2`, `core_foundation`,
`security_framework`, `zbus`, `nix`, `rustix`, `winreg`, `windows`) or of a portable-layer entry point (`paddock_proc::command`,
`paddock_env::winpath`/`atomic`, `std::process::Command`, file locks) gives the module or package an `os` mark; `imports` (Rust
`crate::<m>` and `use` paths, module granularity for paddock-dev, package granularity elsewhere) carries it to its importers and to
the test targets that cover them. `select_os` is the union of marks; a portable node runs on Linux only (ER-9, user 2026-11-22).
`hostTests` shrinks to exceptions with a reason (`os.add`/`os.skip`, ER-46: shrink-only, growth needs the user) and C155 becomes the
graph's consistency check (an exception the derivation already implies is stale). Tags and the nightly still run every OS.

### 8. Migration: the goldens are the net

The planner's recorded answers (`tools/paddock-dev/tests/golden`: 29 cases, 34 ci cases, 30 expected files, the ci-jobs and
ci-run tables) stay the regression net. The new planner runs beside the old one (`./dev plan --compare`) over every golden case
and over the last 200 commits of `main`; every difference is either in this list or a defect:

1. `.gitea/**/*.md`: docs, not global.
2. A plumbing-only reach: smoke instead of full; `--app <app>` answers `false` (no installers).
3. A test-only reach: affected test targets instead of full.
4. `conformance/src`, `conformance/bin`: no app marks.
5. OS legs from derivation instead of `hostTests` (only where C155 already reports the list wrong).

The regenerated goldens (ER-40, with their generators) carry exactly these differences; review reads the diff of the goldens.
The old rule tables are deleted in the last work package, not kept as a fallback.

### 9. Performance, parallelism and cost

Built once per run: one `git ls-files`, one `git cat-file --batch` for every file text a query needs (CI files, contracts, test
files, Rust sources for the OS and import extractors), one `cargo metadata` per Cargo workspace (today's count), no spawn per file.
A spawn budget test pins the count (ER-20). Today `./dev plan --all` takes 2.9 s and `./dev plan origin/main~20 --explain` 12.8 s on
the dev Mac (measured 2026-11-23 on this base under load; [benchmarks](../benchmarks.md#planner-run-time-before-the-repository-graph-model-2026-11-23-docsadr-repo-graph-model-base-803aed12d)); the target is the same or less, with the
file reads batched. Extractors run in parallel over files (rayon, cores minus one), results deterministic (sorted).

### 10. Edges come from parsers, not language servers

- **Project level**: `cargo metadata` and the npm workspaces, as today. **CI**: the YAML reader of ADR 0060 plus script imports.
- **TypeScript and JavaScript files**: `oxc_parser` with `oxc_resolver` (Rust libraries, no server, the parser and resolver that
  Rolldown and Biome build on; MIT), which resolve `tsconfig` paths and package `exports` that a text scan cannot. The text scanner
  drafted on `feat/infra-only-smoke` (`imports.rs`, relative specifiers only) is the fallback if WP-3 measures oxc as too slow or
  too large a dependency.
- **Rust within a crate**: `mod`, `use crate::` and `cfg` attributes, through `syn` (already a direct dependency of paddock-dev, `tools/paddock-dev/Cargo.toml:30`; new to `paddock-repo-graph`)
  or the C155 text scan; WP-6 picks by measurement.
- **Why not a language server** (rust-analyzer, tsserver): they are stateful, start slowly (seconds to minutes to index a
  workspace), use gigabytes of memory, and answer editor questions incrementally; CI needs one deterministic cold-start answer per
  run. Common practice agrees: Nx scans imports with its own parser (nx.dev, "Project Graph": "Nx analyzes source files"), Bazel
  reads declared deps, Turborepo reads manifests, Pants infers from imports with its own parsers ("Dependency inference").
- **Granularity limit**: Rust tests are selected by test binary (`binary_id`), TypeScript tests by spec file, Playwright by spec file
  and project. No test-function level, as Bazel's target-level practice.
- **Budget (ER-20)**: one `git cat-file --batch` for every text, one `cargo metadata` per Cargo workspace, no other spawn; the whole
  extraction at most 3 s on the dev Mac and 5 s on the Linux runner, pinned by a test that counts spawns and a benchmark row. The
  repository today: 3,780 `.ts` files (24.1 MB, generated ones included), 633 `.mjs` (4.8 MB), 3,732 `.rs` (46.3 MB); a text scan of
  their import and `cfg` lines takes 0.8 s and 0.9 s in one process ([benchmarks](../benchmarks.md#import-and-cfg-scan-of-the-whole-repository-2026-11-23-docsadr-repo-graph-model)),
  the lower bound a parser is compared with in WP-3.

## Common practice (ER-37)

- **Nx**: a project graph from package manifests and plugins, `namedInputs` and `targetDefaults.inputs` per target, `nx affected`
  over the graph, and computation caching keyed by the inputs' hash (nx.dev: "Project Graph", "Inputs and Named Inputs",
  "Run Only Tasks Affected by a PR", "Cache Task Results"). A file outside every input reaches nothing.
- **Bazel**: every target declares its sources and deps; `bazel query 'rdeps(//..., set(<files>))'` is reach, tests are cached by
  input digest, OS placement is `target_compatible_with` on platforms, declared by hand (bazel.build: "Query guide", "Platforms",
  "Remote caching").
- **Turborepo**: task `inputs` and `globalDependencies` in `turbo.json`, `--filter=...[HEAD^1]` for changed packages and their
  dependents, content-hash caching (turbo.build: "Configuring tasks", "Filtering").
- **Pants**: dependency *inference* from imports (Python, Go, JVM, JS), `--changed-since=<ref> --changed-dependents=transitive`, and
  per-file test targets, so a test-only change runs only the tests that import it (pantsbuild.org: "Dependency inference",
  "Advanced target selection").
- None of them model CI workflows, jobs and shared actions as nodes; they treat CI config as outside the graph (Nx, Turborepo) or as
  a global input. Paddock needs CI nodes because its CI is code that its own changes test (the train 20 case). Inferring OS
  placement (§7) goes beyond Bazel's hand declarations; it replaces a hand list that already drifts.

## Work packages and cost

| WP | Content | Agent-days |
|---|---|---|
| WP-1 | Node and edge types, `docs/repo-graph.toml` and its accessor, owners, the bulk file reader with a spawn budget, `reach` with kinds, the benchmark | 1.0 |
| WP-2 | The CI reader moved from `gates/ci.rs` into the crate (`gates` keeps using it), `uses`/`runs` edges, action and script nodes; `plan_gitea` deleted | 1.0 |
| WP-3 | Test files and targets, the `imports` extractor (drafted), contract `tests`/`smoke` declarations for the apps and root projects, `selection`, the ci.yml and app-gates wiring (`PADDOCK_RUN_SCOPE`, filters) | 1.5 |
| WP-4 | Run scope v2 (`smoke`, `affected`) and `complete()`; the old `complete=` line | 0.3 |
| WP-5 | `--compare` over the goldens and 200 commits, the listed differences, regenerated goldens, old rule tables deleted | 1.0 |
| WP-6 | OS derivation (C155 detectors as extractors), `hostTests` cut to exceptions | 1.0 |
| WP-7 | Generators as nodes (`due`, stale after merge); `input_hash` for the 0116 draft | 0.5 |
| | **Total** | **6.3** (the user was told about 4 to 6) |

Common-practice cost, for the decision: the five behaviours as five more planner rules (the ADR 0113 amendment's design) cost about
3 to 4 days and leave the class open; WP-1 to WP-5 alone (no OS derivation, no generators) are 4.8 days and close it for planning.

## Decisions

- **D1. Scope.** (a) WP-1 to WP-7, 6.3 days. (b) WP-1 to WP-5 now, WP-6 and WP-7 after the next release, 4.8 days.
  (c) Common practice: the five behaviours as planner rules, 3 to 4 days, no model. *Recommended: (b)*: it closes the planning class
  within the told budget; OS placement and generators have working hand lists with checks today.
- **D2. Where the model lives.** (a) `paddock-repo-graph`, extractors as modules behind the existing ports. (b) A new crate
  `paddock-repo-model` beside it. *Recommended: (a)*, share first (ER-22); the crate is already the planner's dependency.
- **D3. Inputs file.** (a) One `docs/repo-graph.toml` merged over crate defaults, project facts in `paddock.ci.json`. (b) Everything
  in `paddock.ci.json` files. *Recommended: (a)*: repository-wide inputs (toolchain, root npm files) belong to no project, as Nx's
  `nx.json` and Turborepo's root `turbo.json`.
- **D4. `.gitattributes`.** (a) Keep it a global (today). (b) An input of generators and conformance only (line ends change checked-out
  text, which the generators and C57/C-line checks read; builds and tests read text through git's normalisation). *Recommended:
  (b)*, with a golden difference listed; common practice (Nx, Turborepo) does not treat it as an input at all.
- **D5. Reach kinds.** (a) The five kinds of §5 (`code`, `release`, `test`, `plumbing`, `docs`). (b) Only `code` and `plumbing`
  (smoke) now. *Recommended: (a)*: the user approved all four behaviours.
- **D6. A test helper no spec reaches.** (a) Full suite (cannot prove). (b) Nothing. *Recommended: (a)*; Pants and Jest
  `--findRelatedTests` run nothing, but a setup file referenced by config is reached by no import.
- **D7. Smoke and affected runs on a push to main.** (a) Allowed; such a commit has no complete verdict, so a tag on it waits for the
  nightly or a full dispatch. (b) A push to main always runs full. *Recommended: (a)*, as approved ("the full suite stays on the
  nightly, on app or crate code and on tags").
- **D8. Migration.** (a) Old and new side by side with `--compare` until the goldens match except the listed differences, then the old
  tables go in the same train. (b) Switch at once. *Recommended: (a)*.
- **D10. Extraction.** (a) Parsers as §10 (`oxc_parser`/`oxc_resolver`, `syn` or the C155 scan), cold start, one answer per run.
  (b) Language servers (rust-analyzer, tsserver) queried by the planner. (c) Text scans only (relative imports, `cfg` lines).
  *Recommended: (a)*, with (c) as the measured fallback; (b) is not common practice for build graphs and is too slow and stateful
  for CI.
- **D9. Trains.** (a) Hold all trains until WP-1 to WP-5 land (the user's instruction). (b) Let release-only trains through. *As
  instructed: (a)*.

## Consequences

- A new planner question is a query or a declared input, not a rule; a file no node owns fails the graph's check instead of planning
  everything.
- `plan_gitea.rs`, the `GLOBAL` and input regex tables of `plan.rs`, the CI job table and the duplicate CI reader go; `plan_ci.rs`
  shrinks to output. `plan_since.rs` (reruns from logs) stays: it reads runs, not the repository.
- The ADR 0113 amendment's behaviours keep their text; its mechanism (planner rules, the `plumbingOnly`/`affectedTests` lists) is
  replaced by `selection`.

Recurring: "a planner fact that is a rule over paths instead of a node or an edge of the graph". Earlier instances: the table above
(train 20 commit `298cc726b`'s plan; `plan.rs` `CONFORMANCE_CODE`; `hostTests` on `c61339b9f`; `docs/generators.toml:40`).
