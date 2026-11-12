# CI logic in Rust: YAML steps become `./dev ci <step>`

- Status: plan (low-priority lane). Decided by the user on 2026-11-12: move CI logic out of the workflow YAML into the Rust
  tool, the xtask pattern. No new ADR; this note carries out ADR 0022's R1 and R10 (and the release part of R2 after the
  tags) and adds the wrapper contract and the order below. A short amendment in ADR 0022 points here.
- Date: 2026-11-12
- Heads read (ADR 0091): `origin/train/2026-11-11-5` at `607bb7928` (the base of this branch), `origin/main` at `e2a318345`,
  `origin/docs/adr-0022-generic-tools` at `9e3ae2bea` (an ancestor of the train: its ADR 0022 amendment is in the text read).
  `origin/ci/app-failed-only-and-schedule-skip` (another agent) at `6f72f2d74`, re-read at `bb539cde7`: it edits debrief.yml,
  dev-tool.yml and ci-actions.test.mjs too; this branch's edits there (job `env:` lines, tests) sit in other hunks, and a trial
  `git merge-tree` of this branch with it is clean. Review fixes of 2026-11-12 read `origin/feat/ci-in-rust-setup-rust` at
  `c46349f30`; re-read before committing: `origin/train/2026-11-11-5` at `d27c31090` (one Debrief test fix, none of this
  branch's files; trial merge clean), `origin/main` unchanged.
- Lands after train 5: the port calls `dev ci isolation` in process (`tools/paddock-dev/src/ci_isolation`), as on the train.
- Related: [ADR 0022](../adr/0022-rust-developer-and-ci-tool.md) (§3.3 how CI gets `./dev`, §3.6 testing, §3.7 one command at a
  time, §7 R1/R2/R10, G.5 tool logging, G.7 the release-path guard), ADR 0025 (job isolation), ER-27 (logs explain every
  failure), ER-37 (common practice first).

## 1. Inventory (measured)

Measured on `607bb7928` with one script: every `run:` value in `.gitea/**/*.yml`; "lines" are the non-blank lines of a block
scalar (or 1 for a one-line `run:`), "code" leaves out comment lines, "long" counts blocks of more than 10 code lines (ADR 0022
§3.7's definition). Total: **1,288 lines (1,183 code) in 287 `run:` blocks, 26 of them long.** The brief's figures (about
1,300; debrief.yml 317) were measured another way; the per-file numbers below are the ones this plan is held to.

| File | Lines | Code | Blocks | Long | Release path? | Step |
|---|---:|---:|---:|---:|---|---|
| workflows/debrief.yml | 280 | 260 | 72 | 6 | `bundle`, `release`, `keychain` jobs | 3 (rest), 5 (release jobs) |
| workflows/ci.yml | 148 | 140 | 70 | 1 | rule 54 gates only | 3 |
| actions/setup-rust | 143 | 119 | 5 | 3 | used by release `bundle` jobs | **1 (this branch)** |
| actions/stable-workspace | 106 | 85 | 1 | 1 | used by Debrief's `bundle` | 2 |
| workflows/app-release.yml | 50 | 49 | 11 | 1 | yes | 5 |
| actions/app-gates | 52 | 45 | 8 | 1 | gates of release dispatches | 3 |
| workflows/debrief-bench.yml | 44 | 42 | 8 | 2 | no | 3 |
| actions/host-tools | 36 | 33 | 1 | 1 | no | 2 |
| workflows/shell-demo-release.yml, workbench-demo-release.yml | 33 + 33 | 33 + 33 | 13 + 13 | 0 | yes | 5 |
| workflows/service-smoke.yml | 29 | 27 | 7 | 1 | no | 3 |
| workflows/canary.yml | 27 | 26 | 4 | 0 | no | 3 |
| actions/node-caches | 27 | 24 | 1 | 1 | used by release jobs | 2 |
| workflows/app-fuzz.yml | 26 | 26 | 4 | 2 | no | 3 |
| actions/publish-release | 25 | 21 | 1 | 1 | yes | 5 |
| workflows/nightly.yml | 20 | 20 | 12 | 0 | no | 3 |
| workflows/publish.yml | 18 | 16 | 4 | 0 | yes (packages) | 5 |
| debrief-fuzz, -mutants, -runner-check, -corpus | 15 + 13 + 12 + 10 | 50 | 15 | 1 | no | 3 |
| actions/sidecar-self-test, keychain, strip-signing-keys, macos-verify, tauri-bundle, release-verdict | 15 + 12 + 9 + 8 + 3 + 1 | 46 | 7 | 1 | yes | 5 |
| actions/plan, install-test-browser, host-cleanup, sccache-stats, prune-build-folder, conformance, nightly-mark, and six one-line actions | 12 + 14 + 7 + 5 + 4 + 4 + 2 + 6 | 52 | 13 | 2 | plan runs in release workflows | 2 |
| other workflows (ci-token-check, ci-image, bench template, dev-tool, per-app release callers) | 12 + 9 + 8 + 4 + 6 | 39 | 14 | 1 | the release callers | 3 / 5 |

**Scripts** (`conformance/ci` and `scripts`, `.sh`, `.mjs`, `.ps1`): **182 files**. `conformance/ci`: 22 files, 2,316 lines.
`scripts`: 160 files, 20,774 lines, of which 58 are tests and 115 are `scripts/vm-smoke` (the local VM start check, ADR 0022
R5, not CI). The CI-relevant ones, by size: `affected.mjs` 1,237 (ported as `./dev plan`, retirement only), `job-isolation.sh`
464 + `job-tmpdir.sh` 170 + `ci-home.sh` 28 (ported as `./dev ci isolation`, retirement only), `publish.mjs` 436 (release),
`ci-timings.mjs` 355 (ported as `./dev ci timings`, retirement only), `kept-clone.sh` 241, `build-jobs.sh` 215, `demo-release.mjs`
197 (release), `install-test-browser.mjs` 183, `coverage-gate.mjs` 131, `pin-runtime.mjs` 127, `runtime-catalogue.mjs` 118,
`tauri-bundle.sh` 100 (release), `dev-tool-publish.sh` 87, `first-release-notes.sh` 84 (release), `guardian-as-user.sh` 83,
`boot-volume.sh` 79, `release-verdict.mjs` 62 (release), `release-notes.mjs` 45 (release), `build-folder.sh` 38, `plan.mjs` 38,
`check-tag.mjs` 36 (release), `bounded.sh` 36, `build-env.mjs` 33, `check-index.mjs` 25, `run-commands.sh` 24.

## 2. Order

ER-1 and ADR 0022 G.7 hold: R1, R2 and R10 are release-path packages, so nothing here changes what a release run executes
before the tags, and at most this one low-priority lane runs while a release is in flight.

1. **setup-rust** (this branch, `feat/ci-in-rust-setup-rust`). See §5 for the bootstrap and the release guard. It also adds
   `paddock_proc::run_waited` (additive: no existing function changes), so every crate and app is in its CI plan; ADR 0022
   G.7 keeps `crates/` changes out of an app's release window, so it lands outside one.
2. **stable-workspace**, then the other non-release composite actions (host-tools, node-caches, install-test-browser, plan,
   host-cleanup, sccache-stats, prune-build-folder) and the scripts they call (`build-jobs.sh`, `kept-clone.sh`,
   `build-folder.sh`, `build-env.mjs`, `bounded.sh`). Each action used by a release job gets the same guard as setup-rust.
3. **The non-release parts of ci.yml and debrief.yml**, then the small workflows (bench, canary, service-smoke, app-fuzz,
   nightly, Debrief's fuzz/mutants/corpus/runner-check). Debrief.yml and the app workflows wait until
   `ci/app-failed-only-and-schedule-skip` (pushed at `6f72f2d74`) has landed.
4. **The scripts:** retire the three already ported (`affected.mjs` after the R4 shadow, `job-isolation.sh` family,
   `ci-timings.mjs`), then `coverage-gate.mjs`, `install-test-browser.mjs` with `pin-runtime.mjs` and `runtime-catalogue.mjs`
   (R3), `plan.mjs`, `guardian-as-user.sh`, `boot-volume.sh`, `run-commands.sh`, `dev-tool-publish.sh`. The VM tools stay
   local (R5, its own lane).
5. **The release path, after the tags:** app-release.yml and the per-app release workflows, tauri-bundle, publish-release,
   strip-signing-keys, macos-verify, sidecar-self-test, keychain, release-verdict, release notes, check-tag, publish.mjs,
   demo-release.mjs; and the removal of every release guard and shell copy kept by steps 1 to 3. Each with a
   `workflow_dispatch` dry run of `app-release.yml` per app (ADR 0022 §7, phase 2 acceptance).

What stays in YAML at the end: `uses:` of actions that must stay (`actions/checkout`, `actions/cache`, the v3 artifact
actions), the `./dev` bootstrap (CARGO_BUILD_JOBS and the toolchain, §5), `if:` conditions and step wiring, and one
`./dev ci <step> ...` line per step.

## 3. The wrapper contract

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

## 4. Proving parity, per step

- **A behaviour test against the old shell, before it is deleted.** While a ported step's shell still exists, a test runs
  the shell (extracted from the YAML or script, run as the runner runs it: `bash -e -o pipefail`) and the Rust command on the
  same inputs, folders and shim programs, and compares everything that reaches the job: exit code, `$GITHUB_ENV`,
  `$GITHUB_PATH`, `$GITHUB_OUTPUT`, and every call of the wrapped programs with the variables that steer them. For a script
  with fixtures, a golden file of its output (ADR 0022 §3.6) instead. When the shell is deleted the comparison becomes a
  golden of the Rust output, and the test stays.
- **The probe can see a difference** (ER-33): each new parity test is shown to fail on a deliberate difference before it
  counts. For setup-rust the first run caught a real one (§5).
- **One CI run per ported file**, on the branch, with the jobs that use the step; and the step's existing conformance tests
  green. The release path's version of this is the dry run per app (step 5).

## 5. Step 1: setup-rust

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
step 5, and the copy is registered in `docs/known-copies.toml` (`ci-setup-rust`).

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

## 6. Cost in agent-days

| Item | Days | Notes |
|---|---:|---|
| 1 setup-rust (this branch), plan note | 1 | done; after the tags, removing its guard and shell copy: 0.25 |
| 2 stable-workspace | 1 | one block of 85 code lines, macOS link and Windows junction branches |
| 2 the other non-release actions and their scripts (build-jobs, kept-clone, build-folder, build-env, bounded) | 2.5 | build-jobs and kept-clone carry most of it |
| 3 ci.yml non-release jobs | 1 | 140 code lines, mostly short |
| 3 debrief.yml non-release jobs | 1.5 | 260 code lines, 6 long blocks; after the other agent's branch |
| 3 small workflows | 1.5 | bench, canary, service-smoke, app-fuzz, nightly, Debrief's four |
| 4 retire three ported scripts | 0.5 | affected.mjs after its shadow (R4), the job-isolation family, ci-timings |
| 4 remaining CI scripts | 3 | coverage-gate, the test-browser trio (R3), plan.mjs, guardian-as-user, boot-volume, run-commands, dev-tool-publish |
| 5 release path, after the tags | 4.5 | ADR 0022 R2's 3 days plus app-release.yml, the demo releases and the guard removals; dry run per app |
| Review rounds (one per item, rule: one round) | 2 | |
| **Total** | **about 18.5** | against ADR 0022's R1 + R10 + R2 + R3 of 9.5 days; the difference is mostly the workflows' growth since 2026-10-27 and the parity tests |

## 7. Common practice, and the cheaper option (ER-37)

- **The xtask pattern.** cargo-xtask (<https://github.com/matklad/cargo-xtask>, read for ADR 0022) describes repository
  automation as a Rust binary in the repository, called from CI with one command per task; rust-analyzer's own CI runs
  `cargo xtask` steps (<https://github.com/rust-lang/rust-analyzer/tree/master/xtask>, not re-read for this note). This plan is
  that pattern, through the `./dev` wrapper.
- **What the platform documents.** GitHub's composite actions run shell steps or scripts
  (<https://docs.github.com/en/actions/sharing-automations/creating-actions/creating-a-composite-action>, not re-read for this
  note); Gitea Actions follows GitHub's syntax and shows annotations as plain log text
  (<https://docs.gitea.com/usage/actions/comparison>, read for ADR 0022 G.5). Neither asks for logic to leave YAML; most
  projects keep long steps as checked-in shell scripts with shellcheck.
- **The cheaper common-practice option (a decision for the user).** Move each long `run:` block into a checked-in `.sh`
  script under `conformance/ci` with shellcheck and the existing conformance tests, and leave short steps inline: about 0.1 day
  per file, about 3 days for the inventory above, and no bootstrap cost: no tool to build or download before the step, no
  release guard and no parallel shell copy. It keeps two languages and the
  OS branching in shell (Git Bash on Windows, bash 3.2 on macOS), which is where most of the kit's past CI fixes were
  (ADR 0022 §Context). The user's 2026-10-31 and 2026-11-12 decisions chose Rust; this option is recorded so it stays a choice.

## 8. Checks that keep it so

The tests of §4 per step; the conformance test "setup-rust uses ./dev only off the release path" (ci-actions.test.mjs); the
copy register entry until step 5; and ADR 0022 §3.7(c)'s counted baselines of long and OS-branching `run:` blocks, which are
not built yet (this note's §1 table is their first measurement).
