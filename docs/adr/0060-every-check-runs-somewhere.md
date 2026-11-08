# 60. Every check runs somewhere: a gate register and a meta-check over checks, CI and the docs that name them

- Status: Proposed (2026-11-08). The user decided on 2026-11-08 to do all ER-21 redesigns now; this ADR waits for an
  independent review and the user's acceptance.
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
`/Volumes/d7sst7/dev-targets/handoff/2026-10-29-overnight.md` (cited as "log" with its line), 2026-11-08.

| # | Instance | Verified by |
|---|---|---|
| 1 | **C13 runs in no CI job.** The only conformance run in CI is ci.yml:257, `--app paddock --root . --only C06,C14,C15,C16,C24,C29,C30,C31,C34,C35,C36,C37,C39,C43,C48,C53,C54,C56,C57`; C13 is not in it, and no app leg or workflow runs conformance at all. Run by hand per app (`--app <app> --root apps/<app> --only C13`): Debrief 52 errors (xtask and four example plugins not opted in to the workspace lints, missing `[workspace.lints]` entries, missing `disallowed-methods`), Dyno 3, Pitcrew 3, Spotter 3, Pitwall 2 (all missing `disallowed-methods` entries such as `std::env::temp_dir`, `reqwest::Client::new`, `tokio::process::Command::new`). | log 1292, 1295, 1296; the runs above |
| 2 | **C13's probe is blind on four apps** (ER-33): it reports "there is no workspace Cargo.toml at the repository root" for Pacenotes and Sighter, which have no Rust yet (the check does not apply), and for shell-demo and workbench-demo, whose workspace is `src-tauri/` as their `paddock.ci.json` says (`"cargo": "src-tauri"`). A check nobody runs also never gets its probe fixed. | the runs above |
| 3 | **`.gitea/actions/conformance` has no caller.** No workflow, action or app contract uses it; `.gitea/actions/README.md:36` still lists it as "new"; the rust tooling inventory counts 0 callers (`docs/research/2026-10-rust-tooling-inventory.md:254`). | `grep -rn actions/conformance .gitea apps docs` |
| 4 | **Five reusable workflows have no caller in the repository**: `app-e2e.yml`, `app-never-panics.yml`, `app-fuzz.yml`, `app-licences.yml`, `app-mutants.yml` (each declares `workflow_call`; only a test fixture in `scripts/affected.test.mjs:384` names one). They date from the separate repositories (`.gitea/actions/README.md:41`). | found while writing this ADR |
| 5 | **Dyno's helper-wiring checks are ignored tests that CI never runs**: `every_advertised_reading_has_a_production_sender` and `the_user_guide_and_the_changelog_claim_what_readings_advertises` (`apps/dyno/crates/dyno-agent/tests/helper_wiring.rs:429`, `:490`), ignored "until WP-INT". No CI line passes `--ignored`, `--run-ignored` or `--include-ignored` anywhere (`.gitea`, every `paddock.ci.json`); only two hand-run scripts do (`apps/dyno/scripts/linux-units-check.sh:22`, `apps/debrief/scripts/perf/perf-post.sh:19`). The family has 86 `#[ignore]` attributes in 44 files (Debrief 28, `paddock-setup` 28, Dyno 27, `paddock-secrets` 1, Spotter 1), with no record of which are measurements (rule 48), VM-only rows, or checks waiting to be switched on. | log 1296; `grep -rc '#\[ignore'` |
| 6 | **AGENTS.md names a command and a check that do not exist**: "register check `paddock-conformance.mjs copies --check-register` (ADR 0034, C58)". `node conformance/bin/paddock-conformance.mjs copies --check-register` exits 2 ("unexpected argument: copies"); `paddock-conformance list` has no C58. ADR 0034 §3 planned both as its WP-1. | the commands above |

**Pattern.** A check is written, reviewed and merged, but the step that makes CI (or a release harness) run it is a
separate edit in a separate file, usually YAML, and nothing notices when that edit is missing, later removed, or narrowed
to a list that leaves the check out. The same happens to docs: a rule or AGENTS.md line names a check or a command that was
planned and never built, and readers trust it. The check then reports green by its absence, drift builds up (instance 1),
and its own bugs go unseen (instance 2).

**Invariant.** Every check the repository defines is, at every commit on `main`, either run by a named CI job (or a named
release harness), or registered as not gated with a reason, an owner and a review date. Every check id and every tool
command that a rule-bearing doc names exists.

## Decision

### 1. What counts as a check

The meta-check enumerates these kinds. Each kind has one enumerator, and the list of kinds is data in the register (open/
closed: a new kind is a new enumerator registered in one place).

| Kind | Enumerated from | Its id |
|---|---|---|
| `conformance` | `paddock-conformance list --json` (a new `--json` flag; each check module also gets a declared `scope`: `root`, `apps` or `both`), and later the Rust `./dev check` registry of ADR 0022's amendment | `C13` |
| `dev-check` | the checks `./dev` itself implements (`./dev help --json`, from clap), including C100 | `C100` |
| `ignored-test` | every `#[ignore]` and `#[cfg_attr(…, ignore)]` in a Rust file under `crates/`, `apps/`, `tools/`, with its function name (a line scanner, not a Rust parser; fixtures cover attributes over several lines and attributes in `mod tests`) | `apps/dyno/crates/dyno-agent/tests/helper_wiring.rs::every_advertised_reading_has_a_production_sender` |
| `script` | files named as checks: `**/check-*.{mjs,sh,ps1}` and `**/*.test.mjs` under `scripts/`, `conformance/`, `apps/*/scripts/`, `tools/` (globs in the register) | the path |
| `npm-check` | scripts named `check*`, `test*`, `lint*` in every workspace `package.json` | `ui:test:tokens` |
| `action` | every `.gitea/actions/*/action.yml` | `action:conformance` |
| `workflow` | every `.gitea/workflows/*.yml` with `workflow_call` | `workflow:app-e2e.yml` |

TypeScript skips are not a kind yet: the scan found no unconditional `test.skip`/`it.skip`/`describe.skip`/`fixme` and no
`node:test` `skip: true` (2026-11-08); conditional skips (`test.skip(browserName === 'webkit', …)`) are legitimate. A first
unconditional one adds the kind (YAGNI until then).

### 2. What counts as gated

The meta-check reads what CI runs from the same sources CI does:
- every `run:` block and `uses:` line in `.gitea/workflows/*.yml` and `.gitea/actions/*/action.yml`, with each workflow's
  triggers;
- every command list in `apps/*/paddock.ci.json`, and, as ADR 0038 WP-1 and WP-4 move the root jobs into data, every
  root `paddock.ci.json` (the conformance `--only` and `--strict` lists become fields there; the meta-check reads both
  forms until the YAML form is gone);
- one level of indirection: `npm run <script> -w <ws>` resolves to that script's text, an action's `run:` to the action
  that `uses:` it, a reusable workflow to its callers, and `beforeBuildCommand` in a `tauri.conf.json` counts as a caller.

A check is gated at a **level**, taken from the triggers of the jobs that run it: `branch` (pull requests and branch
runs), `main`, `nightly`, `release` (a tag or a release dispatch), or `harness:<name>` (a release harness from the
register, for example the VM rows, which never run in CI by the user's decision: memory `local-test-vms`). For each
kind:

| Kind | Gated when |
|---|---|
| `conformance` | a run line invokes `paddock-conformance` (or the Rust successor) with the id in `--only`, or with no `--only`. Scope `root` needs a run with `--app paddock --root .`; scope `apps` needs a run for every app in `apps/` that the check applies to (the check's own `applies(app)` answer, so Pacenotes without Rust is not a gap but a declared non-applicable app). |
| `dev-check` | a run line invokes that `./dev` subcommand. |
| `ignored-test` | a run line in the named job runs ignored tests (`--run-ignored`, `--ignored`, `--include-ignored`) in the test's workspace and names the test or its target (`--test helper_wiring`, a nextest filter naming the function), or the register names a harness whose file names the test. |
| `script` | a run line names its path, a glob that matches it, or a `node --test <dir>` that contains it. |
| `npm-check` | a run line runs that script. |
| `action`, `workflow` | something uses it. |

The required level defaults to `branch` for `conformance`, `dev-check`, `script` and `npm-check` (a check that runs only
nightly lets a branch land red), and to "any level" for the rest. The register can lower it per check with a reason.

### 3. The register: `docs/gates.toml`

Beside `docs/known-copies.toml` (ADR 0034) and `docs/harnesses.toml` (ADR 0042):

```toml
version = 1

# Which docs are rule-bearing (§5). ADRs, research, plans, lessons and changelogs are history: they may name what was planned.
[docs]
include = ["AGENTS.md", "apps/*/AGENTS.md", "docs/rules.md", "docs/design-guide.md", "**/README.md", "docs/wiki/**/*.md"]
exclude = ["docs/adr/**", "apps/*/docs/adr/**", "docs/research/**", "docs/plans/**", "docs/lessons.md", "**/CHANGELOG.md"]

[[harness]]
name = "vm-smoke"
paths = ["scripts/vm-smoke/**"]

# A check that is not gated, on purpose, for now.
[[not_gated]]
id = "C13"
scope = "apps"
reason = "drift in Debrief, Dyno, Pitcrew, Pitwall, Spotter; the probe misses src-tauri workspaces (ADR 0060 WP-4)"
owner = "ADR 0060 WP-4"
since = 2026-11-08
review_by = 2026-11-29
approved = "coordinator"

# A check id named in a rule-bearing doc before it exists.
[[planned]]
id = "C58"
adr = "0034"
owner = "ADR 0034 WP-1"
since = 2026-11-08
review_by = 2026-11-29
approved = "coordinator"

# Ignored tests: every one is listed, by file, with the reason it is ignored.
[[ignored]]
path = "apps/dyno/crates/dyno-agent/tests/helper_wiring.rs"
tests = ["every_advertised_reading_has_a_production_sender", "the_user_guide_and_the_changelog_claim_what_readings_advertises"]
gate = "not-gated"            # or "job:<workflow>#<job>", "harness:<name>", "measurement"
reason = "red until WP-INT wires every reading (Dyno 1.1.1 list); WP-INT adds the CI line"
owner = "Dyno WP-INT"
since = 2026-11-08
review_by = 2026-12-06
approved = "coordinator"
```

Rules the meta-check enforces on the register itself:
- `not_gated` and `planned` entries need `reason`, `owner`, `since`, `review_by` and `approved` (`coordinator` or
  `user`). A `review_by` in the past is an error. `review_by` is at most 60 days after `since`; it moves only in a commit
  that also changes `approved`, the same discipline as ADR 0034's `Copies-date:`.
- `gate = "measurement"` (rule 48: a measurement run by hand, not a check) needs a `reason` and no date. The report counts
  them, so a review can ask whether one is a check in disguise (for example a test that asserts wall-clock budgets).
- An entry whose check no longer exists, or that is now gated, is an error ("remove the entry"): the register cannot go
  stale.
- `tests = ["*"]` is allowed only for a file whose every ignored test shares one gate.

### 4. The meta-check C100: `./dev gates check`

A Rust subcommand of `./dev` (ADR 0022; memory `rust-tooling-all`: new tooling is Rust), with the enumerators and CI reader
as a pure core over file contents (tested with in-memory trees) and a thin edge that reads the checkout. Output:
- one line per finding, `::error file=…,line=…::C100 <id> <what is missing> Fix: <next step>` (ER-27), for example
  "C100 C13 (scope apps) runs in no CI job for debrief, dyno, … Fix: add it to the apps' conformance step, or register it
  under [[not_gated]] in docs/gates.toml with a reason, owner and review_by";
- a summary table (kind, total, gated per level, not gated, measurement, planned) to the job summary;
- `--json` for tools; exit codes from `paddock-cli` (0 clean, 1 findings, 2 usage).

It runs:
- in CI's `conformance` job, as its own line (`./dev gates check`), and the planner marks `conformance` for every input it
  reads: `.gitea/**`, every `paddock.ci.json`, `package.json` files, `docs/gates.toml`, the rule-bearing docs, the check
  sources, and Rust files whose diff adds or removes an `#[ignore]` (a content rule in `affected.mjs` and `plan.rs`, with a
  golden case each);
- in nightly over the whole tree;
- in ADR 0038's `./dev check --pre-push` and in the per-commit gate (its cost is a text scan, measured in WP-1 and recorded
  in `docs/benchmarks.md` before anyone quotes it).

C100 lists itself: the register's own consistency is part of its run, and C100 must be gated at `branch`, so removing its
CI line fails the next run of whatever still runs (and nightly, which runs everything).

### 5. Docs name real checks and real commands

For each rule-bearing doc (`[docs]` in the register), the meta-check finds:
- every token `C<2 or 3 digits>` outside code that is plainly something else (a register `[docs] ignore_tokens` list for
  exceptions, empty today): it must be a known check id or a `[[planned]]` entry;
- every command in inline code or a code block that starts with `paddock-conformance`, `node conformance/bin/
  paddock-conformance.mjs`, `./dev`, `node <path>`, `bash <path>` or `npm run <script> -w <ws>`: the subcommand must exist
  (`paddock-conformance commands --json`, a small addition to the existing CLI, and clap's command tree for `./dev`), the
  path must exist, the npm script must exist in that workspace.

Instance 6 fails both: `copies` is not a subcommand and C58 is not a check. The fix is either to build them (ADR 0034
WP-1) or to register C58 as planned and mark the AGENTS.md text "(planned, ADR 0034 WP-1)".

**What this leaves to "doc facts from code".** This ADR checks only names of checks and of developer-tool commands and
paths, in the rule-bearing docs. The doc-facts redesign keeps the rest: user-facing reference facts in the apps' docs
(app CLI commands, flags and limits, settings and labels, install paths, service names, Run values, MCP tools), generated
from the code or checked against it, and the flags of developer commands. To avoid a second scanner (rule 55), §5's doc
scanner is a module of its own in `paddock-dev` (doc file selection, inline-code and code-block extraction, a `FactKind`
trait with `CheckId` and `ToolCommand` as the first two implementations); doc facts adds its kinds to it.

### 6. Fixing today's instances through the mechanism

- **C13:** its probe reads each app's `cargo` field from `paddock.ci.json` and answers "does not apply" for an app without
  Rust; the drift in five apps is fixed (the `disallowed-methods` lines from `lint/clippy.toml`, Debrief's member opt-ins
  and missing lint entries); then it runs in each app leg (a conformance line in each `paddock.ci.json` `checks`, through
  `app-gates`), and its `[[not_gated]]` entry is removed.
- **`.gitea/actions/conformance`:** the app legs' new conformance line uses it (it already produces the job summary), so
  it gets callers; otherwise it is deleted.
- **The five reusable workflows:** each is either called by an app workflow or deleted. Their history is in git; the
  monorepo has no other repository that calls them (ADR 0014).
- **Dyno's helper-wiring tests:** registered not gated, owner Dyno WP-INT; WP-INT un-ignores them, which removes the entry.
- **Every other `#[ignore]`:** registered by file in the baseline, each with its gate (`harness:vm-smoke` for the
  `PADDOCK_VM=1` tests, `measurement` for the rule-48 runs, `not-gated` with an owner for the systemd units of
  `bench_units.rs`, which a CI container cannot run).
- **AGENTS.md C58:** registered as planned, text marked planned, or built by ADR 0034 WP-1, whichever lands first.

## Checks that make the class detected

| Check | What it enumerates | Fails today on |
|---|---|---|
| C100 gating | every check of §1 against every CI run line, contract and harness | C13; the conformance action; five reusable workflows; Dyno's helper-wiring tests and 84 other ignored tests without a register entry; every `script` and `npm-check` found not run (the baseline run in WP-3 lists them) |
| C100 register | every `[[not_gated]]`, `[[planned]]`, `[[ignored]]` entry: fields, dates, still needed | nothing yet (the register is new) |
| C100 docs | check ids and tool commands in rule-bearing docs | AGENTS.md: `copies --check-register`, C58 |
| C13 probe (ER-33) | each app's declared Cargo workspace | four apps where the probe sees no workspace |

## Work packages and estimates (agent-days)

All new tooling goes into `tools/paddock-dev` (`./dev`), Rust. The two small JavaScript additions (`list --json` with
`scope`, `commands --json`) are to the existing conformance CLI, which ADR 0022's amendment ports later; they are not new
scripts.

| WP | Content | Days |
|---|---|---|
| WP-1 | `./dev gates check` core: CI reader (workflows, actions, triggers to levels, `paddock.ci.json`, one level of npm, action and workflow indirection), the enumerators of §1, the register parser and its rules, report, `--json`, exit codes; pure core with in-memory fixture trees, one good and one bad fixture per kind. Picks the YAML reader (candidates: `yaml-rust2`, MIT/Apache-2.0, or the job splitter `plan.rs` already has); the choice and its source are recorded in the module's doc comment. Measures its run time (rule 48). | 4 |
| WP-2 | §5: the doc scanner module (`FactKind`), check ids and tool commands; `paddock-conformance list --json` with `scope` per check and `commands --json`. | 1.5 |
| WP-3 | Baseline: run on `main`, write `docs/gates.toml` with an entry for every finding (owner and date each, or the trivial fix), wire C100 into CI's `conformance` job and nightly, planner inputs in `affected.mjs` and `plan.rs` with golden cases. From this commit on, C100 is an error. | 1.5 |
| WP-4 | §6 fixes: C13's probe (the `cargo` field, "does not apply"), the drift in five apps, C13 in every app leg through the conformance action; the five reusable workflows called or deleted; AGENTS.md C58 line. Each removes its register entry. | 3 |
| WP-5 | Rules and docs: a new family rule ("every check runs somewhere or is registered as not gated, with a reason, owner and review date; docs name only real checks"), numbered by the integrator at landing; the lessons entry with the six instances; `.gitea/actions/README.md` and the `integrate` skill name `./dev gates check`. | 0.5 |
| **Total** | | **10.5** |

Order: WP-1 and WP-2 can run in parallel (two files of `paddock-dev`, owned separately); WP-3 after both; WP-4 after WP-3
(each fix then shows as a removed entry); WP-5 with WP-3. WP-4's app edits touch each app's `paddock.ci.json` and
`clippy.toml`, so it is scheduled outside a release freeze of that app (ER-1). Inside ADR 0022 G.7's cap of two tooling
agents during a release.

**Rollback.** C100 can become a warning by one flag on its CI line (`./dev gates check --warn`, which still prints every
finding). The register stays as documentation.

## Parallelism and cost (rule 51)

The work is a text scan of a few thousand files: one thread, no build. Cheaper first: the enumerators read only the
globbed paths, Rust files are scanned for the byte string `ignore` before any parsing, and the CI reader parses each YAML
file once. The run time is measured in WP-1 and recorded before it is quoted; if it ever exceeds a few seconds, the files
are scanned in parallel with a bounded pool (CPU-bound: cores minus one).

## Consequences

- Adding a check means adding its CI line in the same change, or registering it with a reason; reviewers no longer need to
  remember to ask.
- Ignoring a test means registering it; 86 attributes get one line each by file (about 44 entries).
- The register shows, in one file, what is deliberately not gated and until when, which today exists nowhere.
- Docs that name a planned check say so, or fail.
- The meta-check reads YAML that ADR 0038 is moving into contracts. Until then it reads both; after ADR 0038 WP-4 the YAML
  reader shrinks to set-up steps and `uses:`.

## Alternatives considered

- **Run every conformance check everywhere (no `--only`).** It closes the conformance part of instance 1, but not the
  ignored tests, scripts, actions or docs, and it turns every app-scoped check into an error on the root run. Rejected.
- **A convention in the check file ("CI: ci.yml#conformance") instead of reading CI.** It is a second place that can be
  wrong; the CI files are the truth. Rejected.
- **A review checklist item.** That is today's state; it missed all six instances. Rejected.
- **Fold it into ADR 0038's parity golden.** That golden proves the local gate equals CI; it says nothing about checks CI
  does not run at all. Kept separate, sharing its CI reader once ADR 0038 WP-1 exists.
