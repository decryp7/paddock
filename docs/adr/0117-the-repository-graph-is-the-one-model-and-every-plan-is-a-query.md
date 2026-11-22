# 0117. The repository graph is the one model of the repository, and every plan is a query on it

- Status: Accepted 2026-11-23 (the user, through the coordinator): D1 (a), all seven work packages now; D6 (a); D7 (a); D2, D3,
  D4 (b), D5 (a), D8 (a), D9 (a), D10 (a) as recommended; and D11 (the landing run tests the train's diff from main's tested head,
  WP-8). One independent review of this ADR runs beside the build; a P1 stops the build. The direction is the user's ("fix the
  graph first, and hold all trains until it lands"). Review round 1 (2026-11-23, at `a073aae56`) found 10 P1s and 8 P2s; the
  build stopped and every finding is answered in this text (§2 to §14, the work packages and the decisions); a second review round
  follows. Cost: the user approved the increase over 8 days on 2026-11-23 "as long as it fixes the root cause once and for all", so
  the scope is the complete one (§14 says how each instance becomes impossible or checked).
- Date: 2026-11-23
- Related: ER-20 (bulk work, spawn budgets), ER-21 (a recurring class gets a redesign), ER-22 (share first; a value another
  module decides is read from it), ER-15 and ER-49 (a smoke or partial run is never complete; test a commit once), ER-37 (common
  practice and cost), ER-40 (generated files), ER-42 (read declared data through its accessor), ER-46 (lists only shrink); ADR 0022
  (paddock-dev, `paddock-repo-graph`), ADR 0038 (CI commands as data in `paddock.ci.json`), ADR 0060 (the CI model of `./dev gates`),
  ADR 0080 (CI speed), ADR 0113 and its amendment of 2026-11-23 (the declared run scope; plumbing-only and test-only runs),
  ADR 0116 draft (`a test target whose inputs did not change reuses its pass`, `origin/docs/adr-test-result-cache` `4e984ce2c`:
  its WP-0 needs this ADR's `input_hash`; it measured about 75 to 110 job-minutes per round, 28 to 34 %, in jobs whose test inputs
  did not change), and the paused `a merge regenerates what it made stale` draft (local `feat/merge-regenerates`, to be renumbered).
- Supersedes as a mechanism (not as behaviour): the planner rule tables of `tools/paddock-dev/src/plan.rs` and `plan_gitea.rs`,
  the ADR 0113 amendment's planner-rule design (its behaviours stay and become queries, §6), the hand lists `hostTests` (C155) and
  `docs/generators.toml` `paths`.
- Number: 0117. Searched 2026-11-23: `git ls-tree docs/adr/` over every `origin` branch and every local branch, and `docs/adr/` of
  every worktree under the worktree root, again before the last commit. Highest found: 0116 on `origin`
  (`docs/adr-test-result-cache`); a second local 0116 on `feat/merge-regenerates` (to be renumbered); no 0117 anywhere.
- Evidence heads (ADR 0091), read 2026-11-23 and again before the last commit: `origin/main` = `b3c254a2d`, then `d04af7b46` (one
  commit, the local sccache fix; no change to the planner, `verdict.rs` or `paddock-repo-graph`), `origin/train/2026-11-22-20` =
  `803aed12d` (this branch's base), `origin/fix/run-scope-reuse-p2` = `bab7a4a97` (ADR 0113), `origin/feat/os-scoped-tests` =
  `c61339b9f` (`hostTests`, C155), `origin/feat/infra-only-smoke` = `29864b700` (the ADR 0113 amendment, docs only; its planner work
  stopped unbuilt), `origin/docs/ci-run-audit` = `365d15bb7` (the gap list G1 to G14, `docs/research/ci-run-audit-2026-11-23.md`
  section 3), `origin/fix/plan-actions-readme-not-global` = `3461629bd` (the README fix with its red-first test).

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
| A README in an action folder is a global | `.gitea/actions/README.md` is an "action nobody uses", so `gitea_targets` answers `None` (`plan_gitea.rs:325-328`) and `plan.rs:536-540` marks everything | `./dev plan 298cc726b^ 298cc726b` (train 20's cache-step commit): `"all": true` |
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

Every tracked file belongs to exactly one *owner* node, by this precedence (review P1-4):

1. a named input (§3) claims it (the root `package.json`, `.gitattributes`, `docs/lessons.md`);
2. otherwise the deepest **project folder** that contains it: a project's own docs, changelog and data belong to the project
   (`apps/dyno/CHANGELOG.md` is compiled in by `apps/dyno/app/src-tauri/src/lib.rs:276`, `include_str!`). Its reach kind is `docs`
   unless a `reads` edge (§4) or a declared input names the file, then `code`;
3. otherwise a declared owner rule for the paths outside every project (`docs/**`, root `*.md`, `.gitea/**/*.md`, `spikes/**`,
   `conformance/reserved/**`). There is no catch-all rule.

A file none of these claims is **unowned**: the plan itself answers "everything" for that run and prints the file in an `::error::`
line with the next step (declare an owner); the owner check runs inside the plan job, not only in a separate check, so it cannot be
skipped. It starts report-only (ER-41, review P2-6): until its first green run is recorded, an unowned file widens the plan to
everything and nothing fails.

### 3. Declared inputs, replacing `GLOBAL` and the path regexes

One file, `docs/repo-graph.toml` (schema documented in the file, read by one accessor, ER-42), merged over defaults built into the
crate, in the shape of Nx `namedInputs` and Turborepo `globalDependencies`:

```toml
version = 1

[[input]]                           # a named input: a file set and what it reaches
name = "rust-toolchain"
files = ["rust-toolchain", "rust-toolchain.toml"]
reaches = ["all"]                   # all | cargo | npm | apps | project:<id>
reach = "code"                      # code | release | test | plumbing | docs (§5)

[[input]]
name = "npm-root"
files = ["package.json", ".npmrc", ".nvmrc", "tsconfig/**"]
reaches = ["all"]

[[input]]
name = "npm-lock"
files = ["package-lock.json"]
reaches = ["npm"]
extractor = "npm-lock"              # a content extractor narrows it (below)

[[input]]
name = "git-attributes"
files = [".gitattributes"]
reaches = ["project:conformance"]
extractor = "attributes"            # text/eol/binary/filter/diff changes reach the owners of the files they match (G2, D4)

[[owner]]                           # paths outside every project
paths = ["spikes/**", "conformance/reserved/**"]
node = "none"
```

**Content extractors** (review P2-2) are the named concept for an input that is narrowed by reading its text before and after the
change, each with its own golden: `attributes` (the attribute diff, G2), `npm-lock` (`lock_diff`, `crates/paddock-repo-graph/src/lock.rs:90`,
used at `plan.rs:599`), `guards` (`guards_changed`, `plan.rs:173`, used at `plan.rs:507`: an ignore attribute or an environment
guard changed, which C100 reads). A new one is a new variant, registered once.

Narrowing an entry of `docs/repo-graph.toml` (fewer files or targets) is the unsafe direction: it needs a golden diff in the same
change and the user's approval (review P2-6, ER-46); widening needs neither.

A project contract (`paddock.ci.json`) declares what only the project knows: `tests` (test-file globs per runner), `smoke` (the
smoke set per phase and a nextest filterset), and OS overrides with a reason (§7). The current regex tables move into this file
one by one, each with the golden cases that cover it (§8).

### 4. Edges

| Edge | From → to | Extractor |
|---|---|---|
| `depends` | crate → crate, package → package | Cargo metadata, npm workspaces (today) |
| `path-ref` | project → project | `frontendDist`, `tsconfig`, `angular.json`, ESLint config (today) |
| `uses` | workflow/job/step/action → action, workflow → reusable workflow | the CI reader |
| `runs` | step → script, step → project phase (`./dev ci run <p> <phase>`), step → test-target, contract check line → script (`node conformance/bin/paddock-conformance.mjs --only C13` in the app contracts of dyno, pitcrew, pitwall and spotter) | the CI reader plus the contract (review P1-10) |
| `reads` | target → data file it reads at build or test time | `syn` over Rust: `include_str!`/`include_bytes!` (253 sites, 45 of them three folders up, for example `debrief-plugins/tests/it/windows_markers.rs:89,106` reading `debrief-plugin-windows` fixtures), `#[path]`, `build.rs` `rerun-if-changed` (13 build scripts); string-literal paths a test passes to a script or a file read (`conformance/test/cache-report.test.mjs:9` runs `conformance/ci/cache-report.sh`); declared runtime reads (`reads` in `paddock.ci.json`). A read file selects its reader target; a script run by a path that does not resolve runs its owning project in full (review P1-1, P1-2) |
| `imports` | script → script, test-file → module (relative TS/JS imports, transitively), test-file → package | `oxc_parser` and `oxc_resolver` with **no `node_modules`** (CI's plan job has none): a workspace package resolves through the crate's npm graph, an external package is a package-level edge, an unresolved relative import runs the importer's project in full; a test shows the same answer with and without `node_modules` (review P1-7). The text scanner `imports.rs` (`feat/repo-graph-model` `de3f23c08`, §13) is the fallback |
| `tests` | test-target → test-file → project | Cargo targets from `cargo metadata` (`targets[].src_path` and `kind`, review P2-5; a file under `tests/` that is no target's `src_path` is a helper of every test target of its crate), contract runner globs |
| `input` / `output` | generator → files or nodes | `docs/generators.toml` `outputs`, inputs as references to nodes (`kind:project/cargo` manifests and their closure, `paddock.ci.json`, `wf:ci.yml`, `action:*`, the Rust sources of TS bindings, `scripts/notices.sh`) instead of hand globs |
| `os` | node → OS set | §7 |
| `reads-script` | script → script it runs (a `.sh` calling another) | the shell lines' invoked programs, never a path inside other text (G4) |

### 5. Queries

All bulk: each takes the whole changed-file set and answers per node (ER-20, PRINCIPLES "Bulk APIs").

- **`reach(changed) -> {node: Reach}`.** The owners of the changed files, then the reverse closure over the edges. `Reach` is the
  strongest *kind* over the paths that arrive at the node: `code` > `release` > `test` > `plumbing` > `docs`. A path's kind is the
  kind of its first edge out of the changed file's owner: a file owned by an `action` or `script` node reached only through
  `uses`/`runs` is `plumbing`; a file owned by a `test-file` node is `test`; source, manifests, config and every file a `reads` edge
  names are `code`. `release` comes from the edges, not a list (review P2-3): an action or script reached through a `uses` edge out of
  a release workflow's bundle or publish job (the jobs that call `tauri-bundle`, `publish-release`, `macos-verify`, the signing
  steps) is `release`, unless `docs/repo-graph.toml` declares the action setup-only (`setup-rust`, `build-jobs`, the cache actions),
  which is then `plumbing` for that job too (rule 54).
- **`selection(reach) -> {job or leg: Full | Smoke | Affected(test-targets) | Installers | Skip}`.** `code` → full for that project;
  `test` → the reached `test-target`s with their files (nextest `binary_id(...)`, vitest `--include`, Playwright file arguments);
  `plumbing` → the project's declared smoke set; `release` → installers (rule 54); `docs` → nothing beyond the docs checks. A
  project without a `smoke` or `tests` declaration falls back to full (never less than declared).
- **`input_hash(node)`** (a prerequisite of ADR 0116's WP-0), built on the mechanism `tools/paddock-dev/src/input_hash.rs` already
  uses for the tool's binary (ADR 0022 §3.3: `git ls-tree` object ids, the `commit-hash:` and `release:` lines of `rustc -vV`, the
  hashed environment variables and a digest of each cargo config file), over: the node's own files and every file its `reads`
  edges name; its dependencies' hashes; the build-script environment (`paddock-setup`'s `build.rs` reads variables); the runner
  command lines of its phases in `paddock.ci.json`; the nextest, Vitest and Playwright config files that select and run it; the CI
  image digest; and the OS (review P1-8).
- **`select_os(node) -> OS set`** — §7.
- **`due(generators, changed)`** and **`stale_after_merge(generators, a, b)`** — the generators whose input closure the change reaches.

### 6. The approved behaviours become queries (golden differences, §8)

| Behaviour (user, 2026-11-23) | As a query |
|---|---|
| A README or other doc in `.gitea/` is not global | a `.md` under `.gitea/` is owned by a `doc` node (`reach = docs`); only C100 and conformance read it |
| Plumbing-only → smoke | `reach = plumbing` for a project → its declared smoke set |
| Test-only → affected tests | `reach = test` → `Affected(test-targets)` through `imports`/`tests` edges; a helper no spec reaches → full (cannot prove) |
| Rule 54: installers only on real release changes | `Installers` only for `reach ∈ {code, release}` of an app; plumbing never; tags always (`all`) |
| Foundation changes plan no app tests | `tools/` and docs own no edge into an app. A `conformance/bin` or `conformance/src` change reaches the conformance job and, through the `runs` edge from each app contract's check line, that app leg's **conformance step** (Linux, static), not the app's tests (review P1-10); `conformance/reserved/*` reaches nothing |

The **declared run scope** (ADR 0113) is produced from `selection`: `ran` gains `smoke` and `affected` (the jobs and legs that ran
less than full) and the line becomes `"v":2`; `Declared::complete` refuses a run with either ("it ran a smoke set ... / only the
affected tests ... not the full suite"), and an older reader refuses v2 as not measured (ER-15, ER-33, ER-49). Until ci.yml is
recorded (ADR 0113 §5) the old `plan verdict: complete=` line also says false when any job ran less than full.

"Complete" stays exact (review P1-3). The base of a push to main or a landing run is **the last commit with a complete verdict from
the one verdict reader** (`./dev ci verdict`), not the last green run as `choose_base` takes it today (`plan_ci.rs:285` and below),
so a smoke or affected run never becomes the base and diffs accumulate from the last complete commit. A v2 run is complete only
when its selection was full for everything its plan reached **and** its base was itself complete. A golden pins the sequence: a
plumbing-only push (smoke, not complete) followed by a code push: the code push diffs from the commit before the smoke push, so it
runs the plumbing change's jobs in full too, and is complete.

**The verdict reader's "last fully tested commit" is wrong today, and WP-4 fixes it.** Read-only check on 2026-11-23:
`./dev ci verdict --workflow ci.yml --sha d04af7b46...` names `f7bbe2b86` (a train-7 commit of 2026-11-13) as the last fully tested
commit on main, although trains 8 to 19 landed after it. Cause, in `last_full` (`tools/paddock-dev/src/ci_runs/verdict.rs` on
`origin/main` `b3c254a2d`): it takes as candidates only the commits whose **push run on main concluded `success`**, then asks the
reader about each. Since 2026-11-13 no ci.yml push run on main has: the API's newest 50 push runs show ci.yml runs 8054, 8134, 8198,
8241, 8315, 8555 and 8626 `failure` and 8341, 8392 and 8461 `cancelled` (pushes cancelled because a train run had already tested the
commit, ER-49), and 8045 (`f7bbe2b86`) the newest `success`. A commit that its train's dispatch run tested in full is therefore
never a candidate, even though `verdict_core` would accept it. It is not the marker: the train runs log `complete=true`; the
candidate filter never asks. Fix (WP-4): candidates are main's first-parent commits (newest first, bounded), whatever their push
run's conclusion, each asked of the one reader; a golden pins a main whose pushes were all cancelled after green train runs. Until
then the base of §6 falls back to "everything" (safe, not cheap).

### 7. OS placement is derived, with exceptions kept by hand

The C155 detectors become edge extractors: an OS `cfg`, a use of an OS-API crate (`windows_sys`, `libc`, `objc2`, `core_foundation`,
`security_framework`, `zbus`, `nix`, `rustix`, `winreg`, `windows`) or of a portable-layer entry point (`paddock_proc::command`,
`paddock_env::winpath`/`atomic`, `std::process::Command`, file locks) gives the module or package an `os` mark; `imports` (Rust
`crate::<m>` and `use` paths, module granularity for paddock-dev, package granularity elsewhere) carries it to its importers and to
the test targets that cover them. `select_os` is the union of marks; a portable node runs on Linux only (ER-9, user 2026-11-22).
The signals also include `CARGO_CFG_TARGET_OS` in build scripts, `cfg!(...)` expressions, `std::env::consts::OS`, `process.platform`
in TypeScript and dependencies whose behaviour differs by OS (review P2-4). Narrowing by `std::process::Command` sites is measured in
WP-6 before it is used (how many test targets it moves on or off a host OS). `hostTests` shrinks to exceptions with a reason:
`os.add` is free; `os.skip` (taking an OS away) is the unsafe direction and needs the user (ER-46). C155 becomes the graph's
consistency check, report-only until its first green run (P2-6). Tags and the nightly still run every OS.

### 8. Migration: the goldens are the net

The planner's recorded answers (`tools/paddock-dev/tests/golden`: 29 cases, 34 ci cases, 30 expected files, the ci-jobs and
ci-run tables) stay the regression net. The new planner runs beside the old one (`./dev plan --compare`) over every golden case
and over the last 200 commits of `main`; every difference is either in this list or a defect:

1. `.gitea/**/*.md`: docs, not global. The test of `3461629bd` (`a_markdown_file_under_gitea_actions_marks_only_conformance`, with
   its positive control: a real action file with no user still marks everything) is carried over as a golden case.
2. A plumbing-only reach: smoke instead of full; `--app <app>` answers `false` (no installers).
3. A test-only reach: affected test targets instead of full.
4. `conformance/src`, `conformance/bin`: no app marks.
5. OS legs from derivation instead of `hostTests` (only where C155 already reports the list wrong).
6. The audit's gaps (below): each closed gap changes the goldens it covers, and only those.
7. A train dispatch with `base` (WP-8): the landing run plans the train's diff, not everything.

The audit's gap list (`docs/research/ci-run-audit-2026-11-23.md` §3), each mapped to the work package that closes it:

| Gap | What over-marks today | Closed by | As |
|---|---|---|---|
| G1 | a train dispatch has no base, so `choose_base` plans everything (runs 8610, 8635, 8647) | WP-8 | the dispatch passes `base` = main's tested head; ADR 0113's `need` for that run and for a push = the run's own plan |
| G2 | `.gitattributes` is a whole-file global | WP-1 | an attribute-aware extractor: only `text`, `eol`, `binary`, `filter`, `diff` changes reach the owners of the paths they match; `merge=` and comments reach conformance only (D4) |
| G3 | an unknown root file (`spikes/*/Cargo.lock`) marks everything | WP-1 | owners: every root folder is declared (owned or "not built"); an unowned file fails the graph's check |
| G4 | a mention is a use (the plan job's grep naming `nightly.yml`) | WP-2 | `uses` and `runs` edges come only from `uses:` and a step's invoked program and script arguments, never from a path inside other shell text; the "C100 stays wired" grep moves to the conformance job |
| G5 | a ci.yml header or plan-job change is global | WP-2 | the plan job's env and step text reach paddock-dev's planner tests and goldens; a changed output name reaches the jobs that read it |
| G6 | a shared action or `conformance/ci` script marks every job that uses it, kind all | WP-3 | `reach = plumbing` → the declared smoke set |
| G7 | `.gitea/actions/README.md` is global | WP-1 | a `doc` node (difference 1) |
| G8 | any `conformance/` file (even `reserved/*`) marks every app, kind all | WP-1 | conformance reaches the conformance job and each app's conformance step only; `conformance/reserved/*` reaches nothing |
| G9 | an npm dependent ending in `src-tauri` gets kind all | WP-1 | an npm edge gives the UI half; a Rust half only from a Cargo edge or the Tauri config |
| G10 | `paddock-e2e` env and guard files mark every app's UI half | WP-3 | kept, through `imports` edges (every Playwright config imports them) |
| G11 | tests run per workspace, not per reached package, except on member runs | WP-3 | `selection` names the reached packages (`-p`) on every run kind; whole workspaces only on the nightly and tags |
| G12 | no test-level graph | WP-3 | `test-file` nodes and `imports` edges |
| G13 | `docs/lessons.md` marks conformance and tools | WP-1 | kept as a declared input (C145 reads it) |
| G14 | a push's base is the last green push run, so a red push widens the next one | WP-8 | the base reader (the ADR 0113 verdict reader, the one place) accepts the train's complete run on the same tree |

The regenerated goldens (ER-40, with their generators) carry exactly these differences; review reads the diff of the goldens.

The net is wider than the goldens (review P1-6):

- **Train replay.** Every non-merge member commit of trains 15 to 20 is planned against its parent by the old and the new planner;
  differences outside the list above are defects.
- **One golden per migrated rule.** Each regex table and content rule that moves gets its own golden before it moves:
  `guards_changed`, `PER_MACHINE_INPUT`, `APP_CAPABILITY_INPUT`, `NOTICES_INPUTS`, `C99_INPUT`, `PARITY_INPUT`, `HOOKS_PARITY_INPUT`,
  `GENERATED`, `LINT_WIRING`, `OWNERS`, `.gitattributes`, `rust-toolchain`, `lock_diff`.
- **`--compare` prints what a narrowing drops**: for every node the new planner selects less of, the targets it no longer runs.
- **Escape monitor** (the practice of Google TAP and Meta's predictive test selection): each nightly's full results are compared with
  the selection of every commit since the last green nightly; a test that fails in the nightly, bisected to a commit whose selection
  did not include it, is an escape, reported with the commit and the dropped edge. The old rule tables are deleted only after five
  consecutive nightlies with zero escapes (D12); until then they stay behind `--compare`.

**Positive controls** (ER-33, review P1-5), each a fixture that removes what the control watches and asserts that reach or
selection shrinks (or that the check goes red):

| Control | Fixture removes | Asserts |
|---|---|---|
| each node kind (project, workflow, job, step, action, script, test-file, test-target, generator, doc) | the node's declaration or file | its reach entry disappears |
| each edge kind (`depends`, `path-ref`, `uses`, `runs`, `reads`, `reads-script`, `imports`, `tests`, generator input, `os`) | the edge's source line (`include_str!`, `uses:`, the import, the cfg) | the downstream node is no longer reached or selected |
| each content extractor (`attributes`, `npm-lock`, `guards`) | the attribute, lock or guard change | the narrowed targets drop |
| the owner check | the owner rule of a folder | the plan answers everything with an `::error::` |
| `--compare` | a rule from the new planner | `--compare` reports the difference |
| the spawn budget | the bulk reader (one read per file) | the budget test fails |
| C155 consistency | an OS cfg in a module | the module's `os` mark disappears and C155 reports the exception stale |


### 9. Performance, parallelism and cost

Built once per run: one `git ls-files`, one `git cat-file --batch` for every file text a query needs (CI files, contracts, test
files, Rust sources for the OS, `reads` and import extractors), one `cargo metadata` per Cargo workspace (about 12 that CI builds:
the seven app workspaces, `crates`, `tools/paddock-dev`, `tools/heavy` and the demos; the fuzz and spike workspaces are not
planned), no spawn per file. The plan job also builds or downloads `./dev` (the prebuilt download is the normal case; a rebuild is
about a minute, ADR 0022), which the budget counts (review P2-8).
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
- **Features** (review P2-7): the extractors sit behind crate features (`ci-yaml`, `ts` for oxc, `rust-syn`, `parallel` for rayon);
  paddock-dev enables them, other users of the crate do not. Measured during WP-1: adding one dependency (`toml`) to
  `paddock-repo-graph` regenerated `paddock-workspace-hack` and changed all five app `Cargo.lock` files (which would mark every
  app's Rust half), so the TOML accessor moved to paddock-dev instead (`tools/paddock-dev/src/repo_inputs.rs`).
- **Granularity limit**: Rust tests are selected by test binary (`binary_id`), TypeScript tests by spec file, Playwright by spec file
  and project. No test-function level, as Bazel's target-level practice.
- **Budget (ER-20)**: one `git cat-file --batch` for every text, one `cargo metadata` per Cargo workspace, no other spawn; the whole
  extraction at most 3 s on the dev Mac and 5 s on the Linux runner, pinned by a test that counts spawns and a benchmark row. The
  repository today: 3,780 `.ts` files (24.1 MB, generated ones included), 633 `.mjs` (4.8 MB), 3,732 `.rs` (46.3 MB); a text scan of
  their import and `cfg` lines takes 0.8 s and 0.9 s in one process ([benchmarks](../benchmarks.md#import-and-cfg-scan-of-the-whole-repository-2026-11-23-docsadr-repo-graph-model)),
  the lower bound a parser is compared with in WP-3.

### 11. Prerequisites and landing order (review P1-9)

| Prerequisite | Where it is | How it lands while trains are held |
|---|---|---|
| ADR 0113, `run_scope` and the verdict reader | `origin/fix/run-scope-reuse-p2` `bab7a4a97` (in train 21's local work) | first member of the graph train |
| OS-scoped tests, `hostTests`, C155 | `origin/feat/os-scoped-tests` `c61339b9f` | second member of the graph train (WP-6 builds on it) |
| The TS import scanner (`imports.rs`) and the WP-1 model | `origin/feat/repo-graph-model` `de3f23c08`, `499ed5553` (§13) | the graph train's WP-1 member |

The graph train is the one train while the others are held (D9): 0113 and os-scoped first, then WP-1 to WP-5 with WP-8, each member
gated as ER-8 says; WP-6, WP-7 and WP-9 follow in the next train.

### 12. Review round 1 answers (and two later questions: extensibility, §15; the verdict reader, §6)

P1-1 `reads` edge (§4, §5 `input_hash`); P1-2 scripts run by path (§4 `reads`); P1-3 complete stays exact (§6); P1-4 fail closed and
owner precedence (§2); P1-5 positive controls (§8 table); P1-6 migration net (§8); P1-7 resolution without `node_modules` (§4); P1-8
`input_hash` on `input_hash.rs` (§5); P1-9 prerequisites (§11); P1-10 conformance steps (§4 `runs`, §6). P2-1 D4 and §3 (D4, §3);
P2-2 content extractors (§3); P2-3 release from edges (§5); P2-4 OS signals (§7); P2-5 test targets from `src_path` (§4); P2-6
narrowing needs approval, checks report-only (§2, §3, §7); P2-7 features (§10); P2-8 budget and cost (§9, the table below; the
decisions are in order).

### 13. Work done before the build stopped

WP-1, first slice, on `origin/feat/repo-graph-model` (`de3f23c08` the crate, `499ed5553` the accessor; base `a073aae56`): `crates/paddock-repo-graph/src/model/` (`Reach`,
owners, named inputs and defaults, the attribute extractor, `reach` at the project level) and `imports.rs`, 39 crate tests with
positive controls for G3, G7/G8 and G9 (a mutation of each rule turns its test red); `tools/paddock-dev/src/repo_inputs.rs` (the TOML
accessor, 3 tests) and `docs/repo-graph.toml`. Known gap against this revision: the slice gives declared owner rules precedence over
project folders, the reverse of §2 (P1-4); WP-1 fixes it first.

### 14. Once and for all: how each instance becomes impossible or checked

**No path rule outside the graph.** When WP-5 and WP-9 are done, the planner holds no regex or path list: every fact that today is
a rule (`GLOBAL`, the input regexes, `OWNERS`, `CI_JOBS`, `TRACED_SCRIPT`, `GITEA_FILE`, the `conformance → every app` loop, the
`src-tauri` kind rule, `hostTests`, the generators' `paths`) is a declared input in `docs/repo-graph.toml`, a project contract
field, or an edge an extractor reads from the code. A conformance check (WP-5) fails when `tools/paddock-dev/src/plan*.rs` gains a
`Regex` or a path literal, with a positive control.

"Impossible" means the structure cannot express the defect; "checked" means a named check fails when it comes back.

| Instance | Becomes | By |
|---|---|---|
| Actions README is global (G7) | impossible | a `.md` is owned by its folder's project or a `doc` rule (§2); no rule turns an untraced file into "everything" except fail-closed on an *unowned* file, which also fails the owner check |
| A shared action marks every job whole (G6) | impossible | reach carries a kind; `plumbing` selects only the declared smoke set (§5) |
| Installers on a cache step (rule 54) | impossible | `release` comes only from `uses` edges out of a bundle or publish job; setup-only actions are declared (§5) |
| Test-only change runs the whole suite (G12) | impossible, or full when unprovable | `test` selects the reached test targets; an unresolved import or path runs full (§4, D6) |
| Foundation change runs app jobs (G8) | impossible | conformance reaches only the conformance job and the apps' conformance steps through `runs` edges (§6) |
| OS placement by hand list | checked | derived `os` marks; C155 fails on an exception the derivation contradicts; `os.skip` needs the user (§7) |
| Generator inputs by hand list | impossible | generator inputs are edges to nodes, not globs (§4) |
| G1 no base on a train dispatch | checked | WP-8: the dispatch passes `base`; `./dev ci dispatch` refuses a train dispatch without one; a golden pins it |
| G2 `.gitattributes` global | impossible | the attribute extractor; only content attributes reach (§3) |
| G3 unknown root file marks everything | checked | the owner check fails on an unowned file and the plan prints it; owners are declared for every root folder (§2) |
| G4 a mention is a use | impossible | edges come only from `uses:` and invoked programs (§4) |
| G5 ci.yml header or plan job is global | impossible | the plan job's inputs are edges to paddock-dev's planner tests and the jobs reading changed outputs (WP-2) |
| G9 npm edge gives the Rust half | impossible | an npm edge carries the UI half only (§4, WP-1 test) |
| G10 e2e guard files | checked | kept as `imports` edges; their positive control (§8) |
| G11 per-workspace runs | impossible | `selection` names packages on every run kind (WP-3) |
| G13 lessons file | checked | a declared input with its golden |
| G14 a red push widens the next base | impossible | the base is the last complete verdict from the one reader (§6) |

Every row has its positive control in the §8 table and its golden; the escape monitor (D12) is the net for what no structure covers.

**What still needs a human:** narrowing `docs/repo-graph.toml` (fewer files or targets for an input, or a new `none`/`doc` owner for
a path that was built), adding an `os.skip` exception, declaring an action setup-only, and lowering a project's smoke set. Each is
the unsafe direction, so each needs a golden diff in the same change and the user's approval (ER-46). Widening needs neither.

### 15. Extensible by language: one `Extractor` port

Extraction is an open set (open/closed, ports and adapters). The core knows nodes, edges, inputs and queries, never a language:

```rust
/// One adapter: reads its own manifests and sources from the bulk reader and emits nodes and edges.
pub trait Extractor {
    fn name(&self) -> &'static str;
    /// The files it claims to read (globs), so the owner check and `input_hash` know them.
    fn claims(&self) -> &[&'static str];
    /// Nodes and edges for these files, through `read` (the one bulk reader; no spawn of its own unless declared).
    fn extract(&self, files: &[String], read: &dyn BulkRead) -> Result<Extracted, ExtractError>;
    /// The processes it may start (`cargo metadata` per workspace): the spawn budget test sums these.
    fn spawn_budget(&self, files: &[String]) -> usize;
}
```

Adapters are registered in one place (`model::extractors()`), each behind its crate feature (§10), each with its own positive
controls (§8: remove its source line, its edge disappears) and its own spawn budget.

| Adapter | Reads | Emits | State |
|---|---|---|---|
| Cargo | `cargo metadata` per workspace | crate nodes, `depends`, test targets from `src_path` | today (moves in) |
| npm | workspaces, `tsconfig`/`angular.json`/ESLint path references, Tauri `frontendDist` | package nodes, `depends`, `path-ref` | today (moves in) |
| TS imports | `.ts`/`.mts`/`.mjs` sources (oxc) | `imports` | WP-3 |
| Rust in-crate | `mod`, `use crate::`, `cfg`, `include_str!`, `#[path]` (syn), `build.rs` | `imports` (module level), `reads`, `os` | WP-3, WP-6 |
| CI YAML | workflows and actions (the ADR 0060 reader) | workflow, job, step, action nodes; `uses`, `runs` | WP-2 |
| Generators | `docs/generators.toml` | generator nodes, `input`/`output` | WP-7 |
| Data reads | declared runtime `reads`, string-literal paths in tests | `reads` | WP-3 |
| .NET (planned) | `*.csproj`, `*.fsproj`, `*.sln`, `Directory.Build.props/targets`, `packages.lock.json`: `ProjectReference`, `PackageReference`, read by a parser or MSBuild's static evaluation, never by running a build | project nodes, `depends`, package-level edges | after this ADR, about 1 day |
| C/C++ (planned) | the CMake File API or `compile_commands.json` (targets, `#include` edges), `*.vcxproj` `ProjectReference` | target nodes, `depends`, `imports` | after this ADR, about 1 day |

A new language plugs in as one new adapter and one registry line; `reach`, `selection`, `input_hash` and `select_os` do not change,
because they walk nodes and edges, not languages. Until an adapter exists, its files are owned by the project folder that holds
them and a change to them reaches that project in full (fail closed). The repository has .NET today:
`apps/debrief/tools/dotnet-sample-gen` (a `.csproj` tool) and `apps/debrief/crates/debrief-plugin-dotnet/tests/fixtures/Busy` (a
fixture project). Both are owned by Debrief's folder now, and the Rust tests that build or read them
(`debrief-plugin-dotnet/tests/it/real_runtime.rs`, `debrief-core` and `debrief-plugins` `tests/it/real_libraries.rs`) get `reads`
edges to them in WP-3, so a change there selects those tests; the .NET adapter later adds the `.csproj` edges.

Common practice: Pants backends (one per language, with dependency inference), Bazel rules (`rules_rust`, `rules_dotnet`, `rules_cc`),
and Nx plugins (`createNodes` and `createDependencies`, nx.dev "Extending the Project Graph") are the same shape.

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
| WP-1 | Node and edge types, the `Extractor` port and its registry (§15), the inputs file and accessor, owners with precedence and fail-closed in the plan job, content extractors (`attributes`, `npm-lock`, `guards`), the bulk reader with a spawn budget, `reach` with kinds, the positive-control harness, the benchmark | 1.7 |
| WP-2 | The CI reader moved into the crate, `uses`/`runs` edges (contract check lines included), action and script nodes, mention vs use (G4), plan-job inputs (G5); `plan_gitea` deleted | 1.3 |
| WP-3 | Test files and targets from `src_path`, the `reads` extractor (`syn`), `imports` through oxc without `node_modules` and its determinism test, contract `tests`/`smoke` declarations, `selection`, per-package runs (G11), the ci.yml and app-gates wiring | 2.0 |
| WP-4 | Run scope v2, complete only on a complete base, the base from the verdict reader, the smoke-then-code golden; `last_full` asks main's first-parent commits, not only green push runs (§6) | 0.8 |
| WP-5 | `--compare` with dropped targets, train replay (trains 15 to 20), one golden per migrated rule, the listed differences, regenerated goldens | 2.0 |
| WP-6 | OS derivation with the wider signals, the `Command` measurement, `hostTests` cut to exceptions, C155 as the consistency check | 1.3 |
| WP-7 | Generators as nodes (`due`, stale after merge); `input_hash` on `input_hash.rs` with the added inputs | 0.8 |
| WP-8 | The landing run tests the train's diff (D11, G1, G14) | 1.0 |
| WP-9 | The escape monitor and the five-nightly gate before the old tables go (D12) | 0.7 |
| | The no-path-rule conformance check, positive controls for every node, edge and extractor kind (§8), fail-closed tests | included above |
| | **Total** | **11.6** (accepted 6.3; spent so far about 0.3 on WP-1). Approved by the user on 2026-11-23 ("over 8 d is OK as long as it fixes the root cause once and for all") |

The .NET and C/C++ adapters are not in this total (about 1 day each, when wanted). The total grew 84 % over the accepted 6.3 days, from the review's findings (the `reads` edge, the migration net and the escape
monitor are most of it). It went back to the user (ER-37), who approved it on 2026-11-23 on the condition that it fixes the root
cause once and for all: all nine work packages, in the order of §11.

## Decisions

- **D1. Scope.** *Chosen 2026-11-23: (a), widened to WP-1 to WP-9, 11.6 days (§14).* (a) WP-1 to WP-7, 6.3 days. (b) WP-1 to WP-5 now, WP-6 and WP-7 after the next release, 4.8 days.
  (c) Common practice: the five behaviours as planner rules, 3 to 4 days, no model. *Recommended: (b)*: it closes the planning class
  within the told budget; OS placement and generators have working hand lists with checks today.
- **D2. Where the model lives.** (a) `paddock-repo-graph`, extractors as modules behind the existing ports. (b) A new crate
  `paddock-repo-model` beside it. *Recommended: (a)*, share first (ER-22); the crate is already the planner's dependency.
- **D3. Inputs file.** (a) One `docs/repo-graph.toml` merged over crate defaults, project facts in `paddock.ci.json`. (b) Everything
  in `paddock.ci.json` files. *Recommended: (a)*: repository-wide inputs (toolchain, root npm files) belong to no project, as Nx's
  `nx.json` and Turborepo's root `turbo.json`.
- **D4. `.gitattributes`.** (a) Keep it a global (today). (b) The attribute extractor: a change of `text`, `eol`, `binary`, `filter`,
  `diff`, `working-tree-encoding` or `crlf` reaches the owners of the files its pattern matches (they change checked-out bytes);
  `merge=`, `linguist-*` and comments reach conformance only. *Chosen: (b)* (review P2-1).
- **D5. Reach kinds.** (a) The five kinds of §5 (`code`, `release`, `test`, `plumbing`, `docs`). (b) Only `code` and `plumbing`
  (smoke) now. *Recommended: (a)*: the user approved all four behaviours.
- **D6. A test helper no spec reaches.** (a) Full suite (cannot prove). (b) Nothing. *Recommended: (a)*; Pants and Jest
  `--findRelatedTests` run nothing, but a setup file referenced by config is reached by no import.
- **D7. Smoke and affected runs on a push to main.** (a) Allowed; such a commit has no complete verdict, so a tag on it waits for the
  nightly or a full dispatch. (b) A push to main always runs full. *Recommended: (a)*, as approved ("the full suite stays on the
  nightly, on app or crate code and on tags").
- **D8. Migration.** (a) Old and new side by side with `--compare` until the goldens match except the listed differences, then the old
  tables go in the same train. (b) Switch at once. *Recommended: (a)*.
- **D9. Trains.** (a) Hold all trains until WP-1 to WP-5 land (the user's instruction). (b) Let release-only trains through. *As
  instructed: (a)*.
- **D10. Extraction.** (a) Parsers as §10 (`oxc_parser`/`oxc_resolver`, `syn` or the C155 scan), cold start, one answer per run.
  (b) Language servers (rust-analyzer, tsserver) queried by the planner. (c) Text scans only (relative imports, `cfg` lines).
  *Recommended: (a)*, with (c) as the measured fallback; (b) is not common practice for build graphs and is too slow and stateful
  for CI.
- **D11. The landing run's base** (user, 2026-11-23, from the CI audit). (a) The landing run tests the train's diff from main's
  tested head and the push to main reuses that run, as GitHub merge queues and GitLab merge trains do. (b) The landing run tests
  the whole tree (today, ADR 0113). *Chosen: (a)*; the nightly and tags stay whole-tree on every OS.
- **D12. When the old rule tables go** (review P1-6). (a) After five consecutive nightlies with zero escapes (WP-9). (b) When the
  goldens match (WP-5). *Recommended: (a)*: Google TAP and Meta keep a full-run safety net and measure escapes before they trust
  a selection.

## Consequences

- A new planner question is a query or a declared input, not a rule; a file no node owns fails the graph's check instead of planning
  everything.
- `plan_gitea.rs`, the `GLOBAL` and input regex tables of `plan.rs`, the CI job table and the duplicate CI reader go; `plan_ci.rs`
  shrinks to output. `plan_since.rs` (reruns from logs) stays: it reads runs, not the repository.
- The ADR 0113 amendment's behaviours keep their text; its mechanism (planner rules, the `plumbingOnly`/`affectedTests` lists) is
  replaced by `selection`.

Recurring: "a planner fact that is a rule over paths instead of a node or an edge of the graph". Earlier instances: the table above
(train 20 commit `298cc726b`'s plan; `plan.rs` `CONFORMANCE_CODE`; `hostTests` on `c61339b9f`; `docs/generators.toml:40`).
