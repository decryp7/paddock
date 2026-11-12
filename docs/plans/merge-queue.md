# Merge queue on the branch ledger (design note)

Status: design note, 2026-11-12. Built: the ledger (slice 0) and `./dev queue tick|hold|unhold` (slices 1 to 3 and the landing without rule 54, plan-status and tags: `tools/paddock-dev/src/queue/`). A pull request is taken only with an approving review of its current head and no `owes/*` label; main moves only when every reached workflow has a full green run; the gates run without the queue's credentials. Not built: the Windows leg, the rule 54 dispatch, auto-tag and queue.yml (slices 2, 4, 5 and 6 in part). Owner of `.gitea/` for this work: the integrator.
Inputs: the user's decisions of 2026-11-12 (below), the tools research of 2026-11-12
(`dev-targets/review-scratch/tools-research-2026-11-12.md`, candidates 3 to 5), rule 50, rule 54, ER-8, ER-10, ER-31,
ER-37, ADR [0080](../adr/0080-ci-at-the-speed-of-the-critical-path.md) and [docs/ci-schedules.md](../ci-schedules.md).

## Decided by the user (2026-11-12)

1. The branch ledger is Gitea draft pull requests with labels (`.gitea/labels.toml`, `./dev pr`). Built: slice 0.
2. Trains become a full merge queue that drops failing branches automatically, like GitHub merge queues and GitLab merge
   trains. Built after the ledger.
3. Owed Windows tests (`owes/windows-tests`) run in the queue's Windows leg.
4. Plan status is derived from the ledger, not kept by hand: `./dev pr plan-status` (built in slice 0) is the queue's
   landing step.
5. Release tags `*-v*` are protected and pushed by a release bot account; the queue's tag step uses the bot's token. The
   user creates the bot account.
6. Q1 (a): the fast-forward of main is automatic, with a hold switch. Q2 (a): batches of up to 6. Q3 (a): several suspects
   are all dropped and the rest retried (no bisection). The landed and decided state comes from one place, the ledger.
7. The queue runs as a scheduled Gitea Actions job, not from an agent session (see "Where the queue runs").
8. A landing that changes an app's version source tags that app through tag.yml; release-bot pushes the tag (see "Auto-tag
   on a version bump").
9. Development stays AI-only until the code is stable, so the automatic AI review of PRs is not built now; its design is
   kept under "Later: when people contribute".

## The ledger (slice 0, on `feat/dev-pr-ledger`)

- Every branch gets a draft pull request against main (`./dev pr open <branch>`): `WIP:` title (Gitea's draft prefix), the
  body lists the commits, label `state/built`. Opening it starts no CI run: ci.yml, debrief.yml and debrief-mutants.yml lost
  their `pull_request` trigger, and the contract test `no_workflow_runs_on_a_pull_request` keeps any workflow from getting
  one back.
- State labels are one exclusive scope: `state/built` -> `state/in-review` -> `state/fixes` | `state/approved` ->
  `state/in-train` -> `state/landed`. Debts: `owes/windows-tests`, `owes/vm-rows`, `owes/rule54-bundle`. Also `security`,
  `app/<app>` (one per app folder), `review-round/1|2` (exclusive).
- `./dev pr label <branch> +x -y`, `./dev pr list [--state approved]` (branch, head, labels, newest CI run per workflow),
  `./dev pr sync` (labels from the TOML; never deletes), `./dev pr plan-status [--write]`.
- **Prerequisite: "autodetect manual merge".** The queue lands by a fast-forward push, so Gitea marks a pull request merged
  only when the repository setting `autodetect_manual_merge` is on. On dev.decryptology.net it is **off** (read-only GET of
  `/api/v1/repos/decryp7/paddock`, 2026-11-12: `autodetect_manual_merge: false`, `allow_manual_merge: false`). The ledger does
  not depend on it: `plan-status` counts a pull request as landed when its head is on main, and `pr sync` warns while the
  setting is off. Turning it on (repository settings, pull requests) makes the server's view agree; the user decides.

## How the queue works

One queue for the repository, one train at a time. Runners are the scarce resource (rule 50; 35 min of every runner per
full train run, lessons:1751), so the queue does not run GitLab's or Zuul's parallel speculative pipelines; it runs one
batch, and only what failed is rerun.

1. **Take.** The `state/approved` pull requests, oldest approval first, up to 6 (Q2, decided). A PR that is
   not mergeable with main, is a draft of another base, or has no green review round is skipped with a comment. Each taken
   PR moves to `state/in-train`.
2. **Build the train.** A fresh branch `train/<date>-<n>` from origin/main, in the integrator's locked worktree. Each PR is
   merged with `--no-ff` in order, then `./dev merge check` on the merge (ADR 0089) and the workspace-hack diff; a conflict
   or a merge-check finding drops that PR at once (`state/fixes`, comment with the conflicting files or the finding) and the
   train goes on without it. Lockfiles are regenerated, never hand-merged.
3. **Gates in CI, not in the tick (coordinator decision, round 2 of the review).** The tick runs **no code of any pull request**:
   a process that holds a write token never runs what a pull request wrote (GitHub's and GitLab's merge queues do the same: the
   queue only merges, CI runs the code). So the gate register, the conformance line, commit-lint, the workspace-hack diff,
   `cargo metadata --locked` of every lockfile and the publish dry run are jobs of the dispatched workflow, run without the
   token: ci.yml's `train-gates` job (a train's dispatch has a `base`) and its `commits` job; a failure is an ordinary CI failure,
   triaged like any other (step 5). fmt and clippy stay in their existing jobs. The tick itself does git and the API only: it
   merges in its own clone of an explicit URL (never the job's checkout), with `core.hooksPath=/dev/null`,
   `protocol.file.allow=never`, no global or system git configuration and no filter driver, runs the merge check in process, and
   asks the planner from a checkout of `main` (a child without the token). No cargo, node or npm runs from the merged tree. The
   token reaches exactly one child, the push, through a `GIT_ASKPASS` program scoped to that command (`--authenticated-fetch`
   also gives it to clone and fetch for a private repository). A test plants hooks, a cargo wrapper, a `rust-toolchain.toml`
   path, git attribute filters, a lefthook file, npm scripts and a gate script in a train and checks that nothing runs.
4. **Dispatch (rule 50h, ER-10).** `ci.yml` (and each app workflow the plan reaches) by `workflow_dispatch` with `base` =
   origin/main: the affected plan of the whole train. Two things pull requests used to start must be dispatched or run here:
   debrief.yml's pull-request commit lint is gone (ci.yml's `commits` job lints the train's range on a dispatch, and step 3
   is a CI job), and debrief-mutants' changed-lines run is a dispatch of `debrief-mutants.yml` with
   `base: origin/main` when the train touches `apps/debrief/crates/debrief-core`. The Windows leg runs every crate and app that an `owes/windows-tests`
   PR touches, in one leg (needs a new dispatch input, slice 2). Slower local checks (Linux container, tests under load)
   run beside it; a local failure cancels the run.
5. **On red: find the culprit.** `./dev plan --since-run <run>` lists the failed jobs and tests. Each failed test belongs
   to a project (crate, app, tool); the suspects are the train's PRs whose affected set (`./dev plan origin/main <pr-head>`)
   contains that project. One suspect: it is dropped. Several: all are dropped (Q3, decided). A failure
   that also fails on main (the triage of tools-research candidate 2) is not blamed on any PR; the train stops and the
   coordinator decides. A dropped PR gets `state/fixes` and a comment naming the failed tests, the run and the job logs.
6. **Retry the rest.** The train is rebuilt without the dropped PRs and only what failed is rerun (`since_run`, ER-10 fix
   rounds). A suspected flake gets at most one rerun of the failed jobs on the same commit, never to turn red green (ER-12).
7. **Final run (ER-8).** When every round is green, one full affected run against main on the final train head. Nothing
   lands without it.
8. **Rule 54.** If any PR in the train touches an app's release path (`owes/rule54-bundle`, or the plan says so), the queue
   dispatches `<app>-release.yml` with gates on and the installers built, on the final head, before landing (rule 54(c):
   gates skipped only where a green ci.yml run on the same commit covered them).
9. **Land.** `git push origin <train-head>:main` as a fast-forward only (never a force, never a rebase), then check that
   origin/main equals the train head. Automatic (decision 6), unless the queue is on hold. Gitea marks each PR merged only
   with "autodetect manual merge" on (see the prerequisite above); the queue sets `state/landed` either way, clears `owes/windows-tests` for the PRs the Windows leg covered,
   and runs `./dev pr plan-status --write` on main's plan files (a docs-only commit, rule 50d) so no plan row says "open"
   for landed work.
10. **Tags (ER-31, decision 8).** After the fast-forward, the landing step looks for apps whose version source changed in
    the landed range and requests each tag through tag.yml (see "Auto-tag on a version bump"). Nothing else tags. ER-31(a)
    still lets an app tag on its release branch's tested commit when the failures are in code it does not ship; that request
    goes to tag.yml by hand, and the branch then goes through the queue like any other.

**One train's CI at a time per shared runner class.** Every dispatch of the queue (step 4, a fix round, the final run, a
rule 54 run) goes through `./dev ci dispatch`, which already waits while another paddock run holds a runner class the
dispatch needs (the macOS runner `d7-mac-mini` above all, which is one host) and gives up at `--max` without dispatching
(`tools/paddock-dev/src/ci_runs/dispatch.rs`, from `fix/scratch-root-guard-and-runner-busy`, carried on `fix/hooks-shellscan`
`ab3d98f0b`). The queue reuses it and builds no second guard. This removes a recurring class (ER-21),
**overlapping CI runs on one shared runner**: runs 7960 and 7961, and 7969 to 7971 with 7967, overlapped on d7-mac-mini, and
heavy Debrief tests then hit their fixed 90 s budgets. Hand dispatches made that overlap possible; the queue is the one
dispatcher of train runs, so it can hold the rule (coordinator, 2026-11-12).

**The Windows backup window (00:00 to 04:00 local).** The queue does not dispatch a run with a Windows leg that would start
in, or is likely to run into, 00:00-04:00 local (docs/ci-schedules.md). In that window it either holds the train, or runs
the Linux and macOS legs and keeps `owes/windows-tests` on the PRs, which then cannot land until a later Windows leg runs
(decision Q4).

## Where the queue runs (decision 7)

`.gitea/workflows/queue.yml` runs `./dev queue tick` (take, build, dispatch, judge, land: one step forward per tick, with the
state in the PR labels and the train branch, so a tick can stop anywhere and the next one carries on).

- **Triggers.** `schedule` every 5 minutes (`*/5 * * * *`; Gitea runs schedules from the default branch), plus label
  changes. Gitea Actions matches label changes as `pull_request` with `types: [labeled, unlabeled]` (the label hook is routed
  to the pull-request matcher and mapped to those activity types: `modules/actions/workflows.go` in go-gitea/gitea, read
  2026-11-12; without `types:` only opened, reopened and synchronize start a run, Gitea FAQ, docs.gitea.com/usage/actions/faq).
  The trigger is **`pull_request_target`**, so the workflow file comes from main, never from the PR's branch; the FAQ lists
  `pull_request_target` with the same types rule, but the matcher source above does not show how it is matched, so slice 6
  first proves on the server that a label change starts it. If it does not, the 5-minute schedule alone is the trigger.
  The contract test `no_workflow_runs_on_a_pull_request` then allows exactly this form: `pull_request_target` with `types`
  only `labeled`/`unlabeled`, in queue.yml, and still fails on any other pull-request trigger (a label change
  starts a queue tick or a review, never a build of the branch).
- **One queue run at a time.** `concurrency: {group: merge-queue, cancel-in-progress: false}`: a tick that starts while one
  runs waits instead of cancelling it (a cancel mid-push could leave a half-landed train). Concurrency groups already work on
  this server (ci.yml and debrief.yml use them; tools research: one run in 14 days outlived a newer push). Whether Gitea 28.3
  queues rather than drops a waiting run is checked in slice 6; the fallback is a lock the tick takes first (the train
  branch's lease, compare-and-swap on its ref) and a tick that finds it held exits at once.
- **Runner: Linux** (`ubuntu-latest`, the paddock-ci image). The tick merges (git only), dispatches and waits; it never takes the
  macOS or Windows runner, which the dispatched runs need, and it runs nothing a pull request wrote, so the token can be set for
  the whole step (slice 6 still passes it only to the tick's step).
- **Token: a new `queue-bot` account**, secret `QUEUE_BOT_TOKEN`, scopes: repository write (branches and main), issues and
  pull requests write, actions dispatch. Not release-bot: release-bot's token is the only one that can push protected `*-v*`
  tags (tag.yml on `ci/release-bot-tag-workflow`), and a queue job's token must not be able to tag whatever happens in it. A separate bot also makes the history say who did what. Its push to main starts the push-to-main
  runs, which the job's own token would not (Gitea ignores events whose doer is the Actions user, as tag.yml notes). The
  user creates queue-bot, as with release-bot.
- **Hold.** The backup-window hold stays: a tick between 00:00 and 04:00 local dispatches nothing with a Windows leg (Q4),
  and a `queue/hold` label on any PR, or `./dev queue hold`, stops every tick after its read step.

## Auto-tag on a version bump (decision 8)

- **Trigger: the queue's landing step** (step 10), not a push-to-main path filter, so landing and tagging stay in one
  place: the step that knows what it just landed also knows which apps' version sources changed
  (`git diff --name-only <old main>..<new main>` against each app's version source from `paddock.ci.json` and the
  tag checker's table). A path filter on main would also fire for a commit landed by hand and would need its own copy of that
  table.
- **What runs.** For each such app the step dispatches tag.yml (`POST .../actions/workflows/tag.yml/dispatches`, ref main,
  inputs `tag = <app>-v<version>`, `sha = <new main head>`) with queue-bot's token. tag.yml runs its same checks (the
  version source says that version, the newest full run of the app's gate workflow on exactly that sha is green, the tag
  gates) and release-bot's token pushes the tag in tag.yml's own `push` job. The queue holds no release token.
- **Timing.** The full run on the landed sha is the push-to-main run that the fast-forward starts, so the request waits for
  it (`./dev wait --ci-run`, bounded); a red run means no tag and a comment on the PRs that bumped the version.
- Depends on `ci/release-bot-tag-workflow` (`52e060a3a`) landing and on tag protection being on.

## Landing prerequisites

- **Verdicts.** `./dev ci verdict` counts a run only with the plan job's `plan verdict: complete=` line, which ci.yml and
  debrief.yml print after the plan decides: `fix/ci-verdict-reader-review` (177b0663c) must be on main before the queue lands
  anything, or every verdict is red or not measured.
- **Approval.** The queue accepts an approval only from `review-bot`: an official, undismissed, approving Gitea review of the
  pull request's current head, never from the pull request's own author, queue-bot or release-bot. Reviewer agents post their
  approval through review-bot after a passing review round. Main's protection rule has `enable_approvals_whitelist=true` with
  `[review-bot]` (so its approvals are official), `required_approvals` 0, normal push allowed, force-push off; review-bot has
  Write on paddock and its token is the secret `REVIEW_BOT_TOKEN`.
- **The train branch.** `train/queue` is protected: push by queue-bot only, no force-push. The tick still checks the branch
  before it trusts it: the first-parent chain from main is exactly one `--no-ff` merge per marker pull request, each merge's
  second parent is that pull request's approved head and its tree is what merging its parents gives, then one marker commit on
  an unchanged tree; any other head is rebuilt, never landed.
- **Labels.** `queue/hold` and `queue/blocked` exist on the server (`./dev pr sync`).
- **Workflow files.** ci.yml is read-only (`permissions: contents: read, actions: read`) and its checkouts keep no credentials
  (`persist-credentials: false`), as tag.yml's do. A train is dispatched with `base` set to main's commit id, never a ref name:
  the commit lint and the plan resolve it as one.
- **A blocked train.** One whose run fails only in tests that also fail on main is `queue/blocked` (not triaged again) until main
  moves or another pull request is approved.

## Common practice (ER-37)

| System | Batches | On a failure | Lands | Source (fetched 2026-11-12) |
|---|---|---|---|---|
| GitHub merge queue | temporary `merge_group` branch per entry, with the entries ahead; build concurrency 1-100; min and max group size, wait time | the failing PR is removed and the branches behind are rebuilt; no bisection is described | automatic once the required checks pass | [docs](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue) |
| GitLab merge trains | each MR's pipeline includes the MRs ahead; up to 20 parallel pipelines (minimum 1, sequential) | the failing MR is removed; the pipelines behind it restart without it | automatic; "merge immediately" exists for emergencies | [docs](https://docs.gitlab.com/ci/pipelines/merge_trains/) |
| bors-ng | one batch on `staging`, one collecting | bisects the batch until it isolates the failing PR | fast-forwards master to the tested commit | [repo](https://github.com/bors-ng/bors-ng) (archived 2024-04-04; recommends GitHub's queue) |
| Mergify | `batch_size`, `max_parallel_checks` for speculative batches | splits the failed batch to find the culprit; `batch_max_failure_resolution_attempts: 0` drops the whole batch | automatic; fast-forward in serial mode | [docs](https://docs.mergify.com/merge-queue/batches/) |
| Zuul (OpenStack) | dependent pipeline: each change tested with all ahead of it; window starts at 20, halves on a failure | the failing change is removed; changes behind are re-tested from the new tip | automatic, in queue order | [docs](https://zuul-ci.org/docs/zuul/latest/gating.html) |
| Gitea 28.3 (ours) | none: no merge queue; "merge when checks succeed" exists since 1.17 (secondary sources, not verified here) | - | - | tools research, appendix "Gitea" |

What this design does the same: tests the combination before main gets it (all five), removes the failing entry and
retries the rest (GitHub, GitLab, Zuul), main gets exactly the tested commit by fast-forward (bors, Mergify serial mode).
What it does differently and why: one train at a time instead of parallel speculative pipelines, because the runners are
shared and scarce (rule 50; a full train run costs about 35 min of every runner); culprits are first narrowed by the
affected plan, which none of the five can do, so most failures need no bisection run at all.

## Slices and cost (agent-days)

| Slice | What | Cost |
|---|---|---|
| 0 | Ledger: labels, `./dev pr open/label/list/sync/plan-status`, no `pull_request` triggers | 1.5 (this branch) |
| 1 | `./dev queue build`: take approved PRs, build the train branch with merge check and hakari diff, fast gates, labels | 1.5 |
| 2 | `./dev queue run`: dispatch through `./dev ci dispatch` (its runner-busy wait) with `base` and `since_run`, a Windows-leg dispatch input for owing crates and apps, the debrief-mutants dispatch, wait with `./dev wait --ci-run`, the backup-window guard | 1.5 |
| 3 | Culprit and retry: failed tests to projects to suspect PRs, drop all suspects with `state/fixes` and a comment (Q3), rebuild, failed-only rerun, the final full run | 1.5 |
| 4 | Landing: rule 54 dispatch, fast-forward with the remote-head check, `state/landed`, `plan-status --write` commit | 1 |
| 5 | Auto-tag: version-source diff per app in the landing step, tag.yml dispatch, wait for the full run (after tag.yml lands) | 0.5 |
| 6 | queue.yml: schedule plus `pull_request_target` labeled, concurrency, Linux runner, queue-bot token scoped to its steps, the hold, the contract-test exception, proving the trigger and the queueing on the server | 1 |

Full queue with the scheduled job and auto-tag: **7 agent-days** after slice 0 (slices 1 to 6).

**Cost against common practice, plainly.** On GitHub or GitLab a merge queue is a setting: 0 build days, the platform runs
it. Gitea has none, so here it is built. The research recommended the **thin `./dev train` first** (tools research D3a,
2 d; the full queue only if trains still take more than 12 h after the triage tool). The full queue is about 5 days beyond
that, so under the ER-37 amendment (more than 1 day beyond common practice needs the user's yes) its cost needs the user's
explicit yes: the 2026-11-12 decision for a full queue was made before this cost was on the table. Options, always on
offer:

- **Thin `./dev train`** (tools research D3a, 2 d, recommended there as the first step): slices 1 and 4 only; the
  integrator still reads failures and drops PRs by hand with `./dev pr label`. Keeps most of the speed-up of the ledger.
- **Scripted checklist** (0.5 d): the integrator's steps as a script that prints the next command; no automation of drops.
- **Gitea's "merge when checks succeed"** (0 d to try): merges one PR when its own checks pass, but tests no combination,
  so it gives up what trains are for; not recommended.

## Decisions

Decided by the user (2026-11-12):

- **Q1. Fast-forward of main: automatic**, after the final full run, the merge check and rule 54 are green, with a hold
  switch (a `queue/hold` label or `./dev queue hold`).
- **Q2. Batch size: up to 6** PRs per train.
- **Q3. Several suspects for one failure: drop them all** and retry the rest; each dropped author gets the failed tests.
  No bisection.

Open for the user:

- **Cost (ER-37 amendment).** (a) The full queue as a scheduled job with auto-tag, 7 agent-days after slice 0. (b) The thin
  `./dev train` first, 2 d, and the rest only if trains still take more than 12 h (the research's recommendation).
  (c) The scripted checklist, 0.5 d.
- **Bot account.** The user creates queue-bot (`QUEUE_BOT_TOKEN`).
- **"Autodetect manual merge".** Turn it on for decryp7/paddock (recommended; the ledger works either way), or leave it off.
- **Q4. Owed Windows tests during the backup window.** (a) Hold the train until 04:00 (recommended when anything owes
  Windows tests). (b) Run Linux and macOS now and land only the PRs that owe nothing.
- Also decided (user, 2026-11-12): protected `*-v*` tags with a release bot account; the queue's tag step uses the bot's
  token; the user creates the bot account.

## Later: when people contribute

Development stays AI-only until the code is stable (user, 2026-11-12), so nothing below is built now.

### Auto AI review of PRs marked ready

Not built now (decision 9). When it is, it is one slice of about 1.5 agent-days: ai-review.yml and its script, the SDK
call, the diff fetched as data, the size cap, one comment, a test against a stubbed API, and token measurement on 20 past
diffs. It also adds `ai-review.yml` to the contract test's label-trigger exception.

- **Trigger.** `.gitea/workflows/ai-review.yml` on `pull_request_target` with `types: [labeled]`, running only when the
  added label is `state/in-review` (the ready label). It posts one PR comment with P1 and P2 findings and changes no label:
  the human or agent review round still decides, and `state/approved` is still required to queue.
- **Call.** A small script with the official SDK (`@anthropic-ai/sdk`, the root npm workspace): model `claude-opus-5-5`,
  adaptive thinking (`thinking: {type: "adaptive"}`), `output_config.effort: "high"`, streamed with `.finalMessage()`,
  `max_tokens` 64000, and server-side refusal fallbacks (`fallbacks: "default"`, beta `server-side-fallback-2026-07-01`).
  The system prompt (the reviewer role, `AGENTS.md`, `docs/rules.md`, the P1/P2 format) is stable and cached
  (`cache_control`, 1 h); the diff goes after it. No tools are declared: the model can only return text.
- **Secrets.** One secret, `ANTHROPIC_API_KEY`, which the user provides. The job is a single Linux job that references no
  other secret; the reviewer step gets only the API key, and a separate later step posts the comment with the job's own
  token (issues write). Because the trigger is `pull_request_target`, the workflow file and the script come from main, so a
  PR cannot edit them to read other secrets; the PR's diff is fetched through the API as data (`.diff` of the pull request)
  and nothing from the PR branch is checked out or run.
- **Prompt injection.** PR content (diff, commit messages, file text) is untrusted. It goes into a user message inside clear
  delimiters, and the system prompt says it is data to review, never instructions. The worst a hostile diff can do is bend
  the comment's text: the model has no tools, the job has no write access beyond one comment, the comment is posted
  verbatim as the bot's review, marked "automated, advisory", and no label or merge depends on it.
- **Cost (billed per review, estimates, measure before trusting them).** Claude Opus 5.5 is $4 per million input tokens and
  $20 per million output tokens. An average branch diff here is about 20-40k tokens (to be measured with `count_tokens` on
  the last 20 landed branches before the job is switched on); the stable prompt about 15k, cached after the first review of
  the hour (cache reads $0.20 per million). Output with thinking is about 5-15k tokens. That is about $0.25-$0.50 per
  review (40k input $0.16, 10k output $0.20, plus the first cache write), and about $1-$2 for a large train-sized diff of
  150k tokens. A cap: diffs over 200k tokens are not sent; the job comments "too large for the automated review".
- Common practice for the cost (ER-37): GitHub and GitLab offer review bots as installable apps (0 build days); here it
  would be 1.5 d plus the per-review bill, so it needs the user's yes when it is picked up.
