# NEW. The CI job model is read from ci.yml once, and every reader of job names, legs and projects asks it

- Status: Proposed 2026-11-26 (redesign, ER-21: the class was seen twice in train pq). Needs the user's D1 to D3.
- Date: 2026-11-26
- Number: assigned at landing (user decision); the file is `NEW-ci-job-model.md` until then.
- Related: ADR [0060](0060-every-check-runs-somewhere.md) (the CI model, `gates/ci.rs`, `gates/yaml.rs`, `gates/expr.rs`), ADR
  [0113](0113-a-run-declares-its-scope-as-data-and-one-reader-decides-complete.md) (the scope line and `run_scope`), ADR
  [0117](0117-the-repository-graph-is-the-one-model-and-every-plan-is-a-query.md) (WP-2 deletes `plan_gitea`; §14 lists `CI_JOBS`
  as a rule to remove), ADR 0116 (the test-result cache and the Linux partitions, on the train), [ci-in-rust](../plans/ci-in-rust.md)
  WP-7, ER-21, ER-22, ER-33, ER-37, ER-41, ER-42, ER-44, rule 55.
- Evidence heads read (ADR 0091), re-read right before committing: `origin/main` f8970d88ce0d8b2629caa1182cf9b74c9c48d647;
  `origin/train/2026-11-26-pq` 9c390b56789f65dedf5f64f972a236e5a544ef1f (its merge base with main is main's head). Code was read with
  `git show` and `git diff` between the two; nothing was built or run. No claim here is about released code: `git tag --contains
  495f27fb1` is empty, and the partition split (6284b311f) is only on the train.

## The class

**The shape of ci.yml's jobs (ids, display names, matrix legs, partitions, the project each job runs for, the gates every job must
carry) is written down again, by hand, in each program that reads a run. A layout change in ci.yml is correct in ci.yml and breaks
the copies, and each copy is caught, if at all, by its own late check.**

The invariant this ADR wants: ci.yml is the only place the layout is written; one typed model is built from it by the parser that
already reads it (ADR 0060's), and every reader asks that model. A layout change then fails, or passes, in one place, in the run
that makes the change.

## Instances

| When | Change in ci.yml | Copy that broke | How it showed | Source |
|---|---|---|---|---|
| 2026-11-26, train pq | the Linux `crate` leg split into `crate (ubuntu-latest, k/3)` (6284b311f) | `run_scope::names_of_job` matched `crate (<os>)` exactly; the fix added `JobName::Starts`, and the merged tree did not compile until 495f27fb1 added the variant to the partition test | lib tests failed to compile on the merged train | 495f27fb1, 6284b311f |
| 2026-11-26, train pq | same split: the coverage step became nightly/release/tag only, the Linux all-features tests moved into the partitioned leg with `sel=(--workspace)` on its own shell line | `scripts/publish.mjs` `crateStepsMissing` regex-matches `--workspace` on the `cargo nextest` line of the gates model's command text | publish check failed in run 9135 (per the brief; the log was not read here) | `scripts/publish.mjs:195`, ci.yml `crate` job on the train |
| 2026-11-26, train pq (latent, found while reading) | same split | `plan_since::leg_of` reads the OS with `^[a-z-]+ \(([a-z0-9]+-[a-z0-9.]+)\)$`, which `crate (ubuntu-latest, 2/3)` does not match | not seen yet: a failed partition gets `leg.os = None`, so a failed-only round reruns every crate leg whole (`plan_since.rs` `os_legs`, fail-safe but wider), and the escape monitor reports the job as "names no project or OS" (`escape/cmd.rs` `failures_of`) | train `plan_since.rs:601`, `:1148` |
| 2026-11-25/26 | the short Linux checks folded into `quick` (ad2de361f), reverted and reapplied (4f39829f0, ffd93bc7f) | `run_scope::PROJECT_JOBS` named `C99 (` and `heavy tool`; recorded runs kept the old names | fixed by name alternatives `quick checks (|C99 (` and `JobName::AnyPrefix` | cf0a7090c, 0bf900d12 |
| 2026-11-09 onward | every new job | `plan_gitea::CI_JOBS`, `tests/golden/ci-jobs.json`, the `rerun_jobs` gate, the `dev-tool-cache` step before the first `./dev` | caught only by `tests/it/tree.rs` (`every_ci_yml_job_is_in_the_job_table_and_asks_rerun_jobs`, member audits), `tests/it/golden.rs` and `conformance/test/ci-reuse-wiring.test.mjs`; a missed job broke since-run reruns (06d15063d, gitleaks) | 202e53d28, 06d15063d |

## Where the copies are (origin/main, and what the train adds)

| Copy | What it re-types | Lines (about) |
|---|---|---|
| `plan_gitea::CI_JOBS` and `tests/golden/ci-jobs.json` | job id to plan projects (the train adds `test-cache-shadow`) | 15 + 42 |
| `run_scope::PROJECT_JOBS`, `JobName` (`Exact`, `Prefix`, `AnyPrefix`, train: `Starts`), `names_of_job`, `name_of_leg` | project to display-name prefixes, leg to display name, old names | 75 |
| `run_scope` test `name_of(...)` asserts | the three `name:` templates, verbatim | 25 |
| `ci_runs/green.rs` (around line 340) | display-name prefixes `app `, `crate (`, `paddock-dev (` to flags | 15 |
| `active_apps::app_of_job` | `app <a> (` and `installers (<App>` | 5 |
| `plan_since::leg_of` | OS and app regexes over display names (while `name_templates`/`matrix_of` in the same file already invert templates generically) | 20 |
| `escape/cmd.rs` `failures_of` | job to project through `CI_JOBS`, OS-less jobs by name | 10 |
| `scripts/publish.mjs` `crateStepsMissing` and its tests | "every crate is clippied and nextested with every feature" as regexes over shell text | 12 + 25 |
| `tests/it/tree.rs`, `conformance/test/ci-reuse-wiring.test.mjs` | the per-job conventions (`needs: [plan]`, `rerun_jobs` gate) by text search | 20 + 15 |

Three Rust readers of ci.yml already exist: `plan_gitea::workflow_jobs` (a line splitter), `gates/ci.rs` (ADR 0060, `yaml-rust2`,
jobs, steps, matrices, conditions, levels, exposed as `./dev gates model --json`) and `repo_model/ci.rs` (ADR 0117 WP-2, which reads
YAML through `gates::yaml` and what a job runs for from its `if:` through `plan_ci::PROJECT_OUTPUTS`). The facts every copy above
re-types are already in what `gates/ci.rs` parses; nobody exposes them as jobs.

## Common practice (ER-37)

- **One aggregator job.** GitHub/Gitea projects that must not list matrix job names make one `always()` job that `needs` all others
  and fails when any needed job failed or was skipped wrongly; branch protection and readers look only at it
  ([re-actors/alls-green](https://github.com/re-actors/alls-green): matrix leg statuses "are not exposed by the GitHub platform",
  and a skipped check reads as passed). It answers "did the run pass" in one name, not "which project and OS passed", which
  Paddock's reuse (ER-49, ADR 0113), failed-only reruns and the escape monitor need. Cost here: about 1 day.
- **A declarative job file that generates the matrix.** rust-lang/rust keeps job definitions in
  [`src/ci/github-actions/jobs.yml`](https://github.com/rust-lang/rust/blob/master/src/ci/github-actions/jobs.yml) ("loaded
  dynamically in CI from ci.yml"), and its `citool` computes the matrix and checks PR and CI jobs agree. Paddock already does the
  dynamic half: the plan job emits the legs (`fromJSON(needs.plan.outputs.*)`, the
  [documented pattern](https://docs.github.com/en/actions/how-tos/write-workflows/choose-what-workflows-do/run-job-variations)),
  and `plan_ci::partition_linux_legs` makes the partitions on the train.
- **Test partitioning** is usually the test runner's: nextest has `--partition slice:m/n` and `hash:m/n`
  ([nexte.st](https://nexte.st/docs/ci-features/partitioning/)). Paddock partitions by whole packages instead so each package's
  pass can be cached (ADR 0116); that is why the legs come from the planner and not from nextest.

## Options

| | Option | What it is | Deletes | Cost (agent-days) |
|---|---|---|---|---|
| A | Job model file generates ci.yml parts | a `ci-jobs.toml` (ids, name templates, legs, projects, gates); a generator writes the job headers of ci.yml; CI checks they match (ER-40) | all copies | 5 to 7: a generator over hand-edited YAML, a second source for names until every job moves, and every ci.yml edit becomes a regenerate |
| **B** | **ci.yml is the source, read once into a typed job model in `gates/ci.rs`** | `JobModel` built by ADR 0060's parser; readers call it; publish.mjs reads its JSON until ci-in-rust WP-11 ports publish | all copies; no new file, no generator | **4.5 (3.5 to 6)** |
| C | Aggregator job only | `ci-ok` job; the verdict reads it | the "all jobs passed" part of `PROJECT_JOBS` | about 1; leaves reruns, escape, publish and per-project reuse on their copies |

**Recommended: B.** It is the smallest change that removes the cause (ER-44): the parser and the expression evaluator exist,
`name_templates`/`matrix_of` already show that templates can be inverted, and nothing new is written by hand. A adds a source and a
generator for the same facts. C is cheaper and common, but it answers a question Paddock's readers do not ask; it is the fallback if
the user wants to stop at 1 day, and it can be added to B later for branch protection. B is about 3.5 days beyond C, so it needs the
user's yes (ER-37).

## Design (option B)

1. **The model.** `gates::ci::job_model(&Sources) -> Result<JobModel, Vec<Problem>>`, built from the same parsed workflow as
   `CiModel`. Per job: `id`; `name` (the raw `name:` template, the id when absent); `legs`: `Static(values)` from a literal
   `matrix:` or `Planned(output)` when the matrix is `fromJSON(needs.plan.outputs.<output>)`; `projects` from the `if:`'s
   `needs.plan.outputs.<name>` through `plan_ci::PROJECT_OUTPUTS` (the planner's own table, as `repo_model/ci.rs` does), `always`
   for a job with no project gate; `needs`; and the conventions as facts: `asks_rerun_jobs`, `tool_cache_before_first_dev`.
   Anything the parser cannot tell (an anchor, an expression `gates::expr` does not evaluate) is a `Problem`, never a guess (ER-33).
2. **Two queries, both ways.** `render(job, leg) -> String` evaluates the template with `gates::expr` and the leg's values (the
   `app` template's `format(', {0}', ...)` is in its subset). `identify(display_name) -> Option<(JobId, Leg)>` inverts it, the
   generalised `matrix_of`; an ambiguous or unmatched name is `None` and the reader reports it as not measured.
3. **The scope line names exact jobs.** The planner already knows the legs it emits, so `./dev plan --ci` writes the rendered
   display names of every job and leg it switches on into the scope line (`expected`, scope version 3). `run_scope::check_ran`
   compares names exactly: a missing `3/3` partition is then missing, which today's `Starts("crate (ubuntu-latest")` cannot see.
   `PROJECT_JOBS`, `JobName`, `names_of_job` and `name_of_leg` are deleted.
4. **Every other reader asks the model.** `green.rs`, `active_apps::app_of_job` (the ci.yml part), `plan_since::leg_of` and
   `key_by_name`, `escape::failures_of`: `identify` and `projects`. `plan_gitea::CI_JOBS` and `tests/golden/ci-jobs.json` go; the
   job-to-project fact is `projects` (this is the `CI_JOBS` row of ADR 0117 §14, done as model data).
5. **publish.mjs.** The crate test and Clippy commands move from inline shell into `crates/paddock.ci.json` phases run by
   `./dev ci run crates <phase>` (this is ci-in-rust WP-7's work for the `crate` job), so the gates model sees each command as
   written, `--workspace` included. A Rust query `./dev gates crate-coverage --json` answers "which workspace crates no
   branch-level gated clippy or nextest with `--all-features` covers, per OS leg", using `gates::shell` tokens, not regexes (ER-42);
   `publish.mjs` reads it and `crateStepsMissing` is deleted. When WP-11 ports publish, the query is called in-process.
6. **One check at the source.** `./dev ci job-model check` runs as a step of the plan job, so it runs in every ci.yml run, whatever
   the change (ER-41): it builds the model and fails on any `Problem`; on a job without `asks_rerun_jobs`, without
   `needs: [plan]`, or without the tool-cache step before its first `./dev`; and on a round trip that fails: for every job and
   every leg the planner can emit for the current tree (`plan_ci` with every project on, partitions included),
   `identify(render(job, leg)) == (job, leg)`. Its unit test carries a positive control per rule (a fixture with a partition
   label added, a job with no rerun gate, an ambiguous template). It starts report-only (`::warning::`) and blocks after its first
   green run on the real tree, recorded where it is defined. It replaces `tests/it/tree.rs`'s job-table test, the golden and the
   ci.yml half of `ci-reuse-wiring.test.mjs`.

With this, the pq split would have been one ci.yml change plus the planner's partition code: `render` gives `crate (ubuntu-latest,
2/3)` from the template, `identify` reads it back with its OS, and the scope line expects all three names. No reader has a list to
update.

## Migration

- No backward compatibility before v1 (user, 2026-11-06). A v2 scope line (no `expected`) is read as "not complete for reuse": that
  commit is run again instead of reused (fail closed, ER-49 stays safe). The cost is at most one run per commit whose verdict would
  have been reused across the switch, mostly the open train. The `quick checks (|C99 (` alternatives go with it (D2).
- Order: WP-1 lands report-only beside the old copies; WP-2 switches readers one by one, each with its failing test first (a
  partition name, an old-name run); WP-3 removes `crateStepsMissing` after the crates phases exist; the copies leave in the WP
  that switches their last reader.
- ADR 0117 WP-2 deletes `plan_gitea`; this ADR's WP-1 is the place its `CI_JOBS` fact goes. If 0117 WP-2 lands first, WP-2 here
  only re-points `escape::failures_of`.

## Work packages

| WP | Content | Agent-days |
|---|---|---|
| WP-1 | `JobModel`, `render`, `identify` in `gates/ci.rs` (no new parser), `./dev ci job-model check` in the plan job, report-only; unit tests with positive controls for each rule | 1.5 |
| WP-2 | Scope line v3 with `expected`; `check_ran` exact; `green.rs`, `active_apps`, `plan_since::leg_of`/`key_by_name`, `escape::failures_of` on the model; delete `PROJECT_JOBS`, `JobName`, `names_of_job`, `name_of_leg`, `CI_JOBS` (if 0117 WP-2 has not), `ci-jobs.json`, the tree.rs and golden tests; failing tests first for the partition and the `leg_of` instance | 1.5 |
| WP-3 | The `crate` job's clippy and nextest commands as `crates/paddock.ci.json` phases (the `crate` part of ci-in-rust WP-7, so WP-7 shrinks by about 0.3), `./dev gates crate-coverage --json`, `publish.mjs` reads it, `crateStepsMissing` and its tests deleted | 1.0 |
| WP-4 | One independent review round and its fixes; flip the check to blocking after its first green run | 0.5 |
| | **Total** | **4.5 (3.5 to 6)**, of which about 0.3 moves out of ci-in-rust WP-7 and about 0.3 overlaps ADR 0117 WP-2 |

Adds: one model view and one plan-job step. Removes: nine hand copies (about 300 lines with their tests), one golden, two text
tests (one in, one out).

## How it fits ci-in-rust WP-7

WP-7 moves ci.yml's inline shell to `./dev ci <step>`. That is what makes the model see commands, not shell variables: once the
`crate` job's `sel=(--workspace)` logic is a Rust step or a contract phase, the gates model knows which crates each leg covers, and
publish's question becomes a query. This ADR takes the `crate` job's part of WP-7 now (WP-3) because it is the part a reader
depends on; the rest of WP-7 keeps its order. Each later WP-7 step that moves a job's shell keeps the job's `name:`, `if:` and
matrix in ci.yml, so the job model is unchanged by it.

## Checks this ADR relies on (ADR 0096)

- Built on origin/main: `gates/ci.rs`, `gates/yaml.rs`, `gates/expr.rs` (ADR 0060), `./dev gates model --json`,
  `plan_since::name_templates`/`matrix_of`, `plan_ci::PROJECT_OUTPUTS`, `repo_model/ci.rs`, the member audits.
- Held on `origin/train/2026-11-26-pq`: the partitioned `crate` job, `plan_ci::partition_linux_legs`, `JobName::Starts`, the
  `test-cache-shadow` job (ADR 0116).
- Prerequisites, not built: ci-in-rust WP-7 (only its `crate` part is taken here); ADR 0117 WP-2's deletion of `plan_gitea`
  (coordinated, not required); ADR 0096's register file (none on main; this list stands in for it).

## Decisions for the user

- **D1. Scope.** (a) Option B, 4.5 days. (b) Option C, the aggregator job, about 1 day, the copies stay. (c) Option A, 5 to 7 days.
  *Recommended: (a).* It is about 3.5 days beyond common practice (C), so it needs a yes (ER-37).
- **D2. Old scope lines.** (a) A v2 line is not reused: those commits run again (no compatibility before v1). (b) Keep reading v2
  with today's prefix table until the open trains have landed, then delete it. *Recommended: (a).*
- **D3. Order with ADR 0117.** (a) This ADR's WP-1 first; 0117 WP-2 then deletes `plan_gitea` with no `CI_JOBS` left. (b) After 0117
  WP-2. *Recommended: (a)*, as the partition split is already on a train.
