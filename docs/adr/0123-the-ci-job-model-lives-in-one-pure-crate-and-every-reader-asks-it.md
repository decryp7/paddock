# 0123. The CI job model and the repository settings live in one pure crate, read from ci.yml, and every reader asks it

- Status: **Accepted (user 2026-11-26)**: 81, 81a, 82, D1 = B (8 days approved), D2 = (a), D3 = (a). Redesign (ER-21: the class was
  seen twice in train pq). Revision 3: the independent review's 4 P1s (cleared in round 2) and its 8 + 5 P2s are taken. Revision 3's
  P2s grow the total to 9.0 days, 12.5 % over the approved 8 and inside its 6.5 to 10.5 range (§Work packages).
- Date: 2026-11-26
- Number: 0123, assigned when train 2026-11-26-r landed (user decision 16).
- Related: ADR [0060](0060-every-check-runs-somewhere.md) (the CI model: `gates/ci.rs`, `gates/yaml.rs`, `gates/expr.rs`), ADR
  [0113](0113-a-run-declares-its-scope-as-data-and-one-reader-decides-complete.md) (the scope line, `run_scope`), ADR
  [0117](0117-the-repository-graph-is-the-one-model-and-every-plan-is-a-query.md) (WP-2 deletes `plan_gitea`; D12 keeps the old
  planner's tables until five clean nightlies; §14 lists `CI_JOBS`), ADR 0116 (the test-result cache and the Linux partitions, landed
  with train pq), ADR [0022](0022-rust-developer-and-ci-tool.md) row 40 (`./dev packages publish`), [ci-in-rust](../plans/ci-in-rust.md)
  WP-7 and WP-11, ER-21, ER-22, ER-33, ER-37, ER-41, ER-42, ER-44, rule 55.
- Evidence heads read (ADR 0091), re-read right before committing: `origin/main` 42a884158af0fb035f4f71a30dd3e7a1c3cdc070 (train pq
  landed; it contains 6284b311f, 495f27fb1, a1ff097ab and the publish fix 8cc32c884). Revisions 1 and 2 read `origin/main` f8970d88c
  and `origin/train/2026-11-26-pq` 42a884158; line numbers below are of 42a884158. Code was read with `git show` and `git diff`;
  nothing was built or run. No claim here is about released code: `git tag --contains 495f27fb1` is empty.

## The class

**The shape of ci.yml's jobs (ids, display names, matrix legs, partitions, the project each job runs for, the gates every job must
carry) is written down again, by hand, in each program that reads a run. A layout change in ci.yml is correct in ci.yml and breaks
the copies, and each copy is caught, if at all, by its own late check.**

The invariant: ci.yml and the planner are the only places the layout is written (ci.yml the templates and gates, the planner the
legs it emits); one typed model is built from them, and every reader asks that model. A layout change then fails, or passes, in one
place, in the run that makes the change.

## Instances

| When | Change in ci.yml | Copy that broke | How it showed | Source |
|---|---|---|---|---|
| 2026-11-26, train pq | the Linux `crate` leg split into `crate (ubuntu-latest, k/3)` (6284b311f) | two branches each added a variant to one hand-kept enum, `run_scope::JobName`: main `AnyPrefix` (for `quick`'s old names), the switch-on branch `Starts` (for the partitions); the merged match lacked one | lib tests failed to compile on the merged train until 495f27fb1 | 495f27fb1, 6284b311f, cf0a7090c |
| 2026-11-26, train pq | same split: the coverage step became nightly/release/tag only; the Linux all-features tests moved into the partitioned leg, with `sel=(--workspace)` on its own shell line | `scripts/publish.mjs` `crateStepsMissing` regex-matches `--workspace` on the `cargo nextest` line | publish check failed in run 9135 (per the brief; the log was not read here). Fixed in 8cc32c884 by another hand copy: `crateCommands` rewrites `"${sel[@]}"` to `--workspace` for `job === 'crate' && os === 'ubuntu-latest'` | 8cc32c884, train `scripts/publish.mjs:186` |
| 2026-11-26, train pq (latent, found while reading) | same split | `plan_since::leg_of` reads the OS with `^[a-z-]+ \(([a-z0-9]+-[a-z0-9.]+)\)$`, which `crate (ubuntu-latest, 2/3)` does not match | not seen yet: a failed partition gets `leg.os = None`, so a failed-only round reruns every crate leg whole (`plan_since.rs` `os_legs`, fail-safe but wider), and the escape monitor reports the job as "names no project or OS" | train `plan_since.rs:601`, `:1148` |
| 2026-11-25/26 | the short Linux checks folded into `quick` (ad2de361f), reverted and reapplied (4f39829f0, ffd93bc7f) | `run_scope::PROJECT_JOBS` named `C99 (` and `heavy tool`; recorded runs kept the old names | fixed by name alternatives `quick checks (\|C99 (` and `JobName::AnyPrefix` | cf0a7090c, 0bf900d12 |
| 2026-11-09 onward | every new job | `plan_gitea::CI_JOBS`, `tests/golden/ci-jobs.json`, the `rerun_jobs` gate, the `dev-tool-cache` step before the first `./dev` | caught only by `tests/it/tree.rs` (member audits), `tests/it/golden.rs` and `conformance/test/ci-reuse-wiring.test.mjs`; a missed job broke since-run reruns (06d15063d, gitleaks) | 202e53d28, 06d15063d |

## Where the copies are (train pq at 42a884158)

| Copy | What it re-types | Fate |
|---|---|---|
| `plan_gitea::CI_JOBS`, `tests/golden/ci-jobs.json` | job id to plan projects | **kept** until ADR 0117 WP-2's remaining deletion (D12); a WP-1 test keeps it equal to the model |
| `run_scope::PROJECT_JOBS`, `JobName` (4 variants), `names_of_job`, `name_of_leg` | project to display-name prefixes, leg to name, old names | deleted (WP-2) |
| `run_scope::DEFAULT_OS_LEGS` (:24), `PROJECTS` (:27) | the OS leg set, the project flags | deleted; read from the model |
| `run_scope` test `name_of(...)` asserts | the three `name:` templates verbatim | deleted |
| `gates/ci.rs:237-246` | the crate and paddock-dev legs as a hard-coded three-OS axis | the leg set comes from `plan_ci` (the one source), passed in |
| ci.yml:197 `PLAN_OS_LEGS`, :213-214 fallbacks | the three OS legs in the plan step | deleted: the planner always writes the legs |
| `plan_since::leg_of`, `key_by_name` | OS and app regexes, template matching over names | lookup in the run's scope line, then the `Job:` log line (:466) |
| `plan_since` :894, :1093-1105, :1152 | jobs known through `CI_JOBS`; rerun legs written without `part` or `test_packages` | the model's job list and legs; a rerun leg keeps its partition |
| `ci_runs/green.rs` :300-311 and :340-365 | `crate (`, `crates (`, `paddock-dev (`, `app ` prefixes to project and workspace | lookup |
| `ci_runs/triage.rs:580`, `queue/real.rs:66`, `active_apps::app_of_job` | `app <name> (` parsing | lookup |
| `escape/cmd.rs` `failures_of` | job to project through `CI_JOBS`, OS-less jobs by name | the model |
| `scripts/publish.mjs` `crateStepsMissing`, `crateCommands` (8cc32c884) and tests | "every crate is clippied and nextested with every feature" as regexes, plus a text rewrite for the Linux leg | ported to Rust (81a), answered by the crate-coverage query |
| conformance `C37-test-binaries.mjs` :41-43 (`workspaceTests`) and :103 | the "member is tested" half, by regex over command text | that half reads the crate-coverage query (`./dev gates crate-coverage --json`) until ci-in-rust WP-10 ports C37 |
| `tests/it/tree.rs`, the ci.yml half of `ci-reuse-wiring.test.mjs` | per-job conventions by text search | replaced by the model check |

About 400 lines with their tests leave (CI_JOBS and its golden later, with ADR 0117).

## Common practice (ER-37)

- **One aggregator job.** GitHub/Gitea projects that must not list matrix job names make one `always()` job that `needs` all others
  and fails when any needed job failed or was wrongly skipped ([re-actors/alls-green](https://github.com/re-actors/alls-green):
  matrix leg statuses "are not exposed by the GitHub platform", and a skipped check reads as passed). It answers "did the run pass",
  not "which project and OS passed", which Paddock's reuse (ER-49, ADR 0113), failed-only reruns and the escape monitor need.
- **A declarative job file that generates the matrix.** rust-lang/rust keeps job definitions in
  [`src/ci/github-actions/jobs.yml`](https://github.com/rust-lang/rust/blob/master/src/ci/github-actions/jobs.yml) ("loaded
  dynamically in CI from ci.yml"), and its `citool` computes the matrix and checks PR and CI jobs agree. Paddock already does the
  dynamic half: the plan job emits the legs (`fromJSON(needs.plan.outputs.*)`, the
  [documented pattern](https://docs.github.com/en/actions/how-tos/write-workflows/choose-what-workflows-do/run-job-variations)).
- **Test partitioning** is usually the runner's: nextest's `--partition slice:m/n` and `hash:m/n`
  ([nexte.st](https://nexte.st/docs/ci-features/partitioning/)). Paddock partitions by whole packages so each package's pass can be
  cached (ADR 0116), so the legs come from the planner.
- **Typed settings** (decision 82): Rust tools commonly parse environment configuration once into a typed struct at the edge (crates
  such as `envy`, `figment`, or clap's `env` feature; named from knowledge, not fetched for this ADR). Here the declaration is
  hand-written in the crate, with no new dependency.

## Options

| | Option | What it is | Cost (agent-days) |
|---|---|---|---|
| A | Job file generates ci.yml parts | a `ci-jobs.toml`, a generator for ci.yml's job headers, an ER-40 match check | 10 to 12 (B's work plus a generator over hand-edited YAML) |
| **B** | **ci.yml is the source; one pure crate builds the job model** (decisions 81, 81a, 82) | this ADR | **9.0 (7.5 to 11.5)**, of which 2 move out of ci-in-rust WP-11 and about 0.3 out of WP-7: **about 6.7 new** |
| C | Aggregator job only | `ci-ok`; the verdict reads it | about 1; the copies stay, and reruns, escape and publish keep breaking |

**Chosen: B** (the user, 2026-11-26: 81 and D1). It is about 5.7 days of new work beyond C; the user approved 8 days, and revision
3's 1.0 is within ER-37's 20 % line. C does not remove the cause; it can be added to B later for branch protection.

## Design

### 1. The crate (decision 81)

`crates/paddock-ci-config`: a pure core with no file or process I/O; texts and the environment are passed in. Two modules, `jobs` and
`settings`. It takes ADR 0060's one YAML reader and expression evaluator (`gates/yaml.rs`, `gates/expr.rs`) as its own, and
`gates/ci.rs` uses them from there, so there is still one CI parser (rule 55). The planner's output-to-project table
(`plan_ci::PROJECT_OUTPUTS`) moves into `jobs`, and `plan_ci` reads it. A shared crate: `publish = false`, coverage >= 90 % (the
new-crate skill's steps). `yaml-rust2` is MIT/Apache-2.0 and already in the tree.

### 2. `jobs`: the model

`jobs::model(workflow_text, declared) -> Result<JobModel, Vec<Problem>>`. Per job: `id`; `name` (the raw `name:` template, the id
when absent); `legs`: `Static(values)` or `Planned(output)` for `fromJSON(needs.plan.outputs.<output>)`; `needs`;
`asks_rerun_jobs`; `tool_cache_before_first_dev`; and **`projects`: the union of the plan outputs the job's `if:` and its steps'
`if:`s read** (`needs.plan.outputs.<name>` through the moved `PROJECT_OUTPUTS`), plus a declared field for what cannot be derived
(`gitleaks` and `quick` run for `conformance` by choice, `plan` for `everything`, `test-cache-shadow` for `crates`). The declared
field is one `[ci.jobs]` table in `docs/repo-graph.toml` (ADR 0117's inputs file; shrink-only, ER-46). A job left with no project is
a `Problem`, never `always`: a failed-only `ui` rerun must still turn `ui`, `eslint` and `icons` on, not go vacuously green. Anything
the parser cannot tell (an anchor, an expression `expr` does not evaluate) is a `Problem` (ER-33).

Outputs that are not in `PROJECT_OUTPUTS` are one of two named kinds, so an unknown output is a `Problem`: **app outputs**
(`any_app`, `apps`) map to every app, as `repo_model/ci.rs` `output_projects` (:698) already does, so the `app` job's projects are
derived, not declared; **known non-project outputs** (`webkit`, `selection`, every `*_legs` and every `*_filter`) shape legs or
filters and add no project.

`render(job, leg) -> String` evaluates the template with `expr` and the leg's values.

### 3. Names are looked up, not parsed

The planner knows every leg it emits, so `./dev plan --ci` writes into the run's own scope line (version 3) an **`expected` map
`{display name: {job, leg}}`**, the leg with all its fields (`os`, `app`, `e2e_leg`, `part_label`, `test_packages`). Readers of a
run look a job's display name up in that run's line; when a run has no v3 line they fall back to the `Job:` line of the job's log
(`plan_since::job_key`, :466) for the id, and report the leg as not measured. No reader inverts a template: `crate (${{ matrix.os
}}${{ matrix.part_label }})` (ci.yml:905) has two adjacent expressions and the app template (:1130) hides `e2e_leg` inside
`format()`, so inversion is ambiguous by construction.

A name read from a calling workflow's run (the nightly's `<caller> / <job>`) has its caller prefix stripped inside the lookup, as
`active_apps.rs:226` does today, so no reader strips it on its own.

**App workflows write the same map.** `./dev ci scope` (the app workflows' scope writer) renders its legs with the same `render` and
writes `expected` too, so `plan_since::app_leg_of` (template inversion over names such as `end-to-end (shard ${{ matrix.shard }}/4)`)
becomes a lookup. Why this and not leaving them out: `app_leg_of` is the same inversion this ADR removes from ci.yml's readers, and
keeping it would leave a second mechanism for one job (rule 55). **Out of scope, deliberately:** the nightly's `installers (<App>,
...)` jobs. They are nightly-only, never reused, and their one reader (`active_apps::app_of_job`) needs only the app name; that match
stays, registered in `docs/known-copies.toml` with a delete-by tag, so it is a named copy, not a forgotten one.

`run_scope::check_ran` compares the map with the run's jobs exactly: a missing `3/3` partition is missing, which today's
`Starts("crate (ubuntu-latest")` cannot see. Reruns read the failed job's leg from the map, partition and `test_packages` included,
so a failed-only round reruns only that partition.

### 4. The leg set has one source

`plan_ci` produces every leg set (OS legs, partitions, app legs, rerun legs). `gates/ci.rs:237-246` takes the set from it instead of
a hard-coded three-OS axis; ci.yml's `PLAN_OS_LEGS` and its fallbacks are deleted, as the planner always writes the outputs;
`run_scope::DEFAULT_OS_LEGS` and `PROJECTS` are read from the model.

### 5. `settings` (decision 82)

One typed declaration of every repository variable and secret the tools read: name, type, parser, default, secret flag. No new
types: a secret is `paddock_secrets::Secret` (crates/paddock-secrets/src/secret.rs:11), and the environment is passed in as
`paddock_env::Vars` (crates/paddock-env/src/dirs.rs:34; `ProcessVars` in the adapter, `MapVars` in tests). Derived setup is returned
as data, never applied: the sccache environment, the test-cache store address. Adapters in paddock-dev apply it.
`ci_setup_rust::parse_endpoint` (:470, already shared by the local compile cache, 4af688922) moves here.

A report-only Rust check flags a direct `PADDOCK_*` read outside the crate; `tools/paddock-dev/src` names about 128 `PADDOCK_*`
variables in 66 non-test files (a text count; some are writes), its starting list (shrink-only, ER-46). Its positive control: a
fixture with one direct `PADDOCK_X` read goes red, the same read through `settings` stays green. It has a review date three months
after it starts blocking (ER-41).

### 6. Publish and the crate-coverage query (decision 81a)

`jobs::crate_coverage(model_lines, members)` answers, per OS leg at branch level, which workspace members no gated clippy or nextest
with `--all-features` covers, from `gates::shell` tokens (ER-42), with shell arrays assigned in the same block resolved (so
`sel=(--workspace)` counts without a rewrite). `scripts/publish.mjs` is ported to `./dev packages publish` (ADR 0022 row 40), which
calls the crate directly; no JSON bridge. `crateStepsMissing`, `crateCommands` and their tests go with the script. C37's
member-tested half reads the same query through `./dev gates crate-coverage --json` until C37 is ported.

**Callers, each changed in the same WP** (origin/main 42a884158): `.gitea/workflows/publish.yml:115` (`node scripts/publish.mjs
--publish`) and its header comment (:5); `ui/package.json:24` (`release`); ci.yml:546 (`./dev ci run ui publish-dry-run`, through
`ui/paddock.ci.json`); `tools/paddock-dev/src/plan.rs:1261` (the planner's mark for `scripts/publish.mjs`);
`conformance/tooling-files.json:2006` (the tooling register); the ci.yml:1015 comment (`scripts/publish.test.mjs and conformance C37`).
The app release publishers (`conformance/release/publish.mjs` and the Pacenotes and Sighter copies) are other programs and stay.

**Planner reach.** The Rust publisher's module and the crate become declared inputs of `ui`'s publish dry run (the `plan.rs:1261`
mark becomes a declared input in `docs/repo-graph.toml`), so a change to either still runs the dry run; a planner golden case shows it.

**Rule 54.** The port is on the release path: before it lands, the batch dispatches `publish.yml` as a dry run and
`tag-rehearsal.yml` once, both green on the landing head.

**Behaviour kept** (moved code keeps every behaviour). Of the script's 30 functions, `crateCommands`, `readCiModel` and
`crateStepsMissing` are replaced by `jobs::crate_coverage`; the other 27 get Rust counterparts with the same messages and error paths,
each pinned by a test ported from `scripts/publish.test.mjs` or written new where none exists:

| JS function(s) | Behaviour kept |
|---|---|
| `parseArgs` | only `--publish`, `--skip-build`, `--offline`; `--publish` refuses the other two |
| `indexPath` | sparse-index paths for 1, 2, 3 and 4+ character names; an empty name is an error |
| `workspaceCrates`, `publishOrder` | members from `cargo metadata`; dependency order, a dev-dependency with a version first (d7f4a00cd) |
| `crateProblems` | `publish` is the family registry or false, else a problem naming the fix |
| `lockstepProblems`, `shellVersionProblems` | npm packages carry `@decryp7/paddock-ui`'s version; `paddock-shell` carries it too |
| `tagProblems` | `--publish` only in CI and only on the tag `v<version>` |
| `entryPoints`, `missingExports`, `npmPackages`, `prepareNpm` | the built `@decryp7/paddock-ui` exports every entry point; `--skip-build` skips the build |
| `planRows`, `formatTable` | the publish or skip decision and reason per package, the same table text |
| `indexVersions`, `registryVersions` | versions from the registry; not reachable or `--offline` decides "publish" with that reason |
| `cargoToken`, `mask` | the token from the environment, masked in every line (now a `Secret`) |
| `linkUrl`, `linkAuth`, `linkPackages` | every package linked to the repository (257ddc183) |
| `listCrate`, `listNpm`, `publishCrate`, `publishNpm`, `createRelease` | the dry-run listing; publish with `cargo hakari publish` and npm, skipping published versions; the Gitea release of the tag |
| `main` | `cargo metadata --locked` when publishing; all problems refused together; then build, check exports, list, publish |

### 7. One check at the source

`./dev ci job-model check` runs in two places: as a step of ci.yml's plan job (every ci.yml run, whatever the change, ER-41) and as
a member-gate row (ER-9, ER-53), so a layout change fails before it reaches a train. It fails on:

- any `Problem` of the model;
- a job without `needs: [plan]`, `asks_rerun_jobs`, or the tool-cache step before its first `./dev`;
- **`render` not injective over every leg shape the planner can emit**: whole and partitioned Linux, each OS alone, the since-run
  rerun legs (one failed partition, one app leg with an e2e leg), the fallbacks; two legs with one name would make the lookup
  ambiguous;
- while `CI_JOBS` exists: `model.projects != CI_JOBS` for any job (the bridge to ADR 0117 D12).

Each rule has a positive control in the crate's tests (a template without `part_label`, a job without a rerun gate, a step-gated job
with no declared project). A test fails when the plan-job step or the member-gate row is removed (ER-33). The check starts
report-only (`::warning::`), blocks after its first green run on the real tree, recorded where it is defined, and has a review date
three months after it blocks (ER-41). It replaces `tests/it/tree.rs`'s job-table test and the ci.yml half of
`ci-reuse-wiring.test.mjs`.

With this, the pq split is one ci.yml change plus the planner's partition code: the planner renders `crate (ubuntu-latest, 2/3)` and
writes it into `expected`, every reader looks it up, and the check proves the three names differ. No reader has a list to update.

## Migration

- No backward compatibility before v1 (user, 2026-11-06). A run with only a v2 scope line is not reused (D2). Costs: an app tag on
  a commit whose only complete run has a v2 line needs a new full run; open trains whose verdict is a v2 run rerun once after the
  switch. The `quick checks (|C99 (` alternatives go with v2.
- Order: WP-1 lands report-only beside the old copies; WP-2 switches readers one by one, each with its failing test first (the
  partition name, the `leg_of` partition gap, a failed `ui` rerun); WP-3 replaces publish.mjs; copies leave with their last reader.
- `CI_JOBS` and `ci-jobs.json` stay until ADR 0117 WP-2's remaining deletion, after its five clean nightlies (D12); the check keeps
  them equal to the model until then.

## Work packages

| WP | Content | Agent-days |
|---|---|---|
| WP-1 | `crates/paddock-ci-config` with `jobs`: `yaml` and `expr` moved in (gates uses them), `PROJECT_OUTPUTS` moved in, the model with derived and declared `projects`, `render`, the leg set from `plan_ci` (`gates/ci.rs:237-246`; `PLAN_OS_LEGS` deleted), the projects == `CI_JOBS` test | 2.0 |
| WP-2 | Scope line v3 with the `expected` map, written by `./dev plan --ci` and by `./dev ci scope` for the app workflows; lookup with the caller-prefix strip and the `Job:` fallback; the `installers (` match registered in `docs/known-copies.toml`; readers switched (`plan_since::app_leg_of` too): `run_scope` (`PROJECT_JOBS`, `JobName`, `names_of_job`, `name_of_leg`, `DEFAULT_OS_LEGS`, `PROJECTS`), `plan_since` (`leg_of`, `key_by_name`, :894, :1093-1105, :1152 with partitions), `green.rs` (both sites), `triage.rs`, `queue/real.rs`, `active_apps`, `escape`; failing tests first | 2.5 |
| WP-3 | `crate_coverage` in `jobs`; `scripts/publish.mjs` ported to `./dev packages publish` with the 27 kept behaviours and their tests (taken from ci-in-rust WP-11's count); every caller of §6 changed; the publisher declared as an input of `ui`'s dry run with a planner golden; the rule 54 dry-run and tag-rehearsal dispatch; C37's member half reads the query | 2.5 |
| WP-4 | `settings`: the declaration on `paddock_secrets::Secret` and `paddock_env::Vars`, derived setup as data, `parse_endpoint` moved, the report-only direct-read check with its positive control and review date | 1.0 |
| WP-5 | `./dev ci job-model check`: plan-job step and member-gate row, injectivity over every leg shape, positive controls, removal tests, report-only then blocking, review date | 0.5 |
| WP-6 | One independent review round and its fixes | 0.5 |
| | **Total** | **9.0 (7.5 to 11.5)**; 2.0 moves out of ci-in-rust WP-11 and about 0.3 out of WP-7, so about **6.7 new**. The user approved 8.0 on 2026-11-26; revision 3's P2s (the app-workflow map, WP-3 at its upper end) add 1.0, 12.5 %, under ER-37's 20 % line |

Adds: one crate (two modules), one check (two call sites), one declared table. Removes: twelve hand copies (about 400 lines with
tests), the publish script, two text tests, and later `CI_JOBS` and its golden.

## How it fits ci-in-rust WP-7 and WP-11

WP-7 moves ci.yml's inline shell to `./dev ci <step>`; each step it moves keeps the job's `name:`, `if:` and matrix in ci.yml, so the
job model is unchanged by it, and every moved step makes the crate-coverage query read Rust arguments instead of shell. Only the
`crate` job's selection is touched here (about 0.3 days of WP-7). WP-11 was to port the publish scripts after the tags; decision 81a
takes `scripts/publish.mjs` now, so WP-11 shrinks by its count (2 days).

## WP-4 as built (settings)

Counted by a text search of `tools/paddock-dev/src` at 43c28c456: **141 distinct `PADDOCK_*` names in 91 files** (the 128 in 66 non-test files above was an estimate; the count here includes test files and doc comments). All 141 are declared in `crates/paddock-ci-config/src/settings/declared.rs` with their type, parser, default, secret flag and who sets them (`Scope`: repository variable, repository secret, machine, handoff between tools, test only). Names that are not configuration are declared with scope `Handoff` (set by one tool for the tools it starts, such as `PADDOCK_CI_SLOT_TOKEN`, `PADDOCK_RELEASE_*`) or `TestOnly` (stand-ins and test hooks such as `PADDOCK_TRIAL_DIR`, `PADDOCK_DF`, `PADDOCK_PS`); none is out of scope, and the table says which they are.

Readers switched to `settings`: the compile cache's S3 level (`S3Ci::from_vars`, with its release and tag wall), the local server's `backend` and `choose`, the test cache's store address and key, and `./dev disk` (its seven `PADDOCK_DISK_*` names, defaults read from the declaration). `parse_endpoint`, `ci_prefix`, the chain settings, the survival variables, the base folders and the CI key environment moved into the crate and the adapters apply what they return.

Deferred, one cluster per later change (the report of check C175 is the shrinking list; ER-46): the other readers, grouped as `ci_build_jobs` and `ci_isolation` (`PADDOCK_CI_*`, `PADDOCK_SLOT_*`, `PADDOCK_SOCKET_*`), `ci_host` (`PADDOCK_KEPT_CLONE_*`, guardian), `release_isolation`, `pre_push` (gate switches), `places`/`worktree`, `merge_check` and `ci_runs` (stand-in programs), `heavy_install`, `deps_cache`, `test_cost`. Each reads its variables through its own lookup shape today (a closure, a `HashMap`, the step's `Ctx`); each needs an adapter of the same kind as `EnvFn` or `EnvMap` and its own failing test, and none shares behaviour with the S3 cluster.

## Checks this ADR relies on (ADR 0096)

- Built on origin/main: `gates/ci.rs`, `gates/yaml.rs`, `gates/expr.rs`, `gates/shell.rs` (ADR 0060), `./dev gates model --json`,
  `plan_since::job_key`, `plan_ci::PROJECT_OUTPUTS`, `repo_model/ci.rs`, the member audits, the member gate.
- Landed on origin/main with train pq (42a884158): the partitioned `crate` job, `plan_ci::partition_linux_legs`, `JobName::Starts`,
  `test-cache-shadow`, `crateCommands`' Linux rewrite (8cc32c884), `parse_endpoint`'s second caller (4af688922); also built:
  `paddock_secrets::Secret`, `paddock_env::Vars`, `repo_model/ci.rs` `output_projects`, `./dev ci scope`, `docs/known-copies.toml`.
- Prerequisites, not built: ADR 0117 WP-2's remaining deletion and its D12 gate (WP-9); ci-in-rust WP-7, WP-10 (C37's port) and
  WP-11 (shrunk here); ADR 0096's register file (none on main; this list stands in for it).

## Decisions

- **81 (user, 2026-11-26).** The model is its own shared crate, `crates/paddock-ci-config`, a pure core with `jobs` and `settings`.
- **81a (user, 2026-11-26).** WP-3 ports `scripts/publish.mjs` to `./dev packages publish`, calling the crate directly; no JSON
  bridge; taken from the Rust port's count.
- **82 (user, 2026-11-26).** `settings` as in §5, about 1 day.
- **D1 = B (user, 2026-11-26).** 8 days approved (about 5.7 new) against about 1 for the aggregator job; revision 3 makes it 9.0
  (12.5 %, §Work packages). A cost to name: `yaml` and `expr` move into a `crates/` crate, so an edit of the CI parser now also marks
  the crates workspace's legs on three OS, a few more CI minutes per parser change.
- **D2 = (a) (user, 2026-11-26).** A v2 line is not reused: an app tag on a v2-only commit needs a full run, and open trains with a
  v2 verdict rerun once. (Not chosen: (b), read v2 with today's prefix table until the open trains have landed.)
- **D3 = (a) (user, 2026-11-26).** This ADR's WP-1 first; 0117 WP-2 later deletes `CI_JOBS` against the model. (Not chosen: (b),
  after 0117 WP-2.)
