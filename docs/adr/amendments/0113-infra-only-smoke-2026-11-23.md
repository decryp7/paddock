# ADR 0113 amendment: plumbing-only changes run a smoke set, test-only changes run the affected tests, neither is complete (2026-11-23)

- Status: Accepted 2026-11-23 (the user, through the coordinator: "when a change touches only CI plumbing and no app or crate
  code, each affected job runs its setup, a build and a small smoke set"; about 1 to 1.5 agent-days). Two more user decisions
  of the same day are folded in (§5 to §7): rule 54 narrowed, and test-only changes run only the affected tests; the user's
  framing: "logically speaking we are working on foundation, so none of the app CI should run at all". ER-15, ER-33, ER-37, ER-49.
- Amends: [ADR 0113](../0113-a-run-declares-its-scope-as-data-and-one-reader-decides-complete.md). ADR 0113 is not on `main` yet
  (it lands with `fix/run-scope-reuse-p2` in train 21), so this is an amendment beside it, not a new ADR.
- Evidence heads (ADR 0091), read 2026-11-23: `origin/main` = `b3c254a2d`, `origin/fix/run-scope-reuse-p2` = `bab7a4a97`,
  `origin/train/2026-11-22-20` = `803aed12d`.

## Context

Train 20 changed shared actions (`build-jobs`, `dev-tool-cache`, `cache-report`, `node-caches`) and their scripts under
`conformance/ci/`, and no app or crate code. The planner traces an action to the ci.yml jobs that use it
(`plan_gitea::gitea_targets`) and marks each job's project; a marked project runs its whole suite. So every app leg, the crates
coverage run, the three crate legs, the UI specs and the e2e shards ran in full to test a cache step.
(`./dev plan 298cc726b^ 298cc726b --explain` lists them.)

## Decision

1. **Infra-only reasons.** The planner records, for each reason it marks a project with, whether the reason is CI plumbing. A
   project whose *every* reason is plumbing is listed in the plan's new `smoke` list (beside `cargoOnly`). Plumbing is:

   | Changed path | Plumbing when | Otherwise |
   |---|---|---|
   | `.gitea/actions/<action>/**` | always, for the ci.yml jobs the action is traced to | an action nobody uses: everything (as today) |
   | `.gitea/workflows/ci.yml` | every changed step of each changed job (split by job as today, then by step) is an action step (`uses:`, no `run:`) and the job's header (before `steps:`) is unchanged | full for that job's projects |
   | `conformance/ci/**`, `conformance/src/util/**` | no project contract (`paddock.ci.json`) names the script | full (a script a contract runs is a check, `check-index.mjs`) |

   Not plumbing, whatever they touch: an app's own workflow, other workflows, a release action (§5), `conformance/release`, globals, and any
   app, crate, UI or tool source. Conformance is always full (its `conformance always runs` reason is not plumbing). A plan with
   `all` (the nightly, a tag, an app's gates, a manual run without a base) has no plumbing reasons, so it is never smoke; a rerun of
   failures (`since_run`) keeps its own plan.
2. **A smoke set is declared per project, never in the planner.** A project contract gets a `smoke` object: `nextest` (a filterset)
   and, per phase, the lines that replace that phase in a smoke run. `./dev ci run <project> <phase>` with `PADDOCK_SMOKE=1` runs
   `smoke.<phase>` when it exists and the phase itself otherwise, so setup, lint and build phases stay as they are. A nextest step
   with a smoke filter still builds every test binary and fails when the filter matches nothing (`--no-tests=fail`). A project with
   no `smoke` object runs in full: the list holds only projects that declare one.

   | Job kind | Smoke set | Declared in |
   |---|---|---|
   | Rust (`crates`, `crate@os`, app Rust half) | the build plus `smoke.nextest`; the coverage gate and doc tests are skipped | `crates/paddock.ci.json`, `apps/<app>/paddock.ci.json` |
   | `paddock-dev` | the build plus one filterset | the planner's `SMOKE_NEXTEST` table (paddock-dev has no contract) |
   | UI (`ui`, app UI half) | the build steps plus one spec (`smoke.specs`, `smoke.ui`) | the same contracts |
   | e2e (`e2e`, app e2e legs) | one spec per engine on one shard (`smoke.e2e`) | the same contracts |
3. **The run declares it.** The scope line (now `"v":2`; the reader takes 1 and 2) carries `ran.smoke`: the jobs and app legs that
   ran their smoke set. `Declared::complete` refuses a run with any: "it ran a smoke set (a CI-plumbing-only change) for crates,
   dyno@ubuntu-latest, not the full suite". `need` never has one. So `./dev ci verdict`, `ci reuse` (ER-49) and `tag.yml` never count
   a smoke run as complete for any commit (ER-15); an older reader refuses the v2 line as not measured (ER-33). Until ci.yml is
   recorded (ADR 0113 §5), the old `plan verdict: complete=` line also says `false` when the plan's `smoke` list is not empty.
4. **Everything still runs on the nightly, on tags and on code.** The nightly and tags plan with `all`; a push or dispatch that
   touches app or crate code marks those projects with a code reason, so they run in full, and only the projects reached by plumbing
   alone run smoke (a mixed diff). A commit whose last run was smoke has no complete verdict: a tag on it waits for the nightly or a
   full dispatch, which is the price of the cheaper push.
5. **Rule 54 narrowed: installers rebuild only on real release changes.** The planner path that decides "release path" is
   `plan::affected` with `plan_gitea::RELEASE_ACTIONS` (`tauri-bundle`, `macos-verify`, `keychain`, `strip-signing-keys`,
   `publish-release`, `release-verdict`, `sidecar-self-test`): a reason from a release action, an app's own workflow
   (`<app>.yml`, `<app>-release.yml`, the shared `app-release.yml` through its callers), `conformance/release/**`, the app's code or
   its Tauri config is a release reason. `./dev plan --ci --app <app>` (the plan action of every app workflow, so the installers
   jobs) answers `false` when the app's every reason is plumbing: a shared setup or cache action used by a release workflow gets
   ci.yml's smoke leg only, no installer rebuild. Tags plan with `all` and always rebuild. Rule 54's text says this.
6. **Foundation changes plan no app jobs.** A change to `tools/`, `conformance/` (except the plumbing scripts of §1, which give
   smoke legs) or docs marks no app. Before this, `conformance/src` and `conformance/bin` marked every app ("its checks run
   paddock-conformance"); the conformance job tests conformance itself, and the apps' own conformance lines run again on the
   nightly and on any app-code change.
7. **Test-only changes run only the affected tests.** A project contract may declare `tests`: `paths` (globs, relative to the
   project folder: what counts as test code, specs and helpers) and one entry per runner (`vitest`, `playwright`, `nextest`) with
   its `specs` glob. A change whose files in a project are all test code marks the project `affectedTests` instead of full, with
   the spec files to run: the changed specs plus the specs that import a changed helper, following relative `import`/`require` in
   TypeScript transitively (as the planner follows Cargo and npm), and for Rust every integration test binary of the crate whose
   `tests/` holds the changed helper. The jobs read them through `./dev ci run` with `PADDOCK_TESTS=1`: a contract's
   `affected.<phase>` lines use `$PADDOCK_VITEST`, `$PADDOCK_PLAYWRIGHT` (space-separated paths from the `cd` folder) and the leg's
   nextest `filter` (`binary_id(pkg::bin)`). A project without a `tests` declaration runs in full. The scope line carries
   `ran.affected` and `Declared::complete` refuses it like smoke ("it ran only the affected tests ... not the full suite", ER-15).
   Whole suites stay for app-code changes, the nightly and tags.

## Common practice (ER-37)

- **Test-only changes**: Jest and Vitest `--changed`/`--related` and Bazel's per-test targets run the tests whose import graph
  reaches the change (Vitest docs, "CLI: --changed"; Jest docs, "--findRelatedTests"); Nx reruns the whole project's test target.
- **Release builds**: GitHub-hosted projects usually build installers only on tags or release branches (GitHub Docs, "Workflow
  syntax: on.push.tags"); Paddock's rule 54 (installers before landing) is stricter, now limited to release changes.

- **Bazel**: CI scripts are outside the build graph, so a change to them invalidates no test target; with a remote cache every test
  is a cache hit and nothing reruns (Bazel docs, "Remote caching"). A wrong cache key is caught only by a clean build.
- **Nx affected**: a change to a file in a project's `inputs`/`namedInputs` reruns its targets; a CI file outside every input reruns
  nothing (Nx docs, "Run only tasks affected by a PR", "Inputs and named inputs").
- **GitHub Actions path filters**: `on.push.paths` / `paths-ignore` run or skip a whole workflow; there is no "run lighter" mode,
  so most projects either rerun everything on a workflow change or skip it (GitHub Docs, "Workflow syntax: on.push.paths").
- This design is between them: it reruns setup and a build (what a plumbing change can break), not nothing (Bazel, Nx) and not
  everything (path filters).

## Cost and the cheaper option

About 1 to 1.5 agent-days for §1 to §4 (the classification, the smoke declarations and `ci run`, the ci.yml and app-gates wiring,
scope v2, the tests); about 0.5 for §5 and §6; about 1.5 to 2 for §7 (the test-path data, the TypeScript import walk, three filter
wirings). Total about 3 to 4 agent-days against the approved 1 to 1.5: §7 is more than 1 day beyond common practice and needs the
user's yes with both costs shown (ER-37); its common-practice option is Nx's (a test file is an input of its project, so the
project's whole test target reruns: 0 extra days, the current behaviour minus the other apps). Cheaper common practice, offered as a decision: **(a)** keep running the full suite on plumbing changes (0 days; the train 20
cost), or **(b)** Nx-style: a plumbing-only job runs setup only, no build and no tests (about 0.5 day; misses a broken build cache or
build env until the nightly). The user chose this design on 2026-11-23.

## Falls short

- `e2e-webkit` runs its lines inline in ci.yml, not through a contract phase, so it stays full in a smoke run until it moves to
  `ui/paddock.ci.json`. It runs only on main, tags, the nightly and dispatches that ask for WebKit.
- App workflows of their own (`debrief.yml`) answer only "affected or not"; they stay full.
- A changed `run:` step of ci.yml is never plumbing, even when it only sets up a cache: the split cannot tell a setup command from
  a test command without a list that would drift, so it errs toward the full suite. Move such a step into a shared action to get
  the smoke run.
