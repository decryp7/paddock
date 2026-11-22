# ADR 0113 amendment: a CI-plumbing-only change runs a smoke set, and a smoke run is never complete (2026-11-23)

- Status: Accepted 2026-11-23 (the user, through the coordinator: "when a change touches only CI plumbing and no app or crate
  code, each affected job runs its setup, a build and a small smoke set"; about 1 to 1.5 agent-days). ER-15, ER-33, ER-37, ER-49.
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
   | `conformance/ci/**`, `conformance/release/**` | no project contract (`paddock.ci.json`) names the script | full (a script a contract runs is a check, `check-index.mjs`) |

   Not plumbing, whatever they touch: an app's own workflow, other workflows, `conformance/src`, `conformance/test`, globals, and any
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

## Common practice (ER-37)

- **Bazel**: CI scripts are outside the build graph, so a change to them invalidates no test target; with a remote cache every test
  is a cache hit and nothing reruns (Bazel docs, "Remote caching"). A wrong cache key is caught only by a clean build.
- **Nx affected**: a change to a file in a project's `inputs`/`namedInputs` reruns its targets; a CI file outside every input reruns
  nothing (Nx docs, "Run only tasks affected by a PR", "Inputs and named inputs").
- **GitHub Actions path filters**: `on.push.paths` / `paths-ignore` run or skip a whole workflow; there is no "run lighter" mode,
  so most projects either rerun everything on a workflow change or skip it (GitHub Docs, "Workflow syntax: on.push.paths").
- This design is between them: it reruns setup and a build (what a plumbing change can break), not nothing (Bazel, Nx) and not
  everything (path filters).

## Cost and the cheaper option

About 1 to 1.5 agent-days (the classification, the smoke declarations and `ci run`, the ci.yml and app-gates wiring, scope v2, the
tests). Cheaper common practice, offered as a decision: **(a)** keep running the full suite on plumbing changes (0 days; the train 20
cost), or **(b)** Nx-style: a plumbing-only job runs setup only, no build and no tests (about 0.5 day; misses a broken build cache or
build env until the nightly). The user chose this design on 2026-11-23.

## Falls short

- `e2e-webkit` runs its lines inline in ci.yml, not through a contract phase, so it stays full in a smoke run until it moves to
  `ui/paddock.ci.json`. It runs only on main, tags, the nightly and dispatches that ask for WebKit.
- App workflows of their own (`debrief.yml`) answer only "affected or not"; they stay full.
- A changed `run:` step of ci.yml is never plumbing, even when it only sets up a cache: the split cannot tell a setup command from
  a test command without a list that would drift, so it errs toward the full suite. Move such a step into a shared action to get
  the smoke run.
