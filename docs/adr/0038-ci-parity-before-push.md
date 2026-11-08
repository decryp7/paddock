# 38. CI parity before push: `./dev check --pre-push` runs CI's own plan for the changed code

- Status: Accepted (2026-11-03), by the user, who approved this redesign on 2026-11-03, after the independent review
  approved it (round 1 and a delta re-review, 2026-11-03). Amended 2026-11-08 (accepted: user-approved redesign and
  independent review): the per-commit gate before landing (`./dev check --commits`), at the end of this file.
- Date: 2026-11-03
- Related: ER-7, ER-9, ER-10 / rules 3 and 4 (affected tests, fix locally first, push once), ER-21 / rule 40, ER-24 /
  rule 41 (machine load, `heavy`), rule 50 (CI economy), rule 55 (ADR 0034, copies); ADR 0014 §6 (affected CI), ADR 0021
  §3 (`xwin clippy` before push), ADR 0022 (the Rust developer tool `./dev`, R4 `dev plan`, R17 `./dev check
  --pre-push`), and its amendment of 2026-10-31 on branch `docs/adr-0022-generic-tools` (Accepted 2026-11-01 there, not yet
  on `main`: generic tools go to `devkit`, Paddock-specific ones to `./dev`, G.2). Also the research note
  `docs/research/2026-10-dev-tooling-features.md` on branch `docs/dev-tooling-features` (feature 7 "pre-push affected
  gate", work package R17; G13 hooks through `heavy`, G17 dispatch guard, G26 local gate stamps).
- Evidence (read at `origin/main` `9da2ccf59` and on the named branches, 2026-11-03):
  - `.gitea/workflows/ci.yml`: the plan job runs `node scripts/affected.mjs --ci`. The root jobs (`ui`, `e2e`,
    `e2e-webkit`, `conformance`, `tools`, `icons`, `eslint-preset`, `crates`, `crate`, `commits`, `paddock-dev`) carry
    their commands inline in YAML. The conformance job's check list (`--only C06,C14,…,C57 --strict C36`, line 255) exists
    only there.
  - The app legs already read their commands from data: `apps/<app>/paddock.ci.json` (`checks`, `ui`, `e2e`, `notices`,
    `windowsClippy`), run by `.gitea/actions/app-gates`.
  - `tools/paddock-dev/src/plan.rs` (772 lines): the port of `affected.mjs`'s core, checked against it by golden cases
    (`tools/paddock-dev/tests/golden/cases.json`). Its header says the CI matrix and `--ci` are R4's, and `affected.mjs`
    stays CI's planner until then.
  - `lefthook.yml` `pre-push`: only Pitwall's `pitwall-scripts` (broken: it names files that do not exist, verified in the
    features note) and `pitwall-deny`.
  - OrbStack on this Mac: the machine `paddock-gate-arm` (Ubuntu noble, arm64) exists and is stopped (`orb list`,
    2026-11-03). CI's Linux image `paddock-ci:v2` is linux/amd64 only (`.gitea/workflows/ci-image.yml:54`), and Chrome
    for Testing in it is amd64 only (`ci-image/README.md`).
  - Linux container copies: `apps/dyno/scripts/linux-container.sh`, `apps/pitwall/scripts/linux-container-verify.sh`, and
    Debrief's documented OrbStack loop (`apps/debrief/docs/development.md:223`).

## Context

Local gates are chosen by hand from CI's steps. Each time a step was left out, CI found it after the push, and the fix was
a second push and a second run (rule 50). Since 2026-10-26:

| Kind | Instances (commits) | Count |
|---|---|---|
| Third-party notices stale after a dependency change | `2706e495d`, `a78de2d86` (Debrief, 10-29), `d59718a2d` (Dyno, 10-30), `353a514a6` (Dyno, 11-01), and others | 39 commits mention notices since 10-26, most of them regenerations |
| Conformance found only in CI or in the merged batch | `e7ab83545` ("conformance of the merged batch", 10-30), `15edd3c78` (C39 waivers, fmt, 10-31) | the conformance job's `--only` list exists only in YAML |
| Formatting missed across apps or crates | `402de329b` ("the files the widget and login branches left unformatted", 10-31), `fda51f6c7`, `2fcb46418`, `f73a3d286`, `78cdb999a`, `f3e5827e1` ("its last CI failed on Format") | about 20 rustfmt-only commits from 10-28 to 11-01 |
| Platform-only clippy: Linux | `297a656e1` (shell: dead code on Linux, 10-29), `1f701932b` (proc, 10-27). The coordinator also reports paddock-chromium and Debrief's chromium-driver; I found no single commit for those (unverified) | |
| Platform-only clippy: Windows | `63379e552`, `bf95295f1`, `606d113e1` (10-28) | ADR 0021's `xwin clippy` is documented, but its wrapper script was never written and no hook runs it |
| Gallery overflow (the `--project=gallery` Playwright run) | reported by the coordinator; commit not identified (unverified) | |
| A pre-push hook that fails every push | Pitwall's `pitwall-scripts` (features note, verified there) | |

**Pattern.** A check that CI runs fails after the push whenever the local gate is a hand-picked subset of CI's steps.
CI's steps live in YAML text that no local tool can run, so every new CI step widens the gap without anyone noticing.

**Invariant.** For a commit range, the commands `./dev check --pre-push` runs are the commands CI's plan would run for
that range. The exceptions are declared, each with a reason and, where one exists, a local equivalent. The gate prints the
difference.

## Decision

### 1. CI's commands are data, and ci.yml runs them through `./dev`

Every root job's commands move out of ci.yml into the same kind of contract the apps already have:
- `crates/paddock.ci.json`, `ui/paddock.ci.json`, `conformance/paddock.ci.json`, `tools/…`, `icons/…`, `eslint/…`;
- the conformance `--only` and `--strict` lists become fields of `conformance/paddock.ci.json`.

A job's YAML keeps only checkout, caches, toolchain set-up and `./dev ci run <project> <phase>`. The app legs keep
`paddock.ci.json` as they are, and `app-gates` calls the same `./dev ci run`.

One executor runs both CI and the local gate. It follows the existing rules: one command per line, a `cd` holds for the
lines after it, and each line is logged with its duration and exit code (rule 56 / ER-27).

### 2. One planner for both

`./dev plan --json` (R4, ADR 0022) emits CI's matrix. The plan job calls it, and so does `--pre-push`. Until R4 lands,
both call `node scripts/affected.mjs --ci`, which prints the JSON outputs. `plan.rs` reaches R4 through its existing golden
oracle, so the switch changes no plan. `--pre-push` asks for the range `merge-base(origin/main, HEAD)..HEAD` plus dirty
files (`--files` mode), the same base rule CI's branch runs use (rule 50 (e)).

### 3. `./dev check --pre-push` (R17)

It expands the plan into (project, phase, OS, commands) and runs each entry where it can run:

| CI leg | Local run |
|---|---|
| macOS | natively |
| Linux (ubuntu-latest in `paddock-ci:v2`) | in the OrbStack machine `paddock-gate-arm`, through `orb -m paddock-gate-arm`, with the worktree mounted at the same path and a per-worktree `CARGO_TARGET_DIR` inside the machine (rule 2), under `heavy` |
| Windows | `./dev clippy --target windows`, a new Rust subcommand that runs `cargo xwin clippy` for the plan's Windows clippy lines (ADR 0021 §3 named a `scripts/xwin-clippy.sh` that was never written, and ADR 0022 G.10 decision 9 allows no new shell tooling); tests and bundles are declared `ci-only: "no Windows test host (2026-10-27)"` |
| Linux steps that need the amd64 image (real-browser tests that need Chrome for Testing) | declared `ci-only: "Chrome for Testing is amd64 only"` (user decision D2: the Linux leg is native arm64) |
| WebKit e2e, bundles, release legs | declared `ci-only` with their reasons (rule 3: Chromium only in a branch) |
| The gallery Playwright run (`--project=gallery`, including its overflow checks) | in the Linux leg in `paddock-gate-arm`, not on the Mac: text widths depend on the installed fonts, so the check is only equal to CI's on Linux. The provisioning installs the image's font packages. The ARM Playwright Chromium is assumed to lay out like CI's amd64 build; WP-3 verifies that on the gallery's goldens, and if it does not hold, the run is declared `ci-only: "font or engine metrics differ on arm64"` |

The output is one table: what ran (with durations), what was skipped (with the declared reason), and what failed, with the
exact re-run command. The exit code is non-zero on any failure. Every heavy line runs through `heavy` with
`CARGO_BUILD_JOBS=2` (ER-24). `--only fmt,clippy` exists for quick iterations, but the hook always runs the full gate.

`--pre-push` is Paddock-specific (it reads `paddock.ci.json`, the apps and the CI image), so it is a `./dev` command (G.2
class "Paddock"). The generic pieces it uses (`heavy`; the G26 stamps, `devkit run --stamp`) stay in the generic tools.

### 4. Linux parity in `paddock-gate-arm`

The tool versions have no single source today:
- Rust, cargo-nextest, cargo-llvm-cov and cargo-deny come from the garage base image
  (`ci-image/Dockerfile`: `GARAGE_IMAGE=…garage:ubuntu-24.04-20261023-r5`);
- cargo-hakari, cargo-machete and Chrome for Testing are set in `ci-image/Dockerfile` itself;
- cargo-about is pinned in `.gitea/actions/app-gates/action.yml:76` (`cargo-about@0.9.2`, commit `256512cae`);
- Node comes from `.nvmrc`.

This ADR introduces one versions file, `ci/tool-versions.toml`. Every tool the gates run gets its version and checksum
there. The CI image build, the actions that install tools, the `paddock-gate-arm` provisioning and `./dev doctor` all read
it. A test fails when a Dockerfile `ARG`, an action input or a hook names a version not taken from the file. The garage
image tag is an entry too, and the tools it brings are listed under it, so a garage update is a change of that file.

Provisioning is a `./dev` Rust subcommand, `./dev gate provision linux`, not a shell script (ADR 0022 G.10 decision 9:
the bootstrap is Rust after the minimal launcher). It installs into the machine the versions from that file: the Rust
toolchain resolution, cargo-nextest, cargo-hakari, cargo-machete, cargo-deny, cargo-about, Node, the Tauri Linux
libraries and the image's font packages.

`./dev doctor` (G16) reports a machine whose versions differ. The app copies of the Linux container scripts (Dyno,
Pitwall, Debrief's notes) are listed in `docs/known-copies.toml` and deleted by WP-4 (rule 55).

### 5. Hooks and the push

lefthook's `pre-push` becomes one command, `./dev check --pre-push`, and workspace clippy leaves `pre-commit` (G13, decided
2026-10-31). The broken Pitwall entries are deleted.

After a green gate, a G26 stamp records the head and the input hash. The coordinator's dispatch (`devkit ci dispatch`,
G17) checks for that stamp. This ties the gate to rule 50: no CI run for a head that has not passed locally. The user
decided (2026-11-03) that the guard **warns first**. In the first phase it dispatches and prints a warning naming the head
and the missing stamp, and the warnings are counted. Once WP-1 to WP-5 are in use and the count has been reviewed, it
**refuses** a head without a green stamp, unless `--no-stamp <reason>` is given. The switch is one setting in
`devkit.toml`, changed by the coordinator with the user's approval.

### 6. Parallelism and cost (rule 51)

A shared-crate change reaches every app, so a full pre-push can be several workspaces of clippy and tests. Bounded:
- workspaces run one at a time per agent through `heavy`'s slots, and `heavy` bounds the machine (ER-24);
- fmt, conformance and the notices check are cheap and run first, so the common failures surface in seconds;
- tests follow the plan's nextest narrowing (R4), not whole workspaces;
- the Linux leg runs in parallel with the macOS leg only when `heavy` has two free slots;
- a stamp skips a gate whose inputs are unchanged since it was green (another worktree after a merge).

The first implementation records its run times in `docs/benchmarks.md` (rule 48) before anyone quotes a number.

## Checks that make the class detected

| Check | What it enumerates | Fails today on |
|---|---|---|
| Workflow step allow-list (extends the ci-actions test) | every `run:` in `ci.yml`, `<app>.yml` and the shared actions: only set-up steps from a list, or `./dev ci run …` | every inline root job step (conformance list, fmt, hakari, coverage, gallery) |
| Parity golden in `paddock-dev` | for each golden case: the commands `--pre-push --dry-run` selects equal the commands CI's expansion selects, minus declared `ci-only` entries | any step CI runs and the gate does not |
| `ci-only` entries need a reason | contract schema | a silent exclusion |
| `./dev doctor` | the hook is installed, `paddock-gate-arm` exists and its versions match the image | a missing hook or a stale machine |
| Dispatch refusal | the stamp per head | a push that skipped the gate |

## Rollout and order

1. **WP-1:** the executor `./dev ci run` with logging, and `paddock.ci.json` for `crates/` and `conformance/` (where the
   misses were). Their ci.yml jobs call it.
2. **WP-2:** `--pre-push` over `affected.mjs --ci` (R17), the table output, the macOS leg, and `./dev clippy --target windows`.
3. **WP-3:** `ci/tool-versions.toml` and its test; `./dev gate provision linux`; the Linux leg in `paddock-gate-arm`, the gallery on arm64; `doctor` checks.
4. **WP-4:** the remaining root jobs (`ui` with the gallery, `e2e`, `tools`, `icons`, `eslint-preset`, `commits`,
   `paddock-dev`) as data; the allow-list test turns strict; the app Linux scripts are deleted.
5. **WP-5:** the parity golden; the hooks (G13) and the Pitwall fix.
6. **WP-6:** the stamp and the dispatch refusal (subsets of G26 and G17).
7. When R4 lands, the planner switches to `./dev plan --json`, and the golden proves nothing changed.

These are tooling waves, inside ADR 0022 G.7's cap of two tooling agents during a release. WP-1 and WP-2 come first
because they cover four of the five triggers.

**Rollback.** ci.yml jobs can go back to inline commands one job at a time. The hook can be switched off without touching
CI.

## Costs (agent-days, estimates)

| WP | Days |
|---|---|
| WP-1 executor, two root contracts | 2 |
| WP-2 `--pre-push` (R17, 1.5 in the features note), report, macOS leg, and the new `./dev clippy --target windows` subcommand (0.5) | 2.5 |
| WP-3 `ci/tool-versions.toml` with its consistency test (1), `./dev gate provision linux` in Rust, the Linux leg, the gallery check on arm64 | 3.5 |
| WP-4 remaining root jobs as data, allow-list strict, copies deleted | 1.5 |
| WP-5 parity golden, hooks | 1 |
| WP-6 stamp and dispatch refusal | 1 |
| **Total (new work)** | **11.5** |
| R4 `./dev plan --json` (already planned in ADR 0022, 2 days; needed only for step 7) | (2) |

## Consequences

- A push costs local minutes instead of a CI run. Shared-crate changes cost the most, which is the same work CI does
  anyway, and it is bounded by `heavy`.
- CI and local runs share one executor, so the logs read the same in both places.
- Adding a CI step means adding a contract line, and the gate gets it automatically. Adding inline YAML fails the
  allow-list test.
- The arm64 machine is not CI's amd64. Architecture-specific code (`target_arch`) and amd64-only tools remain CI-only, and
  are declared as such. `target_os = "linux"` code, which is where all the misses were, is covered.
- Windows tests stay CI-only (no Windows test host, user 2026-10-27). Windows clippy moves from optional to the gate.

## Alternatives considered

- **Run the Gitea workflow locally** (`act` or `act_runner exec`). This reproduces YAML, including set-up that does not
  apply to the Mac, and it cannot run macOS or Windows legs. It still leaves the commands in YAML. Rejected; the data
  contract makes this unnecessary.
- **A hand-written pre-push list per app.** This is today's state, and it drifts (Pitwall's hook). Rejected.
- **Only `--only fmt,clippy` before push.** It is cheaper, but notices, conformance and the gallery were among the misses.
  Rejected as the default; kept as a flag.
- **A Docker container from `paddock-ci:v2` under Rosetta instead of `paddock-gate-arm`.** This has exact parity with CI,
  including Chrome for Testing, but it is much slower under emulation. Rejected by the user's decision D2 (2026-11-03).

## Decisions taken (user, 2026-11-03)

- **D1: warn first, refuse later.** The dispatch guard warns about a head without a green stamp, and refuses such heads
  once the gate is in use and its warning count has been reviewed (§5).
- **D2: native arm64.** The Linux leg runs natively on arm64 in the OrbStack machine `paddock-gate-arm`. Steps that need
  the amd64 image stay CI-only and are declared as such.

## Amendment 2026-11-08: every commit of a landing passes its fast gate (`./dev check --commits`)

- Status: Accepted (2026-11-08), revision 3. The user approved the ER-21 redesigns on 2026-11-08 ("do all redesigns
  now"); the coordinator asked for this pattern to be written as an amendment here, because it is `./dev check` run once
  per commit. Independent review with ADR 0060: revision 1 (`69ef3c161`) CHANGES NEEDED (the cost on one Mac); revision 2
  (`16657fcc2`) fixed it, and the delta review raised the nightly backstop (a fixed window, an overstated invariant);
  revision 3 answers that and its P2s. Its sibling, ADR 0060 (every check runs somewhere), is the other half of the same
  CI-hygiene batch.
- Related: ER-4 (only complete work is committed; half-done commits break bisecting), ER-21 / rule 40, ER-24 / rule 41
  (machine load, `heavy`), ER-31 (d) (landing is a merge, never a squash), ER-33 / rule 71 (`not measured` is not a pass),
  rule 50's pre-landing commit lint (the integrator lints `origin/main..<branch>` before landing; on
  `ci/commit-lint-first-parent` `598188283` and `debrief/1.2-batch1`, not yet on `main`, kept by the user on 2026-11-08,
  log 1350; written there as (f), which ADR 0049 already gives to the push check, so its letter is being moved and this
  text cites it as "rule 50 (pre-landing commit lint)" until the letter is confirmed), ADR 0022 R0 (`./dev ci slot`,
  `paddock-slots`), ADR 0042 (batch caps: 60 commits, 200 files, 72 hours; the `Batch-size-waived:` trailer), ADR 0049
  (small merges, no squash copies), ADR 0060.
- State of this ADR on 2026-11-08: Accepted but not started. `./dev` has `self hash`, `ci slot`, `wait` and
  `graph affected`; there is no `./dev ci run`, no `./dev check`, and no `paddock.ci.json` outside `apps/`
  (`tools/paddock-dev/src/cli.rs`, `ls crates/paddock.ci.json conformance/paddock.ci.json`).

### Evidence

Four branches had commits that are red on their own on one day, each found by a reviewer reading the diff, not by a tool
(coordinator log `/Volumes/d7sst7/dev-targets/handoff/2026-10-29-overnight.md`, "log"):

| Branch, commit | Why it is red alone | Source |
|---|---|---|
| `pitwall/1.1-c99`, `617825f1e` (C99 layers 3 and 4) | its test needs the Clippy preset that the next commit (`085d6533f`) adds | log 1292, 1295 ("red first commit"), 1300 |
| `pitwall/1.1-b6b` (shared updater wiring, the per-machine include; `3b37f0e01`, `f6b013af1`, `743df59d4`) | a sync test is red at an intermediate commit; the log records it as the "b6b sync-test gap" without naming the commit | log 1297, 1300 |
| `pitwall/1.1-c99b`, `143175a11` (the c99 job and planner entries: ci.yml, app-release.yml, `affected.mjs`, `plan.rs`, golden cases) | red alone, found in review round 3; the log does not record the failing test | log 1300, 1301 |
| `483162cff` (WP-4b, `paddock-assistant` bounded history; `483162cff..cd05b02c3`) | red alone, found in re-review; to be recomposed green per commit before landing | log 1312 ("4th red-intermediate instance today") |

**Pattern.** A change is split into commits for review, but only the branch head is ever tested: the author's local runs,
ADR 0038's pre-push gate and CI's branch run all look at the head. Landing by merge (ER-31 (d), no squash) puts every
intermediate commit on `main`'s history, where `git bisect`, a revert or a cherry-pick onto a release line reaches it.

**Invariant.** Before a branch lands, every commit in `origin/main..<branch>` passes the fast gate (the `commit` phase
below) of the projects its own change reaches, with the app compiles a shared-crate commit reaches checked for the
changed apps plus a sample (A1); and every commit that lands is checked in full, without sampling or stamps, by the
nightly audit within N nights of landing, N set from the Linux runner's measured capacity (A3, WP-B5). Until it has been
checked in full, a sampled-out app compile is `not measured`, not green.

### A1. A `commit` phase in every project contract

§1 makes each project's CI commands data in a `paddock.ci.json`. Each contract gets one more phase, `commit`: the fast
subset that must hold at every commit, with a declared budget in seconds (warm build folder), `commitBudget`.

**Prepare is part of the phase.** Before a project's `commit` lines run, the gate runs what they depend on, only when
its inputs changed since the commit last run in that gate worktree: `npm ci` when `package-lock.json` changed;
`npm run build -w ui` when an Angular project is planned and `ui/` sources changed (the `build-guard.ts` content stamp
says whether `ui/dist` is current); the app's `prepare` list (for example `cargo fetch --locked`) when its `Cargo.lock`
changed. Prepare time counts against the commit's limit, and the prepare inputs are part of the stamp key (A2.6).

| Project kind | `commit` phase | `commitBudget` (warm, target) |
|---|---|---|
| Shared Rust crates (`crates/`) | `rustfmt --check` on the commit's changed `.rs`; `cargo clippy --locked --all-targets -p <the plan's packages> -- -D warnings`; `cargo nextest run --locked -p <the plan's packages>` (default profile: no coverage, no doc tests, no ignored tests) | 240 s |
| An app's Rust workspace reached only through a shared crate | `cargo check --locked --all-targets` of that workspace (compile only: an API break is the common red-alone cause); its tests run at the head. Subject to the reach limit below | 120 s per app |
| An app's own Rust (`apps/<app>`) | as for shared crates, over the app workspace's planned packages | 240 s |
| Angular projects (`ui`, `apps/<app>/app`) | ESLint on the changed files; the TypeScript compile of the app and its specs (`tsc --noEmit -p` both configs); the unit tests without coverage (`ng test --watch=false`), or the tests related to the changed files where the measured full run exceeds the budget | 180 s |
| Node tooling (`conformance`, `scripts`, `icons`, `eslint`) | `node --test` over the test files beside the changed files, and the conformance checks whose own sources changed (`--only`) | 60 s |
| `tools/paddock-dev` | clippy and nextest of the tool, including the golden cases | 180 s |
| CI files (`.gitea/**`, a `paddock.ci.json`) | `scripts/affected.test.mjs`, `conformance/test/ci-actions.test.mjs`, `./dev gates check` (ADR 0060) | 60 s |
| Docs only | the commit's own commit lint, C24, C31, the docs part of `./dev gates check` | 15 s |

Never in the `commit` phase (head only, through §3 and CI): e2e, WebKit, the gallery, coverage, notices, `deny` and
`audit` (they read the world), bundles and installers, fuzz, mutants, benches, VM rows, the Linux and Windows legs.

**The budgets must fit under the per-commit limit, and a test proves it.** A contract test computes, from every
contract's `commitBudget` and `prepare`, the worst commit the planner can produce, and fails when it exceeds the
per-commit limit (A2.5, 15 minutes). Today it would fail: a change to a widely used shared crate reaches seven app
workspaces, so 240 s + 7 × 120 s = 1,080 s, over 900 s, before any prepare. The rule that makes it fit, rather than
raising the limit:
- **Reach limit.** Within one commit, the reached-app compile step runs for at most `reach_max` apps (in the contract of
  `crates/`, 4 to start, which gives 240 + 4 × 120 = 720 s and leaves 180 s for prepare). The apps chosen are, in order,
  those the same commit also changes, then a sample chosen deterministically from the commit id.
- **Every reached app at the last touch.** At the last commit of the range that touches the crate, and at the head,
  every reached app is compiled, without the reach limit, as an extra step allowed past the per-commit limit only
  through its own bound (`reach_last_max`, 25 minutes).
- The report marks apps left out by the sample as `not measured (sampled)` for that commit, and the trailer counts them
  (A3). The nightly audit (A3) runs without sampling, so a red intermediate commit that the sample missed is still found,
  within its backlog bound.
- **macOS-only code in a sampled-out app** (`target_os = "macos"` code paths) is unchecked at that commit until the
  head's gate, and the Linux nightly audit cannot compile it. So the gate runs the sampled-out compiles afterwards on the
  Mac, in an idle window: when a pool worktree is free and `heavy` reports spare capacity, it works through the
  sampled-out (commit, app) pairs of recent landings, newest first, and records them under the same stamps; a pair still
  unchecked when the audit has passed its commit is reported in the audit's summary as `macOS not measured`.

The budgets are targets until WP-B5 measures them on the four instances and records the numbers in `docs/benchmarks.md`
(rule 48); a project whose measured `commit` phase exceeds its budget narrows the phase in its contract (for example,
related tests only) rather than raising the budget.

### A2. `./dev check --commits <range>`

1. **Range.** `git rev-list --reverse --topo-order <range>` (default `origin/main..HEAD`): every commit `main` will get,
   merge commits included, because bisect can stop on any of them.
2. **Gate worktrees: a pool, not one per branch.** The gate uses a fixed pool of detached worktrees,
   `$PADDOCK_GATE_ROOT/g<N>` (agents: `/Volumes/d7sst7/dev-targets/wt/gate/g<N>`), one per agent slot, leased through
   the same mechanism as `./dev ci slot` (`paddock-slots`: a held lease, released when the holder ends; rule 53). Any
   branch can use any free worktree; each keeps its per-app `CARGO_TARGET_DIR`s and `node_modules` between runs, so disk
   use is bounded by the pool size (ER-24 sets it: about 5 on this Mac) and builds stay incremental across branches,
   which share most of their tree with `main`. Never the author's checkout (rule 2). **Disk budget:** each pool worktree's
   build folders stay under a cap (`gate_max_gb`, 40 GB to start, set from WP-B5's measurement), kept by the existing
   prune mechanism (`tools/prune-build-folder`, the one the `prune-build-folder` action runs on CI hosts), run after each
   gate; a pool worktree unused for 14 days is removed (`git worktree remove`) and re-created on demand.
3. **Base.** The `commit` phase runs first at the merge base for the projects the whole range reaches. A project that is
   red there is reported as "red on main" and the gate stops with exit 4 (refused): the branch is not to blame, and main
   is fixed first, or the integrator passes `--base-red-ok <project>`, which the trailer records.
4. **Per commit.** For an ordinary commit, the plan is §2's planner for `c^1..c`. **For a merge of `main` into the
   branch** (a merge whose second parent is reachable from `origin/main`), what `main` brought was gated on `main`; the
   plan is the projects the branch side reaches (`merge-base(c^1, c^2)..c^1`), joined with the projects of the files the
   merge resolved differently from both parents (the conflict resolutions). Any other merge (a feature branch merged into
   an integration branch) is planned as `c^1..c`. Each planned project runs its prepare and `commit` lines through §1's
   executor, each line logged with duration and exit code (ER-27).
5. **Bounds.** Each commit has a hard limit of 15 minutes (with the reach rule of A1); the range has one of 60 minutes,
   **provisional**: until WP-B5 has measured real ranges and recorded them, the range limit only warns ("this range would
   exceed the provisional limit") and the gate runs to the end; WP-B5 sets the enforced value from the measurements. At an
   enforced limit the gate stops, reports the commits not run as `not measured` (ER-33), and exits non-zero; it does not
   pass. `--resume` continues where it stopped.
6. **Load.** Every build and test line runs through `heavy` (the plock lane, `CARGO_BUILD_JOBS=2`), so a gate never
   competes with a release or with more agents than the machine allows (ER-24). Waiting on a gate uses `./dev wait`.
7. **Stamps.** A green commit leaves a stamp keyed by: the git tree id, the hash of the commands run (prepare and
   `commit` lines after expansion), the toolchain, the OS and architecture, the hash of `package-lock.json`, and the
   prepare outputs' stamps (`ui/dist`'s content stamp). They live in the stamp store of §5 (G26). A commit whose stamp
   exists is not run again: the author's run makes the integrator's free, and a recomposed branch pays only for its new
   trees.
8. **Report.** One table: commit, subject, projects, result, duration, sampled-out apps; the first red commit with its
   failing line and the exact command to re-run it at that commit. On success it prints the trailer for the landing
   merge: `Commit-gate: <n> commits green, <k> app compiles not measured (sampled), tip <tip abbrev>`.

A red intermediate commit is fixed by recomposing the branch under a new name (as `c99b`, `c99c` were), with the fix
folded into the commit that needs it; never by force-pushing a reviewed branch, and never by squashing at landing.

### A3. Where it runs and how it is enforced

- **Author:** before handing a branch to review, `./dev check --commits origin/main..HEAD` (the stamps carry over).
- **Integrator:** rule 50 (pre-landing commit lint) gains a second step beside the lint: `./dev check --commits
  origin/main..<branch>` before every landing on `main`, and the merge commit carries its `Commit-gate:` trailer, or
  `Commit-gate-waived: <reason>; approved: <coordinator|user>`.
- **CI:** the first-parent commit lint on `main` (C23) requires one of those two trailers on every merge commit `M` that
  lands a branch, and checks the trailer against the history: `<n>` must equal `git rev-list --count M^1..M^2` and the
  tip must be `M^2`. The result itself is the tool's claim, not proven by CI, but a landing without the gate, or with the
  gate run on a different range, becomes visible.
- **C23** also checks the trailer's form, `<n> commits green, <k> app compiles not measured (sampled), tip <abbrev>`.
- **Nightly audit, from a watermark.** A job in `nightly.yml` keeps a watermark: the last first-parent merge of `main`
  it has audited, saved in the CI cache store under a fixed key (and printed in the job summary, so a lost cache can be
  restored by hand; with no watermark it starts from the newest tag's merge). Each night it walks the first-parent
  merges after the watermark, oldest first, and runs `./dev check --commits --no-stamps --no-sample <M>^1..<M>` for each.
  It ignores stamps (it is the independent check of the landing's claim) and the reach sample. It stops at a nightly
  commit bound (60 to start, set from WP-B5's measured capacity of the Linux runner), advances the watermark past the
  last merge it finished, and **carries the rest over** to the next night. The job is red with one `::error::` per red
  commit (commit, project, line), and **red while the backlog is above zero**, with the backlog's size and oldest landing
  in the summary, so a capacity shortfall shows as a red nightly instead of silently unchecked commits.
- **macOS and Linux.** The `commit` phase is the same on both. The landing gate runs on the developer Mac (where the
  integrator works); the nightly audit runs on the Linux runner, so between them each commit is checked once on each
  OS. A commit that the audit finds red only on Linux cannot be fixed on `main`'s history (no rewrites); it is fixed
  forward, and the audit's report names it so a later bisect can `git bisect skip` it.
- **The bisect-skip trade-off.** A red commit that reaches `main` stays in its history: `git bisect skip` steps over it,
  but if the change being hunted is in that commit or next to it, bisect can only narrow it to the skipped span, and it
  says so. The landing gate makes that rare; the audit makes it known (a list of known-red commits in its summary, fed to
  the bisect scripts as skips); neither can undo it.
- `git bisect` scripts keep bisecting every commit; `--first-parent` is not a substitute (A7).

### A4. Parallelism and cost (rule 51)

- **Threads.** Commits run one after another in one leased worktree: the cheapest order, because each build is
  incremental on its parent. Parallel worktrees per commit were rejected: each starts cold and costs a full build of
  every reached workspace. Different branches' gates run in parallel only as far as the pool and `heavy` allow; inside
  one commit, projects share `heavy`'s slots.
- **Cheaper.** A commit runs only its own plan, not the head's; a merge of `main` runs only the branch side and the
  resolutions; a commit that touches only docs runs the docs line; a shared-crate commit compiles reached apps within the
  reach limit instead of testing them (the rest later, in idle windows); prepare runs only when its inputs changed; a stamp skips an unchanged tree; the
  base run is shared by the whole range; the pool keeps build folders warm across branches. The usual landing (3 to 47
  commits, ADR 0042) should cost about the head gate plus one incremental build and the narrowed tests per commit;
  WP-B5 measures it before anyone quotes a number or the range limit is enforced.

### A5. Work packages and estimates (agent-days)

| WP | Content | Days |
|---|---|---|
| (dependency) | §1's WP-1: the executor `./dev ci run` and the contracts of `crates/` and `conformance/` | (2, already counted above) |
| WP-B1 | the `commit` phase, `commitBudget`, `reach_max` and `reach_last_max` in the contract schema; values for every app contract and for the root contracts (created where §1 has not yet moved the job; only the phase is filled until WP-4 moves the job); the contract test that every contract declares them and that the worst commit fits under the per-commit limit | 2 |
| WP-B2 | `./dev check --commits`: range walk, the leased worktree pool, prepare with change detection, per-commit plan including the merge-of-`main` rule and the reach sample, base run, limits (the range limit warning only), `heavy`, report, `--resume`; the stamp store with the full key if §5's WP-6 has not built it (shared, one implementation) | 4 |
| WP-B3 | the trailers: printed by the tool; C23 requires one on a landing merge and checks count and tip; the rule 50 (pre-landing commit lint) text and the `integrate` skill | 1 |
| WP-B4 | the nightly audit job (`--no-stamps --no-sample`) with its watermark, carry-over and red-while-backlog, the known-red list for bisect; the idle-window macOS compiles of sampled-out apps | 1.5 |
| WP-B5 | replay the instances: the gate must fail at `617825f1e`, `143175a11` and `483162cff` and pass on the recomposed branches; for b6b, whose red commit the log does not name, the replay runs the gate over `pitwall/1.1-b6b`'s range and records which commit it finds (or that it finds none, and why). Measure the per-project budgets, prepare times, typical ranges, the pool's disk use and the Linux runner's nightly capacity (commits per night at the audit's settings); record them in `docs/benchmarks.md`; set the enforced range limit, `gate_max_gb`, the nightly commit bound and N from them | 1.5 |
| **Total (new work)** | | **10** |

Revision 1 estimated 7.5 days; the worktree pool, prepare, the merge-of-`main` rule, the reach rule with its test and the
stricter trailer check added 2 in revision 2; the audit's watermark and carry-over add 0.5 in revision 3.

Order: WP-B1 needs §1's WP-1; WP-B2 after WP-B1; WP-B3 and WP-B4 after WP-B2; WP-B5 last. Until WP-B2 exists the
integrator can approximate it by hand (checking out each commit of the range in a separate worktree and running the
affected tests), which is what found the four instances.

### A6. Checks that make the class detected

| Check | What it enumerates | Fails today on |
|---|---|---|
| `./dev check --commits` at landing | every commit of the landing range | `617825f1e`, `143175a11`, `483162cff`; the b6b commit once WP-B5 names it |
| C23 landing trailer | every merge into `main` on the first-parent line: the trailer present, its count and tip equal to `M^1..M^2` | every landing merge (no trailer exists yet) |
| Contract budget test | every contract's `commitBudget`, `prepare` and reach settings against the per-commit limit | today's budgets without the reach rule (1,080 s > 900 s) |
| Nightly audit | every commit landed after its watermark, without stamps or sampling; red while the backlog is above zero | any red intermediate commit that escaped the landing gate or its sample, within N nights |

### A7. Alternatives considered

- **Squash at landing.** Every commit on `main` would be tested, but ER-31 (d) forbids it for release landings (tags must
  be ancestors of `main`), ADR 0049 rejected squash copies, and reviewed commits would lose their ids. Rejected.
- **`git bisect --first-parent`** (git 2.29 and later). Bisect then visits only `main`'s merges, which CI tested. It stops
  at the landing merge, and bisecting inside that branch meets the same red commits; reverts and cherry-picks of single
  commits are not helped. Kept as a manual fallback, not a fix.
- **`git rebase --exec` on the branch.** It runs a command at each commit, but in the author's checkout, it stops in
  the middle of a rebase on failure, and it can rewrite commits. The leased gate worktree does the same without
  touching the branch. Rejected.
- **One gate worktree per branch.** Revision 1's design; on one Mac it multiplies cold build folders by the number of open
  branches. Replaced by the pool (A2.2).
- **Raise the per-commit limit to fit a shared crate's whole reach.** 1,080 s plus prepare per commit, for every commit of
  a shared-crate branch, is most of the range limit for a few commits. Rejected for the reach rule plus the unsampled
  nightly audit.
- **Run the full pre-push gate per commit.** Exact, but each commit would pay the Linux leg, e2e and Windows clippy:
  hours for a normal landing. Rejected; the head still gets the full gate.
- **Leave it to review.** Today's state; reviewers found all four, each costing a review round. Rejected.
