# Rust tooling, refreshed plan (2026-11-10)

- Asked by the user on 2026-11-10: "move to rust tooling asap". This plan replaces the ordering of ADR 0022's waves
  (G.7) for the next packages. The ADR, its 2026-10-31 amendment (accepted 2026-11-01) and its 2026-11-09 amendment
  still decide *what* the tooling is: compiled Rust only; generic commands in `devkit`, Paddock commands in `./dev`; one
  runtime; plugins with no sandbox; a parity test before each script is retired. This plan decides *in what order* and
  *who owns which files*.
- Read at `origin/train/2026-11-09-3` `66b600669` unless a row names a branch. ADRs 0081, 0082, 0083 and 0085 are
  accepted on `origin/docs/adr-0081-0082` (`76952834a`), `origin/docs/adr-0083-0084` (`ee3f54485`) and
  `origin/docs/adr-0085-release-cache-isolation` (`3a49d49a1`). The ADR 0025 and 0038 amendments are **proposed** (in
  review) on `origin/docs/adr-redesigns-2026-11-10` (`f0979961a`). Counts are `git ls-files` plus `wc -l`; fixtures are
  left out.
- Estimates are raw agent-days for one implementer, as in ADR 0022 §7. They are estimates, not measurements.

## 1. Where we are

### 1.1 Already Rust (on the train)

| Tool | What it does today | Lines (src) |
|---|---|---|
| `tools/paddock-dev` (`./dev`) | `self hash`; `ci slot acquire/release/status/idle-run/hold` (job-isolation slots on kernel locks, ADR 0025); `wait` (ER-32); `gates model/check/list/inputs/ratchet` (C100, ADR 0060); `check ipc-endpoints` (C102, ADR 0072); `plan` (the port of `scripts/affected.mjs`, including `--since-run`, which reads Gitea runs and nextest/vitest logs); `graph affected` | 16,527 in `src/` (largest: `plan.rs` 1,588, `gates/ci.rs` 1,560, `gates/gating.rs` 1,232, `plan_since.rs` 1,228, `plan_ci.rs` 1,207, `wait.rs` 1,099, `ipc_endpoints.rs` 1,012) |
| `dev` wrapper | content-addressed binary; prebuilt download from `paddock-build-inputs` (R0b, merged from `ci/dev-prebuilt`) | shell, bootstrap exception (c) |
| `tools/heavy` | the heavy-step semaphore (replaces `heavy.sh`, plock) | 3,024 |
| `tools/wiki` | wiki lint, log and page tool | 2,285 |
| `crates/paddock-repo-graph`, `paddock-repo-files`, `paddock-slots`, `paddock-cli`, `paddock-proc` | the libraries under `./dev` | – |

`devkit` does not exist yet, and neither does the `paddock-tool` runtime (G1). CI has its own legs for `paddock-dev`
(three OSes), `wiki` and `heavy` (`ci.yml:1079`, `:406`, `:528`).

**In progress now:** `feat/rust-planner-ci` (locked worktree `wt/rust-planner-ci`, no commits yet) switches every
`node scripts/affected.mjs` call to `./dev plan` (today `ci.yml:115` uses `./dev plan` only for `--since-run`; the plan
action already uses it for `--app`). Not counted in this plan's estimates; it is **T0** below.

**Unmerged tooling branches, all 1,800 to 2,400 commits behind the train:** `tools/dev-host-prune` (11 commits, `./dev
host prune`), `tools/dev-fetch-r3` (4, `./dev fetch`), `tools/dev-notices` (1, `./dev notices`), `feat/dev-vm-broker` (6,
the VM broker), `tools/vm-smoke-unified` (42), `docs/dev-tooling-features` (2, the features note that ADR 0022 G.6 adopts
by reference; the note is **not** on the train, so that link is broken there). The first three are salvaged inside RT-6,
RT-18 and RT-19; the broker waits for the VM harness redesign (memory, 2026-11-08).

### 1.2 Still JavaScript, shell, PowerShell or Python

| Area | Source files | Source lines | Test files | Test lines |
|---|---|---|---|---|
| `scripts/` top level and `demo/`, `shell-demo/`, `perf/`, `vm-win/` | 31 (11 `.mjs`, 16 `.sh`, 3 `.ps1`, 1 `.py`) | 4,765 | 14 | 3,373 |
| `scripts/vm-smoke/` | 72 (62 `.mjs`, 5 `.sh`, 4 `.ps1`, 1 `.py`; plus 2 `.swift`) | 9,253 | 44 | 3,088 |
| `conformance/src` (engine, 40 checks, sync, notices, sidecar, window weight) | 70 | 6,675 | – | – |
| `conformance/test` | – | – | 67 | 10,090 |
| `conformance/ci` | 21 (12 `.sh`, 9 `.mjs`) plus 3 data files | 2,242 | (in `conformance/test`) | |
| `conformance/release` | 6 | 1,092 | (in `conformance/test`) | |
| `conformance/bin` | 4 | 61 | – | – |
| `tools/*.sh`, `.ps1` | 8 | 716 | 6 | 771 |
| `ci-image/*.sh`, diagram export wrappers | 4 | 211 | – | – |
| Inline `run:` shell in the shared actions | 26 actions | about 491 | `ci-actions.test.mjs` | |
| Inline `run:` shell in the workflows | 22 workflows | about 780 (largest: `debrief.yml` 280, `ci.yml` 143) | – | |

Counted for completeness, outside this plan's first packages: about 129 scripts (12,314 lines) under `apps/*/` (Debrief
66, Dyno 25, Pitwall 17), owned by R2, R5, R11, R21 and the app batches. `conformance/` has 156 JS files in all (the
brief's "about 156"). `conformance/ci` has 12 shell scripts (the brief said 13). `scripts/` has 73 non-test `.mjs`, 62 of
them in `vm-smoke`.

### 1.3 Inventory by script: wave, class, CI use, tests

Wave and class are from ADR 0022 G.6 and Appendix G (G = generic, P = Paddock, S = split). "CI use" is a grep of
`.gitea/` (workflows and actions); "Local" means only people and agents run it. "RT" is the package of this plan that
retires it.

**CI kit (`conformance/ci`, `tools/`)**, the per-job critical path of every host job:

| Script | Lines | ADR WP | Class | CI use | Tests | RT |
|---|---|---|---|---|---|---|
| `ci/job-isolation.sh` | 464 | R1 (row 32) | S | `job-isolation` action (28 uses) plus 12 direct calls in workflows and other actions (ports, paths) | `job-isolation.test.mjs` (594), `ci-actions.test.mjs` | RT-1 |
| `ci/job-tmpdir.sh` | 175 | R1 | S | `job-tmpdir` (6), inside `job-isolation` | `job-isolation.test.mjs` | RT-1 |
| `ci/ci-home.sh` | 28 | R1 | S | sourced by the kit | `job-isolation.test.mjs` | RT-1 |
| `ci/build-folder.sh` | 38 | R1 | S | `build-folder` (7), `prune-build-folder` (3) | `ci-actions`, `prune-and-cleanup` | RT-6 |
| `tools/prune-ci-build-folder.sh`, `prune-build-folders.sh`, `.ps1` | 117 | G10/R5 | G | `prune-build-folder` | `prune-and-cleanup.test.mjs` | RT-6 |
| `ci/build-jobs.sh` | 215 | R1 | S | `build-jobs` (29 uses) | `ci-actions` | RT-13 |
| `ci/kept-clone.sh` | 241 | R1 | S | `kept-clone` (4) | `kept-clone.test.mjs` (647) | RT-13 |
| `ci/bounded.sh`, `run-commands.sh`, `guardian-as-user.sh`, `build-env.mjs` | 176 | R1/R10 | P | `app-gates`, `ci.yml:680` | partly (`ci-actions`); `bounded`, `guardian-as-user` none | RT-13 |
| `tools/host-cleanup.sh` | 108 | R1/R5 | S | `host-cleanup` (2) | `prune-and-cleanup.test.mjs` | RT-13 |
| `ci/plan.mjs`, `release-verdict.mjs`, `check-tag.mjs`, `check-index.mjs` | 161 | R4/R2 | P | `plan` (6), `release-verdict` (8), `app-release.yml`, `debrief.yml` | each has a test | RT-15 |
| `ci/install-test-browser.mjs`, `pin-runtime.mjs`, `runtime-catalogue.mjs` | 428 | R3 | S | `install-test-browser` (3) | `pin-runtime.test.mjs`, `ci-scripts` | RT-18 |
| `ci/tauri-bundle.sh`, `first-release-notes.sh`, `release-notes.mjs` | 229 | R2 | P | release workflows | `ci-scripts`, `first-release-notes.test.mjs` | RT-16 |
| `ci/dev-tool-publish.sh` | 87 | R0b | P | `dev-tool.yml` | none | RT-16 |
| Inline: `setup-rust` (195 yaml lines), `stable-workspace` (122), `app-gates` (189), `node-caches`, `host-tools`, `sccache-stats` | about 491 | R1 | S | 21, 3, 2, 12, 6, 15 uses | `ci-actions.test.mjs` (structure only) | RT-13 |

**Release kit (`conformance/release`, `scripts/`)**, on every app's release path:

| Script | Lines | ADR WP | Class | CI use | Tests | RT |
|---|---|---|---|---|---|---|
| `release/publish.mjs` | 117 | G11 | G | `publish-release`, `publish.yml`, `ci.yml`, `debrief.yml` | `publish.test.mjs`, `c07`, `c08` | RT-16 |
| `release/latest-json.mjs` | 118 | R2 | P | `publish-release`, `debrief.yml` | `latest-json.test.mjs`, `release.test.mjs` | RT-16 |
| `release/strip-keys.mjs` | 52 | G8 | G | `strip-signing-keys` | `c06`, `release.test.mjs` | RT-16 |
| `release/macos-verify-bundle.sh` | 156 | G12 | G | `macos-verify`, three release workflows, nightly | `macos-verify-bundle.test.mjs` | RT-16 |
| `release/windows-crt-imports.mjs` | 305 | G12 | G | `debrief.yml` | `windows-crt-imports.test.mjs` | RT-16 |
| `release/check-per-machine-installer.mjs` | 344 | R2 | P | four release workflows, nightly | `per-machine-installer-gate.test.mjs` | RT-16 |
| `scripts/stage-paddock-setup.mjs` | 149 | R2 | P | four release workflows | yes | RT-16 |
| `scripts/demo-release.mjs`, `scripts/demo/*` | 355 | R2 | P | demo release workflows, `service-smoke.yml` | `demo-release`, `check-agent-version` | RT-16 |
| `scripts/publish.mjs` | 436 | R7 | P | `publish.yml`, `ci.yml`, `debrief.yml` | `publish.test.mjs` | RT-21 |

**Planner, checks and the rest of `scripts/`:**

| Script | Lines | ADR WP | Class | CI use | Tests | RT |
|---|---|---|---|---|---|---|
| `scripts/affected.mjs`, `affected-marks.mjs` | 1,268 | R4 | P | every plan job (`ci.yml`, every app workflow, nightly) | `affected.test.mjs`; the `./dev plan` golden uses it as oracle | T0 (in progress) |
| `conformance/src` engine and `util/` | 844 (Appendix G row 23) | R8 | S | `conformance` job, `keychain` action (C99), `debrief.yml` | every check has a good and a bad fixture | RT-5, RT-17 |
| Checks C14, C15, C16, C24, C30, C31, C36, C57 | 531 | R8 (row 24) | G | `conformance` job | per-check tests | RT-17 |
| Checks C39, C53 | 287 | R8 (row 25) | P | `conformance` job | `c39`, `c53` | RT-5 |
| Check C71 and `baselines/C71.json` (2,820 lines) | 372 | R8 | P | `conformance` job | `c71-test-timing.test.mjs` | RT-9 |
| The other 29 family checks | about 2,500 | R8 (row 25) | P | `conformance` job | per-check tests | RT-17 |
| `conformance/src/commit-lint.mjs` (+ Debrief's copy) | 112 (+67) | G4 | G | `commit-lint` job (`ci.yml:901`) | `c23-commit-lint.test.mjs` | RT-11 |
| `conformance/src/sync/*` | 364 | R8 (row 26) | S | `conformance` job (C23, C28) | `sync-guard`, `c23-blocks`, `c28` | RT-17 |
| `conformance/src/notices`, `bin/paddock-notices.mjs`, `scripts/notices.sh` | 457 | R6 | S | `app-gates`, `debrief.yml` | `notices.test.mjs` | RT-19 |
| `conformance/src/sidecar`, `bin/paddock-sidecar.mjs` | 174 | R6 | P | `sidecar-self-test` (release path) | `sidecar*.test.mjs` | RT-19 |
| `conformance/src/window-weight`, `bin/paddock-timing-baseline.mjs` | 201 | R6 | P | Local | `c29-window-weight` | RT-19 |
| `scripts/ci-timings.mjs` | 355 | G9 | G | none | none | RT-8 |
| `scripts/coverage-gate.mjs` | 92 | G7 | G | `ci.yml:669` | `coverage-gate.test.mjs` | later (G7) |
| `scripts/dialog-commands.mjs` | 79 | R15/R19 | P | `ui` package scripts | yes | later (R19) |
| `scripts/new-app.mjs` | 580 | R7 | P | none (tests only) | `new-app.test.mjs` | RT-21 |
| `scripts/setup-dev.sh`, `tools/bootstrap.sh`, `ci-image/*.sh` | 305 | R20 | P | `ci-image.yml` (verify) | `bootstrap.test.mjs` | later (R20) |
| `scripts/vm-lock*.sh`, `vm-win.sh`, `vm-win/*`, `vm-test-support.mjs` | 963 | R5 | S | Local (unit tests in `ci.yml:399`, macOS) | `vm-lock`, `vm-win` tests | later (after the VM redesign) |
| `scripts/vm-smoke/*`, `tools/macos-vm-smoke.sh` | 9,439 | R5 (row 30) | S | Local (unit tests in `ci.yml:373`) | 44 test files | later (after the VM redesign) |
| `scripts/shell-demo/*`, `tools/linux-container.sh`, `verify-installed.sh` | 488 | R5 | S | `service-smoke.yml` (two scripts) | partly | later (R5) |
| `scripts/perf/link-time/*` | 107 | R22 | P | none | none | later (R22) |

## 2. The new ADRs' tools, born in Rust

None of these may be written as a script first (ADR 0022 G.10 decision 9: no new tooling in Node, shell, Python or
PowerShell). Where an ADR's text names a script, the right column says what changes; each such change needs a one-line
amendment note in that ADR (decision D1 below).

| ADR | Tool the ADR asks for | Text of the ADR | Built as | RT |
|---|---|---|---|---|
| 0083 (accepted) | worktree tool with owner locks; `check primary` | `./dev worktree add/claim/reassign/release/drop/list/clean` (Rust already) | as written | RT-3 |
| 0083 (accepted) | guard hook R1-R8, owner map under `flock` | `.claude/hooks/own-worktree.sh` plus a Python matcher and `shellparse.py` | `./dev hook pre-tool-use` (Rust), behind a 6-line POSIX shim; §2.1 | RT-4, RT-7 |
| 0082 (accepted) | `./dev ci flakes --app <app> --days 30`; C71 sees counts (eight rows) | C71 extended in the JS conformance | `./dev ci flakes`; C71 **ported** to `./dev check` with its baseline, then extended there | RT-8, RT-9 |
| 0081 (accepted) | the liveness check over every look, in C53's scanner (`maskRust`), with `docs/liveness.toml` | a JS check next to C53 | `./dev check`: C39's masker and C53 ported first, the liveness check added beside them | RT-5, RT-10 |
| 0085 (accepted) | `release-isolation setup` and `verify` with a ledger, identity freshness, `.crate` hashes | `conformance/ci/release-isolation.sh` | `./dev ci release-isolation` with `setup`, `verify` and `record` (Rust); §2.2 | RT-2 |
| 0025 amendment (proposed) | `{src}` in the kept-folder key, the build stamp, `verify`; `./dev target-dir` | shell first (D5 (a)), Rust `./dev target-dir`, registered as a copy | Rust only: `./dev ci build-folder` and `./dev target-dir` share one stamp module; no copy to register | RT-6 |
| 0038 amendment (proposed) | `./dev push-gate`, `./dev hooks install`, tracked git hooks, an agent push hook | Rust `push-gate`; agent hook `.claude/hooks/push-gate.sh` | as written, with the agent hook as one more rule of `./dev hook` | RT-4, RT-7, RT-12 |
| 0022 (R4) | the planner switch | – | `feat/rust-planner-ci` | T0 |

### 2.1 Can ADR 0083's hook be Rust? Yes, if it never runs the `./dev` wrapper

Measured on this Mac mini at a load average of 9.6 to 11.8 (results in [benchmarks](../benchmarks.md#hook-process-start-cost-2026-11-10)):

| Per call | Time |
|---|---|
| A compiled `paddock-dev` binary started directly | 4.1 to 6.4 ms |
| The current `no-pgrep-wait.sh` (bash plus `jq`, no Python) | 9.8 to 16.6 ms |
| `python3 -I` that only imports `json`, `sys`, `re`, `shlex` and reads the input (no matching yet) | 21.6 to 23.9 ms |
| `./dev --version` through the wrapper (git, `rustc -vV`, hashing the inputs) | about 160 to 200 ms |

So a Rust hook costs less than the Python matcher ADR 0083 §2 plans (ADR 0083 measured about 50 ms for shell plus Python
at a load of 53), on condition that the hook calls an **installed** binary and never the wrapper: the wrapper adds 150 ms or
more per call and, on a cache miss, a cargo build of minutes inside a tool call. The design:

- `./dev hooks install` copies the running `paddock-dev` binary to `<git-common-dir>/paddock/hooks/paddock-dev` (atomic
  rename; shared by all worktrees, as `core.hooksPath` is), writes a stamp with its input hash, and sets
  `core.hooksPath=tools/githooks` (ADR 0038 amendment A2). It refuses a binary whose hash is not `origin/main`'s unless
  `--local` is given, so a branch's half-built rules never guard everyone.
- The tracked `.claude/settings.json` runs one shim for every guarded tool, `tools/githooks/claude-pre-tool-use` (POSIX
  `sh`, bootstrap exception (c)): it `exec`s the installed binary `hook pre-tool-use`. With no binary installed it prints a
  deny whose reason is "run `./dev hooks install`", and lets exactly that command through.
- One process per tool call runs every rule (no-pgrep-wait, no-force-push, own-worktree R1-R8, the push-gate stamp), so
  three hooks cost one start, not three.
- The owner map and its `flock` are one Rust module used by `./dev worktree` and the hook (rule 55: one mechanism, no
  Python copy of the map format). `paddock-slots` already locks with `File::try_lock` (`crates/paddock-slots/src/lib.rs:3`).
- Shell parsing: ADR 0083 removes quoted text and heredoc bodies and resolves literal variables. Candidates, read on
  crates.io on 2026-11-10: `brush-parser` 0.4.0 (MIT, updated 2026-05-03, <https://crates.io/crates/brush-parser>), a
  full POSIX/bash parser; `shlex` 2.0.1 (MIT OR Apache-2.0, <https://crates.io/crates/shlex>), word splitting only;
  `tree-sitter-bash` 0.25.1 (MIT, C grammar); `conch-parser` (last release 2019, unmaintained); `yash-syntax` (GPL-3.0,
  not allowed). RT-4 starts with `brush-parser` and keeps a hand lexer as fallback if its start cost or binary size is
  out of budget (measured in RT-4, recorded in `docs/benchmarks.md`).
- `./dev check primary` (RT-3) also reports an installed hook binary older than `origin/main`'s hash.

### 2.2 Release isolation in Rust, and the bootstrap

ADR 0085 §4 puts `setup` first, before `build-jobs` and `job-isolation`, so that the slot holder is already "the run's own
`paddock-dev`". With `setup` in Rust, the tool must exist before `setup` runs. The order becomes: the `dev` wrapper, with
`PADDOCK_RELEASE_MODE=1`, makes its own `CARGO_HOME` and build folder with `mktemp -d` under `$RUNNER_TEMP` and builds
the tool fresh there (it already does a fresh build for release jobs: `PADDOCK_DEV_FRESH=1`, and never downloads, ADR 0022
§3.3); then `./dev ci release-isolation setup` records those two folders (from `current_exe()` and the environment) in the
ledger as `dev-tool` along with the folders it makes for the job. `verify` then checks the tool's own `CARGO_HOME`
`.crate` hashes against `tools/paddock-dev/Cargo.lock` like the app's. The wrapper change is a few lines of the bootstrap
(exception (c)); everything else is Rust. Actions append to the ledger with `./dev ci release-isolation record <kind>
<path> <mode>` instead of `printf`, so the format has one writer.

## 3. Rules for every package

- **Ownership.** Each package owns a new module folder under `tools/paddock-dev/src/<module>/` (or `tools/devkit/`),
  its tests, and the scripts it retires. Shared integration points are touched only by **declared additions**: in
  `tools/paddock-dev/src/cli.rs` one enum variant and its match arm; in `lib.rs` one `mod` line; in `Cargo.toml` the new
  dependencies. `Cargo.lock` conflicts between parallel packages are resolved by the integrator (`cargo update -w`, then
  `--locked` build). `.gitea/` and the shared actions are integration points: a package hands the integrator the exact
  one-line switch per action; it does not edit workflows itself.
- **Generic code stays generic.** RT-1, RT-3 and RT-6 land in `./dev` (where `ci slot` already is) but their engines read
  no Paddock path, app id or `PADDOCK_` name directly: those come from a config struct filled by a thin Paddock layer.
  When `devkit` gets its `ci` and `worktree` commands (G2, G10), the engine moves without a rewrite (G.8's check).
- **Parity before retirement.** Each package names its oracle: the old script's output (stdout, the `::error::` lines and
  exit code) on fixtures as goldens (`paddock_testkit::golden`), or the old script's own test cases run against both
  implementations through one variable (for example `ISOLATION_CMD`). A package with no old script (RT-2, RT-12) has
  positive controls instead (ER-33): a planted fault the new tool must catch.
- **Retirement: one release of parallel running** (decision D5). *Read-only commands* (plan, checks, verify, timings) run
  in shadow beside the old script in CI; a difference prints `::warning::` with the diff and fails the shadow step only.
  *Mutating commands* (slots, folders, clones) cannot run twice; the action switches to Rust and keeps the old script one
  variable away (`PADDOCK_CI_KIT=shell`) for one release. After the next app tag whose release run went through the Rust
  path green, the package's retirement PR deletes the script, its JS test, the fallback variable and its line in
  `docs/known-copies.toml` if any, and shrinks ADR 0022 §3.7's allow-list.
- **Load.** At most four tooling agents, each with `CARGO_BUILD_JOBS=2`, every build and test through `heavy`; no new agent
  while the 5-minute load is above about 12, hold above 20 (memory "agent concurrency"). Branch CI runs the Linux leg on
  every push and macOS and Windows only when an OS-specific module changes or before merge (rule 50).
- **Release windows** (ADR 0022 G.7): RT-1, RT-2, RT-6, RT-13 and RT-16 are on every app's release path. They land outside
  a release freeze the coordinator names; RT-2 lands before the next tag, because ADR 0085 gates it.

## 4. Work packages, in order

Order: first what takes the most shell and JS off the per-job critical path of CI, and what the accepted ADRs need now;
then what the proposed amendments need; then the rest of ADR 0022.

| # | Package | Retires | Days | Depends on | Lane |
|---|---|---|---|---|---|
| T0 | Planner switch (`feat/rust-planner-ci`, in progress) | `affected.mjs`, `affected-marks.mjs` (1,268) after the shadow release | – | – | 1 (now) |
| RT-1 | `./dev ci isolation` | `job-isolation.sh`, `job-tmpdir.sh`, `ci-home.sh` (667) | 3 | – | 2 (now) |
| RT-2 | `./dev ci release-isolation` (ADR 0085 WP-1a tool part, WP-2) | never writes `release-isolation.sh` | 3 | – | 3 (now) |
| RT-3 | `./dev worktree` and `./dev check primary` (ADR 0083 WP-1, WP-3) | Paddock's use of `worktree.sh` | 4 | – | 4 (now) |
| RT-4 | `./dev hook` dispatcher and `./dev hooks install` (ADR 0083 WP-0 in Rust; 0038 A2 install) | the two untracked shell hooks | 2.5 | – | 1 (after T0) |
| RT-5 | `./dev check` host, the Rust masker, C39 and C53 | C39, C53 (287 + tests) | 3.5 | – | 2 (after RT-1) |
| RT-6 | `./dev ci build-folder` with `{src}`, stamp, `verify`; `./dev target-dir`; `host prune` salvaged | `build-folder.sh`, three prune scripts, Debrief's `clean-build-dirs.ps1` (211) | 3 | RT-1; 0025 amendment accepted | 3 (after RT-2) |
| RT-7 | Guard rules R1-R8 and the push-gate stamp rule in `./dev hook` (ADR 0083 WP-2; 0038 WP-5) | – | 4 | RT-3, RT-4 | 4 (after RT-3) |
| RT-8 | `./dev ci flakes` and `./dev ci timings` (ADR 0082 WP-4 part; G9) | `ci-timings.mjs` (355) | 2 | – | 1 (after RT-4) |
| RT-9 | C71 port and its eight new rows; per-file `scope` metadata (0082 WP-4; 0038 WP-2) | C71 (372) | 3.5 | RT-5 | 2 |
| RT-10 | The liveness check and `docs/liveness.toml` (0081 WP-3, conformance part) | – | 1.5 | RT-5 | 3 |
| RT-11 | `tools/devkit` skeleton (G2 without the registry tag) and `devkit commit-lint` (G4) | `commit-lint.mjs` and Debrief's copy (179) | 2.5 | – | 1 |
| RT-12 | `./dev push-gate` and the git hooks (0038 WP-1, WP-3, WP-4); lefthook's push and commit entries removed | lefthook `pre-push`, `commit-msg` | 5 | RT-5, RT-6, RT-11; 0038 amendment accepted | 4 |
| RT-13 | CI kit, part 2 (rest of R1) | `build-jobs.sh`, `kept-clone.sh`, `bounded.sh`, `run-commands.sh`, `guardian-as-user.sh`, `build-env.mjs`, `host-cleanup.sh` (847) and about 490 inline action lines | 4.5 | RT-1 | 3 |
| RT-14 | `devkit run`, `devkit ci error`, `skip`, `summary` (G3) | nothing on the train (`ci-log.*` and `run-native.mjs` were on unmerged branches); stops new copies | 1.5 | RT-11 | 2 |
| RT-15 | `./dev ci plan-gate`, `release-verdict`, `check-tag`, `check-index` | four `ci/*.mjs` (161) | 1.5 | T0 | 1 |
| RT-16 | Release kit (R2 with G8, G11, G12), dry run per app (rule 54) | `conformance/release/*` (1,092), `stage-paddock-setup.mjs`, `demo-release.mjs`, `scripts/demo/*`, `tauri-bundle.sh`, `first-release-notes.sh`, `release-notes.mjs`, `dev-tool-publish.sh` | 6 | RT-2 (ADR 0085 also edits `tauri-bundle.sh`) | 2 |
| RT-17 | Conformance remainder in four batches of about eight checks, `sync`, the engine split into `paddock-repo-rules` and `devkit check` (R8) | `conformance/src`, `conformance/test` | 7 | RT-5; R8a note approved for the split | 3, 4 |
| RT-18 | `./dev fetch`, salvaged from `tools/dev-fetch-r3` (R3) | `install-test-browser.mjs`, `pin-runtime.mjs`, `runtime-catalogue.mjs` (428) | 2 | – | any |
| RT-19 | Notices (salvage `tools/dev-notices`), sidecar self-test, window weight (R6) | 832 | 2 | – | any |
| RT-20 | App workflows' inline `run:` blocks (R10) | about 780 inline lines | 2.5 | RT-13 | any |
| RT-21 | `new-app`, packages publish (R7) | `new-app.mjs`, `scripts/publish.mjs` (1,016) | 2 | – | any |
| RT-22 | `paddock-tool` runtime (G1) and the switch of `./dev`, `devkit`, `wiki`, `heavy` | – | 2.5 | RT-11 | any (outside release windows: `crates/`) |
| | **Total RT-1 to RT-22** | about 9,400 source lines plus their tests | **69** | | |

The first eight (RT-1 to RT-8) are **25 agent-days**. With T0 in lane 1 and RT-1 to RT-3 in lanes 2 to 4, the dependency
chains are RT-3 (4) then RT-7 (4), and RT-1 (3) then RT-5 (3.5) then RT-9 (3.5); at four agents the first eight finish in
about 8 working days if load allows, and RT-1 to RT-22 in about 4 calendar weeks.

**Not scheduled here (ADR 0022's long tail, re-estimate before scheduling):** R5 hosts and containers (3), the VM harness
after its redesign and `feat/dev-vm-broker` (not estimated in the ADR at today's 9,439 lines; about 8, unverified), R11
diagrams (5), R12 icons (4.5), R13 images (2), R14 npm glue (1), R19 UI tooling (10), R20 setup and image (3), R21 Debrief
xtasks (8), R22 (1.75), R23 (0.5), G5 to G7 (2.5), R16 clean-up (1), and the adopted features G13 to G26 (23, of which
R17 `--pre-push`, G15 worktree hooks and G26 gate stamps overlap RT-3, RT-7 and RT-12). About 70 agent-days.

## 5. Briefs for the first eight packages

**RT-1 `./dev ci isolation` (3 days).** Port `conformance/ci/job-isolation.sh` (464), `job-tmpdir.sh` (175) and `ci-home.sh`
(28) to `./dev ci isolation acquire|release|status|slots|path|temp|owns|port|pattern|sweep-slot`, with the same environment
exports (printed as `KEY=value` lines the action appends to `$GITHUB_ENV`), the same `kept-folders.txt` and `ports.txt`
data, and the slot holder it already uses (`./dev ci slot`). Owns `tools/paddock-dev/src/ci_isolation/`, its tests, and a
variable `ISOLATION_CMD` in `conformance/test/job-isolation.test.mjs` so its 594 lines of cases run against both
implementations (the oracle). Hands the integrator the one-line switches for the `job-isolation` and `job-tmpdir` actions
and the 12 direct `job-isolation.sh` calls in workflows and other actions, with `PADDOCK_CI_KIT=shell` as the fallback for one release. Engine
reads no Paddock name directly (config struct). Retirement: after the next app tag built through it, delete the three
scripts and move the remaining JS cases into Rust tests. On the release path: land outside a freeze.

**RT-2 `./dev ci release-isolation setup|record|verify` (3 days).** The tool part of ADR 0085 WP-1a and all of WP-2, born
in Rust instead of `conformance/ci/release-isolation.sh`. `setup` makes `CARGO_HOME`, the build folder, the npm cache and
the clone folder with fresh temp folders, records device, inode (and ctime on Unix) per kind, and adopts the tool's own
folders as `dev-tool` (§2.2; the few wrapper lines for `PADDOCK_RELEASE_MODE=1` are part of this package). `record` is the
only ledger writer. `verify` checks the ledger (§5 item 1, `check broken` for a missing expected kind), identity
freshness, every `.crate` in `CARGO_HOME/registry/cache` against the `Cargo.lock` checksums, the configuration probes, and
prints one `::error::` per finding with the next step. Owns `tools/paddock-dev/src/release_isolation/` and its tests
(ADR 0085 §5.1: the old-mtime fixture, the mutation check, synthetic folders under the shared test scratch). No oracle
script exists: the positive control is a planted leak per kind that `verify` must name. The workflow and action wiring
(WP-1a's `setup-rust`, `build-folder`, `kept-clone`, `node-caches` changes, WP-3's dispatch) stays with ADR 0085's
integrating branch. Gates the next app tag.

**RT-3 `./dev worktree` and `./dev check primary` (4 days).** ADR 0083 WP-1 and WP-3: `add` (always `--lock` with the
owner note, sparse cones, `--detach` review copies, prints path, branch and `CARGO_TARGET_DIR`), `claim`, `reassign`,
`release`, `drop`, `list` (lock age, claimed agent, merged into `origin/main`, dirty, a `lsof` hint), `clean` (only
unlocked, merged, clean, no process inside; never `-f -f`, never `rm`), JSON output, and `check primary` with `--live` and
its positive control. Owns `tools/paddock-dev/src/worktree/` including `owners.rs` (the owner map read-check-write under
an exclusive `File::lock` on the sidecar lock file, atomic rename), which RT-7 reuses. Tests on throw-away repositories
under the shared test scratch, never the real repository. Oracle: engineering-rules `scripts/worktree.sh add|list|clean`
on the same throw-away repositories (same worktrees, same clean decisions, except where ADR 0083 changes them: locked and
unmerged are kept). Root from config (`/Volumes/d7sst7/dev-targets/wt` is Paddock's value, not code). Retirement: the
Paddock skills and brief templates name `./dev worktree`; `worktree.sh` goes when `devkit worktree` ships (G10).

**RT-4 `./dev hook pre-tool-use` and `./dev hooks install` (2.5 days).** The dispatcher of §2.1: read Claude Code's
PreToolUse JSON from stdin, run every rule, print one `hookSpecificOutput` deny or nothing; malformed input passes with
one stderr line (ADR 0083). First rules: no-pgrep-wait and no-force-push, ported from the two shell hooks (their regexes
are the oracle on a corpus of at least 40 commands each, including the cases ADR 0083's table lists). The shell lexer
(quotes, heredocs, `&&`/`;`/pipes, `cd` and `git -C` tracking, literal variables) with `brush-parser` first; record start
cost and binary size growth in `docs/benchmarks.md`. `hooks install` copies the binary to `<git-common-dir>/paddock/hooks/`,
refuses a non-`origin/main` hash without `--local`, sets `core.hooksPath=tools/githooks`. Owns
`tools/paddock-dev/src/hook/`, `tools/githooks/claude-pre-tool-use`, and the new tracked `.claude/settings.json` (the
coordinator moves the untracked copies aside, ADR 0083 WP-0). Retirement: delete `.claude/hooks/*.sh` after one release.

**RT-5 `./dev check` host, the masker, C39 and C53 (3.5 days).** The Paddock half of R8, enough for the accepted ADRs:
findings, the report JSON in `conformance/src/report.mjs`'s shape (so the conformance job can merge both), expiring waivers
(`util/waivers.mjs` semantics), shrink-only baselines by text (C71's, lesson 2026-11-04), `--only`, `--files` (per-file
scope, for `push-gate`), `--json`. Port C39's `maskRust` (comment and string masking) with a golden over **every** tracked
`.rs` file (masked output byte-equal to the JS one), then C39 and C53. Owns `tools/paddock-dev/src/check/` and extends the
existing `CheckCommand` enum by declared lines only. Oracle: the JS runner's findings for C39 and C53 on the train tree
and on their fixtures; shadow in the conformance job for one release. Retirement: remove C39 and C53 from
`conformance/src/checks/index.mjs` and their JS tests.

**RT-6 `./dev ci build-folder` and `./dev target-dir` (3 days).** The ADR 0025 amendment in Rust only: the `{src}` part of
the kept-folder key (from RT-1's `path`), the `.paddock-build-source` stamp, clean on mismatch with one log line, `verify`
after the build (0 files counted is `check broken`), and `./dev target-dir <workspace>` for local worktrees with the same
stamp module, so no copy is registered. Salvage `tools/dev-host-prune`'s `host prune` (11 commits, its golden cases of
the old prune scripts) by rebasing onto the train. Owns `tools/paddock-dev/src/build_folder/` and the prune module.
Failing-first test: the amendment's two-roots stale reproduction with a real tiny `cargo build`. Oracle: `build-folder.sh`
behaviour in `ci-actions` and `prune-and-cleanup` tests. Hands the integrator the `build-folder` and `prune-build-folder`
action switches. Needs the amendment accepted with D5 changed to Rust (decision D2).

**RT-7 Guard rules R1-R8 and the push-gate stamp rule (4 days).** ADR 0083 WP-2 on RT-4's dispatcher and RT-3's owner
map: R1-R8 as specified (primary checkout writes, locked-worktree deletes, owner binding, read-only roles, worktree tool
calls), the Serena allow-list (D11), the `agent_id` record on `add`/`claim` under the same lock; plus the ADR 0038
amendment's agent rule (deny `git push` without a green stamp for the head, deny `--no-verify` and `-c core.hooksPath=`,
coordinator bypass with a logged reason). Oracle: ADR 0083's case table (about 90 cases) as a Rust table test, the
concurrent-claim `flock` test (exactly one of two passes), and one real subagent call confirming `agent_id` and
`agent_type` reach the hook. Owns `tools/paddock-dev/src/hook/rules/worktree.rs` and `push_gate.rs`. Records the hook's
cost (rule 48).

**RT-8 `./dev ci flakes` and `./dev ci timings` (2 days).** `flakes --app <app> --days 30` lists tests and specs that
failed or were marked FLAKY (nextest `flaky-result = "fail"`, Playwright `failOnFlakyTests`) in the app's runs, as ADR
0082 D6 (a) needs before Debrief's WP-0 gate and for Dyno's and Pitcrew's next tags. Reuse `plan_since.rs`'s `Http`
trait, `GiteaRuns` and its nextest and vitest log parsers; add the run listing in a new file. `timings` replaces
`scripts/ci-timings.mjs` (G9; the repo list as config). Owns `tools/paddock-dev/src/ci_runs/`. Oracle: `ci-timings.mjs`
on recorded API replies (no live server in tests); `flakes` on recorded logs with planted FLAKY and failed tests.
Retirement: delete `scripts/ci-timings.mjs` (no CI caller) on landing; engineering-rules' copy after one release.

## 6. Decisions for the user

| # | Question | Recommendation |
|---|---|---|
| D1 | ADR 0083 §2 (hook in shell plus Python) and ADR 0085 §4 (`release-isolation.sh`) name scripts. Amend both to the Rust commands of §2? | Yes: one-line amendment notes. The Rust hook measured cheaper (§2.1), and the owner map then has one implementation. |
| D2 | ADR 0025 amendment D5: shell first (1.5 days, a registered copy) or Rust only (RT-6, after RT-1)? | Rust only. It delays the CI fix by about 3 days (RT-1 first); if the user wants the CI class closed first, keep (a) and RT-6 deletes the shell copy. |
| D3 | Start the Paddock-side `./dev check` host (RT-5) before the R8a joint rules-model note is approved? | Yes, for Paddock checks only; R8a (1 day, researcher) runs in parallel and gates only RT-17's generic split. |
| D4 | Start `devkit` (RT-11) before the `paddock-tool` runtime (G1)? | Yes, on `paddock-cli` and `paddock-log` directly; G1 (RT-22) extracts the runtime later. |
| D5 | "One release of parallel running": shadow for read-only commands, a fallback variable for mutating CI commands, deletion after the next app tag built through the Rust path. | Yes. |
| D6 | ADR 0022 G.7 caps tooling at two agents during a release. "asap" and this brief allow four. | Four, bounded by load; release-path packages (RT-1, RT-2, RT-6, RT-13, RT-16) still never land inside a release freeze. |

## 7. Risks

- **Lockfile and `cli.rs` conflicts** among four parallel packages in one Cargo workspace: declared additions only, the
  integrator merges in package order and builds `--locked` once per batch.
- **The hook binary goes stale or missing:** a deny with "run `./dev hooks install`" stops every tool call until it is run;
  that command alone passes. `check primary` reports a stale hash.
- **Bootstrap in release mode (RT-2):** a wrong wrapper change can make the tag job build the tool from the runner's kept
  `CARGO_HOME`, the very leak ADR 0085 closes; `verify` checks the tool's own `.crate` hashes too.
- **Golden drift:** the planner's golden uses `affected.mjs` as oracle; after T0 retires it, freeze the oracle's last
  outputs as fixtures before deleting the script (same for every retirement).
- **Release windows** can stall the CI-kit packages; RT-3 to RT-5, RT-7 to RT-11 are off the release path and fill the gaps.
- **Stale salvage branches** (`dev-host-prune`, `dev-fetch-r3`, `dev-notices`) are 2,400 commits behind; a rebase may cost
  as much as the estimate saves. Each salvage package decides in its first hour and says so.
