# CI and dev scripts in Rust: the rest of the port, with costed work packages

- Status: plan (low-priority lane), for the user's estimate. Decided by the user on 2026-10-31 and confirmed on 2026-11-12:
  move **all** CI and dev scripts to compiled Rust, `./dev` (`tools/paddock-dev`) for Paddock-specific ones and generic
  standalone tools where general. The port is not limited to the OS-branching steps. ADR 0038's pre-push check
  (`./dev check --pre-push`) is part of it. No new ADR: this note carries out ADR 0022's R1, R2, R3, R5, R6, R7, R8, R10 and
  R17 and ADR 0038's work packages, and puts them in one order with one cost. The short ADR 0022 amendment of 2026-11-12
  points here.
- Date: 2026-11-12. Supersedes this file's first version (`c46349f30`, the YAML-steps plan); its wrapper contract, parity
  method and setup-rust record are kept as §6 to §8.
- Heads read (ADR 0091), all re-read right before committing: `origin/feat/ci-in-rust-setup-rust` at `3ec1e195c` (the base of
  this branch, approved; every count below is measured on it), `origin/main` at `e2a318345`, `origin/docs/rust-tooling-plan` at
  `2791c634c` (the RT-1 to RT-22 plan of 2026-11-10, whose order this note replaces for the CI and dev scripts),
  `origin/docs/adr-0022-generic-tools` at `9e3ae2bea`, `origin/fix/lefthook-pre-push` at `b47998345` and
  `origin/fix/prepush-linux-clippy` at `f37a35f38` (both unmerged; WP-2 salvages them), `origin/tools/dev-fetch-r3` at
  `23e642f00` and `origin/tools/dev-notices` at `dcab304eb` (unmerged; WP-6 and WP-10 salvage them),
  `origin/tools/vm-smoke-unified` at `892b2c29c` (unmerged; WP-14), `origin/ci/app-failed-only-and-schedule-skip` at
  `64441ef30` (another agent's, not landed; it edits debrief.yml, so WP-7 waits for it).
- Related: [ADR 0022](../adr/0022-rust-developer-and-ci-tool.md) (§3.3 bootstrap, §3.6 testing, §3.7 one command at a time
  and the shrink-only baselines, §4 what stays non-Rust, §7 and G.6 the package list, G.7 the release-path guard),
  [ADR 0038](../adr/0038-ci-parity-before-push.md) (the pre-push check and its 2026-11-08 `--commits` amendment), ADR 0025
  (job isolation), ADR 0085 (release isolation), ER-9 (fast gates before dispatch), ER-27, ER-33, ER-37.
- Estimates are raw agent-days for one implementer, as in ADR 0022 §7, plus one review fix round per WP (the user's
  2026-11-11 rule: one independent review, then fix and land). They are estimates, not measurements.

## 1. Where the port stands (checked on the branches named above)

| Piece | State | Evidence |
|---|---|---|
| `./dev plan` (R4, the planner) | on `origin/main`; CI's plan job uses it; `PLAN_PLANNER=node` still falls back to `scripts/affected.mjs` "for one release". No release tag contains it yet (`git tag --contains` of the last `plan.rs` commit on main is empty), so the fallback's release has not happened | `.gitea/workflows/ci.yml:112-120`, `tools/paddock-dev/src/plan*.rs` |
| `./dev ci isolation` (RT-1) | built on `origin/main` (`tools/paddock-dev/src/ci_isolation`), **not switched**: every job still runs `conformance/ci/job-isolation.sh`, which calls `./dev ci slot` for the locks | `.gitea/actions/job-isolation/action.yml:13`, 12 direct `job-isolation.sh path/port` calls in workflows and actions |
| `./dev ci release-isolation` (RT-2) | built on `origin/main`, not called by any workflow yet (`grep -rl release-isolation .gitea` is empty) | `tools/paddock-dev/src/release_isolation` |
| `./dev ci flakes`, `./dev ci timings` (RT-8) | on `origin/main`; `scripts/ci-timings.mjs` (337 lines) still in the tree, no CI user | `tools/paddock-dev/src/ci_runs` |
| `./dev check run` C39, C53 (RT-5) | on `origin/main`; the JS checks `C39-freed-port.mjs` and `C53-pid-kills.mjs` still run in the conformance job | `tools/paddock-dev/src/check` |
| `./dev ci setup-rust` (WP-0) | approved on `origin/feat/ci-in-rust-setup-rust`; release, tag and rule 54 jobs keep the shell copy (§8) | `tools/paddock-dev/src/ci_setup_rust.rs`, `ci_tool.rs` |
| `./dev ci run` (ADR 0038 WP-1); `./dev check --pre-push` (R17, ADR 0038 WP-2) | `./dev ci run`: **built** (WP-3, `tools/paddock-dev/src/ci_run`). `./dev check --pre-push`: **built** on `feat/ci-in-rust-wp2` (`tools/paddock-dev/src/pre_push`): lefthook's `pre-push` is its one command (`--refs-from-stdin`: the pushed commit is checked out in a throw-away worktree and checked there; several refs: one is checked, the others are not measured), the Linux Clippy script is retired, and Linux Clippy is opt-in (`--linux`; CI covers it); `--commits` (X-3) is its declared extension point, not built. It runs ER-9's fast gates and prints the CI gates it does not run (`gates::NOT_RUN`: hakari, machete, deny outside Pitwall, wiki lint, TS lint and type-check, app Linux Clippy, tests). Clippy and the Windows lint run only for workspaces with a changed file of their own (dependents are CI's). Total budget 15 min; a timeout, a missing tool or Docker is "not measured" (exit 6, blocks; exit 7 a broken check beats 5 and 6); one announced, logged override `PADDOCK_ACCEPT_NOT_MEASURED=<full row ids>`. | `tools/paddock-dev/src/pre_push`, `lefthook.yml` |
| `devkit`, `paddock-tool` runtime (G1, G2) | not built. `ci_tool.rs` (`ToolCall`) is the one wrapper until G1 lands | `ls tools/` |

## 2. Inventory (measured on `3ec1e195c`)

How it was measured: one script over the tree. "Lines" are non-blank lines (comments included). "Users" are job definitions
in `.gitea/workflows/*.yml` that name the script or use an action that does, followed through actions that use actions (a
matrix job counts once; a reusable workflow's jobs count in the callee, so the per-app release callers show under
`app-release`). "OS" counts the places that test the OS (`RUNNER_OS`, `runner.os`, `uname`, `OSTYPE`, `process.platform`,
`MINGW`, `MSYS`, `cygpath`, `Windows_NT`, `win32`, `darwin`, and `shell: pwsh|powershell|cmd`); "yes" means the script or step
branches on the OS. **Risk**: High = on the release path (ADR 0022 G.7) or cross-OS with 10 or more jobs; Medium = cross-OS or
5 or more jobs; Low = the rest.

**Totals.** `scripts/` and `conformance/ci` and `conformance/release`: 192 files, 22,962 lines, of which 58 test files (6,254
lines); 74 of the non-test files (8,879 lines) are `scripts/vm-smoke`. Inline shell: 504 lines in 26 actions, 827 lines in 22
workflows. Also counted, because "all scripts" includes them: `conformance/src` 72 files (6,362 lines; 41 check files, 4,104
lines), `conformance/test` 9,516 lines, `tools/*.sh|ps1` 795, `ci-image/*.sh` 129, and under `apps/*/` 153 script files
(14,565 lines), of which 36 (2,653 lines) are run by CI.

| Script or group | Lines | Users (jobs / workflows) | OS-specific | Risk | WP |
|---|---:|---|---|---|---|
| `conformance/ci/build-jobs.sh` (and the `build-jobs` action) | 206 | 37 / 15 (every workflow but the image and token checks) | yes (6) | High | WP-1 |
| `job-isolation.sh`, `job-tmpdir.sh`, `ci-home.sh` (ported, not switched) | 621 | 32 / 8; `job-tmpdir` 17 / 8 | yes (20) | High | WP-1 (switch), WP-9 (delete) |
| `setup-rust` action | 155 | 21 / 6 | yes (14) | High | WP-0 done; release copy WP-11 |
| `tools/linux-clippy-touched.sh`, lefthook `pre-push` | 72 + 12 | local (every push) | Linux container | Medium | WP-2 (done: both replaced by `./dev check --pre-push`) |
| `app-gates` action, `run-commands.sh`, `build-env.mjs`, `bounded.sh` | 52 + 24 + 30 + 35 | `app-gates`: every app leg of ci.yml (1 matrix job); `build-env` 4 / 4 | yes (9) | High | WP-3 |
| ci.yml inline shell (18 jobs, 80 blocks, 1 long) | 158 | 18 / 1 | yes (39) | High | WP-3 (root jobs as contracts), WP-7 |
| `node-caches` action | 27 | 12 / 4 (app-release, ci, debrief, nightly) | yes (9) | Medium | WP-4 |
| `host-tools` action | 36 | 5 / 2 | yes (10) | Medium | WP-4 |
| `stable-workspace` action | 106 | 3 / 1 (debrief, incl. `bundle`) | yes (7) | Medium | WP-4 |
| `sccache-stats` action | 5 | 15 / 4 | no | Low | WP-4 |
| `build-folder.sh`, `prune-build-folder`, `tools/prune-*.sh`, `.ps1`, `tools/host-cleanup.sh` | 38 + 4 + 60 + 57 + 108 | `build-folder` 7 / 3; `host-cleanup` 2 / 2 | yes | Medium | WP-4 |
| `kept-clone.sh`, `guardian-as-user.sh`, `boot-volume.sh` | 232 + 76 + 75 | 4 / 2 (app-release, ci); 1 / 1; via `job-tmpdir` | yes (11) | Medium | WP-5 |
| `install-test-browser.mjs`, `pin-runtime.mjs`, `runtime-catalogue.mjs`, Spotter's `install-test-browser.mjs`, Debrief's `fetch-msedgedriver.mjs`, `fetch-libduckdb.sh` | 397 + 50 + 108 + 57 | 4 / 2 (ci, debrief) | yes (16) | Medium | WP-6 |
| debrief.yml inline (13 jobs, 80 blocks, 8 long) | 288 | 13 / 1 | yes (52) | High (its `bundle`, `release` jobs) | WP-7; release jobs WP-11 |
| Small workflows: debrief-bench, service-smoke, canary, app-fuzz, nightly, debrief-fuzz, -mutants, -runner-check, -corpus, ci-token-check, ci-image, dev-tool | 238 | 37 / 12 | yes (30) | Low | WP-7 |
| App CI scripts, not release: Debrief `scripts/ci/*` (startup-bench, check-temp-leaks, scan-fixtures, macos-smoke, libduckdb-env, check-index, windows-node-modules, check-quarantine), `check-icons.mjs` (three copies: Debrief, Pitwall, Spotter), Dyno `check-capabilities.mjs`, Pitwall `ui-coverage-guard.mjs`, `privacy-check.sh` | 1,211 | app workflows and `paddock.ci.json` | yes (macos-smoke, windows-node-modules, libduckdb-env) | Medium | WP-8 |
| `coverage-gate.mjs`, `check-index.mjs`, `commit-lint.mjs` (and Debrief's copy) | 124 + 23 + 105 + 61 | 1 / 1; 1 / 1; the `commits` job | no | Low | WP-9 |
| Retirements: `affected.mjs`, `affected-marks.mjs`, `ci-timings.mjs` | 1,173 + 29 + 337 | fallback only; none | no | Low | WP-9 |
| Conformance: engine, `util`, `sync`, 41 checks, `bin/`, `notices`, `sidecar`, `window-weight`, `scripts/notices.sh` | 6,362 + 56 + 21 | the conformance job, `keychain` (C99), `app-gates` (notices), `sidecar-self-test` | partly | High (runs in every gate) | WP-10 |
| Release kit: `conformance/release/*` (per-machine installer, CRT imports, macOS verify, latest.json, publish, strip keys) | 1,023 | per-machine 6 / 3; macOS verify 9 / 5; publish 4 / 4 | yes | High | WP-11 |
| Release CI scripts: `tauri-bundle.sh`, `first-release-notes.sh`, `release-notes.mjs`, `release-verdict.mjs`, `dev-tool-publish.sh`, `plan.mjs`, `check-tag.mjs` | 413 | release-verdict and plan 5 / 5 (every app's release caller and nightly); check-tag 1 / 1 | yes (tauri-bundle 7) | High | WP-11 |
| `scripts/publish.mjs`, `stage-paddock-setup.mjs`, `demo-release.mjs`, `scripts/demo/*` | 396 + 137 + 175 + 150 | 4 / 4; 6 / 3; 5 / 3; `bundle.sh` 12 / 8 | yes (`bundle.sh` 5) | High | WP-11 |
| Release inline shell: app-release.yml, the demo releases, publish.yml, the per-app callers, and the actions publish-release, sidecar-self-test, keychain, strip-signing-keys, macos-verify, tauri-bundle, release-verdict | 216 | every release | yes (37) | High | WP-11 |
| App release scripts: Dyno `check-bundle.sh`, `check-release-notes.mjs`, `check-pe-subsystem.mjs`; Pitwall `stage-vc-redist.mjs`; four `sidecar.mjs`; Pacenotes and Sighter `publish.mjs`; Debrief `macos-verify-bundle.sh` (a copy of the shared one) | 693 | the app release workflows | yes | High | WP-11 |
| `scripts/new-app.mjs`, `dialog-commands.mjs`, `scripts/perf/link-time/*`, `ci-image/verify.sh` | 550 + 73 + 98 + 94 | tests only; `ui` package scripts; none; `ci-image.yml` | no | Low | WP-12 |
| VM harness: `scripts/vm-smoke/*` (74 files, plus 2 Swift), `vm-win.sh`, `vm-win/*`, `vm-lock*.sh`, `vm-test-support.mjs`, `tools/macos-vm-smoke.sh`, `linux-container.sh`, `verify-installed.sh`, `scripts/shell-demo/*` | 10,581 | local only (their unit tests run in ci.yml; `build-linux-arm64.sh` in service-smoke) | yes (guest OS) | Medium (no CI user, largest) | WP-14 |
| Bootstrap: the `dev` wrapper, `tools/bootstrap.sh`, `scripts/setup-dev.sh`, `ci-image/fetch-verified.sh`, the hook shim | about 330 | every `./dev` call; machine setup | yes | stays (§5) | – |

Outside this plan (ADR 0022 wave 3, not CI): the app generators and UI tooling under `apps/*/` and `icons/` (diagram
generators R11, icons R12, images R13, npm glue R14, UI tooling and i18n R19, Debrief's xtasks R21, the rest of R22, R23),
about 11,900 lines of the 14,565 under `apps/*/`. §4 gives their total from ADR 0022's own estimates.

## 3. Work packages, in order

Order: first what is cross-OS and most used (the per-job critical path of every host job), with the pre-push check second so
that every later WP is pushed through it; then the gate executor that makes CI's commands data; then the rest of the
non-release kit; then the conformance engine; the release path only after the tags (ADR 0022 G.7); the local VM harness last.

Each WP: its old scripts are deleted in the same WP once parity holds (ADR 0022 §3.7, "one command at a time"), except a
mutating step used by release jobs, which keeps its shell copy for release, tag and rule 54 runs until WP-11 (the setup-rust
pattern, §8). "Review" is the expected fix round after the one independent review, 0.5 days (0.25 to 1).

| WP | Content | Retires (lines) | Build days | Review | Total (range) | Depends on |
|---|---|---|---:|---:|---:|---|
| WP-0 | `./dev ci setup-rust` | setup-rust's sccache and Tauri steps | done | done | done | – |
| **WP-1** | **Kit switch and build jobs.** Switch `job-isolation` and its 12 direct `path`/`port` calls to `./dev ci isolation` (built); port `build-jobs.sh` to `./dev ci build-jobs` (cores share per runner class, three OS). Release jobs keep the shell (guard as setup-rust) | `build-jobs.sh` 206; the isolation family after WP-9 (621) | 2.5 | 0.5 | **3** (2.25-4.5) | WP-0 |
| **WP-2 (built; review round 1 fixed)** | **`./dev check --pre-push` (R17, ADR 0038 WP-2, cut to ER-9's fast gates).** Reads `./dev plan` for the pushed commit (`--refs-from-stdin`: checked out in a throw-away worktree; a person's run without it checks the working tree) from `merge-base(origin/main)`; runs fmt, host Clippy for each workspace with a changed file of its own through `heavy` (dependents are CI's, ER-9 overlap), `./dev clippy --target windows` (xwin), Linux Clippy in the pinned CI image, opt-in with `--linux` (port of `linux-clippy-touched.sh`, Docker `--rm --cpus 4 --memory 12g`, "not measured" without Docker, ER-33; exit 6), conformance with the check list of conformance/paddock.ci.json (WP-3; it read ci.yml's line before), commit-lint, lockfiles `--locked`, lint and type-check of touched TS files; one table (ran, skipped with reason, failed with the re-run command); lefthook `pre-push` becomes that one line. Salvages `fix/lefthook-pre-push` and `fix/prepush-linux-clippy` | `linux-clippy-touched.sh` 72; lefthook's three push entries | 2.5 | 0.5 | **3** (2.25-4.5) | `./dev plan` (main) |
| **WP-3 (built)** | **The gate executor `./dev ci run <project> <phase>` (ADR 0038 WP-1 and WP-4).** One command per line, `cd` holds for later lines, each line logged with duration and exit (ER-27); `paddock.ci.json` for `crates/`, `ui/`, `conformance/`, `tools/`, `icons/`, `eslint/` (the conformance `--only` and `--strict` lists become fields); `app-gates` and ci.yml's root jobs call it; `--pre-push` reads the same contracts, so the conformance list has one copy. Built: `./dev ci run <project> <phase> [--keep-going] [--list] [--heavy] [--max-secs]` (`tools/paddock-dev/src/ci_run`) with a `paddock.ci.json` for `crates`, `ui`, `conformance`, `tools`, `icons` and `eslint` (a phase is a top-level array of command lines; `conformance/paddock.ci.json`'s `conformance` object is the one copy of the check list, read by CI's conformance and train-gates jobs and by `./dev check --pre-push`); ci.yml's `ui`, `e2e`, `conformance`, `tools`, `icons`, `eslint-preset`, `crates` and `train-gates` steps and every command list of `app-gates` call it; `run-commands.sh` is deleted; `./dev ci build-env` ports `build-env.mjs` (script kept for release runs and C56, registered as `ci-build-env`); `bounded.sh` stays (registered under `ci-host-steps`: the release path's `kept-clone.sh` sources it). Parity: `tools/paddock-dev/tests/golden/ci-run/commands.tsv` is the model of the commands CI ran before (11c365fc4) and a test compares it with the tree. Not in this WP: the Rust half of `app-gates` (its `step()` shell with the nextest failure names), the `heavy`, `paddock-dev`, `crate`, `wiki`, `commits` and `tools-macos` jobs, which keep their steps (WP-7) | `run-commands.sh` 24, `build-env.mjs` 30, `bounded.sh` 35, app-gates' shell 52, most of ci.yml's 158 | 3 | 0.5 | **3.5** (2.75-5) | WP-2 |
| WP-4 | **Host actions.** `stable-workspace` (macOS link, Windows junction), `node-caches`, `host-tools`, `sccache-stats`, `build-folder.sh` and the prune scripts (sh and ps1), `host-cleanup.sh`, as `./dev ci <step>` | 106 + 27 + 36 + 5 + 38 + 117 + 108 | 2.5 | 0.5 | **3** (2.25-4.5) | WP-1 |
| WP-5 | **Kept clones.** `kept-clone.sh` with `bounded.sh`'s caller, `guardian-as-user.sh`, `boot-volume.sh` | 383 | 1.5 | 0.5 | **2** (1.25-3) | WP-1 |
| WP-6 | **`./dev fetch` (R3).** Pinned download from a TOML catalogue, SHA-256, extract (the setup-rust download code reused); the Chrome for Testing trio, Spotter's copy, Debrief's msedgedriver and libduckdb fetchers. Salvages `tools/dev-fetch-r3` | 612 | 2 | 0.5 | **2.5** (1.75-4) | WP-0 (download code) |
| WP-7 | **Non-release workflow shell (R10).** ci.yml's rest, debrief.yml's non-release jobs (8 long blocks), the 12 small workflows | about 520 inline lines | 3 | 0.5 | **3.5** (2.75-5) | WP-3; `ci/app-failed-only-and-schedule-skip` landed |
| WP-8 | **App CI scripts, not release.** Debrief's eight `scripts/ci` tools, one `./dev check icons` for the three `check-icons.mjs` copies (rule 55), Dyno's capabilities check, Pitwall's coverage guard and privacy check | 1,211 | 2 | 0.5 | **2.5** (1.75-4) | WP-3 |
| WP-9 | **Small CI scripts and retirements.** `coverage-gate` (G7), `check-index` (and Debrief's copy), `commit-lint` (G4, both copies); delete `affected.mjs`, `affected-marks.mjs` and `PLAN_PLANNER` after the first app tag planned by `./dev plan`, `ci-timings.mjs`, the isolation family after WP-1's release, C39 and C53 in JS | 313 + 1,539 + 621 | 1.5 | 0.5 | **2** (1.25-3) | WP-1; one app tag |
| WP-10 | **Conformance engine and checks (R8, R6 parts).** The engine and `util` first, then the 39 remaining checks in three batches by kind (text scans, manifest and lockfile checks, process and CI checks: C23, C28, C71 with its baseline, C99, C100's JS side), then `notices` (salvages `tools/dev-notices`), `sidecar`, `window-weight`. The 9,516 lines of JS tests become Rust tests; each check keeps its good and bad fixture. Four reviews, one per batch | 6,439 | 9 | 2 (4 x 0.5) | **11** (8-16) | WP-3 (contracts) |
| WP-11 | **Release path, after the tags (R2, G8, G11, G12).** The release kit, the release CI scripts, the publish scripts, the release inline shell, the app release scripts (the Debrief copy of macOS verify deleted), ADR 0085's `release-isolation` wired in; every guard and shell copy kept by WP-0 to WP-5 removed. A `workflow_dispatch` dry run of each app's release with gates on and bundles built (rule 54: five apps and two demos) | 2,941 | 7 | 1 | **8** (6-10.5) | WP-1 to WP-6; the tags; no release window |
| WP-12 | **Dev scripts.** `./dev new-app` (R7), the dialog command contract generated with ts-rs so `dialog-commands.mjs` is deleted, not ported (R15), `./dev perf link-time` (R22 part), `ci-image/verify.sh` | 815 | 2 | 0.5 | **2.5** (1.75-3.5) | – |
| WP-13 | **`devkit` and the runtime (G1, G2).** `paddock-tool` (logging, phases, masked spawn lines, `::error::`; `ToolCall` moves onto it), the `devkit` binary, and the move of the generic engines built in WP-1 to WP-12 (fetch, commit-lint, coverage gate, prune, timings, macOS verify, CRT imports, Gitea release, key strip) without a rewrite (G.8: they read no Paddock path). The user's 2026-10-31 decision; see §9 for the cheaper order | – | 4 | 0.5 | **4.5** (3.5-6) | WP-9, WP-11 (outside release windows: `crates/`) |
| WP-14 | **VM harness (R5), after its redesign** (memory 2026-11-08: preconditions first, per-row `--only`, OS in parallel). About 10,600 lines; at R12's measured ratio (4,259 lines in 4.5 days) about 10 days. Guest-side PowerShell that runs before anything exists in a fresh Windows VM stays (§5). Salvages `tools/vm-smoke-unified` | 10,581 | 10 | 1 | **11** (8.5-15) | the VM redesign; WP-6 |
| | **Total of the plan (WP-1 to WP-14)** | about 27,000 source lines plus their tests | **52.5** | **9.5** | **62** (46-88.5) | |

### Rows beyond common practice (ER-37 amendment: more than 1 day needs the user's yes)

These are accepted ADR 0038 parts that go beyond what comparable projects do for a pre-push check (§9). They are **not** in
the total above. Each needs the user's yes under the amended ER-37; until then WP-2's fast gates are the pre-push check.

| Row | Content | Days (range) | Beyond common practice by | Needs yes |
|---|---|---:|---|---|
| X-1 | ADR 0038 WP-3: `ci/tool-versions.toml` with its consistency test, `./dev gate provision linux`, the OrbStack machine `paddock-gate-arm` as the Linux leg of the pre-push gate, the gallery on arm64 | 4 (3-5.5) | the whole row: WP-2 already runs Linux Clippy in the pinned CI image | **yes** |
| X-2 | ADR 0038 WP-5 and WP-6: the parity golden, the G26 gate stamps and the dispatch refusal | 2.5 (2-3.5) | the whole row: common practice is a hook that runs and CI that decides | **yes** |
| X-3 | ADR 0038 amendment of 2026-11-08: `./dev check --commits` (WP-B1 to WP-B5: per-commit gate over a worktree pool, nightly audit, trailers) | 10.5 (8-13) | the whole row: comparable projects lint each commit's message and build the merge result, not every commit | **yes** |
| | **Sum** | **17** (13-22) | | |

## 4. Dependencies, total and calendar

```
WP-0 (done) ─┬─ WP-1 ─┬─ WP-4
             │        ├─ WP-5
             │        └─ WP-9 (also after one app tag)
             └─ WP-6 ─── WP-14 (also after the VM redesign)
./dev plan ── WP-2 ── WP-3 ─┬─ WP-7 (also after ci/app-failed-only-and-schedule-skip)
                            ├─ WP-8
                            └─ WP-10
WP-1..WP-6 + the tags ── WP-11 ── WP-13
WP-12: no dependency (any free slot)
```

- **Total:** 62 agent-days (46 to 88.5) for WP-1 to WP-14. With the three rows that need a yes: 79 (59 to 110.5). The ranges are
  summed end to end, so the high ends are pessimistic.
- **Calendar at 2 parallel agents** (ADR 0022 G.7's cap while a release is in flight), 5 working days a week: 62 / 2 is
  31 working days, about **6 weeks** of pure work. Add about 15% for review and CI turnaround between WPs (one CI run per WP,
  rule 50) and the hand-offs on the chains below: about **7 weeks** (5.5 to 10). The three yes-rows add about 2 weeks.
- **Lanes.** Lane A: WP-1, WP-4, WP-5, WP-9, WP-6, WP-14 (23.5 days). Lane B: WP-2, WP-3, WP-7, WP-8, WP-10, WP-12 (26 days).
  WP-11 and WP-13 (12.5 days) go to whichever lane is free after the tags, so the lanes end at about 31 days each.
- **What can stretch the calendar:** WP-11 waits for the current tags and for a window with no app release in flight;
  WP-14 waits for the VM redesign; WP-9's planner retirement waits for one app tag planned by `./dev plan`; WP-7 waits for the
  other agent's debrief.yml branch. None of these blocks WP-1 to WP-6 or WP-8, WP-10, WP-12.
- **The rest of ADR 0022, not CI (for the full "everything in Rust" figure):** R11 5, R12 4.5, R13 2, R14 1, R19 10, R21 8,
  R22's rest 1.25, R23 0.5: 32.25 days from ADR 0022 §7 and G.6 (not re-measured here), plus a review round each (4): about
  **36 days**. Everything together: 62 + 36 = **98 agent-days**, about 11 calendar weeks at 2 agents, before the yes-rows.

## 5. What stays as a thin shell (bootstrap only)

ADR 0022 §4 and the 2026-11-01 decision ("compiled only"; exceptions only where Rust cannot be the tool):

- **The `dev` wrapper** (it computes the input hash and builds or downloads `paddock-dev`; ADR 0022 §3.3), and the tool's
  own fallback when no binary exists.
- **In YAML:** `uses:` of the actions that must stay (`actions/checkout`, `actions/cache`, the v3 artifact actions), `if:`
  conditions and step wiring, and the two steps before the first `./dev` of a job: CARGO_BUILD_JOBS and the toolchain install
  with its image fast path (§8: `./dev` hashes and builds itself with that compiler). Every other step is one
  `./dev ci <step> ...` line. CARGO_BUILD_JOBS's computation moves to Rust in WP-1; the YAML keeps only a one-line default
  for the tool's own first build.
- **Machine bootstrap:** `tools/bootstrap.sh` and `scripts/setup-dev.sh` (they install Rust) until R20's `./dev setup`
  (outside this plan), and `ci-image/fetch-verified.sh` and the Dockerfile's `RUN` lines (they run before any tool exists in
  the image).
- **The hook shim** `tools/githooks/claude-pre-tool-use` (POSIX `sh`, `exec`s the installed binary) and one-line lefthook
  entries that call `./dev`.
- **Guest-side setup in a fresh Windows VM** (`scripts/vm-win/*.ps1`: first logon, admin and GUI setup), which runs inside
  Windows before anything is installed; WP-14 decides each file and keeps only what runs before the tool can.
- **Framework code in its own language** (ADR 0022 §4): Playwright and Vitest specs, ESLint rules, config files.

ADR 0022 §3.7's shrink-only baselines (no new tooling script; counted long and OS-branching `run:` blocks) are the check that
keeps it so. They are **not built yet** (no enforcing code on `origin/main` or on this branch); WP-1 adds them with this
note's §2 counts as the first baseline, 0.5 days inside WP-1's estimate.

## 6. The wrapper contract

Our own logic is ported fully, with tests. Programs we cannot rewrite (cargo, npm, Playwright, `tauri build`, NSIS, signing
tools, curl, tar, sccache, apt-get) get a thin wrapper, built on `tools/paddock-dev/src/ci_tool.rs` (`ToolCall`), which this
branch adds. Every wrapper:

1. **Builds the exact command line** from typed values: a program and an argument vector, never a shell string. A value
   that goes to a script interpreter (PowerShell) goes as an environment variable, never spliced into the script.
2. **Passes secrets only to the program that needs them:** every child starts from a cleared environment with the base list
   (`paddock_proc::KEPT_ENV` and the Windows system variables), plus the names the call passes (`ToolCall::pass`, exact names
   or `PREFIX_*`), plus the variables it sets for that child alone (`ToolCall::env`). A token of the job never reaches a
   program that was not given it (a test checks). The YAML gives a secret to the one step that runs it (never workflow-wide).
   A secret never goes on a command line.
3. **Logs** one `run:` line before (the masked command line and the *names* of the variables it sets) and one `ran:` line
   after (`exit <n>`, with the NTSTATUS in hex for a Windows crash code, `signal <n>`, or `timed out after <s> s`, and the
   duration). Every value given to `ToolCall::secret` is printed as `***` in those lines and in the captured output. A
   failed captured run prints the last 40 lines of its error output.
4. **Is bounded:** every run has a timeout, after which its whole tree is ended through `paddock_proc` (ADR 0033):
   `run_bounded` for captured output, and `run_waited` (added to `paddock-proc` in the review round: the child leads its own
   process group, a test checks a grandchild is gone after the timeout) for streamed and file output.
5. **Has three output modes:** captured (short programs), streamed to the job log (long ones such as apt-get; the log then
   holds every line, so there is no separate tail), or written to a file (a program that leaves a daemon behind, such as
   `sccache --start-server`, whose server would otherwise hold a pipe open and keep the run waiting).
6. **Fails with exactly one `::error::` line**, the cause then `Next step:` (`ci_tool::error_line`). A failure the shell
   treated as a warning stays a `::warning::` and exit 0. Gitea shows these as log text only (ADR 0022 G.5).

When `paddock-tool` (ADR 0022 G.5) lands, `ToolCall` moves onto its logging and the `DEVKIT_MASK` list; until then it is the
one wrapper in the tool (rule 55: no second copy).

## 7. Proving parity, per step

- **A behaviour test against the old shell, before it is deleted.** While a ported step's shell still exists, a test runs
  the shell (extracted from the YAML or script, run as the runner runs it: `bash -e -o pipefail`) and the Rust command on the
  same inputs, folders and shim programs, and compares everything that reaches the job: exit code, `$GITHUB_ENV`,
  `$GITHUB_PATH`, `$GITHUB_OUTPUT`, and every call of the wrapped programs with the variables that steer them. For a script
  with fixtures, a golden file of its output (ADR 0022 §3.6) instead. When the shell is deleted the comparison becomes a
  golden of the Rust output, and the test stays.
- **The probe can see a difference** (ER-33): each new parity test is shown to fail on a deliberate difference before it
  counts. For setup-rust the first run caught a real one (§8).
- **One CI run per ported file**, on the branch, with the jobs that use the step; and the step's existing conformance tests
  green. The release path's version of this is the dry run per app (WP-11).
- **JS test suites are an oracle first** (WP-9 to WP-12): where a script has its own `node --test` file (the conformance
  checks, `kept-clone.test.mjs`, `pin-runtime.test.mjs`, `publish.test.mjs` and others), the suite is pointed at the Rust
  command through one variable for the overlap, as `PADDOCK_DEV` already does for `job-isolation.test.mjs`; its cases then
  move into Rust tests with the script.
- **Read-only steps run in shadow, mutating steps keep a fallback** (the 2026-11-10 plan's rule, kept): a check, plan,
  verdict or timing command runs beside the old script for one release, a difference prints `::warning::` with the diff and
  fails only the shadow step; a step that changes folders, slots or clones runs once, in Rust, with the shell one variable away
  (`PADDOCK_CI_KIT=shell`) until the next app tag whose release went through the Rust path green.
- **Every OS the step runs on.** A cross-OS step's parity test runs on the macOS, Windows and Linux legs of the
  `paddock-dev` job before the switch (ER-9), not only on the Mac.

## 8. WP-0: setup-rust (done, approved)

**Bootstrap.** `./dev` computes its input hash from `rustc -vV` and builds itself with that compiler (`dev`, the wrapper;
ADR 0022 §3.3). setup-rust is what installs the toolchain in most jobs (in the CI image it only checks the image's). So the
two steps before the first `./dev` stay in YAML: CARGO_BUILD_JOBS (build-jobs.sh: the tool's own build must already get the
runner's share of the cores) and the toolchain install with its image fast path. They also must run first because every
later `./dev` of the job must see the same default compiler, or the job would hash and build the tool twice. Without a
toolchain `./dev` stops with "rustc was not found" (seen in this branch's local end-to-end run before the toolchain was made
visible). The rest moves: installing the pinned sccache (download, SHA-256, unpack, atomic placement, `$GITHUB_PATH`),
configuring it (backend, slot port, slot folder and budget, a fresh server, `$GITHUB_ENV`) and the Tauri Linux libraries.
ADR 0022 R0's open item (the image fast path checked against `stable` with `rustup check`) stays in the YAML step for the
same reason.

**Release guard.** setup-rust runs in the release `bundle` jobs (app-release.yml, Debrief's `bundle`) and in every tag
run. So a step `Implementation` picks `shell` for a tag ref or when `PADDOCK_DEV_DOWNLOAD=0` (release and rule 54 jobs that
run `./dev` set it) and `dev` otherwise; the three old shell steps run only under `shell`, unchanged apart from their `if:`.
A job without the read credential runs `./dev` with `PADDOCK_DEV_DOWNLOAD=0` (built from source, never a stored binary). A
checkout without `tools/paddock-dev` (another repository using the action, a sparse checkout without `tools/`) also takes
`shell`. Every job that uses the action sets `PADDOCK_DEV_DOWNLOAD` by the tag rule in its env, and a conformance test checks
all of them; ci.yml's `wiki`, `heavy` and `crates` and Debrief's `rust-lint`, `rust-test` and `plugin-sdk` got it with the read
credential in the review round, so a rule 54 dispatch never takes a stored binary there. The guard and the shell steps go in
WP-11, and the copy is registered in `docs/known-copies.toml` (`ci-setup-rust`).

**Cost of the switch per job.** Jobs that already ran `./dev` (host jobs through job-isolation, the conformance job) pay a
cache hit. Linux container jobs that did not, with the read credential (ci.yml `crates`, `heavy`, `wiki`; Debrief's
`rust-lint`, `rust-test`, `plugin-sdk`), download the published tool (1-2 s, ADR 0022 §3.5) or, on a branch that changes the
tool, build it once per hash and runner on the kept `/sccache` mount. A job without the credential builds from source the
same way: `app-fuzz` (a reusable workflow that is passed no secret; with its nightly toolchain, once per nightly per runner)
and nightly's `c100`. The `c100` job builds twice: setup-rust's run has no token, so it builds into the source-only folder,
and the job's own `./dev gates check` then misses the download and builds into the other folder; giving the job the read
credential (one env line in nightly.yml) removes the second build. A cold build is 40-60 s by ADR 0022 §3.5's estimate
(dev-tool.yml's header says about 100 s); neither is measured here. The bench template uses the action from the `v0.3.0` tag
of another repository, so it takes `shell` (no `tools/paddock-dev` in its checkout) once it moves to a newer tag.

**Parity evidence** (`tools/paddock-dev/tests/it/setup_rust.rs`, 13 cases, macOS and Linux): host slots s1 and s2 with
budgets and a non-numeric budget; a container with and without the mount; Redis by `auto` and by `redis` with a password
that is never printed; a server that does not start, and one that does not answer; two wrong inputs; no sccache at all; a
pinned copy already in the tool cache (for the macOS and the Linux runner class, so an x86_64 host also covers it); the Tauri libraries through a `sudo` and `apt-get` shim; and a job token that the shell
passed to sccache and the port never does. Rust only, because the shell cannot be pointed there: a checksum mismatch, and
two Windows host cases under `PADDOCK_CI_ASSUME_WINDOWS=1` (the `.exe` names, slot ports and folders, a pinned `sccache.exe`).
A unit test covers a download that matches its pin (made at test time from a `file://` release folder): unpacked, put in
place whole with no part file left, and the download folder removed. The first run failed on a real difference: after a
failed download the port still wrote the tool-cache folder to `$GITHUB_PATH`, which the shell's `exit 0` skips. Fixed before
the switch.

**Deliberate differences** (log text only, nothing a job reads): every external program gets a `run:`/`ran:` line; the
phases print their durations; a password in the Redis endpoint is masked (the shell echoed it); `PADDOCK_CI_SCCACHE_GB` with a
leading zero is read as decimal (bash read `010` as octal 8); on a Windows ARM runner (none exists) the port finds no pin and
warns, where the shell under x64 emulation would have taken the x64 release. Two behaviour changes, both on purpose: a copy
already in the tool cache is used only when `--version` names the pinned release (the shell trusted any executable file
there), and when the rename into place fails because another job already put the release there (Windows cannot replace a
running program) the port checks that copy's version and uses it instead of failing. Every external program starts from a
cleared environment plus a per-program allow-list (sccache: `SCCACHE_*`, `CARGO_*`, `RUSTUP_*`; curl and apt-get: the proxy
and certificate variables), where the shell passed everything on, tokens included.


## 8a. WP-1: the kit switch and build-jobs (built)

`./dev ci build-jobs` replaces `conformance/ci/build-jobs.sh` and `./dev ci isolation` replaces `conformance/ci/job-isolation.sh` in
the actions and workflows. One shim, `conformance/ci/kit.sh`, picks the implementation by setup-rust's rule (a tag run, or
`PADDOCK_DEV_DOWNLOAD=0`, or a checkout without the tool, or no `rustc`, or `PADDOCK_CI_KIT=shell`, the fallback switch: the shell script; one `kit:` line on stderr says which and why) and does the isolation bootstrap
(the tool fetched under the CI home's `tools` folder and pinned as `PADDOCK_DEV`, so release never builds it). Callers say
`bash conformance/ci/kit.sh isolation path ...` and `kit.sh build-jobs`. The tool's own first build runs under a default of 4
build jobs, the only pinned number (`./dev ci build-jobs` then writes the job's real share). setup-rust's build-jobs step moved
after its toolchain step (the tool needs `rustc`). Parity: `tools/paddock-dev/tests/it/build_jobs.rs` against the script (exit
code, `$GITHUB_ENV`, both output streams; two spellings differ on purpose, normalised in the test) and `job-isolation.test.mjs`
with `ISOLATION_CMD`. Left on the shell for WP-4 and WP-5: `kept-clone.sh`, `build-folder.sh`, `tools/*.sh` and Debrief's CI scripts
call `job-isolation.sh` themselves; the `job-tmpdir` action still runs `job-tmpdir.sh` until WP-9 deletes the family. Registered as
`ci-kit-shell` in `docs/known-copies.toml`. Windows: `build_jobs_golden.rs` and `ci_isolation_windows.rs` are the twins of the
Unix-only parity files, and a conformance test (`parity-twins.test.mjs`) requires every Unix-only file of `tests/it/` to name its twin
or a reason. **Planned, not built:** the shrink-only baselines of section 5 (no new tooling script; counted long and OS-branching `run:`
blocks). The 0.5 days for them stay in WP-1's estimate and move to the next WP that touches workflows.

## 8b. WP-5: kept clones, the guardian as a user and the boot-volume rule (built)

`./dev ci kept-clone` replaces `conformance/ci/kept-clone.sh` (and `bounded.sh`, which only it sources) in the `kept-clone` action, and
`./dev ci guardian-as-user` replaces `conformance/ci/guardian-as-user.sh` in ci.yml's Linux crates job, both through `kit.sh host
<step>` (a tag run, `PADDOCK_DEV_DOWNLOAD=0` or `PADDOCK_CI_KIT=shell` keep the shell). `./dev ci boot-volume external|temp` is
`boot-volume.sh`'s rule over a `Probe` seam (`df`, `diskutil` and `getconf` and the folders it looks at), and job-tmpdir's rule in
process now delegates to it, so there is one implementation in the tool (the script stays for `job-tmpdir.sh`, `paddock_testkit` and
Dyno's `check-bundle.sh`). `bounded.sh`'s caller side is `ci_host/phase.rs`, shared with `stable-workspace`. Parity: `host_steps.rs`
runs each script and the command on the same fixtures (a local origin that allows partial clones, shims for `df`, `diskutil`,
`getconf` and `cargo`), and `conformance/test/kept-clone.test.mjs` runs its 23 cases against the binary with `KEPT_CLONE_CMD`.
Differences the parity found are listed in the test's normalisation with their reason (a hung git names itself in the last output, the
exit status of a failed phase, a trailing space). The guardian's root path (setpriv, chown) is tested only through a fake machine;
the shell side of its parity needs bash 4.4 or newer (macOS ships 3.2), so it runs on the Linux legs.

## 9. Common practice, and the cheaper option (ER-37)

Read on 2026-11-12 unless marked.

- **cargo-xtask** (<https://github.com/matklad/cargo-xtask>): repository automation as a Rust package in the repository,
  run through a `.cargo/config.toml` alias; it "fully bootstraps from" `cargo` and `rustc` and needs no other tool. It lists
  rust-analyzer, Helix, bootc and Cargo itself as users. This plan is that pattern: `./dev` is the alias, with a
  content-addressed binary so CI does not compile it in every job (ADR 0022 §3.3).
- **Bevy** (<https://github.com/bevyengine/bevy/tree/main/tools/ci>, doc comment "CI script used for Bevy."): its
  `.github/workflows/ci.yml` runs `cargo run -p ci -- test`, `-- lints`, `-- compile` and `-- doc`, one line per job. That is
  WP-3's executor, without per-project contracts.
- **rust-lang/rust** (<https://rustc-dev-guide.rust-lang.org/building/suggested.html>): `./x` is the tool, `tidy` is the
  in-repository checker CI enforces, and "a prebuilt git hook lives at `src/etc/pre-push.sh`", installed by `./x setup`. The
  hook runs the fast checks (tidy), not CI's whole plan. That is WP-2's scope.
- **just** (<https://just.systems/man/en/settings.html>): a command runner whose recipes run through a shell; on Windows it
  needs `windows-shell` or `windows-powershell`. It moves commands out of YAML but keeps the OS branching in shell, which is
  the class this port removes (ADR 0022 §Context: most past kit fixes were Git Bash and bash 3.2 differences).
- **The platform vendor** (<https://docs.github.com/en/actions/sharing-automations/creating-actions/creating-a-composite-action>):
  composite actions run checked-in scripts (`run: goodbye.sh`, `shell: bash`). Gitea Actions follows GitHub's syntax
  (<https://docs.gitea.com/usage/actions/comparison>, read for ADR 0022 G.5). Neither asks for logic to leave shell; most
  projects keep long steps as scripts with shellcheck.

**Where this plan goes beyond that, and what it costs.** The xtask core (WP-1 to WP-12, WP-14) is common practice for a
Rust repository; the user chose it on 2026-10-31 and 2026-11-12. Three things go further:
1. **Parallel shell copies for release jobs** (WP-0 to WP-5 keep them until WP-11): about 0.25 days per release-path WP,
   inside the estimates. It is the price of not changing a release run before the tags (ADR 0022 G.7, ER-1).
2. **`devkit` as a separate generic tool** (WP-13, 4.5 days): xtask projects keep all of it in the repository. The user decided
   generic tools on 2026-10-31 and confirmed it on 2026-11-12, so this is on record; the cheaper order is to defer WP-13 until a
   second repository needs a command.
3. **The ADR 0038 rows X-1 to X-3** (17 days): beyond what rust-lang, Bevy or xtask users do before a push. Marked "needs yes".

**Cheaper common-practice options (decisions for the user):**
- **(a) Scripts, not Rust:** move each long `run:` block into a checked-in `.sh` under `conformance/ci` with shellcheck, keep
  the JS conformance, and stop: about 0.1 days per file, about 4 days for the inventory's CI part, no bootstrap or release
  guard. It keeps two languages and the OS branching in shell. Recorded so it stays a choice; the user's decisions chose Rust.
- **(b) Rust for the CI kit and the gates only:** WP-1 to WP-11 (about 44 days) and leave the dev scripts (WP-12), the VM
  harness (WP-14) and `devkit` (WP-13) in their languages until each is next changed. Saves about 18 days.
- **(c) Pre-push at ER-9's fast gates only** (WP-2 as planned) and decline X-1 to X-3: saves 17 days against ADR 0038 as
  accepted.

## 10. Checks that keep it so

The tests of §7 per WP; the conformance test "setup-rust uses ./dev only off the release path" (`ci-actions.test.mjs`); the
copy register entries (`docs/known-copies.toml`, `ci-setup-rust` and one per kept shell copy) until WP-11, as a to-do list
that does not block tags (user, 2026-11-11); and ADR 0022 §3.7's shrink-only baselines, built in WP-1 from §2's counts.

## 11. Decisions for the user

1. **The three ADR 0038 rows beyond common practice** (X-1 Linux gate machine and tool-versions file, 4 days; X-2 stamps and
   dispatch refusal, 2.5; X-3 `--commits` per-commit gate, 10.5): yes or no to each. Recommended: no to X-1 and X-2 for now
   (WP-2 already runs Linux Clippy in the CI image, and CI decides). X-3 has a record behind it (the amendment's evidence lists
   red intermediate commits found before landing, among them `617825f1e`, `143175a11` and `483162cff`); the common-practice alternative
   is a commit-message lint plus the merge run, so the choice is the user's.
2. **Scope:** the full plan (62 days), or option (b), CI kit and gates only (about 44 days).
3. **`devkit` now (WP-13) or when a second repository needs it.** Recommended: when needed.
4. **The VM harness (WP-14, 11 days):** in this port after its redesign, or left in JS until the harness is next changed.
