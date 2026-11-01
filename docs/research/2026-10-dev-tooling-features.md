# Features for the Rust developer tooling beyond ADR 0022's plan (2026-10-31)

Date: 2026-10-31. Author: researcher agent. Status: final.
Question (user, 2026-10-31): "Any other features to add? Research?", about the tooling planned in ADR 0022 and its
proposed amendment of 2026-10-31 (branch `docs/adr-0022-generic-tools`: `devkit`, `wiki`, `heavy`, `./dev`, the
`paddock-tool` runtime, external `devkit-<name>` subcommands and TOML packs; no sandbox, decided by the user).
Scope: features that would save time or prevent repeat incidents for one developer, many AI agents in worktrees on one
14-core Mac, and scarce Gitea macOS and Windows runners. Nothing was built. **Verified** means read in the tree
(`origin/main` at `233c1339b`) or measured; **read** means taken from the linked page, accessed 2026-10-31. Estimates are
agent-days for an implementer, marked as estimates.

## Summary

Add 16 features, ranked below; together about 23 agent-days (estimate; the table sums to 23.0). Eight are cheap and prevent incidents that already
happened, so they belong in the tooling waves now (within ADR 0022 G.7's cap of two tooling agents during a release):
hooks through `heavy`, a CI triage command that turns a red run into a digest and the exact local re-run, worktree
creation as a Claude Code `WorktreeCreate` hook, `doctor`, a rule-50 dispatch guard, secret scanning, a pre-push
affected gate and the tool version range check. Adopt no task runner, monorepo tool, remote cache or flaky-test SaaS
(rejected list). The three open decisions were taken by the user on 2026-10-31 (see "Decided 2026-10-31" at the end).

## What the plan already covers (not repeated as candidates)

`--json` and stable exit codes (`paddock-cli`), log file per run, phases, masked spawn logs, NTSTATUS decoding,
`::error::` with next step, a run summary (G.5), commit-lint (G4), flaky bisect and polluter search (G5), temp-leak and
hermetic checks (G6), coverage gate (G7), secret whitespace stripping (G8), CI timings on demand (G9), worktree add/list/
clean/disk and prune (G10), Gitea release (G11), pinned downloads (R3), notices and licence allow-list from `deny.toml`
(R6; `cargo deny` already runs in every app, verified: `apps/*/deny.toml`, `crates/deny.toml`), the rules engine (R8), and
the content-addressed tool binary cache (§3.3). Changelogs already come from git-cliff (`apps/*/cliff.toml`, verified).
**Job-level last green** (WP-A1, research note [Nx, Turborepo or our own scripts](2026-10-nx-vs-scripts.md) §4: 25% of
runner minutes, 37% on main) is planned as phase 0 but **not built**: `scripts/affected.mjs` still takes one run-level
base (lines 27, 824 to 825, verified). It saves more runner time than any candidate below; the user pulled it forward on 2026-10-31.

## Findings that shape the list

- **Verified: the commit hook bypasses the machine-wide lock.** Root `lefthook.yml` runs `pre-commit` with
  `parallel: true` and one `cargo clippy --workspace --all-targets` per app (Debrief, Dyno, Pitcrew, Pitwall, Spotter),
  each triggered by any staged `*.rs` under that app, with no `heavy` and no `CARGO_BUILD_JOBS`. A shared-crate change
  adopted by every app in one commit (AGENTS.md, rules 39/42/43) starts five workspace clippy builds at once in that
  agent's worktree, outside the slots that rule 41 and the lesson "Many agents waiting on 'load below N' start together"
  introduced. `pre-push` runs Pitwall's `cargo deny` the same way.
- **Read: Gitea ignores error annotations and problem matchers** ([comparison](https://docs.gitea.com/usage/actions/comparison)):
  `::error::` lines exist only as log text, so whoever triages a red run must read logs. Job summaries
  (`GITHUB_STEP_SUMMARY`) arrived with Gitea 1.27 and need Gitea Runner 2.0 ([issue 23721](https://github.com/go-gitea/gitea/issues/23721),
  [runner PR 917](https://gitea.com/gitea/runner/pulls/917), [release report](https://linuxiac.com/gitea-1-27-released-with-actions-improvements-and-45-cve-fixes/),
  secondary). Our server answers `{"version":"28.2.0"}` on `/api/v1/version` (verified); whether our runners are on
  Runner 2.0 is **unverified**.
- **Verified: Pitwall's pre-push hook `pitwall-scripts` is broken.** Root `lefthook.yml` (line 85) runs
  `node --test scripts/commit-lint.test.mjs scripts/release/latest-json.test.mjs scripts/ci/` in `apps/pitwall/`, but
  neither file exists under `apps/pitwall/scripts/` (`origin/main` at `2d6f1d84e`: only `ci/`, the bootstrap and verify
  scripts, `sidecar.mjs`, `tauri-before-build.mjs`; the commit-lint and latest-json tests live in `apps/debrief/scripts/`
  and `conformance/test/`). Run in a clean worktree on Node 24.22: `Could not find 'scripts/commit-lint.test.mjs'`, exit 1;
  the hook fails every Pitwall push, or is skipped by habit. Even the directory argument fails on Node 24 (`scripts/ci`
  reported as a failed test), while `node --test scripts/ci/*.test.mjs` passes 9 tests. Fix with feature 7 (the
  pre-push gate takes its commands from the plan, not from a hand-written list), or at once by listing the real test
  files; a check that every path a hook names exists belongs in `doctor` (#4).
- **Verified: no secret scanning exists** (`git grep -i 'gitleaks|trufflehog|secret.?scan'` finds only docs and an MCP key
  module). The release kit handles signing keys and `GITEA_TOKEN`; the user pastes confidential logs (memory note).
- **Read: nextest already has the flake and hang primitives.** `flaky-result = "fail"` from 0.9.131, with JUnit
  `<flakyFailure>` marks ([retries](https://nexte.st/docs/features/retries/)); experimental record, replay and
  `run -R latest` rerun of failed tests, with `store export` for CI artifacts ([record-replay-rerun](https://nexte.st/docs/features/record-replay-rerun/));
  live status of running tests on `t`, SIGINFO or SIGUSR1 since 0.9.86 ([reporting](https://nexte.st/docs/reporting/)).
  Installed here: 0.9.150 (verified). The crates `ci` profile writes JUnit and has `retries = 0` (verified,
  `crates/.config/nextest.toml`), so CI cannot tell a flake from a failure today.
- **Read: Claude Code has `WorktreeCreate` and `WorktreeRemove` hooks** that replace its default worktree creation
  ([hooks](https://code.claude.com/docs/en/hooks)). Every agent worktree here is created by the harness, not by
  `worktree.sh`, so the rules in ER-2 are applied by hand in each brief today.
- **Read: AI-agent tooling converges on a graph-aware MCP server plus generated agent files**: Nx exposes its project graph,
  affected projects and CI failure context over MCP and writes `AGENTS.md`/`CLAUDE.md` ([Nx AI](https://nx.dev/docs/features/enhance-ai)).
  Paddock already owns a local, read-only-by-default MCP server crate, `paddock-mcp` (ADR 0007, verified).

## Ranked candidates

Priority: **now** = the next tooling wave (1a/1b), within G.7's cap; **after** = after the current releases; **later** =
when its trigger appears. Class: **G** generic (`devkit` or a standalone tool), **P** Paddock (`./dev`), **T** template or
config only.

| # | Feature | Has it (read, 2026-10-31) | Incident or cost here | Days (est.) | Priority | Class |
|---|---|---|---|---|---|---|
| 1 | **Hooks through `heavy`, scoped to the change.** Every hook command that compiles runs as `heavy …` with `CARGO_BUILD_JOBS=2`; workspace clippy moves from `pre-commit` to `pre-push` and runs once per affected workspace (from the plan), not once per app in parallel; formatting stays in `pre-commit`. engineering-rules `templates/lefthook.yml` (at `791e941`) already runs clippy at `pre-push` through `heavy.sh`; only the per-workspace scoping is new there, and `heavy.sh` becomes `heavy`. `doctor` (#4) checks hooks are installed | lefthook `jobs`, `group`, `piped`, `skip`, `only` ([config](https://lefthook.dev/configuration/)) | Finding 1; load 150 to 240 (memory "agent concurrency"), lesson "Many agents waiting on 'load below N' start together", rule 41, ER-24 | 0.5 | now | T (+ P for the plan call) |
| 2 | **`devkit ci triage <run>`**: for a red run, one digest of at most 40 lines (failed jobs, their `::error::` lines, decoded exit, stderr tail, failed tests from JUnit) and the **exact local re-run command** per failure (nextest filter expression, `ng test --include`, Playwright `--grep` plus file), ready for `heavy`; `--json` for agents. Optionally downloads the nextest recording artifact for `run -R` | Nx Cloud failure context ([Nx AI](https://nx.dev/docs/features/enhance-ai)); nextest rerun ([rerun](https://nexte.st/docs/features/record-replay-rerun/)); Gitea ignores annotations (finding 2) | ER-10/rule 4 (re-run only failed tests locally), ER-27, the `ci-triage` skill run by hand per failure; CI 6162 incident (memory "fix failed tests first") | 2.5 | now | G (Gitea/GitHub API; test formats by pack) |
| 3 | **Worktree template as the `WorktreeCreate` hook**: `devkit worktree add` becomes the hook command, so every agent worktree gets the branch, sparse cone, its own `CARGO_TARGET_DIR` outside `~`, its e2e port block, the hooks installed, absolute paths for git-ignored local config, and a manifest the clean-up reads; `WorktreeRemove` runs the finish check (#10) | Claude Code hooks ([hooks](https://code.claude.com/docs/en/hooks)) | ER-2 incidents: two agents in one checkout, a shared target dir overwriting artefacts, `reuseExistingServer` on another worktree's port, relative `.cargo/libduckdb.toml` (lessons 2026-10-23 "More findings", "Stale servers and stale builds") | 1 (on G10) | now | G (template values from `devkit.toml`) |
| 4 | **`doctor [--json]`**: tool versions against minimums (nextest ≥ 0.9.131 for ER-12, cargo-deny, lefthook, `heavy`, `wiki`), hooks installed, build dir per worktree and not in `~`, sccache reachable, free disk and load with rule 41's thresholds, e2e port free, `ui/dist` stamp current, leftover temp folders and Chrome clones; each failure names the fix command. Checks are a pack, so `./dev doctor` adds Paddock's | `mise doctor --json` ([mise doctor](https://mise.jdx.dev/cli/doctor.html)) | 98k leaked temp folders (55 GB) and 620 Chrome clones (441 GB) found late (lessons 2026-10-23); stale `ui/dist`; disk housekeeping (memory) | 1.5 | now | G engine + P pack |
| 5 | **Rule-50 dispatch guard**: `devkit ci dispatch` refuses a second run for the same head, cancels queued runs of the branch with an older head, refuses a branch already merged, and says "docs-only: no CI" when the plan is empty; `devkit ci trim` replaces the coordinator's scratchpad `ci-trim.py` | none found off the shelf for Gitea | Lesson "Stale, duplicate and global-trigger CI runs wasted the runners" (2026-10-29); rule 50 (a) to (d); runners are the scarcest resource | 1 | now | G |
| 6 | **Secret scanning** in `pre-commit` (staged) and in CI over the push range, with a baseline; Paddock adds patterns for the Tauri signing key, `GITEA_TOKEN`, the registry token, and a "looks like a real user log" pattern set | gitleaks (MIT, Go; "future releases will be security patches only", [gitleaks](https://github.com/gitleaks/gitleaks)); its successor betterleaks (MIT, Go, v2 in development, v1 maintained, [betterleaks](https://github.com/betterleaks/betterleaks)) | Finding 3; confidential user logs (memory); signing secrets in the release kit (lessons "Releasing", 2026-10-23) | 0.75 | now | T (binary pinned through `devkit fetch`) + P pack |
| 7 | **Pre-push affected gate**: `./dev check --pre-push` runs lint, type-check and format for exactly what `./dev plan` selects for `origin/main..HEAD`, through `heavy`, and prints what it skipped | Turborepo `--affected` ([run](https://turborepo.dev/docs/reference/run)) | Rule 4/ER-9 ("lint and type-check the touched files before every push": a quoting typo cost a whole CI run, lessons 2026-10-22) | 1.5 | now | P |
| 8 | **Flake ledger**: CI runs nextest with `retries = 1` and `flaky-result = "fail"` (ER-12 allows exactly this), Playwright with `failOnFlakyTests`; `devkit flaky ingest` reads the JUnit files of every run into an append-only history keyed by test id and commit; `devkit flaky report` ranks by flake rate and first-seen commit and hands the top one to `bisect-flaky` | Trunk Flaky Tests (SaaS, quarantine, dashboards, [Trunk](https://docs.trunk.io/flaky-tests/overview)); nextest flaky marks | Flakes found one by one (lessons "Tests that measure the wall clock…", "A Unix socket on a loaded Mac…"; the 11 false greens of the replay); ER-12, ER-29 | 2 | after | G |
| 9 | **Stall dump**: `devkit run` (and `heavy`) judges a child as stalled when it shows no output and no CPU progress for a configured patience (never a wall-clock limit, ADR 0031), then records the process tree, `sample <pid>` on macOS, `/proc/<pid>/stack` and `wchan` on Linux, thread states on Windows, and nextest's SIGUSR1 status into the run log before ending the tree by identity (ADR 0033) | nextest status query ([reporting](https://nexte.st/docs/reporting/)) | ER-29's leaked fake timer that hung specs for days; tests waiting 120 s (paddock-mcp, `crates/.config/nextest.toml`); ER-27 asks the log to show the cause | 2 | after | G |
| 10 | **Agent finish check** (`devkit worktree finish`, also `WorktreeRemove`): lists what the worktree left (processes whose cwd or executable is in it, containers with its label, temp folders with its prefixes, held e2e ports, target-dir size) and removes only those, never a glob of a shared root | – | Docker wipe (memory "agent Docker hygiene"), shared scratchpad wiped by a reviewer (memory "briefs name cleanup paths"), ER-14 and `checklists/agent-cleanup.md` done by hand | 1.5 | after | G |
| 11 | **MCP mode and output schemas**: `devkit mcp` serves the read-only commands (plan, doctor, ci triage, ci timings, flaky report, worktree list, heavy status) over `paddock-mcp` with its caps and paging; `devkit schema <cmd>` prints each `--json` shape as JSON Schema, tested against the output | Nx MCP server ([Nx AI](https://nx.dev/docs/features/enhance-ai)); nextest config JSON Schema (0.9.134, [changelog](https://nexte.st/CHANGELOG/)) | Agents spend context reading logs and help text; ER-25's scouts do lookups a typed tool answers | 2 | after | G |
| 12 | **Timing history and trends**: every run of `heavy` and every CI run appends phase durations to a history; `devkit ci timings --trend` flags a job or phase more than a set share slower than its median over N runs, with load recorded beside local numbers | Turborepo `--summarize` run files ([run](https://turborepo.dev/docs/reference/run)); moon `lastRun.json` per task ([cache](https://moonrepo.dev/docs/concepts/cache)) | `ci-timings.mjs` is run by hand and its numbers expire with job logs; rule 48's "numbers without conditions" lesson (2026-10-29) | 1.5 | after | G |
| 13 | **Release version bump**: `./dev release bump <app> [--level]` takes the next version from Conventional Commits (git-cliff's bumped version) and writes every version site of the app (Cargo, `package.json`, `tauri.conf.json`, changelog) in one change, with a check that they agree | git-cliff, already used (`apps/*/cliff.toml`) | Lesson "Read the version from its single source" (Debrief 1.0.0), `check-tag.mjs` exists because tags and versions drifted; ER-13 | 1 | after | P |
| 14 | **Tool version pin and skew check, static completions**: `devkit.toml` names the required `devkit`, `wiki` and `heavy` ranges; a binary outside the range stops with the install command (never a download, per the trust rule); `devkit completions <shell>` from `clap_complete`'s static generator | mise tool versions ([tasks](https://mise.jdx.dev/tasks/)); clap_complete ([docs](https://docs.rs/clap_complete/latest/clap_complete/env/index.html)) | Agents on different worktrees run different tool builds; the one-release overlap of G.3 makes skew likely | 0.75 (with G2) | now | G |
| 15 | **Per-agent resource ledger**: `heavy` records who held a slot (worktree, agent label), for how long and the peak load; `heavy status --by-agent` and `devkit worktree disk` give the coordinator CPU-minutes, disk and CI dispatches per agent | – | Rule 41/ER-24 is enforced by watching `uptime`; the 2026-10-29 load of 55 to 130 with five Rust agents (memory) | 1.5 | later | G |
| 16 | **Local gate stamps**: after a green local gate, `devkit run --stamp <inputs>` records the content hash of its inputs per machine; the same gate on the same inputs (another worktree after a merge, a re-run) prints "unchanged since green at …" and skips unless `--force`. Never used in CI | moon input hashing ([cache](https://moonrepo.dev/docs/concepts/cache)); mise task sources ([tasks](https://mise.jdx.dev/tasks/)); Paddock's own `ui/dist/.source-hash` stamp | Coordinator re-runs of affected gates after merges (memory "test once after merges"); ER-8 still runs the full suite once | 2 | later | G |

Total 23.0 agent-days (estimate; the 16 rows sum to 23.0); the eight "now" items (1 to 7 and 14) about 9.5.

## Rejected or not needed

| Feature or tool | Why not |
|---|---|
| Nx, Turborepo, moon, Bazel, Buck2, Pants as the runner | Decided in [2026-10-nx-vs-scripts.md](2026-10-nx-vs-scripts.md) and ADR 0014 §7; nothing new since (moon stays the fallback) |
| just, cargo-make, mise tasks as a task layer | A second task language beside `./dev` and `devkit`; ADR 0022 chose one Rust tool. `just` would only rename the commands |
| Remote or shared task-result cache | The cache server is writable by every job (nx note §7); job-level last green (WP-A1) gets the saving from Gitea's own records |
| Pinned toolchains (`rust-toolchain.toml`, mise pins) | Decided against on 2026-10-28 (ADR 0022 §3.3: floating stable, compiler in the input hash). `doctor` reports versions instead |
| Trunk Flaky Tests, BuildPulse | SaaS with uploads of test names and results; no Gitea support named ([BuildPulse](https://buildpulse.io/) lists GitHub Actions, CircleCI, Travis, Semaphore); paid ($99/month analytics); against "local first". Candidate 8 does the useful part from our own JUnit |
| Automatic quarantine of flaky tests | ER-12 and rule 5: never silently skip; at most one retry that keeps the run red |
| prek or pre-commit instead of lefthook | lefthook works and is in every template; prek (MIT, Rust, [prek](https://github.com/j178/prek)) is worth it only if Python-ecosystem hooks arrive |
| git-branchless | README says alpha ([git-branchless](https://github.com/arxanas/git-branchless)); its interactive history tools do not fit agents; `git test` overlaps G5 |
| git-absorb as a devkit feature | Useful as is (BSD-3-Clause, [git-absorb](https://github.com/tummychow/git-absorb)) for folding review fixes; at most a line in `doctor`'s optional tools, no wrapper |
| Self-update that downloads | Breaks G.4's trust rule (nothing fetched at run time); candidate 14 refuses and names the install command instead |
| Dynamic shell completions | `clap_complete`'s `CompleteEnv` is behind the `unstable-dynamic` feature; static completions are enough |
| A CI web dashboard | Gitea's UI plus job summaries (if the runners are on Runner 2.0) and candidate 2's digest cover it without a server to run |
| LLM "self-healing" CI fixes | ER-10 wants one deliberate local fix and one push; an automated fix loop multiplies runs (rule 50) |
| Licence checks, changelog generation | Already in place (`cargo deny`, R6 notices, git-cliff) |
| A `cargo mutants` wrapper | Rule 5's review step is one command line; `heavy cargo mutants …` suffices |
| nextest partitioning and archives | Real CI-speed options (nx note §7) but CI layout work, not tool features; keep them in the CI-speed backlog |

## Risks

| Risk | Mitigation |
|---|---|
| Hooks through `heavy` make commits wait for a slot | Formatting stays immediate in `pre-commit`; only compiling steps wait, and the wait is logged as a phase |
| Triage re-run commands drift from how CI runs tests | Generate them from the same plan and `paddock.ci.json` the CI jobs use; golden tests on recorded runs |
| `retries = 1` hides a real failure | `flaky-result = "fail"` keeps the run red (ER-12); ledger tests assert that |
| Stall detection kills a slow but healthy job | Judged on output and CPU progress with a patience setting (ADR 0031), never on elapsed time; dump first, then end |
| nextest record and replay is experimental | Candidate 2 works from JUnit alone; the recording is an optional extra |
| Generic tools absorb Paddock knowledge | ADR 0022 G.8's "no Paddock in generic tools" check covers every new command; Paddock's parts are packs |

## Work packages (if accepted; add to ADR 0022's G list)

| WP | Content | Depends on | Days (est.) |
|---|---|---|---|
| G13 | Hooks through `heavy` (root and template `lefthook.yml`), clippy to `pre-push` per affected workspace | `heavy` (G0b) | 0.5 |
| G14 | `devkit ci triage` with JUnit parsing and re-run commands; test formats as a pack | G3, G9 | 2.5 |
| G15 | `WorktreeCreate`/`WorktreeRemove` hook commands and the worktree manifest | G10 | 1 |
| G16 | `doctor` engine and generic pack; Paddock pack in `./dev doctor` | G1, G2 | 1.5 |
| G17 | `devkit ci dispatch` and `ci trim` | G9 | 1 |
| G18 | Secret scan: pinned scanner through `devkit fetch`, hook, CI step, Paddock patterns | R3, decided scanner (betterleaks v1) | 0.75 |
| R17 | `./dev check --pre-push` from the plan | R4 | 1.5 |
| G19 | Tool version range check and static completions | G2 | 0.75 |
| G20 | Flake ledger (CI config change, ingest, report) | G14, decided flake policy (one retry, flaky-as-failure; branch `ci/flaky-is-red`) | 2 |
| G21 | Stall dump in `paddock-proc` and `devkit run` | G3, ADR 0036 WP-3 | 2 |
| G22 | `devkit worktree finish` | G15 | 1.5 |
| G23 | `devkit mcp` on `paddock-mcp`, `devkit schema` | G1 | 2 |
| G24 | Timing history and trends | G9 | 1.5 |
| R18 | `./dev release bump` | R2 | 1 |
| G25 | Per-agent ledger in `heavy` | G0b | 1.5 |
| G26 | Local gate stamps | G1 | 2 |

## Decided 2026-10-31

The user took the three open decisions on 2026-10-31, all as recommended:

1. **Secret scanner: betterleaks v1** (MIT, maintained by gitleaks' authors; gitleaks now gets security patches only).
   Offline; pinned through `devkit fetch` (G18).
2. **Flake detection in CI: one retry with flaky-as-failure.** nextest `ci` profiles get `retries = 1` with
   `flaky-result = "fail"`, Playwright `retries: 1` with `failOnFlakyTests: true` (ER-12). Branch `ci/flaky-is-red`.
   Costs one extra run of a failing test; every flake gets a name in JUnit and the run stays red.
3. **Workspace clippy leaves `pre-commit`** for `pre-push`, through `heavy`, once per affected workspace (G13).
   Commits get faster and lighter; a clippy error is found at push instead of at commit.

## Sources (accessed 2026-10-31)

- ADR 0022 and its amendment: `git show origin/docs/adr-0022-generic-tools:docs/adr/0022-rust-developer-and-ci-tool.md`
- nextest: retries and `flaky-result` <https://nexte.st/docs/features/retries/>; record, replay, rerun
  <https://nexte.st/docs/features/record-replay-rerun/>; status query <https://nexte.st/docs/reporting/>; changelog
  <https://nexte.st/CHANGELOG/>
- Nx and AI agents: <https://nx.dev/docs/features/enhance-ai>
- Turborepo `run` reference (`--summarize`, `--dry=json`, `--affected`, `--concurrency`): <https://turborepo.dev/docs/reference/run>
- moon cache and hashing: <https://moonrepo.dev/docs/concepts/cache>
- mise tasks <https://mise.jdx.dev/tasks/> and `mise doctor` <https://mise.jdx.dev/cli/doctor.html>
- lefthook configuration: <https://lefthook.dev/configuration/>
- prek: <https://github.com/j178/prek>
- Trunk Flaky Tests: <https://docs.trunk.io/flaky-tests/overview>; BuildPulse: <https://buildpulse.io/>
- git-absorb <https://github.com/tummychow/git-absorb>; git-branchless <https://github.com/arxanas/git-branchless>
- gitleaks <https://github.com/gitleaks/gitleaks>; betterleaks <https://github.com/betterleaks/betterleaks>
- clap_complete dynamic completion: <https://docs.rs/clap_complete/latest/clap_complete/env/index.html>
- Claude Code hooks (`WorktreeCreate`, `WorktreeRemove`): <https://code.claude.com/docs/en/hooks>
- Gitea Actions compared with GitHub Actions: <https://docs.gitea.com/usage/actions/comparison>; job summaries:
  <https://github.com/go-gitea/gitea/issues/23721>, <https://gitea.com/gitea/runner/pulls/917>,
  <https://linuxiac.com/gitea-1-27-released-with-actions-improvements-and-45-cve-fixes/> (secondary, unverified)
