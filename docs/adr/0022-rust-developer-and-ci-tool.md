# 22. One Rust developer and CI tool (`paddock-dev`) on shared repository crates, after the CI kit consolidation

- Status: Proposed (2026-10-27)
- Date: 2026-10-27
- Related: rules 3–5 (affected tests, fix locally, failing test first), rule 7 (tests never touch the machine), rule 13
  (bounded parallelism), rule 23 (ad-hoc sealing), rule 40 (recurring bugs mean a missing design rule), rules 42/43
  (share first, shared behaviour tested once); ADR 0014 §6–§7 (affected CI, no task runner), ADR 0021 (portable tests,
  C38, counted waivers); Pacenotes ADR 0001 and `apps/pacenotes/docs/proposal.md` §14, §9.7, §23, §24
- Evidence: the research note *Nx, Turborepo or our own scripts* (`docs/research/2026-10-nx-vs-scripts.md`, branch
  `docs/research-nx`, §1, §5, §6, §9); the inventory and measurements in
  [research note 2026-10 Rust tooling inventory](../research/2026-10-rust-tooling-inventory.md) (every script, per file);
  `origin/main` at `932b1728b`

## Context

The user decided on 2026-10-27: first consolidate the apps onto the shared CI kit (WP-C1 to WP-C6 of the research note,
in progress), then move the build and CI scripts to a Rust implementation **where it pays off**. The user then asked two
follow-up questions that this ADR answers too: can **all** scripts be Rust, and what does each option do to **CI speed**.
Parts of the tooling should also be reusable by Pacenotes (dependency graph, rules engine over a repository).

What the research note found, and what still holds after the consolidation:

- About 21,200 non-test lines of CI and build tooling; a task runner would replace only the 6% that is affected
  detection. The rest is release, signing, bundle verification, sidecars, conformance, OS-specific runner set-up and
  app checks. The consolidation deletes about 4,900 lines of copies.
- Of the 111 `fix:`/`ci:` commits on tooling paths since 2026-10-13, **none** fixed a wrong selection in `affected.mjs`.
  They were OS specifics (33), signing and bundles (31), test portability (22), sccache and build folders (19), the
  browser (11), secrets (5) and lints (5). Re-counted for this ADR: 110 distinct subjects (the imported histories repeat
  some commits), with the same shape.

Why a Rust tool is on the table at all: the failures cluster in **bash inside YAML and shell scripts that branch per OS**
(Git Bash on Windows with `cygpath`, `cmd //c mklink //J`, `powershell Expand-Archive`; `sha256sum` versus `shasum`;
`codesign`/`hdiutil`/`xattr` on macOS). That code has no unit tests that run on all three systems: the composite actions
(22 files, 334 lines of inline shell) have **no tests at all** and took 36 fix commits; `ci.yml`'s 48 inline `run:`
blocks took 40. Rust code in `crates/` is built, linted (including `cargo xwin clippy` for Windows, ADR 0021) and tested on
Linux, macOS and Windows by the existing crate legs, and the family already has the building blocks the scripts
re-implement badly: `paddock-proc` (subprocesses with timeouts and output caps), `paddock-download` (hash while writing,
retries), `paddock-archive` (safe unzip/untar), `paddock-env`, `paddock-cli` (exit codes, `--json`), `paddock-chromium`
(the pinned test browser, screenshots) and `paddock-testkit` (`FakeRunner` for scripted OS commands, golden files,
hermetic environment).

## 1. Inventory after consolidation

Method (verified): every tracked tooling file under `scripts/`, `conformance/`, `tools/`, `ci-image/`, `diagrams/`,
`icons/tools/`, `docs/images/src/`, `templates/repo/scripts/`, `.gitea/actions/*` and the apps' `scripts/`, `app/scripts/`
and `docs/diagrams/src/`, excluding tests and fixtures. "fix:/ci:" counts distinct commit subjects since 2026-10-13 that
touched the file (a commit that touched three files counts once per file). "OS refs" counts OS-specific tokens
(`win32`, `darwin`, `codesign`, `hdiutil`, `cygpath`, `RUNNER_OS`, `.exe`, ...) as a proxy for per-OS branches. "Untested"
means no test file names the script (a heuristic). Workflows (`.gitea/workflows/*.yml`) stay YAML and are counted apart.

| Class | Group | Files | Lines | fix:/ci: | OS refs | Untested lines |
|---|---|---|---|---|---|---|
| **Rust, phase 1** | Release, signing, bundles, sidecars, verify (`conformance/release/*`, `conformance/ci/tauri-bundle.sh`, `conformance/src/sidecar`, Debrief `build-sidecars.mjs` and `scripts/macos/*`, Dyno `check-bundle.sh`, demo release and verify) | 15 | 1,267 | 19 | 141 | 148 |
| | Runner set-up, pinned downloads, test browser (`conformance/ci/{build-env,build-folder,install-test-browser,pin-runtime,runtime-catalogue}`, three libduckdb fetchers, Spotter's browser pins, Debrief's two Python runtime updaters) | 12 | 1,089 | 11 | 92 | 296 |
| | Planner, timings, coverage gate (`scripts/affected.mjs`, `conformance/ci/plan.mjs`, `scripts/ci-timings.mjs`, `scripts/coverage-gate.mjs`) | 4 | 1,003 | 7 | 0 | 355 |
| **Rust, phase 2** | Host tools, smokes, verify-installed (`tools/*.sh`, `prune-build-folders.{sh,ps1}`, per-app `linux-*`/`macos-*` verify and container scripts, Debrief CI smokes, shell-demo smokes) | 24 | 1,555 | 6 | 162 | 785 |
| | Notices, window weight, benches, plugin and font builds | 11 | 976 | 2 | 31 | 96 |
| | Scaffolder and package publisher (`scripts/new-app.mjs`, `scripts/publish.mjs`) | 2 | 990 | 8 | 3 | 0 |
| **Rust, phase 3** | Conformance engine and checks C03–C37 (`conformance/src/**`) | 43 | 2,534 | 8 | 44 | 759 |
| | App checks that become rules (Debrief `check-{quarantine,smoke-dialogs,temp-leaks}`, `check-workflows.py`, Dyno `check-capabilities`, Pitwall `privacy-check.sh`) | 6 | 320 | 7 | 17 | 57 |
| **Thin YAML** | Composite actions `.gitea/actions/*` (inline shell moves into the tool; 815 YAML lines, 334 of them shell) | 22 | 334 | 36 | 61 | all |
| **Keep JS/TS** | Playwright screenshot and visual-QA suites (Debrief `screenshots/`, `visual-qa/`) | 5 | 720 | 9 | 6 | 720 |
| | TypeScript type-checker analysis (`check-dialog-commands`, `dialog-commands-analysis`) | 2 | 633 | 4 | 8 | 0 |
| | UI source lints (`lint-effects`, `lint-styles`; move into the ESLint preset) | 2 | 243 | 1 | 0 | 0 |
| | npm lifecycle, Angular i18n, UI coverage glue (`build-info`, `whats-new`, `i18n/*`, Pitwall `tauri-before-build`, `ui-coverage-guard`) | 6 | 485 | 1 | 3 | 76 |
| **Keep JS for now** | Icon pipeline `icons/tools/` (author time; `@resvg/resvg-js`, `playwright-core`) | 23 | 4,098 | 18 | 125 | 2,097 |
| | Diagram and poster generators (`diagrams/tools/`, `docs/images/src/`, each app's `docs/diagrams/src/`) | 77 | 7,277 | 3 | 138 | 6,486 |
| **Keep shell** | Bootstrap (`ci-image/*.sh`, `tools/bootstrap.sh`, `scripts/setup-dev.sh`, Pitwall/Pitcrew `*-bootstrap.sh`): runs before Rust exists | 7 | 485 | 1 | 90 | 373 |
| | Thin wrappers around the draw.io CLI | 6 | 159 | 0 | 6 | 116 |
| **Delete** | One-off migrations (`rename-to-debrief.mjs`, `rename-codebase-tool.mjs`, `windows-retire-legacy-runner.ps1`) | 3 | 491 | 3 | 16 | 491 |
| *(gone before this ADR starts)* | Per-app copies deleted by WP-C1–C6 (release, bundle verify, commit-lint, export-diagrams, check-icons/index, sidecar/bundle/cleanup) | 39 | 2,765 | 36 | 202 | 445 |

**Totals after consolidation** (270 files, 25,140 lines; the 39 copies are gone):

| Decision | Files | Lines | fix:/ci: touches | Share of the fixes |
|---|---|---|---|---|
| Port to Rust (phases 1–3) | 117 | 9,734 | 68 | 55% |
| Thin YAML (logic into the tool) | 22 | 334 shell | 36 | 29% |
| Keep JS/TS (UI toolchain) | 15 | 2,081 | 15 | 12% |
| Keep JS for now (author-time generators) | 100 | 11,375 | 21 | (design iterations, not tooling bugs) |
| Keep shell (bootstrap, draw.io) | 13 | 644 | 1 | 1% |
| Delete | 3 | 491 | 3 | 2% |

The port set holds 84% of the tooling fix touches outside the author-time generators in 40% of the lines. The icon fixes
are almost all visual design changes ("a warmer tile gradient", "no seams in the dark tile"), not tooling defects.

Duplicates inside the port set that a single tool collapses (verified by reading): three libduckdb fetchers
(`libduckdb.mjs`, `fetch-libduckdb.sh`, `libduckdb-env.sh`) beside Debrief's Rust `xtask libduckdb`; four build-folder
pruners (`tools/prune-build-folders.{sh,ps1}`, Debrief `clean-build-dirs.ps1`, Dyno `ci-prune.sh`); four Linux container
runners; six verify-installed scripts; two test-browser installers and two pinners; two notices generators; two
window-weight measurers. About 1,100 lines.

## 2. Options

| | A. Keep the scripts (after consolidation) | **B. Port where it pays (recommended)** | C. Everything in Rust | D. Type the scripts (TypeScript run by Node 24's type stripping) |
|---|---|---|---|---|
| What moves | nothing | 9,734 lines (117 files) and the actions' inline shell | B plus 13,456 lines of UI glue and author-time generators | the 6,000 JS lines get types; shell stays |
| Fixes it addresses | none | 104 of 123 tooling fix touches outside the generators (actions + port set) | B + 36 more (mostly design) | JS typos and shape errors only; none of the OS, signing or path fixes, which live in shell |
| Tested on all three OS | no (shell in YAML is untested) | yes: crate legs on Linux, macOS, Windows plus `cargo xwin clippy` | yes | no |
| Effort | 0 | about 23 agent-days in three phases, go/no-go after phase 1 | about 44 agent-days | about 4 agent-days |
| Real losses | none | none found (§4) | Playwright's runner, traces and WebKit for the screenshot suites; the TypeScript type checker; ESLint integration (§4) | none |
| Pacenotes reuse | none | `paddock-repo-graph`, `paddock-repo-rules` (§6) | same | none |

D is cheap but misses the point: the defects are in shell and per-OS branches, not in JavaScript types. A is the
baseline the research note's job-level cache work (WP-A1) assumes; it stays valid if this ADR is rejected.

## 3. Decision (proposed)

### 3.1 One tool, library crates underneath

- **`paddock-dev`**, one binary with subcommands, built on `paddock-cli` conventions (`--json` writes one object, stable
  exit codes, errors name the next step, progress on stderr). A root `.cargo/config.toml` alias makes it
  `cargo dev <command>` from anywhere in the repository (`run --locked --quiet --manifest-path crates/paddock-dev/Cargo.toml --`),
  which is the xtask pattern without a root workspace (ADR 0014 §1 forbids one).
- Command groups, each replacing a set of scripts: `plan` (affected, explain, CI matrix, job-level last green when
  WP-A1 lands), `ci` (`setup` = sccache configuration, stable workspace, build folder, build env, host tools, node caches;
  `timings`, `coverage-gate`), `fetch <pin>` (every pinned, hash-checked download: sccache, libduckdb, Chrome for
  Testing, runtimes; one pin file), `release` (`bundle`, `sidecars`, `verify-macos`, `self-test`, `latest-json`, `notes`,
  `strip-keys`, `publish`), `host` (`prune`, `container`, `verify-installed`, `vm-smoke`, `cleanup`), `notices`,
  `window-weight`, `new-app`, `packages publish`, and in phase 3 `check` (conformance).
- **Library crates in `crates/`** for the parts with a second consumer (§6): `paddock-repo-graph` (projects, edges,
  file-to-project mapping, affected closure, the walk and the `History` port) in phase 1, `paddock-repo-rules` (rule
  trait, findings, waivers with expiry and counts, baseline, reports) in phase 3. CI-only logic (release, signing,
  runner set-up, Gitea run records) stays in modules of `paddock-dev` until something else needs it.
- **Reuse, not new code:** `paddock-proc` for every external command (timeouts, output caps, whole-tree kill, no console
  window on Windows), `paddock-download` + `paddock-archive` for pinned downloads (they replace the curl/`sha256sum`/
  `shasum`/`unzip`/`Expand-Archive` branches), `paddock-env`, `paddock-chromium` for the test browser, `paddock-plugin-package`
  for SHA256SUMS-style checks, Debrief's `xtask libduckdb` (moved, its tests with it), `paddock-testkit` for tests.

### 3.2 Crate placement

`crates/paddock-dev` (`publish = false`) and the library crates are members of the `crates/` workspace. Measured: a cold
release build of `paddock-cli` + `paddock-proc` + `paddock-env` with the workspace-hack takes 41 s with 2 jobs, so a
separate tools workspace would save little (the path dependencies still pull the workspace-hack) and would lose the
crate legs that test every member on three OSes for free, the 99% coverage gate and hakari's single feature set.
Debrief's `xtask` keeps `gen-samples` (Debrief domain) and loses `libduckdb` to `cargo dev fetch libduckdb`.

### 3.3 How CI gets the binary

Measured on the Mac mini (M4 Pro, `CARGO_BUILD_JOBS=2`, synthetic probe crate with the dependencies the tool needs:
serde, toml, regex, ignore, rayon, sha2, ureq; through the machine semaphore):

| | Cold release | Cold debug | sccache warm, after `cargo clean` | Up to date | Binary |
|---|---|---|---|---|---|
| Probe tool | 33 s | 17 s | 5 s | 0.06 s | 2.9 MB |
| `paddock-cli` + `proc` + `env` with workspace-hack | 41 s | 24 s | not measured | | |

| Option | Per job | Skew between tool source and binary | Verdict |
|---|---|---|---|
| (a) Prebuilt in the CI image | 0 s | the image is rebuilt rarely; a tool change in a PR would run the old binary | only with a hash check |
| (b) `cargo dev` from source in every job | Linux containers: about 10 s warm sccache, about 60 s cold (estimate from the table: tool plus workspace-hack); host runners with a persistent target folder: 0.06 s when unchanged | none | too slow alone on Linux (about 7.5 executed jobs per run, research §4: +1 to +7 min of runner time per run) |
| (c) A tagged release artifact, downloaded and hash-checked | 1–2 s | a tool change needs a release before use | slows tool development; no |
| **(d) Hybrid (recommended)** | Linux: the image's binary when its embedded input hash equals the tree's, else build (sccache); host runners: persistent `~/.paddock-ci/dev-target`, `cargo build --locked` (0.06 s no-op); tag and release jobs always build from source | none: the hash covers the tool's crate closure and `Cargo.lock` | yes |

The input hash is the list of git tree ids of the tool's path-dependency closure plus `Cargo.lock`'s blob id; `git
rev-parse HEAD:<dir>` takes 16 ms. A test checks the list against `cargo metadata`, so a new dependency cannot be missed.
The image build (`ci-image.yml`) builds the tool in a builder stage and embeds the hash; the planner marks `ci-image` affected
when the tool's inputs change. The binary never comes from the `actions/cache` server, which every job can write
(research §7, the CREEP pattern): a poisoned tool binary would run in release jobs with signing secrets.

### 3.4 Windows and macOS host runners

Host runners are persistent, so the tool builds once per tool change (about 40–60 s, then 0.06 s checks). The shell
that YAML steps start stays Git Bash on Windows, but the step is one line (`cargo dev ci setup ...`); the junction,
path conversion, zip extraction and hashing happen in Rust (`std::os::windows`, `junction` crate, `paddock-archive`,
`sha2`), not through `cygpath`, `cmd //c mklink` and PowerShell. Rust is a prerequisite on every runner already (the
crate legs). The bootstrap that installs Rust (`tools/bootstrap.sh`, `ci-image/fetch-verified.sh`, the toolchain step of
`setup-rust`) stays shell, about 600 lines.

### 3.5 Speed (measured, then estimated)

| Measurement (this Mac, warm file cache) | Today | Rust |
|---|---|---|
| Process start (`node -e 0` / Rust binary / `bash -c true`) | 32 ms | 10 ms (bash 9 ms) |
| `affected.mjs` on 1, 20 and 200 commits | 0.36–0.37 s (15 serial `cargo metadata` calls of about 31 ms each, plus git) | est. 0.1 s (the calls in parallel) |
| Conformance, CI subset of 12 checks | 2.9 s (C36 alone 2.0 s, C30 1.1 s) | est. 0.2–0.4 s; C36 is an O(files × crates) owner lookup, so an index fix in JS would also bring it near 0.5 s |
| Scan of 2,691 `.rs` files: read, regex, SHA-256 | Node, one thread: 0.12 s | rayon: 0.09 s (I/O-bound) |
| Conformance tests | 240 tests, 1.8 s | comparable |

From the 142 CI runs the research recorded (1,070 executed jobs, step timings): the tooling steps themselves are short.
Plan steps 0.3 s, release facts 0.2 s, `.app`/DMG verification 0.5–1.0 s, publish dry run 7.8 s, the conformance job
7.5 s in total. Node set-up is not a tooling cost: the conformance, plan and tools jobs run no `npm ci` (conformance has no
dependencies; Node is in the CI image); `setup-node` plus install in the tools job averaged 1.5 s per run. The only
tooling-only `npm ci` is the icons job (5 runs, 3–15 s per install in the saved logs). The slow tooling step,
"Third-party notices are up to date" (113 s), is `cargo` resolving licences, not Node.

**Net per CI run** (about 7.5 executed jobs, 23 runner minutes on average):

| Option | Tool unchanged | Tool changed in the push |
|---|---|---|
| (a) image only | −3 to −4 s (conformance and planner) | wrong results (old binary): rejected |
| (b) build from source everywhere | +1 min warm / +7 min cold | same |
| (c) release artifact | +10 s (downloads) | needs a release first |
| **(d) hybrid** | **−3 to −4 s** | +10 s per Linux job that needs the tool until the image is rebuilt, +40–60 s once per host runner |

**Speed is not a reason to port.** It is roughly neutral with (d) and clearly negative with (b). The case rests on
the fix history (§1) and on tests that run on all three OSes.

### 3.6 Testing

- **Pure core, thin edges.** Every command splits into a plan (pure: inputs to a list of actions or a result) and an
  executor (runs the actions through `paddock-proc`). Plans are unit-tested on every OS; OS commands (`codesign`,
  `hdiutil`, `xattr`, `lipo`, `otool`, `mklink`, `launchctl`) are scripted with `paddock-testkit::FakeRunner`, so the
  macOS bundle verification is tested on Linux and Windows too.
- **Golden tests from today's scripts.** Before a script is deleted, its outputs on fixture inputs are recorded as golden
  files (`paddock_testkit::golden`): `affected.mjs --ci` on the research replay's cases (identical tree, one red job, a
  CI-only change) and on synthetic diffs, `latest-json.mjs` (already "byte-equal to the apps' copies"), notices, release
  notes, conformance findings as JSON per check against `conformance/fixtures` and the existing 240 tests' inputs. The
  Rust port must reproduce them byte for byte; differences are reviewed, not accepted silently.
- **Replay fixtures.** The research tool's run and job records (`/Volumes/d7sst7/dev-targets/research-nx-tools/`,
  reduced to synthetic ids and paths) become planner fixtures, so job-level last green is tested on recorded shapes.
- **Real-OS checks** stay where they are today: the macOS `verify` and `self-test` steps run on the built app in CI;
  `--self-test` of every sidecar from the built app (rule 23). Tests never touch the user's machine (rule 7): host
  commands get a dry-run mode and scratch roots.
- Coverage: 99% lines for the library crates and for `paddock-dev`'s plan modules; executors are covered by the CI
  steps that use them.

### 3.7 Coexistence and migration order

- **One command at a time.** A script is replaced by its command in one PR: the command, its golden tests, the call
  sites (YAML, `paddock.ci.json`, docs), and the deletion of the script and its JS tests. No long-lived dual paths.
- **Shadow run for the risky ones** (planner, release, `latest.json`, conformance): for one week the old and the new run
  side by side on `main` and a step compares their JSON; a difference is a warning with both outputs. Then the old one is
  deleted. Release commands are additionally dry-run with `workflow_dispatch` of `app-release.yml` before the switch.
- **Order:** by fix count per line and OS branching, highest first: (1) the composite actions' setup logic (`setup-rust`'s
  sccache part had 9 fixes, `stable-workspace` 5, `sccache-stats` 3), (2) release, bundle, sidecar and verify, (3) pinned
  downloads and the test browser, (4) the planner (after WP-A1 lands in JS, so the job-level base is ported with its
  fixtures, not designed twice), (5) host tools and smokes, (6) notices, window weight, scaffolder, publisher, (7)
  conformance as `paddock-repo-rules`.
- **Rule 40 check (C39, the next free id):** once phase 1 is done, conformance fails when a new `.sh`, `.ps1`, `.py` or
  `.mjs` appears under the kit paths (`scripts/`, `conformance/ci/`, `conformance/release/`, `.gitea/actions/`, `tools/`,
  `apps/*/scripts/`) outside an allow-list (bootstrap, draw.io wrappers, the kept UI glue), and when a workflow `run:`
  block exceeds 10 lines or branches on the OS. Counted waivers as in ADR 0021.

## 4. Everything in Rust?

Asked by the user. Category by category, what Rust would cost or lose:

| Category (lines) | Rust alternative | Cost | Loss | Verdict |
|---|---|---|---|---|
| Kit, CI, release, host scripts (6,880) | the tool | in B | none | port (B) |
| Conformance engine and checks, app checks (2,854) | `paddock-repo-rules`; checks are regex and file scans, a 1:1 port; `toml` replaces the hand-written waiver parser | 4–5 days; 240 tests become golden fixtures | none; a gain: one rules engine with Pacenotes (§6) | port in phase 3 (B) |
| Playwright screenshot and visual-QA suites (720) | `paddock-chromium` has `Page.captureScreenshot` and a small synchronous DevTools client | 3–4 days to write locators, auto-waiting, retries and a reporter | Playwright's test runner, trace viewer, auto-wait, and **WebKit** (the suites check Chromium and WebKit; `paddock-chromium` drives Chromium only) | keep TS: it is UI test code, like the Vitest specs |
| TypeScript type-checker analysis (633) | `oxc` parses TypeScript but has no type checker; no Rust TypeScript type checker is maintained (the native TypeScript compiler is Go) | 2 days for a name-based approximation | precision (types of the dialog command calls) | keep; better: generate the dialog command contract from Rust (ADR 0009's declared contracts) and drop the analysis |
| UI source lints (243) | oxlint (custom rules need a fork or its JS plugin API) | 1 day | integration with the ESLint preset the apps already run | keep, as ESLint rules in `eslint/` |
| npm lifecycle and i18n glue (485) | `cargo dev` called from `package.json` scripts | 1 day | every `npm start` first needs the tool built (40–60 s once per checkout); `ng extract-i18n` stays Angular's | keep; no defects to fix (1 fix) |
| Icon pipeline (4,098) | `resvg` crate (`@resvg/resvg-js` is its binding), `png`, hand-written `.icns`/`.ico` (the JS already writes them by hand), preview sheets with `paddock-chromium` | 4–5 days | design iteration speed (a cargo build per change instead of a 30 ms Node start); nothing functional | feasible without real loss; not now (no tooling defects) |
| Diagram and poster generators (7,277) | `drawio` XML emitter in Rust (Pacenotes plans `pacenotes-diagram`, §9.7, with these scripts as the oracle); posters: HTML templates and `paddock-chromium` screenshots | 6–8 days | none, once the emitter exists | move when Pacenotes' emitter lands: the per-app generators are mostly data and become input files |

All-Rust in total: about 13,456 more lines (about 15,000 Rust lines), about 21 agent-days on top of B (15 of them for the parts without loss), about 44
agent-days in all. **Feasible without real loss for everything except three UI-toolchain categories (about 1,600 lines):
the Playwright suites, the type-checker analysis and the ESLint-style lints.** Those belong to the Angular app's own
toolchain, like its unit tests; porting them would replace mature tools with weaker home-made ones. Recommendation:
B now; the generators follow Pacenotes' emitter (diagrams) or a later icon refresh (icons) if the user wants a JS-free
kit; the three UI categories stay JS/TS.

## 5. Risks

| Risk | Mitigation |
|---|---|
| A port changes behaviour silently (release, `latest.json`, planner) | golden files from the old script; one-week shadow runs with a JSON comparison; release dry runs before the switch |
| Tool build time on cold runners | the hybrid delivery (§3.3); the binary never from the writable cache |
| Bootstrap loop (the tool needs Rust and sccache that the setup configures) | the toolchain step stays shell; the tool builds without sccache on a miss; on Linux the image carries it |
| Contributors must read Rust to change CI | the family is Rust-first; commands are small modules with a plan/executor split and golden tests; YAML stays as the readable surface |
| The port stalls half-way, leaving two systems | phases end in deletions; go/no-go after phase 1 on a measured fix rate (§7) |
| Scope creep into a build system | the tool runs steps and checks; it never replaces cargo, npm, ng or nextest, and has no cache of its own (research §7–§8) |
| The `crates/` workspace grows and its legs run more often | the tool's crates are small; job-level last green (WP-A1) skips legs whose inputs did not change |

## 6. Reuse by Pacenotes (rule 42)

Pacenotes (proposal §14) plans `pacenotes-index` (walk, ignore, hashing), `pacenotes-git` (gix behind a `History` port),
`pacenotes-rules` (rules, suppressions, SARIF), `pacenotes-review` (findings, baselines, exit codes) and a dependency
graph from repositories down to symbols (§23), plus `pacenotes-diagram` (§9.7). The overlaps:

| Piece | Today (JS) | Shared crate | Fits Pacenotes' design | CI-only parts |
|---|---|---|---|---|
| Project graph: Cargo workspaces and path dependencies, npm workspaces, `tsconfig` paths, Tauri `frontendDist`, file-to-project mapping (longest prefix), reverse-dependency closure | `affected.mjs` `cargoGraph()`, `npmGraph()` | `paddock-repo-graph` | yes: §23's "repositories → packages and modules" level, edges with a source and confidence. Pacenotes cannot assume `cargo` is installed: the crate reads manifests with `toml`/`serde_json` and uses `cargo metadata` only as an optional precise adapter | GLOBAL/DOCS/DOC_INPUTS rules, `kinds`, nextest `packages`, the CI matrix: modules of `paddock-dev plan` |
| Repository walk and hashing | `conformance/src/util/fs.mjs` `walk()` | in `paddock-repo-graph` (`walk`: `ignore` crate, `.gitignore`-aware, bounded, rayon, SHA-256 per file) | yes: `pacenotes-index` adds its secret denylist and workspace allow-list on top | none |
| Git history | `git diff --name-only`, `git log` through `execFileSync` | a `History` port in `paddock-repo-graph` (changed files between revisions, log with subjects, file at revision) with a git CLI adapter over `paddock-proc` | yes: Pacenotes plugs its gix adapter into the same port | Gitea run and job records for last green: `paddock-dev` |
| Rules engine: rule ids with sub-checks, findings with `file:line`, severity, fix in one sentence, waivers with reason, expiry and counts, baseline, reports (text, JSON, job summary) | `finding.mjs`, `waivers.mjs`, `report.mjs`, `runner.mjs`, C28 baseline | `paddock-repo-rules` (phase 3), with SARIF 2.1.0 output added | yes: §24's rule model (id, severity, scope globs, fix hint, examples as tests), suppressions with a reason and expiry, baselines by fingerprint, exit codes. Pacenotes adds tree-sitter pattern rules and Markdown rules as further `Rule` implementations (open/closed) | the family checks C03–C39 themselves |
| draw.io emitter | `diagrams/tools/lib.mjs`, `uml.mjs`, `themes.mjs` | none yet; Pacenotes builds `pacenotes-diagram` first, with these scripts as its oracle | yes | none |
| Gitea REST client | `publish.mjs`, `affected.mjs` (runs), `publish-release` | a module of `paddock-dev`; extract to a shared crate when Pacenotes' `pacenotes-gitea` starts (PR comments) | partly (auth, paging, errors) | releases, packages, run records |

## 7. Work packages

Estimates are agent-days for an implementer (Sonnet) with a researcher or designer for the first package of each phase.
"Deletes" is measured (lines of the scripts, actions' shell and their tests today); "adds" is estimated (Rust source
about 1.2 times the JS it replaces after duplicates collapse, shell about 2 times; tests about 1:1).

| WP | Content | Agent-days | Deletes (measured) | Adds (estimated) |
|---|---|---|---|---|
| **Phase 1** (after WP-C1–C6) | | | | |
| R0 | Skeleton: `crates/paddock-dev` (commands, `paddock-cli` conventions, plan/executor split), `paddock-repo-graph` with walk and the `History` port, the `cargo dev` alias, the hybrid delivery (image builder stage, input hash, host target folder), docs | 2 | 0 | 900 + 600 tests |
| R1 | Composite actions' logic: sccache configuration, stable workspace, build folder, build env, host tools, host cleanup, node caches, sccache stats; the actions become one-line calls | 2 | 334 shell lines (actions shrink from 815 to about 300 YAML lines) | 900 + 800 tests |
| R2 | Release, bundles, sidecars, verify: `tauri-bundle`, `sidecars`, `verify-macos` (with `FakeRunner` tests on all OSes), `self-test`, `latest-json`, `notes`, `strip-keys`, `publish`, Debrief's `build-sidecars`, macOS dev-signing, Dyno `check-bundle`, demo release and verify | 3 | 1,267 + about 900 test lines | 1,600 + 1,200 tests |
| R3 | Pinned downloads and the test browser: one pin file, `fetch` on `paddock-download`/`paddock-archive`, Chrome for Testing, libduckdb (from `xtask`), runtime catalogues; the Python updaters go | 2 | 1,089 + about 500 test lines (incl. `build-folder.sh`, `build-env.mjs`, which R1 calls) | 900 + 700 tests (450 moved from `xtask`) |
| **Go/no-go** | Measure four weeks of `fix:`/`ci:` commits on the ported paths per week against the 66 touches on the same paths in the two baseline weeks (2026-10-13 to 10-27); continue if the rate fell, or stop at phase 1 with no dual systems left | 0.25 | | |
| **Phase 2** | | | | |
| R4 | Planner: `plan` with explain, CI matrix and the job-level last green (WP-A1, ported with its replay fixtures), `ci timings`, `coverage-gate`; shadow week | 2.5 | 1,003 + 341 test lines | 1,300 + 900 tests |
| R5 | Host tools and smokes: one `host prune`, `container`, `verify-installed`, `vm-smoke` with per-app config instead of 24 scripts | 2.5 | 1,555 + 620 test lines | 1,100 + 800 tests |
| R6 | Notices, window weight, startup bench, plugin and font builds | 1.5 | 976 + about 400 test lines | 900 + 500 tests |
| R7 | `new-app` and `packages publish` (with `cargo hakari publish`) | 2 | 990 + about 700 test lines | 1,200 + 800 tests |
| **Phase 3** | | | | |
| R8 | `paddock-repo-rules` and `cargo dev check`: findings, waivers, baseline, reports incl. SARIF; checks C03–C37 and the six app checks ported against golden findings; shadow week | 4.5 | 2,854 + 3,460 test lines | 3,300 + 3,800 tests |
| R9 | Clean-up: check C39 (§3.7), AGENTS.md and README updates, `conformance/` reduced to fixtures, a lesson in `docs/lessons.md` | 0.5 | about 200 (package files, bins) | 200 |
| **Total B** | | **about 22.75** | **about 9,900 source + about 6,900 test lines + 334 shell in YAML** | **about 12,300 source + 10,100 tests** |
| *Optional (C)* | Icons (4–5), diagrams with Pacenotes' emitter (6–8), posters (2), npm glue (1) | about 15 | 11,860 | about 13,000 |

Line deltas: B replaces about 9,900 lines of JS, shell, PowerShell and Python plus 334 lines of untested shell in YAML
with about 12,300 lines of Rust, nearly all of it running in tests on three OSes; about 1,100 duplicated lines go. The
number of languages in the kit drops from five (JS, bash, PowerShell, Python, YAML shell) to Rust plus YAML, plus JS/TS
for the UI toolchain and the author-time generators.

## Consequences

- The kit's OS-specific behaviour has unit tests that run on Linux, macOS and Windows; the composite actions become
  readable one-line calls.
- One way to download a pinned file, run an external command with a timeout, prune build folders or verify an
  installed app, instead of two to six.
- Pacenotes starts with a project graph, a walk, a `History` port and (after phase 3) a rules engine that CI has
  already exercised.
- Contributors change CI logic in Rust; a tool change costs a build (about 40–60 s cold, seconds warm).
- The research note's job-level last green (WP-A1) is built in JS first and ported in R4.

## Decisions for the user

1. **Option B** (port where it pays, about 22.75 agent-days in three phases) over A (keep), C (everything in Rust,
   about 44) and D (types only).
2. **Go/no-go after phase 1** on the measured fix rate, rather than committing to all three phases now.
3. **Delivery (d), hybrid:** the image's binary when its input hash matches, otherwise built from source; never from the
   cache server.
4. **Conformance in Rust in phase 3** as the shared `paddock-repo-rules`, which reverses the default of keeping it in JS,
   because Pacenotes needs the same engine.
5. **Author-time generators:** keep in JS (recommended), or schedule them with Pacenotes' emitter (diagrams) and the next
   icon refresh (icons) for a JS-free kit.

## Sources

- Measurements and per-file inventory: [research note 2026-10 Rust tooling inventory](../research/2026-10-rust-tooling-inventory.md)
- Research note, Nx and Turborepo against our scripts: branch `docs/research-nx`, `docs/research/2026-10-nx-vs-scripts.md`
- cargo xtask pattern: <https://github.com/matklad/cargo-xtask> (read)
- cargo-hakari publishing without the workspace-hack: <https://docs.rs/cargo-hakari/latest/cargo_hakari/publishing/index.html> (read)
- resvg: <https://github.com/linebender/resvg> (read; `@resvg/resvg-js` is its Node binding, <https://github.com/thx/resvg-js>)
- oxc (parser and linter, no type checker): <https://oxc.rs/docs/guide/introduction.html> (read)
- TypeScript's native compiler is written in Go: <https://devblogs.microsoft.com/typescript/typescript-native-port/> (read)
- Playwright browsers (Chromium, Firefox, WebKit): <https://playwright.dev/docs/browsers> (read)
- Node.js type stripping: <https://nodejs.org/api/typescript.html> (read)
- SARIF 2.1.0: <https://docs.oasis-open.org/sarif/sarif/v2.1.0/sarif-v2.1.0.html> (read)
