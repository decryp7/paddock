# 60. Every check runs somewhere: a gate register and a meta-check over checks, CI and the docs that name them

- Status: Accepted (2026-11-08), revision 3. The user approved this redesign on 2026-11-08 ("do all redesigns now"),
  and the independent review approved it: revision 1 (`69ef3c161`) CHANGES NEEDED with 3 P1; revision 2 (`16657fcc2`)
  fixed two of them and the delta review raised two more (the Debrief ratchet, the nightly backstop of the ADR 0038
  amendment) with P2s; revision 3 answers those.
- Trimmed 2026-11-11 (user, comparable-practice review): TRIM: keep WP-5 (rule), WP-7 (run or delete orphan scripts), WP-8 (nightly proptest and `npm audit`) and a reduced WP-4 (Debrief's conformance step in `debrief.yml` only); dropped WP-6; deferred WP-9 (the 27 `paddock-setup` VM rows stay a manual pre-release check).
- Amendment 2026-11-12 (user, cut review of 2026-11-12; Tier A of the Debrief 1.2 plan, [section 10.7](../plans/debrief-1-2.md#107-cut-review-tier-a-applied-user-2026-11-12)): C13 for Debrief only bans `std::env::temp_dir` and `reqwest::Client::new`: there is no crate-by-crate `unwrap`/`expect`/`unsafe` ratchet on existing code (reverses the decisions of 2026-11-08 and 2026-11-09). The Debrief `[[ratchet]]` of §3.2 in `docs/gates.toml` must stop refusing `debrief-v1.2.0`; that needs a code change (C100 does not read the `Gates-date:` trailer that a target move requires), which is not made here. Earlier text is history.
- Date: 2026-11-08
- Related: ER-21 / rule 40 (recurring bugs mean a missing design rule), ER-33 / rule 71 (a check counts only if its probe
  can see the thing), ER-27 / rule 56 (every failing step prints one `::error::` with the cause and the next step), ER-30
  (docs link, they do not restate), rule 50 (CI economy), rule 55 and ADR 0034 (one implementation per mechanism), ADR
  0022 (all tooling in Rust, `./dev`) and its amendment on `origin/docs/adr-0022-generic-tools` (conformance moves to a
  Rust rules engine, `./dev check`), ADR 0038 (CI's commands become data, `paddock.ci.json` per project, and its
  amendment of 2026-11-08, the per-commit gate, which is the other half of the same CI-hygiene batch), ADR 0042 (registers
  as TOML under `docs/`). The redesign "doc facts from code" (user approval 2026-11-06, memory `redesigns-2026-11-06`, not
  started) overlaps §5; the split is stated there.
- Number: 0060, assigned by the coordinator on 2026-11-08 (0058 failed reads retry, 0059 installer declarations on
  `docs/installer-declarative-adr`, 0060 this ADR, 0061 job identity, 0062 build inputs). Debrief's ADRs are a separate,
  per-app sequence. Check ids: C80 to C99 are used or reserved on some branch or ADR (`git log --all -p` over `docs/adr`,
  `docs/rules.md`, `AGENTS.md` and `conformance/src/checks` since 2026-10-20); no three-digit id appears anywhere, so
  this ADR takes **C100**.

## Evidence

Read at `origin/main` `9e18230c5` in the worktree `adr-ci-hygiene`, and in the coordinator log
`/Volumes/d7sst7/dev-targets/handoff/2026-10-29-overnight.md` (cited as "log" with its line), 2026-11-08. Counts marked
"review" are the independent reviewer's spot-checks of revision 1 (2026-11-08); the others I re-ran for revision 2.

| # | Instance | Verified by |
|---|---|---|
| 1 | **C13 runs in no CI job.** The only conformance run in CI is ci.yml:257, `--app paddock --root . --only C06,C14,C15,C16,C24,C29,C30,C31,C34,C35,C36,C37,C39,C43,C48,C53,C54,C56,C57`; C13 is not in it, and no app leg or app workflow runs conformance. Run by hand per app (`--app <app> --root apps/<app> --only C13`): Dyno 3 errors, Pitcrew 3, Spotter 3, Pitwall 2, all missing `disallowed-methods` entries (`std::env::temp_dir`, `reqwest::Client::new`, `tokio::process::Command::new`); fixing them means moving the call sites to the sanctioned helpers too: 44 `temp_dir` call lines and 2 `tokio::process::Command::new` in the four apps (delta review; revision 2's "about 149" was wrong). **Debrief is not drift but a lint preset never adopted** (52 errors): its workspace `Cargo.toml` has no `[workspace.lints]` at all and 46 members do not opt in (review), and the review counted about 7,165 `unwrap` and 244 `unsafe` in non-test code that the preset would flag. | log 1292, 1295, 1296; the runs above; the review |
| 2 | **C13's probe is blind on four apps** (ER-33): it reports "there is no workspace Cargo.toml at the repository root" for Pacenotes and Sighter, which have no Rust yet (the check does not apply), and for shell-demo and workbench-demo, whose workspace is `src-tauri/` as their `paddock.ci.json` says (`"cargo": "src-tauri"`). A check nobody runs also never gets its probe fixed. | the runs above |
| 3 | **`.gitea/actions/conformance` has no caller.** No workflow, action or app contract uses it; `.gitea/actions/README.md:36` still lists it as "new"; the rust tooling inventory counts 0 callers (`docs/research/2026-10-rust-tooling-inventory.md:254`). | `grep -rn actions/conformance .gitea apps docs` |
| 4 | **Five reusable workflows have no caller in the repository**: `app-e2e.yml`, `app-never-panics.yml`, `app-fuzz.yml`, `app-licences.yml`, `app-mutants.yml` (each declares `workflow_call`; only a test fixture in `scripts/affected.test.mjs:384` names one). They date from the separate repositories (`.gitea/actions/README.md:41`; `app-licences.yml:6` still shows the cross-repository `uses:`). Their own `uses:` are dead too: the `licences` and `cache-*` actions are used only from `app-licences.yml` (review), so a naive caller count calls them used. `conformance/test/ci-actions.test.mjs:88` pins this list of six `app-*.yml` files. | found while writing revision 1; the review |
| 5 | **Dyno's helper-wiring checks are ignored tests that CI never runs**: `every_advertised_reading_has_a_production_sender` and `the_user_guide_and_the_changelog_claim_what_readings_advertises` (`apps/dyno/crates/dyno-agent/tests/helper_wiring.rs:429`, `:490`), ignored "until WP-INT". No CI line passes `--ignored`, `--run-ignored` or `--include-ignored` anywhere (`.gitea`, every `paddock.ci.json`); only two hand-run scripts do (`apps/dyno/scripts/linux-units-check.sh:22`, `apps/debrief/scripts/perf/perf-post.sh:19`). The family has **82 ignore attributes in 45 files** (Debrief 28, `paddock-setup` 27, Dyno 25, `paddock-secrets` 1, Spotter 1; one of them `#[cfg_attr(debug_assertions, ignore = …)]`), with no record of which are measurements (rule 48), VM-only rows, or checks waiting to be switched on. Revision 1 said 86 in 44: its grep also counted doc comments. | log 1296; `grep -rE '^\s*#\[(ignore\|cfg_attr\([^]]*ignore)'` |
| 6 | **Rule-bearing docs name a command and a check that do not exist**: "register check `paddock-conformance.mjs copies --check-register` (ADR 0034, C58)" in 8 `AGENTS.md` files, and C58 in `docs/rules.md:277` and `:280`. `node conformance/bin/paddock-conformance.mjs copies --check-register` exits 2 ("unexpected argument: copies"); `paddock-conformance list` has no C58. ADR 0034 §3 planned both as its WP-1. Other check ids in docs that are not checks today: C21 (`tools/README.md:12`, `crates/README.md:1626`) and C55 (review). | the commands above; `grep -rln C58 --include=AGENTS.md` |

**Pattern.** A check is written, reviewed and merged, but the step that makes CI (or a release harness) run it is a
separate edit in a separate file, usually YAML, and nothing notices when that edit is missing, later removed, or narrowed
to a list that leaves the check out. The same happens to docs: a rule or AGENTS.md line names a check or a command that was
planned and never built, and readers trust it. The check then reports green by its absence, drift builds up (instance 1),
and its own bugs go unseen (instance 2).

**Invariant.** Every check the repository defines is, at every commit on `main`, either run, as a gate that can fail, by
a CI job reachable from a trigger (or by a named release harness), or registered with a reason, an owner and a review
date. Every check id and every tool command that a rule-bearing doc names exists.

## Decision

### 1. What counts as a check

The meta-check enumerates these kinds. Each kind has one enumerator, registered in one table in the code (open/closed: a
new kind is a new enumerator added there, with its fixtures); the register (§3) holds only data about instances.

| Kind | Enumerated from | Its id |
|---|---|---|
| `conformance` | `paddock-conformance list --json` (a new `--json` flag; each check module also declares `scope` — `root`, `apps` or `both` — and `mode` — `gate` or `report-only`, C51 being `report-only` today), and later the Rust `./dev check` registry of ADR 0022's amendment | `C13` |
| `dev-check` | the checks `./dev` implements, declared in one table in `paddock-dev` (`CHECKS: &[CheckInfo]`: id, subcommand, scope, mode) and printed by `./dev gates list --json`; `./dev` has no `help --json`, and clap's command tree alone cannot tell a check from a utility | `C100` |
| `ignored-test` | every `#[ignore]` and `#[cfg_attr(<cond>, ignore)]` on a Rust test under `crates/`, `apps/`, `tools/`, with its function name (a line scanner, not a Rust parser; fixtures for attributes over several lines and in `mod tests`). `cfg_attr(debug_assertions, ignore)` is recorded with its condition: it counts as run only where a job runs that workspace's tests in a release profile | `apps/dyno/crates/dyno-agent/tests/helper_wiring.rs::every_advertised_reading_has_a_production_sender` |
| `env-gated-test` | the same class without the attribute: a Rust test whose body starts by returning when an environment variable is missing (`if std::env::var("PADDOCK_VM").is_err() { return; }` and the `.ok()?` shapes), and a TypeScript `test.skip(…)`/`describe.skip(…)` whose condition reads `process.env`, directly or through a module-level constant initialised from it (`const OUT = process.env['POWER_SHOTS_DIR'] ?? ''` and later `test.skip(!OUT, …)`, `apps/dyno/app/e2e/screenshots-power.spec.ts:6`, `:43`; the scanner resolves `const` bindings in the same file, one level). Conditions on the browser or OS (`browserName === 'webkit'`) are not this kind | `path::name` |
| `script` | files named as checks: `**/check-*.{mjs,sh,ps1}` and `**/*.test.mjs` under `scripts/`, `conformance/`, `apps/*/scripts/`, `tools/` (globs in the register) | the path |
| `npm-check` | scripts named `check*`, `test*`, `lint*` in every workspace `package.json` | `ui:test:tokens` |
| `action` | every `.gitea/actions/*/action.yml` | `action:conformance` |
| `workflow` | every `.gitea/workflows/*.yml` with `workflow_call` | `workflow:app-e2e.yml` |

The scan found no unconditional TypeScript skip and no `node:test` `skip: true` today (2026-11-08); the
`env-gated-test` enumerator covers the conditional, environment-dependent ones, which are the ones that pass vacuously.

### 2. What CI runs: a reachability model

The meta-check builds one model of CI and answers every "is it gated?" question from it. It is a pure function from file
contents to the model; the edge only reads files.

**2.1 Roots and edges.** The roots are the workflows with a trigger other than `workflow_call` (`push`, `pull_request`,
`schedule`, `workflow_dispatch`, a tag push). The edges are a job's `uses:` of a reusable workflow and a step's `uses:` of
a local action. Only nodes reachable from a root count: a caller that is itself unreachable is dead, so `app-licences.yml`
does not make the `licences` and `cache-*` actions used (instance 4).

**2.2 Binding.** Each edge binds the callee's inputs from the caller's `with:` (and `secrets:`), and each step its `env:`
over the job's and workflow's `env:`. The callee's run lines are expanded with those values: `${{ inputs.x }}`,
`${{ env.X }}` and `$X`/`${X}` where `X` is bound, and `${{ matrix.x }}` over every matrix combination. Real cases the
fixtures copy:
- `dyno-release.yml` passes `bundle-check: apps/dyno/scripts/check-bundle.sh` (lines 109, 161) and
  `windows-installer-check: node conformance/release/check-per-machine-installer.mjs …` (lines 113, 165) as inputs to
  `app-release.yml`, which runs them; `nightly.yml:221` and `:224` do the same;
- `app-gates` receives `COMMANDS` and runs them through `conformance/ci/run-commands.sh`; the commands are a field of the
  app's `paddock.ci.json`. That binding is declared once in the model (action `app-gates`, input → contract field), and
  a test fails if `app-gates`' inputs or `run-commands.sh`'s contract change without the declaration;
- `run-commands.sh` semantics are modelled: one command per line, a `cd` holds for the lines after it;
- the app matrix: ci.yml's app legs are a matrix built from the plan; the model binds it as one leg per app contract
  and per entry of its `os` list (so an app's `checks` run on each OS it declares), with the per-OS fields
  (`windowsClippy`) applied, and a test fails if the matrix-building step changes without the binding;
- plan outputs that are expressions over `github.*` (for example `full` at `debrief.yml:75`: schedule, a
  `debrief-v` tag, a push to main or a dispatch) are evaluated by the condition evaluator of 2.4 into levels, not
  treated as unresolved; outputs computed by a script stay unresolved unless they are the planner's project marks.

A value the model cannot bind (an output of another job, a secret, a computed string) makes the line `unresolved`. An
unresolved line never gates anything; if a check would be gated only through it, C100 reports "check broken: the CI model
cannot resolve <file:line>" (ER-33), never a pass.

**2.3 Working folder.** Each run line carries its folder: `defaults.run.working-directory` (workflow, then job), the
step's `working-directory`, and every `cd` earlier in the same block (or in the same command list), so relative paths,
`node --test <dir>` and `cargo` workspaces resolve to repository paths.

**2.4 Conditions.** A small evaluator reads `if:` on jobs and steps. It knows `github.event_name`, `github.ref`
(`==`, `startsWith`), `inputs.*`, `vars.*` (the register's `[vars]` table records each repository variable with the value CI sets, and a
`vars.*` not listed there makes the line `conditional`),
`success()`, `failure()`, `always()`, `!`, `&&`, `||`, parentheses, and the plan's outputs
(`needs.plan.outputs.<project> == 'true'`), which it reads as "runs when the planner marks `<project>`". With the root's
triggers it gives each run line a set of **levels**: `branch` (pull requests and branch runs), `main`, `nightly`,
`release` (a tag or a release dispatch). Anything else is `conditional`: a check run only under a `conditional` line
counts as gated only with a `[[conditional]]` register entry that says why it runs on branches.

"Runs when planned" adds an obligation: the check's inputs (its enumerator globs, §4) must be planner inputs of that
project. The planner reads those globs from `./dev gates inputs` (`plan.rs` calls the module directly; `affected.mjs`
gets a golden test against the command's output while it lives), so the inputs have one source.

**2.5 Report-only is not a gate.** A run line is `report-only`, never `gated`, when its step has
`continue-on-error: true`, when the command is followed by `|| true`, `|| :`, `; true` or runs under `set +e`, when a
conformance check's findings are warnings and its id is not under `--strict`, when the check declares `mode =
"report-only"` (C51), and when C100 itself runs with `--warn`. The register can accept a report-only check with a
`[[report_only]]` entry (reason, owner, dates); otherwise it is a finding.

**2.6 Gated, per kind.**

| Kind | Gated when a reachable, resolved, non-report-only line… |
|---|---|
| `conformance` | invokes `paddock-conformance` (or the Rust successor) with the id in `--only`, or with no `--only`. Scope `root` needs `--app paddock --root .`; scope `apps` needs a run for every app the check applies to (the check's own `applies(app)` answer, so Pacenotes without Rust is a declared non-applicable app, not a gap). |
| `dev-check` | invokes that `./dev` subcommand. |
| `ignored-test` | runs ignored tests (`--run-ignored`, `--ignored`, `--include-ignored`) in the test's workspace, with a filter that selects the test (its target via `--test`, or a name filter that matches the function); a line that merely names the test does not count. `cfg_attr(debug_assertions, ignore)`: a release-profile test run of the workspace counts. |
| `env-gated-test` | runs the test with the variable set (in the step's or job's `env:` or on the line). |
| `script` | names its path, a glob that matches it, or `node --test <dir>` that contains it. |
| `npm-check` | runs that script (`npm run <s> -w <ws>`, or `npm test`). |
| `action`, `workflow` | is reachable through it. |

Outside CI a check can also be gated by a **harness** (a release harness listed in the register, for example the VM rows,
which by the user's decision never run in CI: memory `local-test-vms`) or a **tool** (a developer tool or hook:
`lefthook.yml`, a `tauri.conf.json` `beforeBuildCommand`, a `./dev` subcommand). The same rule applies: the harness or
tool file must run it (a cargo or nextest line that selects the test, a command line that invokes the script), not just
mention it. Harness and tool gates are levels `harness:<name>` and `tool:<name>`; neither counts as `branch`.

The required level defaults to `branch` for `conformance`, `dev-check`, `script` and `npm-check` (a check that runs only
nightly lets a branch land red), and to any level for the rest. The register can lower it per check, with a reason.

**2.7 Fixtures.** Every construct of 2.1 to 2.6 has one good and one bad fixture in the pure core's tests, built from the
real examples: the Dyno release inputs, `nightly.yml:224`, `app-gates` with `COMMANDS`, `app-licences.yml` as a dead
caller, a `needs.plan.outputs` job, `defaults.run.working-directory` with a later `cd`, `continue-on-error`, `|| true`,
a non-strict warning, C51, `cfg_attr(debug_assertions, ignore)`, an env-gated Rust test and an unresolvable output.

### 3. The register: `docs/gates.toml`

Beside `docs/known-copies.toml` (ADR 0034) and `docs/harnesses.toml` (ADR 0042):

```toml
version = 1

# Which docs are rule-bearing (§5). ADRs, research, plans, lessons and changelogs are history: they may name what was planned.
[docs]
include = ["AGENTS.md", "apps/*/AGENTS.md", "docs/rules.md", "docs/design-guide.md", "**/README.md", "docs/wiki/**/*.md"]
exclude = ["docs/adr/**", "apps/*/docs/adr/**", "docs/research/**", "docs/plans/**", "docs/planned/**", "docs/audits/**",
           "docs/lessons.md", "**/CHANGELOG.md", "**/vendor/**", "**/fixtures/**", "**/spikes/**", "ui/docs/review/**",
           "**/node_modules/**"]

[vars]                        # repository variables as CI sets them (2.4); an unlisted vars.* is conditional
# NAME = "value"

[[harness]]
name = "vm-smoke"
paths = ["scripts/vm-smoke/**"]

[[not_gated]]                 # a check that is not gated, on purpose, for now
id = "C13"
scope = "apps"
apps = ["dyno", "pitcrew", "pitwall", "spotter", "shell-demo", "workbench-demo"]
reason = "missing disallowed-methods and their call sites; the probe misses src-tauri workspaces (ADR 0060 WP-4)"
owner = "ADR 0060 WP-4"
since = 2026-11-08
review_by = 2026-12-06
approved = "coordinator"

[[ratchet]]                   # an adoption list that may only shrink (§3.2)
id = "C13"
app = "debrief"
not_yet = ["xtask", "examples/plugins/importer-example"]   # the Debrief members not yet opted in (WP-4 fills all 46)
owner = "Debrief 1.2, DBR-LINT"
target = "debrief-v1.3.0"
approved = "user"

[[planned]]                   # a check id named in a rule-bearing doc before it exists
id = "C58"
adr = "0034"
owner = "ADR 0034 WP-1"
since = 2026-11-08
review_by = 2026-12-06
approved = "coordinator"

[[ignored]]                   # every ignored or env-gated test, by file
path = "apps/dyno/crates/dyno-agent/tests/helper_wiring.rs"
tests = ["every_advertised_reading_has_a_production_sender", "the_user_guide_and_the_changelog_claim_what_readings_advertises"]
gate = "not-gated"            # or "job:<workflow>#<job>", "harness:<name>", "tool:<name>", "measurement"
reason = "red until WP-INT wires every reading (Dyno 1.1.1 list); WP-INT adds the CI line"
owner = "Dyno WP-INT"
since = 2026-11-08
review_by = 2026-12-06
approved = "coordinator"
```

**3.1 Rules on the register.**
- `not_gated`, `planned`, `conditional` and `report_only` entries need `reason`, `owner`, `since`, `review_by` and
  `approved` (`coordinator` or `user`). A `review_by` in the past is an error; it is at most 60 days after `since`.
- **Date history.** `review_by` moves only in a commit whose message carries `Gates-date: <id> <old> -> <new>; approved:
  <coordinator|user>` (C100 reads the register's `git log` and matches each change of a date to a trailer, the same
  discipline as ADR 0034's `Copies-date:`); a second move of the same entry needs `approved: user`.
- `gate = "measurement"` (rule 48) needs the test or its file to appear in a results block of `docs/benchmarks.md` or the
  app's `docs/performance.md` (machine-checked: the path or the test name in the block). A "measurement" nobody records
  is a check in disguise and is a finding.
- `tool:<name>` and `harness:<name>` gates name a tool or harness file that runs the test (2.6).
- An entry whose check no longer exists, or that is now gated, is an error ("remove the entry"): the register cannot go
  stale. `tests = ["*"]` is allowed only for a file whose every ignored test shares one gate.

**3.2 Ratchets: adoption that takes longer than 60 days.** Debrief's lint adoption (instance 1) is a release's worth of
work, not a 60-day exception. C13 checks lint *configuration* (the workspace tables, each member's opt-in, the
`disallowed-methods` list), not counts of findings, so the ratchet is over that: an **opt-in list**. A `[[ratchet]]`
entry names the members not yet opted in (`not_yet`); it has no `review_by`, because it cannot go stale silently:
- C13 runs in Debrief's CI as a gate, with the workspace tables required now and the members in `not_yet` exempt from
  the opt-in requirement; a member missing from both the opt-ins and `not_yet` fails;
- the list may only shrink: C100 compares it with the register at `origin/main` and fails on any added member (a new
  crate opts in from its first commit);
- a member leaves `not_yet` only in a change that opts it in and leaves it clippy-clean under the preset, so
  `debrief.yml`'s `cargo clippy … -D warnings` stays valid and green at every commit;
- **the target is enforced**: the entry names a release (`debrief-v1.3.0`), and C100 in that app's release workflow
  refuses a tag of that release, or any later one, while `not_yet` is non-empty. The target moves only with a
  `Gates-date:` trailer approved by the user.

The honest property: opted-in crates gain no new `unwrap`, `expect` or `unsafe` that the preset denies; crates still in
`not_yet` are unchecked by the preset and shrink by crate. It needs `approved = "user"`. For C13 this needs the `not_yet`
exemption in the check (WP-4); the Debrief burn-down itself is Debrief work (§7).

### 4. The meta-check C100: `./dev gates check`

A Rust subcommand of `./dev` (ADR 0022; memory `rust-tooling-all`: new tooling is Rust), with the enumerators, the CI
model and the register rules as a pure core over file contents (tested with in-memory trees), and a thin edge that reads
the checkout and `git log` for the date history. Commands: `./dev gates check`, `./dev gates list --json` (every check
with its gating), `./dev gates inputs` (the enumerator globs, for the planner). Output:
- one line per finding, `::error file=…,line=…::C100 <id> <what is missing> Fix: <next step>` (ER-27), for example
  "C100 C13 (scope apps) is not gated for dyno, pitcrew, … (no reachable conformance line names it) Fix: add it to the
  apps' conformance step, or register it under [[not_gated]] in docs/gates.toml with a reason, owner and review_by";
- a summary table (kind, total, gated per level, conditional, report-only, not gated, measurement, ratchet, planned) to
  the job summary;
- `--json` for tools; exit codes from `paddock-cli` (0 clean, 1 findings, 2 usage).

It runs in CI's `conformance` job, as its own line, planned on its own inputs (`./dev gates inputs`: `.gitea/**`, every
`paddock.ci.json` and `package.json`, `lefthook.yml`, `docs/gates.toml`, the rule-bearing docs, the check sources,
and Rust and TypeScript test files whose diff adds or removes an ignore attribute or an env guard, a content
rule with a golden case in `affected.mjs` and `plan.rs`); in nightly over the whole tree; and in ADR 0038's
`./dev check --pre-push` and per-commit gate. Its run time is measured in WP-1 and recorded in `docs/benchmarks.md`
before anyone quotes it.

C100 is in its own `CHECKS` table, so it must be gated at `branch` like any other check. Its own line cannot be the only
guard of itself (a change that deletes it also deletes the check), so nightly has its own C100 job, separate from the
conformance job, and the `plan` job, which runs on every event, checks with one fixed `grep` that the conformance job's
C100 line and the nightly job exist; removing either fails the next run of `plan`.

### 5. Docs name real checks and real commands

For each rule-bearing doc (`[docs]` in the register), the meta-check finds:
- every token `C<2 or 3 digits>`: it must be a known check id, a `[[planned]]` entry, or a `[[retired]]` entry (an id
  that existed, with the commit that removed it and where its job went). Today this fails on C58 (8 `AGENTS.md`,
  `docs/rules.md:277`, `:280`), C21 (`tools/README.md:12`, `crates/README.md:1626`) and C55; WP-3 corrects the text or registers each;
- every command in inline code or a code block that starts with `paddock-conformance`, `node conformance/bin/
  paddock-conformance.mjs`, `./dev`, `node <path>`, `bash <path>` or `npm run <script> -w <ws>`: the subcommand must exist
  (`paddock-conformance commands --json`, a small addition to the existing CLI, and clap's command tree for `./dev`), the
  path must exist, the npm script must exist in that workspace.

**What this leaves to "doc facts from code".** This ADR checks only names of checks and of developer-tool commands and
paths, in the rule-bearing docs. The doc-facts redesign keeps the rest: user-facing reference facts in the apps' docs
(app CLI commands, flags and limits, settings and labels, install paths, service names, Run values, MCP tools), generated
from the code or checked against it, and the flags of developer commands. To avoid a second scanner (rule 55), §5's doc
scanner is a module of its own in `paddock-dev` (doc file selection, inline-code and code-block extraction, a `FactKind`
trait with `CheckId` and `ToolCommand` as the first two implementations); doc facts adds its kinds to it.

### 6. Fixing today's instances through the mechanism

- **C13 in Dyno, Pitcrew, Pitwall, Spotter:** the `disallowed-methods` lines from `lint/clippy.toml`, and every call site
  they flag moved to the sanctioned helpers (44 `temp_dir` call lines and 2 `tokio::process::Command::new`); then C13 runs in each app leg (a conformance line in each `paddock.ci.json` `checks`, through `app-gates` and
  the conformance action), and the entry's `apps` list shrinks app by app.
- **C13's probe:** it reads each app's `cargo` field from `paddock.ci.json` and answers "does not apply" for an app
  without Rust (Pacenotes, Sighter), and it gets the `not_yet` exemption of §3.2.
- **C13 in Debrief:** the workspace `[workspace.lints]` tables (members not yet opted in are not affected), the
  `not_yet` list with all 46 members, and a conformance step in `debrief.yml` (Debrief has its own workflow, not an
  `app-gates` leg); the adoption is the Debrief work package of §7, not this ADR's.
- **`.gitea/actions/conformance`:** the app legs' new conformance line uses it (it already writes the job summary).
- **The five reusable workflows:** each is either called by an app workflow or deleted, with `ci-actions.test.mjs:88`'s
  pinned list updated in the same change. The `licences` and `cache-*` actions follow: called from a live workflow or
  deleted. The monorepo has no other repository that calls them (ADR 0014).
- **Dyno's helper-wiring tests:** registered not gated, owner Dyno WP-INT; WP-INT un-ignores them, which removes the entry.
- **Every other ignored or env-gated test:** registered by file in the baseline, each with its gate: `harness:vm-smoke`
  only where the harness runs them (the Dyno tests read `PADDOCK_VM=1`, the `paddock-setup` tests `PADDOCK_SETUP_VM`;
  WP-3 checks which harness line runs each, and registers the rest `not-gated` with an owner), `measurement` where a
  results block records them, `not-gated` with an owner for the systemd units of `bench_units.rs`, which a CI container
  cannot run.
- **Docs:** C58 registered as planned and the text marked "(planned, ADR 0034 WP-1)", or built by ADR 0034 WP-1, whichever
  lands first; C21 and C55 corrected or registered as retired.

### 7. Debrief's lint adoption is its own work package (Debrief 1.2)

Adopting the family lint preset in Debrief (46 members to opt in; about 7,165 `unwrap` and 244 `unsafe` in non-test code
by the review's count) is product work: each `unwrap` on I/O or user data is a crash path to replace with a typed error,
and each `unsafe` needs a reason. It goes into the Debrief 1.2 plan as **DBR-LINT**: this ADR delivers the workspace
tables, the `not_yet` list, the CI step and the register entry (WP-4); DBR-LINT opts members in one at a time, each
clippy-clean under the preset when it leaves the list, and is sized by Debrief's planner from the findings per member
(not estimated here). From the first day, opted-in members gain no new denied `unwrap`, `expect` or `unsafe`; the members
still on the list are not checked by the preset until they opt in, and the list only shrinks. A `debrief-v1.3.0` tag is
refused while the list is non-empty (§3.2).

## Checks that make the class detected

| Check | What it enumerates | Fails today on |
|---|---|---|
| C100 gating | every check of §1 against the reachable, resolved, non-report-only lines of the CI model, the harnesses and tools | C13 in every app; the conformance action; five reusable workflows and the `licences` and `cache-*` actions behind them; Dyno's helper-wiring tests and the other 80 ignore attributes without an entry; env-gated tests; every `script` and `npm-check` found not run (the baseline run in WP-3 lists them) |
| C100 register | every entry: fields, dates and their `Gates-date:` trailers, measurement records, ratchet lists (only shrinking; the target release refused while non-empty), still needed | nothing yet (the register is new) |
| `plan` job guard | that the conformance job's C100 line and the nightly C100 job exist | a change that removes either |
| C100 docs | check ids and tool commands in rule-bearing docs | C58 in 8 `AGENTS.md` and `docs/rules.md`; `copies --check-register`; C21; C55 |
| C13 probe (ER-33) | each app's declared Cargo workspace | four apps where the probe sees no workspace |

## Work packages and estimates (agent-days)

All new tooling goes into `tools/paddock-dev` (`./dev`), Rust. The two small JavaScript additions (`list --json` with
`scope` and `mode`, `commands --json`) are to the existing conformance CLI, which ADR 0022's amendment ports later; they
are not new scripts.

| WP | Content | Days |
|---|---|---|
| WP-1 | The CI model of §2: YAML reading (the reader is chosen here, `yaml-rust2` (MIT/Apache-2.0) or the job splitter `plan.rs` already has, recorded in the module's doc comment), roots and the reachability graph, input and env binding with the declared `app-gates` contract binding and `run-commands.sh` semantics, working folders, the app-matrix binding, the condition evaluator (with `github.*` plan outputs and the `[vars]` table), report-only detection, unresolved lines; the good and bad fixture per construct from the real examples. Measures its run time (rule 48). | 7 |
| WP-2 | The enumerators of §1 (including `env-gated-test` and the `cfg_attr` case), the `CHECKS` table and `./dev gates list`/`inputs`, the register parser and the rules of §3.1 and §3.2 (date history from `git log`, measurement records, ratchets), report and exit codes; the doc scanner module of §5 (`FactKind`); `paddock-conformance list --json` with `scope` and `mode`, and `commands --json`. | 3.5 |
| WP-3 | Baseline: run on `main`, write `docs/gates.toml` with an entry for every finding (owner and date each, or the trivial text fix), wire C100 into CI's `conformance` job, a nightly job of its own and the `plan` job's guard, the planner reading `./dev gates inputs` (`plan.rs` directly, `affected.mjs` by golden test). From this commit on, C100 is an error. | 1.5 |
| WP-4 | §6: C13's probe (`cargo` field, "does not apply", the `not_yet` exemption); Dyno, Pitcrew, Pitwall, Spotter: `disallowed-methods` and their call sites (44 `temp_dir` lines and 2 `tokio::process::Command::new`) and C13 in each leg through the conformance action; Debrief: the workspace lint tables, the `not_yet` list and entry, the conformance step in `debrief.yml`; the five reusable workflows and the actions behind them called or deleted, with `ci-actions.test.mjs`; the docs fixes. Each removes or shrinks its register entry. | 3.5 |
| WP-5 | Rules and docs: a new family rule ("every check runs, as a gate that can fail, somewhere reachable, or is registered with a reason, owner and review date; docs name only real checks"), numbered by the integrator at landing; the lessons entry with the six instances; `.gitea/actions/README.md` and the `integrate` skill name `./dev gates check`. | 0.5 |
| **Total (this ADR)** | | **16** |
| DBR-LINT (Debrief 1.2, separate) | Debrief's members opted in one by one, each clippy-clean under the preset: sized by the Debrief plan from the findings per member | (not in the total) |

Revision 1 estimated 10.5 days; the review put WP-1 at about 6 to 7 days and the total at about 13 to 15 plus the Debrief
item. Revision 2 came to 15.5 (it also adds the `env-gated-test` kind, the date history and the measurement and ratchet
rules); revision 3 adds 0.5 to WP-1 for the app-matrix binding, `github.*` plan outputs and the `[vars]` table, so 16.

Order: WP-1 and WP-2 can run in parallel (separate modules of `paddock-dev`, owned separately); WP-3 after both; WP-4
after WP-3 (each fix then shows as a removed entry); WP-5 with WP-3. WP-4's app edits touch each app's `paddock.ci.json`,
`clippy.toml` and sources, so each app's part is scheduled outside that app's release freeze (ER-1). Inside ADR 0022
G.7's cap of two tooling agents during a release.

### Follow-up work packages (added at the review of `feat/gates-c100`, 2026-11-08)

The baseline register (WP-3) lists what C100's first run found. Its entries need owners that exist, so the work behind them is
named here. Estimates are agent-days, not yet planned by an app's planner.

| WP | Content | Days |
|---|---|---|
| WP-6 | **App conformance lines.** A conformance line in each app's `paddock.ci.json` `checks` for C03, C04, C07, C08, C23, C28, C32, C33, C34 and C91 (the apps each applies to), fixing what the first run finds; C13 for Debrief and the two demos once the lint preset is adopted there (DBR-LINT; the demos need their lint tables, opt-ins and a `clippy.toml`). C14 and C29 become gates when an app's windowWeight budgets and stats are wired. | 3 |
| WP-7 | **Orphaned scripts, npm scripts and actions.** Run each in CI or delete it: the `check-*` scripts of Dyno, the Debrief and demo `*.test.mjs`, the five `apps/*/app:test` npm scripts, `strip-signing-keys`, and the `conformance` action (called from the app legs or deleted). | 1.5 |
| WP-8 | **What the deleted workflows used to give.** A nightly job over `crates/` and the apps running the never-panics properties with `PROPTEST_CASES=10000` in release mode (C36 requires the tests to exist; nothing ran them hard once `app-never-panics.yml` went), a record that no CI mutation gate exists outside `debrief-core` (a monorepo version of `app-mutants.yml` is a later decision), and one `npm audit --omit=dev` over the root lock in `ci.yml`. | 1.5 |
| WP-9 | **Ignored and env-gated tests.** Give every file of the baseline a real gate: measurements into results blocks (rule 48), VM rows into the `vm-smoke` harness (the 27 `paddock-setup` Windows rows test hostile files, junctions and signatures and are run by nothing: a release-readiness question, ER-33), the documentation screenshot captures into one on-demand job. | 3 |

### Accepted gaps (recorded at the review, 2026-11-08)

- **Unverified plan outputs.** "Runs when planned" (§2.4) is verified for the outputs of `ci.yml`'s plan job (`crates`, `ui`, `eslint`,
  `conformance`, `tools`, `icons`, `paddock_dev`, `any_app`) with the planner as oracle (`scripts/affected-marks.mjs`). The `run`
  output of an app workflow's plan job and of the nightly gate is taken as satisfied: it says "this app is affected" or "main
  changed since the last green night", and its planner call (`affected.mjs --ci --app <app>`) has no per-file oracle yet.
- **C30 and C15** mix `error()` and `warn()` outcomes under one `mode: gate`; only C29 and the all-warning checks are report-only.
- **`Scope::Both`** is satisfied by the root run alone, on the assumption that it reads each app's files; **`$GITHUB_ENV`** writes are
  invisible to the model.

**Rollback.** C100 can become a warning by one flag on its CI line (`./dev gates check --warn`, which still prints every
finding, and which the model itself counts as report-only). The register stays as documentation.

## Parallelism and cost (rule 51)

The work is a text scan of a few thousand files and one `git log` of one file: one thread, no build. Cheaper first: the
enumerators read only the globbed paths, Rust and TypeScript files are scanned for the byte strings `ignore`, `env::var`
and `process.env` before any parsing, and each YAML file is parsed once into the model. The run time is measured in WP-1
and recorded before it is quoted; if it ever exceeds a few seconds, files are scanned in parallel with a bounded pool
(CPU-bound: cores minus one).

## Consequences

- Adding a check means adding its CI line in the same change, or registering it with a reason; reviewers no longer need to
  remember to ask.
- Ignoring a test, or guarding it on an environment variable, means registering it: 82 attributes in 45 files, one entry
  per file or per gate within a file.
- The register shows, in one file, what is deliberately not gated, conditional, report-only, ratcheted or planned, and
  until when.
- Docs that name a planned check say so, or fail.
- The CI model reads YAML that ADR 0038 is moving into contracts. Until then it reads both; after ADR 0038 WP-4 the YAML
  part shrinks to set-up steps, conditions and `uses:`, and the contract part grows.
- The model is an interpretation of the runner's semantics, not the runner. Where it cannot tell, it says "unresolved" or
  "conditional" and asks for a register entry; it never guesses "gated".

## Alternatives considered

- **Run every conformance check everywhere (no `--only`).** It closes the conformance part of instance 1, but not the
  ignored tests, scripts, actions or docs, and it turns every app-scoped check into an error on the root run. Rejected.
- **A convention in the check file ("CI: ci.yml#conformance") instead of reading CI.** It is a second place that can be
  wrong; the CI files are the truth. Rejected.
- **A plain text search of CI files for each id.** Revision 1's model; the review showed it counts dead callers,
  report-only lines and lines it cannot resolve as gates, and misses checks passed in as inputs. Replaced by §2.
- **A review checklist item.** That is today's state; it missed all six instances. Rejected.
- **Fold it into ADR 0038's parity golden.** That golden proves the local gate equals CI; it says nothing about checks CI
  does not run at all. Kept separate, sharing the CI model once ADR 0038 WP-1 exists.
