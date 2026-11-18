# Tool twin pairs: where a script and its Rust port both live, and how to retire the script

- Status: research note (inventory and plan only; no code changed). For the user's 2026-11-18 decision to retire the duplicated
  pairs first, as the first step of "all tooling in compiled Rust".
- Date: 2026-11-18.
- Heads read (ADR 0091): `origin/train/2026-11-18-15` at `d759f3f6f7f57028d3dd75ec70815095b41e313f` (every count and path below),
  `origin/main` at `f5f08f71e598b2306bc43b28540dfb13d51832bc` (203 commits behind the train). Also read: `origin/feat/e2e-ready-gating`
  for commit `e7141befb` (not on the train). Re-read both heads before any commit of this file.
- Related: [ADR 0022](../adr/0022-rust-developer-and-ci-tool.md) (R4 planner, G.7 release-path guard),
  [docs/plans/ci-in-rust.md](../plans/ci-in-rust.md) (WP-1 to WP-14; this note re-slices its retirement steps by pair),
  [docs/known-copies.toml](../known-copies.toml) (rule 55 register), ADR 0083 (hooks), ADR 0060 (C100).

## 0. How this was measured

- **Which implementation CI runs**: from the CI model, `./dev gates model --json` on the train (10,399 command lines with their
  workflow, job, OS and level), the accessor C100 itself uses (ER-42), not a grep of the YAML. Levels below: b branch, m main, n
  nightly, r release; G gated, C conditional, R report-only, U unresolved. Where the model shows a shim (`kit.sh`), the shim's
  own choice rule decides (read in `conformance/ci/kit.sh`, `kit_choice`).
- **Parity tests**: read their headers and what they compare (files cited per pair).
- **Drift**: `docs/lessons.md`, `docs/known-copies.toml` notes, and `git log --no-merges` on the train. "Co-edited" counts commits
  that changed both files of a pair in one commit (since 2026-11-01; the planner since 2026-11-08). A twin fixed in a later commit
  (the CACHEDIR.TAG case) does not count as co-edited, so the column understates the tax.
- **Releases**: `git tag --contains`. No tag contains the CI planner switch (`a90bca176`, 2026-11-08) or the kit switch
  (`ef5cf3153`, 2026-11-12). The newest tag is `dyno-v1.1.0`. `v0.4.0` and `v0.5.0` (the `delete_by` of most tooling entries)
  do not exist yet.
- Not verified: the CI run that went red on the CACHEDIR.TAG parity. I did not read Gitea runs by id. The evidence is the fix
  commit `036b9dd9f` ("host_steps parity is green again") and the revert and reapply `c173ac835` and `d9053fd9f` on train 14.

## 1. Summary table

"CI runs" is what a normal CI run executes today. "Shell on release" means `kit.sh` picks the script for tag runs, for
`PADDOCK_DEV_DOWNLOAD=0` (every release, rule 54 and nightly-installer job: `app-release.yml:155,426`, `dyno-release.yml:46`, ...)
and after a failed toolchain install. Cost is raw agent-days including one review fix round.

| # | Pair (script : Rust) | CI runs today | Parity test (what it compares) | Drift seen | Retire cost |
|---|---|---|---|---|---|
| P1 | `scripts/affected.mjs` + `affected-marks.mjs` : `tools/paddock-dev/src/plan*.rs` | Rust plans every workflow (`./dev plan --ci`, also the release callers). **JS still answers C100** (`./dev gates check` uses `edge::NodePlanner`, which runs `node scripts/affected-marks.mjs`, `gates/command.rs:89,138`). JS only otherwise under `PLAN_PLANNER=node` (ci.yml:141). `affected.test.mjs` runs in ci.yml `plan` and `tools` | `tests/it/golden.rs`: `recorded_*` (30 frozen answers in `tests/golden/expected`, made by `generate.mjs` which imports the script) and six `live_*` with the script as oracle; `gates/ci.rs` `the_leg_keys_follow_affected_mjs` | 22 script commits since 11-08, 17 also edited `plan*.rs`; `e7141befb` (lint wiring) had to edit both; lesson 2026-10-29 (the Rust port kept the old list) | **3** (2.5-4) |
| P2 | CI kit: `job-isolation.sh`, `job-tmpdir.sh`, `ci-home.sh`, `build-jobs.sh`, `build-env.mjs`, `host/{stable-workspace,node-caches,host-tools,sccache-stats}.sh`, `build-folder.sh`, `tools/prune-*.sh`, `tools/host-cleanup.sh`, `kept-clone.sh`, `bounded.sh`, `guardian-as-user.sh`, `boot-volume.sh`, `safe-rm.sh`, setup-rust's three shell steps : `ci_isolation/`, `ci_build_jobs.rs`, `ci_build_env.rs`, `ci_host/`, `safe_remove/guard.rs`, `ci_setup_rust.rs` | **Both, every day.** Rust on branch, main and nightly test jobs; shell on tag, release, rule 54 and nightly installer jobs. Shell also always for: `job-tmpdir` action (container jobs), `kit.sh` `tools_home` (`job-isolation.sh path tools`, every host job), Dyno `check-bundle.sh` and `paddock-testkit` (`boot-volume.sh`) | `tests/it/host_steps.rs`, `build_jobs.rs`, `setup_rust.rs`, `build_env.rs` (exit code, stdout, `$GITHUB_ENV`/`$GITHUB_PATH`, wrapped calls, folder state); goldens `host_steps_golden.rs`, `build_jobs_golden.rs`; `conformance/test/job-isolation.test.mjs` and `kept-clone.test.mjs` against the binary (`ISOLATION_CMD`, `KEPT_CLONE_CMD`); `safe-remove-parity.tsv` | CACHEDIR.TAG on train 14 (`f09a919a9` Rust only, `036b9dd9f` shell twin, revert/reapply `c173ac835`/`d9053fd9f`); the shell does not know ADR 0025's `{src}` layouts (`tests/it/ci_isolation.rs` header); `root/link/` hole in both copies (lesson, safe-rm); Rust isolation 23 commits, script 6 | **5** (4-7) |
| P3 | 12 conformance checks C39, C53, C119-C125, C129-C131 (`conformance/src/checks/*.mjs`) : `tools/paddock-dev/src/check/c*.rs`, with the shared helpers (allow-list, inline-waiver, outcome, report, scope, md-tables, version, windows-bash, workflows, full-checkout, the shell and Rust lexers) | **JS only.** `./dev ci run conformance paddock` runs `node conformance/bin/paddock-conformance.mjs` (conformance/paddock.ci.json); no CI line runs `./dev conformance` or `./dev check run`. The Rust twins run only in their tests and in no gate | `tests/it/check.rs` (C39, C53: recorded `fixtures/check/expected.json` plus live), `check_engine.rs` (stdout, `--out`, `--summary`, exit code), `parity_corpus.rs` (findings over text spellings; C123 dropped, `ee178cb7b`), `check_allow_list.rs` (error paths), `check_c1xx.rs` (live JS oracle only, no recorded answers except C131) | Error paths diverged in C125, then C121 and C124 on train 12 (lesson 2026-11-15; `865498235`); C131 text shape (lesson 2026-11-16). Co-edited: C125 5/5, C121 7/8, C129 7/7, C53 3/13 | **3.5** (3-5) |
| P4 | Claude hooks `no-pgrep-wait.py`, `no-force-push.py`, `kill-guard.py` (+ `lib/shellscan.py`, `lib/paddockpaths.py`) : `hook/rules/{no_pgrep_wait,no_unbounded_wait,no_force_push,no_kill_by_name}.rs`, `hook/lex.rs`, `places.rs` | **Python only** (the local `.claude/settings.json` runs the `.sh` wrappers; the Rust `dev hook pre-tool-use` is not installed). CI: `bash .claude/hooks/run-tests.sh`, report-only, ci.yml `tools` | `hook/oracle_tests.rs`: corpora checked against **frozen copies of the 2026-11-10 shell hooks** (`hook/fixtures/*.sh`, last changed `fb4c579fa`), not the live Python; `paddockpaths-parity.test.sh` runs `places.rs` and the Python on one table | The live hooks moved to Python and changed on 11-12, 11-15 and 11-18; the Rust oracle did not follow. 0 co-edits | **4** (3-6); cheaper option 0.5 |
| P5 | `scripts/ci-timings.mjs` : `ci_runs/timings.rs` | Rust only; the script has no caller | `timings.rs` compares with a recorded `golden.md` (frozen 2026-11-10) | none | **0.25** |
| P6 | `scripts/npm-audit.mjs` : `audit/` (`allow.rs`, `judge.rs`; on the train, not on main) | **Both.** App jobs run the script (ci.yml `app`, debrief.yml `frontend`); `./dev audit compare` (ci.yml `audit`) and `./dev audit nightly` | `scripts/npm-audit-fixtures.json` read by both test suites | none yet (new) | **1.25** |
| P7 | re-land rule in `conformance/src/commit-lint.mjs` : `merge_check/reland.rs`, `reverted_base.rs` | Both, for different questions: commit-lint (commits job, commit-msg hook) and `./dev merge check` | one case table `conformance/reland-cases.json` read by both suites | none; 1 co-edit in 10 | **1.5** (with commit-lint's port, ci-in-rust WP-9) |

Kept on purpose, not in the total: `scripts/test-kit.mjs` against `paddock_testkit` (the Reaper and `waitProgressing` for Node test
specs: a framework-required test helper, an exception of the user's decision), the `dev` wrapper's two cargo lines against
`cargo_target.rs` (bootstrap), and `kit.sh` itself (the bootstrap shim, ci-in-rust §5).

**Total for P1 to P7: about 18.5 agent-days (15 to 25).** With the cheaper hook option (P4 at 0.5 days): about 15 days.

## 2. Findings that change the plan

1. **The release path can never run the Rust kit, so the register's retirement condition cannot be met.** `known-copies.toml`
   `ci-job-isolation` says the scripts go "after the next app tag whose release run went through the Rust path green". `kit_choice`
   picks the shell for every tag run and for `PADDOCK_DEV_DOWNLOAD=0`, so no release run goes through the Rust path. `kit.sh` has
   only `PADDOCK_CI_KIT=shell`, no switch that forces `dev`. Retirement needs a deliberate flip of the release default (P2, step 1).
2. **C100 judges gating with the JavaScript planner while CI plans with Rust.** `./dev gates check` (ci.yml `conformance`, nightly
   `c100`) asks `node scripts/affected-marks.mjs`. A rule that only one planner has (the lint-wiring files of `e7141befb`) can make
   C100 say a check is gated that the Rust plan never turns on, or the other way. Each question has one reader, but the two
   readers differ. P1 must give C100 a Rust `Planner` before the script goes.
3. **The Rust hook's oracle is stale.** `oracle_tests.rs` holds the 2026-11-10 shell hooks verbatim. The live guards are the Python
   ones, changed since (for example `32e0b32ad`, 2026-11-18, refuses git config overrides). Python is the canonical side here; the
   Rust rules trail it, and four guards (worktree, docker, scratch-root, recurring) and the git-output warning have no Rust rule.
4. **The Rust check tests can't outlive the JS host as written.** The live JS host is the only oracle for C119 to C125, C129 and
   C130. Only C39 and C53 (`expected.json`, `masked.json`) and part of C131 have recorded answers.
5. **The register misses the pairs that drift most.** `known-copies.toml` has no entry for the planner pair, the 12 check twins or
   the hook rules. It does list the kit, timings, npm-audit, places, safe-remove and re-land pairs. C58 (the register check) is not
   built, so nothing catches a missing entry.
6. **Three shell readers for one job.** `.claude/hooks/lib/shellscan.py`, `conformance/src/util/shell-lexer.mjs` (held to
   tree-sitter by parity) and two Rust ones (`hook/lex.rs`, hand-written; `check/lex/shell.rs`, tree-sitter). Retiring P3 and P4
   leaves two Rust readers for different needs (commands and comments). A follow-up could move them onto one; it isn't costed here.
7. **Shared code on the kit's path.** `crates/paddock-testkit/src/boot_volume.rs` runs `conformance/ci/boot-volume.sh`. Retiring
   that script changes a shared crate the apps' tests depend on, so it falls under ER-1.

## 3. Per pair: what retirement needs

### P1 Planner (`scripts/affected.mjs`, `scripts/affected-marks.mjs`)
- **Callers to switch**: (a) C100: a `RustPlanner` in `gates/edge.rs` over `plan::plan_for` that answers the same per-file marks
  (the plan job's output names). This is the only real production caller. (b) ci.yml: drop `PLAN_PLANNER` and the `node
  scripts/affected.mjs --ci` branch (ci.yml:130-146). (c) ci.yml `plan` and `tools` jobs: drop `node --test
  scripts/affected.test.mjs`. (d) `package.json` `affected` script, `lefthook.yml` comment, AGENTS.md "Affected tests" (it tells
  people to run `node scripts/affected.mjs origin/main --explain`; that becomes `./dev plan origin/main --explain`), rules doc
  rule 3 wording, and the comments in `apps/*/paddock.ci.json`, `C91`, `conformance/src/util/full-checkout.mjs`,
  `scripts/full-checkout.test.mjs` and `conformance/test/c126-test-steps.test.mjs` that name the script.
- **Parity proof before deletion**: on the final tree, every `live_*` test of `tests/it/golden.rs` green on the Linux leg
  (`live_range_plans_match_affected_mjs_on_this_repositorys_first_parent_history` covers real history). New: a one-off marks
  comparison, Node planner against the Rust planner for every tracked file (C100's input), with zero differences. Then delete.
- **Oracle and golden**: `recorded_*` and `recorded_command_line_*` already run without node; they stay. `generate.mjs` imports
  the script, so it goes too. A future golden change is a reviewed edit of `tests/golden/expected/*.json` (or a Rust `--bless`
  writer), with the reason in the commit. `gates/ci.rs` `LEG_KEYS` reads `plan_ci::matrix` instead of the script's `matrix()`.
  Port the cases of `affected.test.mjs` (1,316 lines) that no Rust unit test or golden covers. Most of it overlaps, but this has
  to be checked case by case, so it is in the estimate.
- **Precondition**: ADR 0022 R4 keeps the fallback "for one release". No tag yet contains `./dev plan`. The release callers plan
  with Rust (`bash "dev" plan --ci --app dyno`, level r), so the next app tag meets it. **Decision D2**: wait for that tag, or
  waive the fallback now (it is a repository variable; whether it was ever set can't be told from the repository).
- **Risks**: C100's answers move when the planners differ. That is the point, but the first run may show real "not gated"
  findings, and C100 blocks. Run it report-only on the branch first and read the diff (ER-41).

### P2 CI kit (the shell scripts `kit.sh` falls back to)
- **Callers to switch**: (1) `kit_choice`: Rust on tag and release runs too. `./dev` already builds itself from source when it
  may not download (ADR 0022 §3.3), so `PADDOCK_DEV_DOWNLOAD=0` no longer means shell. Keep `PADDOCK_CI_KIT=shell` for one
  release (section 5). (2) The always-shell callers: `kit.sh` `tools_home` (`job-isolation.sh path tools`) becomes a few lines
  of POSIX in the shim or a `./dev --print-home`; the `job-tmpdir` action calls `./dev ci isolation tmpdir`; `build-folder.sh`'s
  own `job-isolation.sh path target` call goes with it; Dyno's `check-bundle.sh` and `paddock_testkit::boot_volume` call `./dev
  ci boot-volume` (shared crate, ER-1); setup-rust deletes the "Install sccache", "sccache" and Tauri-dependency shell steps and
  its `Implementation` step. (3) A toolchain that cannot be installed then fails the job with its own message, which is what other
  projects do. Today the shell kit silently takes over. (4) C56 reads the variable names from `build-env.mjs`'s `buildEnv`. It
  needs another source: the Rust command's `--names`, or a constant C56 reads through the contract.
- **Parity proof before deletion**: the parity suites listed in the table green on the final tree on all three OS legs. Then,
  with the flip, one rule 54 dispatch of each app's release with gates on and bundles built (Dyno, Pitwall, Spotter, Pitcrew,
  Debrief's bundle job, the two demos) and one nightly installer run, all through the Rust kit. The project already requires
  this (rule 54), and it is ci-in-rust WP-11's dry run. Only then delete.
- **Oracle and golden**: `host_steps_golden.rs` and `build_jobs_golden.rs` already hold the shell's answers as literals and stay.
  The live parity files `host_steps.rs`, `build_jobs.rs`, `setup_rust.rs` and `build_env.rs` lose their oracle; keep their cases
  as Rust-only behaviour tests (their inputs and expected outputs are already in them). In `conformance/test/job-isolation.test.mjs`,
  `kept-clone.test.mjs`, `ci-actions.test.mjs`, `safe-rm.test.mjs` and `tools/test/prune-and-cleanup.test.mjs`, the cases that
  target the scripts go; the binary-targeted cases move into `tests/it`. `safe-remove-parity.tsv` becomes the Rust guard's table.
- **What stays**: `safe-rm.sh` while `scripts/vm-win/prepare.sh` (a dev script, WP-14) sources it. It keeps its register entry with
  that one user.
- **Risks**: release path (ADR 0022 G.7): only outside a release window, after the current tags. The Windows junction and macOS
  boot-volume branches are only proven on the real runners (ER-9). `tests/it/ci_isolation.rs` notes Rust layouts the shell
  never had, so a release built with the Rust kit uses different kept folders than past releases did. That is harmless for
  correctness, but the first runs are cold.

### P3 Conformance check twins
- **Option A (recommended)**: split by check. CI runs `./dev conformance --only <the 12>` beside `node ... --only <the rest>`.
  Delete the 12 JS checks and their JS test files, and port each test's good and bad fixtures to the Rust test. Callers:
  `conformance/paddock.ci.json` (a second phase line), `conformance/bin/paddock-check-lists.mjs` (`only` is generated from the
  JS files: add the Rust registry, `./dev conformance list --json`), `docs/gates.toml` and C100 (the new line must count as
  gating), and the pre-push gate, which follows the contract on its own.
- **Option B**: finish ci-in-rust WP-10 (the whole engine and the 39 other checks, 11 days). That retires the helper twins too.
- **Parity proof**: `check_engine.rs`, `parity_corpus.rs`, `check_allow_list.rs` and every `check_c1xx.rs` green on the final
  tree, plus one run of both hosts over the real tree with equal reports. Then, **before** deletion, record the JS answers for
  each `check_c1xx.rs` fixture into `expected.json` files, as `tests/fixtures/check/generate.mjs` did for C39 and C53. After that
  the tests compare with the recorded files.
- **Risks**: two hosts in one job until WP-10. The helper twins (allow-list and the others) still serve the remaining JS checks,
  so their pairs stay until WP-10. The C100 register has to see the Rust line, or C100 reports the 12 checks as not gated.

### P4 Claude hooks
- **Direction is reversed**: Python is canonical, Rust trails. Retirement means porting the live Python rules (about 1,240 lines
  of Python rules plus `shellscan.py`) to `hook/rules`. Their `*.test.sh` cases become corpora checked against the live hooks
  until the switch. Then `./dev hooks install` and `.claude/settings.json` (and the template `tools/githooks/claude-settings.json`)
  point at the shim.
- **Parity proof**: every `.claude/hooks/*.test.sh` case decided the same by the Rust rule, with the deliberate differences
  listed (`DIFFERENCES`, `RESIDUAL`), and one day of shadow use in warn mode on the coordinator's machine.
- **Risks**: the guards protect the user's machine (worktrees, Docker, kills). A gap in the port is a safety gap, not a red test.
  `.claude/settings.json` is the user's local configuration; an agent must not change it, so the switch is the user's (D4).
- **Cheaper option (0.5 day)**: delete the five uninstalled Rust rules and the oracle, and keep the Python hooks as part of the
  bootstrap exception until there is time to port them. That removes the pair at once and loses nothing that runs today.

### P5 `ci-timings.mjs`
- No caller; the golden is recorded. Delete the script and the "regenerate with that command" note. From then on the golden is
  edited by review. The engineering-rules repository's copy is separate (its own register line, one release later).

### P6 `npm-audit.mjs`
- Add `./dev audit app [--dir]` (judge one folder's `npm audit --omit=dev --json` with the shared allow list; `judge.rs` and
  `allow.rs` exist), switch the two app callers, and keep `npm-audit-fixtures.json` as the Rust suite's input. Needs the audit
  work on the train to reach main first (ADR 0096: held on `train/2026-11-18-15`).

### P7 Re-land rule
- Goes with commit-lint's port (ci-in-rust WP-9). The commit-msg hook (`lefthook.yml:61`) calls `./dev` instead of node.
  `reland-cases.json` stays as the one table.

## 4. Comparable practice (ER-37) and the cheaper option

- **git** rewrote shell commands in C and kept the old script one switch away for a few releases, with the new code as the
  default everywhere: `rebase.useBuiltin` (git 2.20-2.21) and `stash.useBuiltin` (2.22-2.26, removed with `git-legacy-stash.sh`
  in commit `8a2cd3f512`, March 2020). Sources: [stash: remove the stash.useBuiltin setting](https://code.googlesource.com/git/+/8a2cd3f5123ac822fd64ca8efc4f84e122a1edb3),
  [rebase: remove the rebase.useBuiltin setting](https://timplab.syktsu.ru/admin/git/commit/d03ebd411c),
  ["Really retire git-legacy-stash.sh"](https://lore.kernel.org/all/xmqqpmocy60i.fsf@gitster.g/T/). Read, not verified against
  git's release notes; the version ranges are from the config docs quoted in those commits.
- **Rust's own build** keeps a small Python `x.py`/`bootstrap.py` that only fetches a stage0 toolchain and builds the Rust
  bootstrap binary; everything else is Rust. Source: [rustc-dev-guide, Bootstrapping](https://rust.googlesource.com/rustc-guide/+/refs/heads/master/src/building/bootstrapping.md).
  This matches Paddock's `dev` wrapper and `kit.sh` shim as the bootstrap.
- **Against that**, Paddock's kit has the new code as default except on the release path, where the old code is forced with no
  switch back. That makes the release path the place where only the old code is tested. git did the opposite. The common-practice
  option is the git pattern: Rust everywhere including release, the shell one variable away (`PADDOCK_CI_KIT=shell`) for one
  release, then delete. P2 is costed that way. Keeping both until WP-11, as planned, costs nothing extra today, but every
  release-path fix has to be made twice in the meantime (the CACHEDIR.TAG case). Nothing here goes beyond common practice, so no
  row needs the 1-day ER-37 approval. The one added proof, the rule 54 dry runs, is a project rule.

## 5. Order, parallelism with the releases (ER-1), and estimate

Order by drift risk and how often the pair changes, then held back where ER-1 forbids it:

| Step | Pair | Days | Touches code a release depends on? | When |
|---|---|---:|---|---|
| 1 | P5 timings | 0.25 | no | now, any free slot |
| 2 | P1 planner | 3 | `tools/paddock-dev` plan code, which the release callers' plan job runs; the goldens prove the plan output does not change | now, in parallel (D2) |
| 3 | P3 check twins (option A) | 3.5 | gates only: ci.yml's `conformance` job also runs at release level (model: `rG`), but it builds and ships nothing; the release workflows' own C85 and C99 lines stay in JS | now, in parallel |
| 4 | P4 hooks | 4 (or 0.5) | no: local only | now, in parallel (D4) |
| 5 | P6 npm audit | 1.25 | the app jobs of ci.yml (gates, not the release build) | after the audit work lands on main |
| 6 | P7 re-land rule | 1.5 | no | with WP-9 |
| 7 | P2 CI kit | 5 | **yes**: release path, setup-rust, and `paddock-testkit` (shared crate) | only after the current tags, outside a release window (ADR 0022 G.7); it then replaces ci-in-rust WP-11's kit part |

- **Total**: about 18.5 agent-days (15 to 25); 15 with the cheaper hook option. Steps 1 to 4 (10.75 days) can run beside the
  releases in two lanes (ADR 0022 G.7 caps tooling at 2 agents while a release is in flight). Step 7 waits.
- **Until step 7**, the highest-drift pair stays dual. Cheap holding measures, already in place or one line each: the train checklist line for kit
  changes (`f09a919a9`, D5), the parity suites, and register entries for the pairs found missing (finding 5). That register
  edit is a 0.25-day docs change, but `known-copies.toml` is a shared integration point, so the integrator makes it.

## 6. Non-Rust tooling with no Rust twin yet (for later)

Non-test files and non-blank lines on the train (`*.mjs|cjs|js|sh|ps1|py`, test and fixture folders left out). Estimates are
ci-in-rust §3 and ADR 0022 §7 figures, not re-measured.

| Area | Files / lines (approx.) | What is in it | Rough port (days) | Plan row |
|---|---:|---|---:|---|
| `scripts/vm-smoke`, `vm-win`, `vm-lock*`, `vm-test-support`, `tools/macos-vm-smoke.sh`, `linux-container.sh`, `verify-installed.sh` | ~85 / ~10,300 | VM and container harness | 11 | WP-14 |
| Rest of `scripts/` (new-app, publish, stage-paddock-setup, demo-release, coverage-gate, svg-drift, dialog-commands, member-gate-windows, train-ci-hold, disk-record wrapper, notices, e2e-ready-report, setup-dev) | ~30 / ~4,700 | dev and release scripts | 2.5 (WP-12) + release part in WP-11 | WP-9, WP-11, WP-12 |
| `conformance/src` engine and the 41 JS-only checks, `bin/`, `notices`, `sidecar`, `window-weight` | ~100 / ~11,000 | conformance | 11 (P3 takes ~1 of it) | WP-10 |
| `conformance/ci` without twin (install-test-browser, pin-runtime, runtime-catalogue, tauri-bundle, tauri-tools, release-notes, release-verdict, request-tag, check-tag, check-index, plan.mjs, first-release-notes, dev-tool-publish, before-build, updater-key, tag-node-caches) | ~16 / ~2,000 | fetchers and release CI | 2.5 (WP-6) + part of WP-11 | WP-6, WP-9, WP-11 |
| `conformance/release` | 7 / ~1,100 | release kit | part of WP-11 (8 in all) | WP-11 |
| Workflow and action inline shell | ~1,300 lines in YAML | ci.yml rest, debrief.yml, small workflows, release steps | 3.5 + part of WP-11 | WP-7, WP-11 |
| `.claude/hooks` without twin (worktree, docker, scratch-root, recurring guards, git-output warning) | 5 / ~1,000 | agent guards | in P4 if ported | ADR 0083 |
| `apps/*/scripts` | ~164 / ~15,300 | app CI, generators, release helpers | WP-8 2.5, app release in WP-11, ADR 0022 wave 3 ~36 | WP-8, WP-11, R11-R23 |
| `icons/`, `ui/scripts` | ~34 / ~4,800 | icon pipeline, UI glue | in ADR 0022 R12, R14 (5.5) | wave 3 |

Not to be ported (the user's exceptions): `eslint/src/rules` (855 lines), framework test specs, config files, the bootstrap
(`dev`, `kit.sh`, `tools/bootstrap.sh`, the hook shim).

## 7. Decisions for the user

- **D1** Order: steps 1 to 4 now, beside the releases; P2 after the current tags. Recommended.
- **D2** Planner fallback: waive ADR 0022 R4's "one release" now, or wait for the next app tag. Recommended: wait only if that
  tag is within about a week, otherwise waive. C100 already depends on the script, so the dual state is not free.
- **D3** Check twins: option A (split host, 3.5 days) now, or wait for WP-10 (11 days, retires the helper twins too). Recommended: A.
- **D4** Hooks: port the Python guards to Rust and switch your `.claude/settings.json` (4 days, your change), or the cheaper
  option, delete the uninstalled Rust rules and keep Python as bootstrap for now (0.5 day). Recommended: the cheaper option now;
  port later with ADR 0083's RT-7.
- **D5** Release kit flip (P2): adopt git's pattern, Rust on the release path with `PADDOCK_CI_KIT=shell` kept for one release,
  then delete, instead of the register's current condition, which can't be met (finding 1). Recommended.
