# A standard monorepo build system instead of our own graph? (2026-11-23)

- Asked by the user, 2026-11-23: would a standard monorepo build system be easier than our own graph (ADR 0117:
  `crates/paddock-repo-graph` plus `./dev plan`)? The graph is about to land; the decision is in about two weeks, with
  measurements. This note prepares that decision: moonrepo, Nx, Turborepo, Pants and Bazel compared for this repository, what
  each would replace and keep, the cost, and what to measure meanwhile.
- Inputs: ADR 0117 (`origin/docs/adr-repo-graph-model`, `docs/adr/0117-the-repository-graph-is-the-one-model-and-every-plan-is-a-query.md`)
  and the CI run audit (`origin/docs/ci-run-audit`, `docs/research/ci-run-audit-2026-11-23.md`, its section 2.1 already compares
  how these tools select; this note adds fit, cache, platform and cost). Nothing here is built or measured on a runner: no builds,
  no CI dispatch.
- **Evidence heads** (ADR 0091), read 2026-11-23 and re-read before the commit: `origin/main` = `d04af7b46`,
  `origin/docs/adr-repo-graph-model` = `fc2c85f71`, `origin/docs/ci-run-audit` = `365d15bb7`,
  `origin/train/2026-11-23-graph` = `64e01dd84` (the graph train, 185 commits over main), `origin/docs/adr-test-result-cache` =
  `4e984ce2c` (ADR 0116 draft). Shipped-code check: the graph model is on no tag (`git tag --contains 64e01dd84` is empty) and not
  on `origin/main` (no `tools/paddock-dev/src/repo_model*` there); every statement about it below is about the train, not shipped code.
- **Tool versions** (GitHub API `releases/latest`, read 2026-11-23): moon v2.6.5 (2026-10-29), Nx 23.3.3 (2026-11-20),
  Turborepo v2.11.13 (2026-11-18), Bazel 9.3.3 (2026-11-19), Pants 2.32.2 (2026-11-01; docs show 2.33), rules_rust 0.75.0
  (2026-10-26), aspect rules_js v3.6.1 (2026-11-12), bazel-remote v2.6.4 (2026-10-20), `@monodon/rust` 3.0.0 (npm, 2026-06-01).
  Licences (GitHub API): moon, Nx, Turborepo MIT; Bazel, Pants, rules_rust, rules_js, bazel-remote Apache-2.0;
  `@monodon/rust` MIT (npm metadata); `ducktors/turborepo-remote-cache` MIT. All inside our allow-list (PRINCIPLES, Dependencies).

## 1. The answer in short

No standard tool removes the part of ADR 0117 that cost the most to get right. **None of the five models CI workflows, jobs and
shared actions as graph nodes** (ADR 0117 "Common practice"; the audit's section 2.1; confirmed again in each tool's docs below),
and the two trains measured in the audit went to "everything" mostly through CI plumbing, the missing landing base and
whole-file globals (G1, G4 to G8), not through the Cargo and npm graph, which was already precise. So any tool would sit beside
our CI layer, run scope (ADR 0113) and verdict reader, not replace them.

What a tool *would* replace is the project/package graph, the declared inputs and the task cache. The best fits are Nx (Angular,
Vitest and Playwright first-party; Rust only through a community plugin that reads **one** Cargo workspace, so we would write our
own) and moon (first-party Rust toolchain that handles several Cargo workspaces; Angular and Playwright as plain tasks). Turborepo
does not fit our Cargo layout (its native Cargo mode needs a single root virtual workspace). Pants has no Rust backend and no
native Windows. Bazel fits the caching problem best and the repository worst (12 lockfiles, 145 Cargo packages, Tauri bundling,
Angular, all to be re-expressed in Starlark).

**Recommendation:** keep ADR 0117 and spend the next two weeks measuring it (section 6); build ADR 0116's test-result cache on its
`input_hash` only if the measurements say the graph holds. The fallback, if the graph's upkeep or escapes cross the thresholds in
section 7, is the cheaper common-practice option ADR 0117 already offers (D15 (b): every reached project runs in full, about 0.2
agent-day), not a tool migration. A tool migration (Nx first, moon second) is the option to take only if we also want a
ready-made task cache and accept running two systems for the CI layer. Bazel and Pants are not options for this repository.

## 2. This repository, as the tools would see it

Counted at `origin/main` `d04af7b46` (`git ls-files`):

- **Rust**: 145 Cargo packages outside fuzz, spike, vendor, research and golden fixtures; 23 tracked `Cargo.lock` files, of which
  about 12 workspaces are built by CI (seven app workspaces, `crates/`, `tools/paddock-dev`, `tools/heavy`, two demos; ADR 0117 §9).
  **There is no root Cargo workspace** (AGENTS.md, "Cargo"): each app keeps its own features and lockfile. 3,730 `.rs` files.
- **Tauri 2**: 7 `tauri.conf.json` under `apps/*/app/src-tauri` plus the two demos; bundles and signing through the
  `tauri-bundle`, `keychain`, `macos-verify` and `strip-signing-keys` actions.
- **Node**: one npm workspace (`package.json` workspaces: `ui`, `eslint`, `icons`, `conformance`, two demos, `apps/*/app`), 87
  `package.json` files, one `package-lock.json`; Angular in `ui/` (an Angular CLI workspace) and each app's `app/`; Vitest;
  Playwright in 10 configs (Chromium and WebKit); 3,777 `.ts` and 630 `.mjs` files; conformance in Node (63 check modules).
- **CI**: 27 Gitea Actions workflows and 30 local actions; `runs-on` counts: `ubuntu-latest` 81, `macos-latest` 6,
  `windows-latest` 1, matrix 12. Runners: six Linux lanes, one Mac, one Windows.
- **Caches**: sccache with local disk first and the shared RustFS S3 store behind it (`.gitea/actions/setup-rust/action.yml`
  input `sccache-backend`, default `s3`; endpoint and bucket in `docs/dev-setup.md`, "The shared RustFS cache"); the runners' Actions
  cache is also on RustFS (`docs/dev-setup.md`, "CI's Actions cache"). No task or test-result cache (ADR 0116 is a draft).
- **The graph train** (`origin/train/2026-11-23-graph`): the model code is about 16,300 added lines with tests
  (`crates/paddock-repo-graph`, `tools/paddock-dev/src/repo_model*`, `plan_graph*`, `run_scope.rs`; `git diff --shortstat`
  against main), and `docs/repo-graph.toml` is 390 lines. ADR 0117 budgets 12.4 agent-days in total.
- **Cost today**: a train round is about 700 to 730 job-minutes (train 19: 731; train 20 round 1: 704), of which 410 to 425 Linux,
  100 to 125 Mac and 180 to 195 Windows (`docs/benchmarks.md` on `origin/docs/ci-run-audit`, "CI rounds of trains 19 and 20 by
  runner class"). ADR 0116's draft measured 75 to 110 job-minutes per round (28 to 34 %) in jobs whose test inputs did not change.

## 3. The tools, question by question

### 3.1 moonrepo (moon v2.6.5)

1. **Graph and affected.** Projects and tasks declared in `moon.yml` per project (inferred by toolchain plugins where they exist);
   task `inputs` decide affected; `moon run :test --affected` and `moon query affected` (moon v2.0 post, 2026-02-18). In v2 relations
   are not included by default (secondary source only, **unverified**). Global config, lockfile and toolchain changes affect every
   project (audit §2.1).
2. **Remote cache.** Bazel Remote Execution API v2 over gRPC or HTTP (`remote.host`, `api: 'http'`), self-hosted since v1.30, example
   `bazel-remote`; HTTP retries since v2.6; TLS/mTLS described as "rudimentary" (moon docs, "Remote caching"). Not S3 directly; the
   path to RustFS is moon → `bazel-remote` (Apache-2.0, `--s3.endpoint` for MinIO-compatible stores) → RustFS. Caches a task's
   outputs plus stdout/stderr, so a cached test task replays its log; nothing test-specific (no per-test result). Not tested here with
   RustFS (**not measured**).
3. **Stack support.** Rust: first-party toolchain (WASM plugin), Cargo manifest and lockfile hashing, and since v1.37 "multiple Cargo
   workspaces in the same moon workspace" (moon v1.37 post, 2025-06-03); moon "is not a build system", runs `cargo`, and advises
   against caching `target/` (moon Rust handbook). Node/npm toolchain first-party. Angular, Vitest, Playwright, Tauri: no dedicated
   support, all hand-declared tasks.
4. **Windows/macOS.** Both supported (PowerShell installer in the v2.0 post); our Windows runner would need the moon binary pinned.
5. **CI config.** Not modelled; `moon ci` runs affected tasks, CI files are at most a task input.
6. **Migration cost: 8 to 12 agent-days** (section 4).
7. **Keeps / replaces** (section 5).
8. **Fit.** MIT, small vendor (about 4,200 stars, frequent releases). One Rust binary, no daemon; the cache server is ours to run.

### 3.2 Nx (23.3.3) with a Rust plugin

1. **Graph and affected.** Project graph from plugins (`createNodesV2`, `createDependencies`), task-level selection: "Nx selects a
   task when your change touches one of its inputs" (audit §2.1); `namedInputs`, `targetDefaults.inputs`, lockfile narrowing with
   `projectsAffectedByDependencyUpdates: "auto"` for JS lockfiles. A Cargo lockfile is only what the Rust plugin makes of it.
2. **Remote cache.** Nx Cloud (paid, hosted) or self-hosted: `@nx/s3-cache`, `@nx/gcs-cache`, `@nx/azure-cache`,
   `@nx/shared-fs-cache` ("free but require an activation key", obtained during install), or a custom server to an OpenAPI spec
   (`PUT`/`GET /v1/cache/{hash}`, tar bodies, bearer token; Nx 20.8 and later). The page warns that bucket-based caches are open to
   cache poisoning (CREEP, CVE-2025-36852) and recommends self-hosting only "for proof of concepts and small teams"
   (21.nx.dev, "Self-hosted caching"). RustFS: `@nx/s3-cache` against an S3-compatible endpoint is a secondary-source claim
   (**unverified**); a custom server in front of RustFS is about a day. Any target with `cache: true` is cached, tests included
   (terminal output replayed).
3. **Stack support.** Angular: first-party (`@nx/angular`; Nx 22.x added Angular 21, Nx 23.1 Angular 22). Vitest and Playwright:
   first-party plugins (`@nx/vite`/`@nx/vitest`, `@nx/playwright`; splitting e2e by file exists, distribution of the split runs is an
   Nx Cloud feature, the Playwright page did not load in this session, **unverified**). Rust: no official plugin; Nx's own docs use
   the community `@monodon/rust` (3.0.0, MIT, needs Nx 22). Its `graph.ts` (read at `Cammisuli/monodon` `main`, 2026-11-23) globs
   every `*/**/Cargo.toml` but runs **one** `cargo metadata` from the workspace root, so with no root Cargo workspace it sees nothing
   or one workspace. We would write our own `createNodesV2` plugin (12 `cargo metadata` calls, path dependencies as edges).
   Tauri: `run-commands` targets. `.NET` is first-party since Nx 23 (`@nx/dotnet` GA, Nx 23 post, 2026-06-17), relevant to Debrief's
   planned ClrMD helper.
4. **Windows/macOS.** Both supported (terminal UI on Windows since 22.1).
5. **CI config.** Not modelled; CI files are inputs only if listed (`sharedGlobals`). Nx Cloud adds distribution and flaky-task
   retries; we would not use it.
6. **Migration cost: 10 to 15 agent-days** (section 4).
8. **Fit.** MIT core, very large community (about 29,500 stars), a Node daemon on every machine. The free self-hosted cache needs a
   vendor activation key (a phone-home at install) or our own server.

### 3.3 Turborepo (2.11.13)

1. **Graph and affected.** Package graph from package manager workspaces; `--affected` or `--filter=...[ref]`; task `inputs`,
   `globalDependencies` (every match busts every task). Package-level, optionally task-level with `affectedUsingTaskInputs` (audit).
2. **Remote cache.** Vercel's, or any HTTP server implementing the Remote Cache API spec (`/api/remote-cache-spec`); community servers
   include `ducktors/turborepo-remote-cache` (MIT, S3 storage); "Remote Caching is free" (turborepo.dev, "Remote Caching", updated
   2026-11-20). Test tasks cache like any task.
3. **Stack support.** JS/TS-first. Rust: either a `package.json` wrapper per Cargo workspace, or the experimental
   `experimentalCargoWorkspaces` flag, which needs **a root virtual Cargo workspace** with `[workspace.metadata].name` and hashes
   files a crate reads outside its folder only if declared per task ("Rust support is experimental and can change at any time",
   turborepo.dev, "Rust"). We have no root workspace by design, so only the wrapper route works, at workspace granularity (coarser
   than our package level, G11). Angular, Playwright, Tauri: plain scripts.
4. **Windows/macOS.** Supported.
5. **CI config.** Not modelled; root `turbo.json`, `package.json` and the lockfile select everything.
6. **Migration cost: 6 to 9 agent-days**, for a coarser result than ADR 0117 (section 4).
8. **Fit.** MIT, Vercel-backed, large community. Cheapest tool, weakest on Rust.

### 3.4 Pants (2.32 / 2.33)

1. Target and file-level graph by dependency inference, `--changed-since --changed-dependents=transitive` (audit §2.1).
2. Remote cache and execution over REAPI (same `bazel-remote` → RustFS path).
3. Languages: "Python, Go, Java, Scala, Kotlin, and Shell" (pantsbuild.org, "Welcome to Pants", 2.33); **no Rust backend** (the
   engine is Rust; the build languages are not). JavaScript support exists in the docs sidebar, level not stated here.
4. **No native Windows**: Pants runs on Windows only under WSL 2 (pantsbuild.org, "Prerequisites").
6. **Excluded**: our core language and one of our three OS are unsupported. A Rust backend would be a plugin we write (more than
   ADR 0117's whole budget, estimate not refined).

### 3.5 Bazel (9.3.3) with rules_rust, crate_universe, rules_js

1. Target graph from declared `BUILD` files; `rdeps` is reach; hermetic actions, so a file no target declares reaches nothing.
   Bazel 9.0 (blog, 2026-01-20) removed WORKSPACE; Bzlmod only.
2. Remote cache over REAPI, test results cached by input digest (`(cached)`); `bazel-remote` with `--s3.endpoint` to RustFS. The best
   cache of the five, tests included; the only one where a passing test target is skipped by construction.
3. Rust: rules_rust with crate_universe (`crate.from_cargo`, one instance per `Cargo.lock`, named repositories; `x86_64-pc-windows-msvc`
   in the default platforms; rules_rust docs). Each of 145 packages needs a `BUILD` target (generated or hand-written), every
   `build.rs` becomes `cargo_build_script` (tauri-build reads `tauri.conf.json` and `frontendDist` and the capabilities folder, which
   must then be declared data). Tauri: no Bazel support found; the only example (`tauri-on-bazel`) is a Tauri 1.5 demo on Bazel 6 that
   wraps the CLI in `bazel run` targets, so bundling and signing stay outside the hermetic graph. JS: aspect rules_js (v3.6.1) is
   pnpm-lockfile based (our `package-lock.json` would be translated or replaced; **unverified** for npm v10 lockfiles); Playwright through
   `rules_playwright` (0.2 to 0.4 on the Bazel Central Registry). Angular: no maintained first-party Bazel rules since the Angular team
   left Bazel (**unverified** current state).
4. Windows needs MSYS2 for shell targets and Visual C++ tools (bazel.build, "Installing Bazel on Windows"); macOS fine.
5. CI config: not a build input at all.
6. **Migration cost: 40 to 70 agent-days**, plus ongoing upkeep of `BUILD` files and rules upgrades.
8. **Fit.** Apache-2.0, the industry reference for large repositories (Google, and Chromium's own GN). Its cost is built for
   hundreds of engineers; for one user with agents it is the opposite of common practice for a project this size (ER-37).

## 4. Comparison table and migration cost

| | ADR 0117 (ours) | Nx | moon | Turborepo | Pants | Bazel |
|---|---|---|---|---|---|---|
| Graph unit | package, job, step, action, test file | task (project:target) | task (project:task) | package (task optional) | target/file | target |
| Several Cargo workspaces, no root | yes (12 `cargo metadata`) | **no** with `@monodon/rust` (one root `cargo metadata`); own plugin needed | **yes** (since v1.37) | **no** natively (root virtual workspace required); wrappers only | no Rust | yes (one `from_cargo` per lockfile) |
| Angular / Vitest / Playwright | via `imports` edges (oxc) and contracts | first-party | tasks | tasks | JS level unclear | rules_js, rules_playwright; Angular unclear |
| Tauri bundle and signing | existing actions | `run-commands` | task | script | n/a | outside the hermetic graph |
| CI workflows, actions as nodes | **yes** | no | no | no | no | no |
| Test-result cache | ADR 0116 draft, on RustFS | any cached target; S3 plugin (activation key) or own server | REAPI → bazel-remote → RustFS | Remote Cache API → ducktors (S3) | REAPI | REAPI, per test target, best |
| Windows / macOS native | yes / yes | yes / yes | yes / yes | yes / yes | **WSL only** / yes | yes (MSYS2) / yes |
| Licence | ours | MIT (Cloud proprietary) | MIT | MIT | Apache-2.0 | Apache-2.0 |
| Migration (agent-days) | 12.4 budgeted, mostly spent on the train | 10 to 15 | 8 to 12 | 6 to 9 | excluded | 40 to 70 |
| Granularity after migration vs ADR 0117 | reference | task; test-file selection lost (no import-based spec selection without Cloud) | task; coarser | workspace; coarser | n/a | finer (target) |

**Breakdown of the migration estimates** (raw agent-days, my estimates from the counts in section 2; none measured):

| Work | Nx | moon | Turborepo | Bazel |
|---|---:|---:|---:|---:|
| Tool setup, pinning on 3 OS, CI image | 0.5 | 0.5 | 0.5 | 2 |
| Rust graph (12 workspaces, 145 packages) | 2 (own `createNodesV2` plugin) | 1 (toolchain config, per-workspace projects) | 1 (12 `package.json` wrappers) | 12 to 20 (`BUILD` files, crate_universe per lockfile, build scripts, sys crates) |
| Node graph, Angular, Vitest, Playwright | 1.5 (plugins, `nx init` over angular.json) | 2 (hand tasks for ~25 projects) | 1 (scripts) | 10 to 20 (rules_js, Angular, Playwright browsers) |
| Inputs (today's `docs/repo-graph.toml`, 390 lines) as named inputs | 1.5 | 1.5 | 1 | in `BUILD` data deps (above) |
| Tauri build, bundle, sign as tasks (Mac, Windows, Linux) | 1 | 1 | 1 | 4 to 6 |
| Remote cache on RustFS (server, auth, eviction, ER-51 sweep) | 1 (own OpenAPI server) | 1 (bazel-remote) | 1 (ducktors) | 1 (bazel-remote) |
| Wiring into our plan job, run scope and verdicts (matrix from `--affected --json`) | 1.5 | 1.5 | 1.5 | 3 |
| Goldens, train replay against ADR 0117's answers, positive controls (ER-33) | 1.5 | 1.5 | 1 | 3 |
| Conformance, docs, rules, `paddock.ci.json` contracts | 1 | 1 | 0.5 | 2 |
| Risk margin (first real trains on all three OS) | 1 to 4 | 1 to 3 | 0.5 to 1.5 | 5 to 12 |
| **Total** | **10 to 15** | **8 to 12** | **6 to 9** | **40 to 70** |

Every tool total is on top of what ADR 0117 already spent, and none removes the CI layer (section 5), so the honest comparison is
"tool migration" against "the remainder of ADR 0117 plus ADR 0116".

## 5. What of ADR 0117 we would keep under any tool, and what a tool replaces

| ADR 0117 part | Under Nx / moon / Turborepo | Why |
|---|---|---|
| CI as graph: workflow, job, step, action, script nodes; `uses`/`runs` edges; mention vs use (G4 to G7) | **keep** | no tool models CI (section 1); this layer decided both measured trains |
| Landing base and run scope (WP-4, WP-8, ADR 0113), verdict reader, ER-15/ER-49 reuse | **keep** | train and verdict semantics are ours; tools only answer "affected since base" |
| Escape monitor and the five-nightly gate (WP-9) | **keep** | Google TAP and Meta practice; no tool ships it |
| OS placement (`os` edges, `hostTests`, C155, WP-6) | **keep** | no tool infers OS placement (Bazel declares it by hand) |
| Owner check, fail closed on an unowned file | keep, or accept the tool's default (an undeclared file reaches nothing: Nx, Bazel) | the tools fail open on undeclared files, which is the class ADR 0117 closes |
| Cargo and npm project graph, lock diffs | replaced | the tool's project graph (with our plugin for Cargo under Nx) |
| `docs/repo-graph.toml` inputs | replaced by `namedInputs` / task `inputs` / `globalDependencies` | same shape (ADR 0117 §3 is modelled on them) |
| `input_hash` and ADR 0116's test-result cache | replaced | the tool's task hash and remote cache |
| Test-file selection (`imports` via oxc, D6) | lost under Nx, moon, Turborepo (whole test task runs) | only Pants and Bazel select below the task; Playwright `--only-changed` covers spec-only edits |
| Generators as nodes (WP-7) | keep or re-express as tasks with outputs | tasks with outputs fit Nx and moon well |

So a tool replaces roughly the Cargo/npm graph, the inputs file and the planned cache; the CI layer, the run scope, the verdicts,
the escape monitor and the OS placement stay ours and would have to read the tool's answer. That is two models of the repository,
which is the class ADR 0117 exists to remove (its "Context: the class").

## 6. Measurements to collect in the next two weeks

All from CI metadata, the planner's own output and git history; synthetic or run data only, never user logs. Record each in
`docs/benchmarks.md` (rule 48) under one heading, with the method of the audit's table.

| # | Measurement | How | Baseline |
|---|---|---|---|
| M1 | **Job-minutes per train round**, by runner class (Linux, Mac, Windows) | the audit's method: Gitea API `/actions/runs/<id>/jobs`, wrapper jobs out, per round | 704 to 731 per round (trains 19, 20) |
| M2 | **Selection size per round**: projects, packages, test targets and e2e specs selected vs total; how many plans answered "everything" and the reason line | `./dev plan --ci --explain` output stored per run; `--compare` with the old tables | trains 19 and 20: everything, every round |
| M3 | **Nightly escapes**: a nightly failure bisected to a commit whose selection dropped it | WP-9's escape monitor; `not measured` when the bisect bound is hit | none recorded yet |
| M4 | **Graph upkeep time** | agent-hours on commits touching `crates/paddock-repo-graph`, `tools/paddock-dev/src/repo_model*`, `plan_graph*`, `docs/repo-graph.toml`, split `fix:` vs `feat:`; count of unowned-file errors and owner declarations added; review findings on graph code | not measured |
| M5 | **Planner time** in the plan job | the plan step's duration (budget: 3 s Mac, 5 s Linux, ADR 0117 §10) | 2.9 s `--all`, 12.8 s `--explain` over 20 commits (benchmarks, ADR 0117) |
| M6 | **Unchanged-input job-minutes** (what a task or test cache would save) | ADR 0116's shadow method over the graph's `input_hash` | 75 to 110 per round (ADR 0116 draft) |
| M7 | **Wall-clock time to a green train** and Mac/Windows queue time | run start to verdict; queue from `created` to `started` | not measured |

M1, M2 and M6 decide whether a cache is the next lever; M3 and M4 decide whether the graph is a burden. A tool would change M4
and M6, not M1's CI-plumbing share.

## 7. Options for the decision (in about two weeks)

Proposed thresholds (mine, for the user to set): the graph "holds" if M3 shows **zero escapes over at least five nightlies**,
M4 is **under about half a day of graph fixes per week** after the first week, and M1 falls **at least 30 %** below 704 to 731 on
trains that do not bump a shared lockfile.

- **A. Keep ADR 0117 and add ADR 0116's test-result cache on its `input_hash`** (*recommended* if the graph holds). Cost: the rest of
  ADR 0117 as budgeted, plus ADR 0116's work packages (its own estimate). Keeps one model; RustFS already holds sccache and the Actions
  cache.
- **B. Keep ADR 0117, switch to whole-project selection** (D15 (b), about 0.2 agent-day): the common practice of Nx, Turborepo and
  Bazel target determinators. Take it if M3 shows escapes from the smoke or affected narrowing; the graph and CI layer stay.
- **C. Adopt Nx (or moon) for the project graph and the task cache, keep our CI layer, run scope and verdicts** (10 to 15, or 8 to
  12 agent-days on top). Take it only if M4 shows the Cargo/npm half of the graph is where the upkeep goes and a ready-made cache is
  worth two models. Nx if the Angular/Playwright half matters more (first-party plugins, but our own Cargo plugin and a vendor
  activation key or own cache server); moon if the Rust half matters more (several Cargo workspaces first-party, cache via
  `bazel-remote` on RustFS).
- **D. Turborepo**: not recommended; workspace-level Rust is coarser than today's member runs, and native Cargo support needs a root
  workspace we do not want (it would unify features and lockfiles across apps, against AGENTS.md "Cargo").
- **E. Bazel or Pants**: not options. Pants lacks Rust and native Windows; Bazel costs 40 to 70 agent-days and leaves Tauri bundling
  and the CI layer outside its graph.

Common practice (ER-37): for a small team with libraries shared by several apps, Nx or Turborepo with whole-project affected
selection and a remote cache is the norm; Bazel is chosen by organisations with build teams. ADR 0117's extra depth (CI nodes,
test-file selection, OS inference, fail-closed owners) goes beyond that norm; the user approved it on 2026-11-23 ("as long as it
fixes the root cause once and for all"). The cheaper common-practice option inside our own tool is B, at 0.2 agent-day.

## 8. Risks

- **Sunk cost bias** cuts both ways: the graph's 16,300 lines are already written, but their upkeep is the real cost; M4 measures it.
- **Two models** (option C) would bring back the class ADR 0117 names: a planner fact kept in two places that drift.
- **Cache poisoning**: any shared task cache that CI and agents write is a supply-chain path (CVE-2025-36852 for bucket caches);
  ADR 0116's draft limits writers to main and train runs, which a tool cache would also need.
- **Vendor drift**: Nx's free self-hosted cache depends on an activation key and Nx's licensing choices; moon is a small vendor.
- **Not measured here**: none of the tools was run on this repository. Option C's estimate assumes a one-day spike first
  (Nx or moon over `crates/` and one app on all three OS) before any commitment.

## Sources

Read 2026-11-23 unless a date is given.

- moon: "Remote caching", https://moonrepo.dev/docs/guides/remote-cache; "Rust handbook", https://moonrepo.dev/docs/guides/rust/handbook;
  "moon v2.0" (2026-02-18), https://moonrepo.dev/blog/moon-v2.0; "moon v1.37" (2025-06-03), https://moonrepo.dev/blog/moon-v1.37.
  v2 affected details from secondary sources only (**unverified**).
- Nx: "Self-hosted caching" (Nx 21 docs; the current-version URL returned 404), https://21.nx.dev/docs/guides/tasks--caching/self-hosted-caching;
  "Nx 23" (2026-06-17), https://nx.dev/blog/nx-23-release; "Nx 22" (2025-10-23), https://nx.dev/blog/nx-22-release;
  Rust guide, https://nx.dev/docs/kb/add-rust-to-nx-workspace; `@monodon/rust` source,
  https://github.com/Cammisuli/monodon/blob/main/packages/rust/src/graph.ts and `packages/rust/src/utils/cargo.ts`;
  affected and inputs: the audit's sources.
- Turborepo: "Remote Caching" (updated 2026-11-20), https://turborepo.dev/docs/core-concepts/remote-caching; "Rust",
  https://turborepo.dev/docs/guides/tools/rust; "Multi-language", https://turborepo.dev/docs/guides/multi-language;
  https://github.com/ducktors/turborepo-remote-cache.
- Pants: "Welcome to Pants" (2.33), https://www.pantsbuild.org/stable/docs/introduction/welcome-to-pants; "Prerequisites",
  https://pantsbuild.org/dev/docs/getting-started/prerequisites.
- Bazel: "Bazel 9.0" (2026-01-20), https://blog.bazel.build/2026/01/20/bazel-9.html; "Installing Bazel on Windows",
  https://bazel.build/install/windows; rules_rust crate_universe (Bzlmod), https://bazelbuild.github.io/rules_rust/crate_universe_bzlmod.html;
  rules_playwright, https://registry.bazel.build/modules/rules_playwright/0.3.0; `tauri-on-bazel`,
  https://github.com/setoelkahfi/tauri-on-bazel; bazel-remote, https://github.com/buchgr/bazel-remote. Bazel test caching:
  the audit's sources.
- Versions and licences: GitHub REST API `repos/<owner>/<repo>` and `releases/latest` for moonrepo/moon, nrwl/nx,
  vercel/turborepo, bazelbuild/bazel, pantsbuild/pants, buchgr/bazel-remote, bazelbuild/rules_rust, aspect-build/rules_js,
  Cammisuli/monodon, ducktors/turborepo-remote-cache; npm registry `@monodon/rust`.

Recurring: none new. This note is an input to ADR 0117's class ("a planner fact that is a rule over paths instead of a node or an
edge of the graph"); option C would reopen it as "one repository fact held in two models".
