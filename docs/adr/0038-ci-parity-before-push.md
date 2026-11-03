# 38. CI parity before push: `./dev check --pre-push` runs CI's own plan for the changed code

- Status: Accepted (2026-11-03), by the user, who approved this redesign on 2026-11-03, after the independent review
  approved it (round 1 and a delta re-review, 2026-11-03).
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
