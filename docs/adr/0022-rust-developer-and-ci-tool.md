# 22. All tooling in Rust: one developer and CI tool (`paddock-dev`) on shared repository crates, after the CI kit consolidation

- Status: Accepted (2026-10-28, after an independent review). Direction decided by the user on 2026-10-27: everything in Rust (option C below), after
  the consolidation WP-C1 to WP-C6, with conformance as a shared Rust rules crate; only the categories shown to lose
  something real stay non-Rust, each with its reason. The five open decisions were taken on 2026-10-28 (Decisions
  taken).
- Amendment 2026-10-31 (generic tools `devkit`, `wiki`, `heavy`; extensibility; tool logging): **Proposed amendment**,
  section "Amendment (2026-10-31, Proposed amendment)". Its three decisions were taken by the user on 2026-10-31 (G.10);
  it stays Proposed until an independent review.
- Date: 2026-10-27
- Related: rules 3–5 (affected tests, fix locally, failing test first), rule 7 (tests never touch the machine), rule 13
  (bounded parallelism), rule 22 (independent review of images), rule 23 (ad-hoc sealing), rule 40 (recurring bugs mean
  a missing design rule), rules 42/43 (share first, shared behaviour tested once); ADR 0009 (declared contracts), ADR 0014
  §1, §6–§7 (no root workspace, affected CI, no task runner); ADR 0021 (portable tests, counted waivers);
  `apps/pacenotes/docs/proposal.md` §9.7, §14, §23, §24
- Evidence: [research note: Nx, Turborepo or our own scripts](../research/2026-10-nx-vs-scripts.md) (§1, §4–§6, §9);
  [research note: Rust tooling inventory and measurements](../research/2026-10-rust-tooling-inventory.md) (every script,
  per file; method; timings); `origin/main` at `1f40b4efe`

## Context

The user decided on 2026-10-27 to consolidate the apps onto the shared CI kit first (WP-C1 to WP-C6 of the research
note; WP-C1 Pitcrew is on `ci/consolidate-pitcrew`) and then to move the build, CI and developer scripts to Rust. After a
first draft of this ADR (which recommended porting only where it pays), the user chose **all Rust**: the kit's scripts,
the conformance checks, the icon, diagram and poster generators and the npm glue. Only what the comparison below shows
to be genuinely lossy stays in JavaScript or TypeScript.

### Why, stated honestly

The case does **not** rest on a fix-density claim. Recounted for this ADR, with this exact filter (details and the
list in the inventory note):

- commits in `git log --since=2026-10-13 --no-merges origin/main`, de-duplicated by subject (the imported app histories
  repeat some commits);
- the subject starts with `fix(<scope>)`, and the scope is one of `ci`, `build`, `scripts`, `macos`, `publish`,
  `conformance`, `release`, `notices` or `chromium,ci`;
- the diff touches a tooling path, including the apps' pre-import paths. The path matches
  `(^|/)\.gitea/(workflows|actions)/`, `(^|/)scripts/`, `^conformance/`, `^tools/`, `^ci-image/`, `^icons/tools/`,
  `^diagrams/(tools|scripts)/`, `^docs/images/src/`, `/docs/diagrams/src/` or `(^|/)xtask/`. This covers, for example,
  `apps/<app>/.gitea/workflows/build.yml` and Debrief's old crate `scripts/` folders.

The result is **37 commits**, the one denominator used below. Not counted:

- `ci:` commits, mostly the creation and set-up of the kit;
- `fix(...)` commits with an app or UI scope that touched a tooling path on the way: 35 more (`icons` 8, `ui` 7,
  `render` 3 and `app` 3 among them);
- 3 `fix:` commits without a scope.

Adding the four borderline scopes `render`, `chromium-driver`, `e2e` and `diagnostics` would give 43.

By defect area:

| Defect area | `fix:` commits (of 37) | Examples |
|---|---|---|
| Release, signing, bundles, sidecars, updater manifest | 13 | unset signing secret overriding the ad-hoc identity; whitespace in signing keys (three apps); re-sign libduckdb; no hardened runtime while ad hoc |
| Runner set-up in composite actions (sccache, stable workspace, host paths) | 6 | stable workspace through a junction on Windows; `CARGO_TARGET_DIR` out of the sccache key |
| Pinned downloads and the test browser | 6 | libduckdb fetched in Node "because Windows has no bash"; `fetch-libduckdb.sh` not executable; `tar` on Windows |
| Scaffolder, package publisher, notices | 4 | dev-dependency publish order; LF in embedded licences |
| CI failures of app code and tests, fixed in workflows | 5 | races, Windows paths in tests, DNS in tests |
| Tests of the tools themselves | 1 | a stubbed global left behind in the prune tests |
| One-off scripts | 2 | a Windows runner retirement script under strict mode |

The fortnight was atypical (the monorepo import), the counts are small, and 14 of the 37 are OS-specific. They do not
prove that Rust would have prevented these defects. What they show is **where** the defects were: in per-OS branches of
shell and JavaScript that no test runs on all three systems. The composite actions (22 files, 334 lines of inline shell)
have no tests at all.

The reasons for all-Rust are therefore:

1. **Cross-OS consistency.** One implementation of each step, using `std`, `paddock-proc`, `paddock-archive` and
   `paddock-download` instead of Git Bash, `cygpath`, `cmd //c mklink`, PowerShell, `sha256sum` versus `shasum` and
   three libduckdb fetchers.
2. **One language and toolchain.** Today the kit is JavaScript, bash, PowerShell, Python and shell in YAML. The family
   is Rust-first, and rules, clippy presets, `cargo deny`, coverage gates and `cargo xwin clippy` (ADR 0021) apply to Rust
   unchanged.
3. **Types and tests on three OSes.** Rust in the repository is built, linted and tested on Linux, macOS and Windows.
   OS commands can be scripted with `paddock-testkit::FakeRunner`, so for example the macOS bundle verification is
   tested on every system.
4. **Reuse by Pacenotes** (rule 42): project graph, repository walk and hashing, a `History` port, a rules engine and a
   draw.io emitter (§6).

## 1. Inventory after consolidation

Method (verified, details in the inventory note): every tracked tooling file under `scripts/`, `conformance/`, `tools/`,
`ci-image/`, `diagrams/`, `icons/tools/`, `docs/images/src/`, `templates/repo/scripts/`, `.gitea/actions/*` and the apps'
`scripts/`, `app/scripts/` and `docs/diagrams/src/`, without tests and fixtures. Today: 309 files, 27,925 lines. After
WP-C1–C6 (39 per-app copies, 2,770 lines, deleted): **270 files, 25,155 lines**. "fix:" is the number of the 37
tooling `fix:` commits that touched the group's current files (a commit can touch several groups; the 7 that touched only
pre-import paths or the tools' own tests are not assigned to a group). "OS refs" counts OS-specific tokens
as a proxy for per-OS branches.

| Group | Files | Lines | fix: | OS refs | Phase | Target |
|---|---|---|---|---|---|---|
| Composite actions `.gitea/actions/*` (334 lines of inline shell) | 22 | 334 | 6 | 61 | 1 | one-line YAML calling the tool |
| Runner set-up, pinned downloads, test browser (incl. three libduckdb fetchers, two Python runtime updaters) | 12 | 1,089 | 4 | 92 | 1–2 | `dev ci`, `dev fetch` |
| Release, signing, bundles, sidecars, verify | 15 | 1,271 | 8 | 142 | 2 | `dev release` |
| Planner, timings, coverage gate | 4 | 1,003 | 1 | 0 | 3 | `dev plan`, `dev ci` |
| Host tools, smokes, verify-installed (sh, ps1) | 24 | 1,555 | 0 | 162 | 3 | `dev host` |
| Notices, window weight, benches, plugin and font builds | 11 | 976 | 1 | 31 | 3 | `dev notices`, ... |
| Scaffolder and package publisher | 2 | 990 | 4 | 3 | 3 | `dev new-app`, `dev packages` |
| Conformance engine and checks C03–C37 | 43 | 2,545 | 2 | 45 | 4 | `paddock-repo-rules`, `dev check` |
| App checks that become rules | 6 | 320 | 0 | 17 | 4 | rules |
| Icon pipeline (`@resvg/resvg-js`, `playwright-core`) | 23 | 4,098 | 0 | 125 | 5 | `dev icons` (resvg, `paddock-chromium`) |
| Diagram and poster generators | 77 | 7,277 | 1 | 138 | 5 | `paddock-drawio` + data files; `dev images` |
| draw.io CLI wrappers (sh) | 6 | 159 | 0 | 6 | 5 | `dev diagrams export` |
| npm lifecycle, i18n, UI coverage glue | 6 | 485 | 0 | 3 | 5 | `dev` commands called from `package.json` |
| UI source lints (`lint-effects`, `lint-styles`) | 2 | 243 | 0 | 0 | 6 | **stay JS**: rules in the ESLint preset |
| TypeScript type-checker analysis | 2 | 633 | 0 | 8 | 6 | **stays TS** until the dialog command contract is generated from Rust, then deleted |
| Playwright screenshot and visual-QA suites | 5 | 720 | 0 | 6 | – | **stay TS** |
| Bootstrap shell (`ci-image/*.sh`, `tools/bootstrap.sh`, `scripts/setup-dev.sh`, two app bootstraps) | 7 | 485 | 0 | 90 | 3 | **stays shell**, merged into one bootstrap per OS |
| One-off migrations | 3 | 491 | 2 | 16 | 1 | deleted |

Duplicates the tool collapses (verified by reading): three libduckdb fetchers beside Debrief's Rust `xtask libduckdb`;
four build-folder pruners; four Linux container runners; six verify-installed scripts; two test-browser installers and
two pinners; two notices generators; two window-weight measurers. About 1,100 lines.

Workflows stay YAML. Their inline shell is in scope: with the definitions of §3.7, `.gitea/workflows/*` and
`.gitea/actions/*` hold 392 `run:` blocks today, **31 longer than 10 non-comment lines and 21 that branch on the OS in
shell**, mostly in the per-app workflows (Debrief 7 long and 4 branching, Dyno 3/4, Spotter 3/3, Pitcrew 3/2).

## 2. Options

| | A. Keep the scripts | B. Port where it pays | **C. All Rust (decided)** | D. Type the scripts (TypeScript via Node 24 type stripping) |
|---|---|---|---|---|
| What moves | nothing | 9,734 lines (117 files) + actions | B + generators, npm glue, draw.io wrappers (12,019 more lines) | the JS gets types; shell stays |
| Languages left in tooling | JS, bash, PowerShell, Python, YAML shell | Rust + JS generators and glue + bootstrap shell | Rust; TS only for the Playwright suites (and the TS analysis until replaced); ESLint rules; bootstrap shell | as A |
| Tested on all three OSes | no | most | all but the bootstrap | no |
| Effort | 0 | about 27 agent-days (phases 1–4) | **about 45 agent-days** in six phases, plus about 1.5 for checkpoints | about 4 agent-days |
| Real losses | none | none | none, if the three UI-toolchain categories stay (§4) | none |
| Pacenotes reuse | none | graph, walk, history, rules | + the draw.io emitter | none |

D was rejected because the OS-specific code is in shell, which types do not reach. B and C share phases 1–4; C adds
phase 5 (generators and glue) and phase 6 (the alternatives for the lossy categories).

## 3. Decision

### 3.1 One tool, library crates underneath

- **`paddock-dev`**, one binary with subcommands, following `paddock-cli` conventions (`--json` writes one object, stable
  exit codes, errors name the next step, progress on stderr). Command groups: `plan`, `ci` (`setup`, `timings`,
  `coverage-gate`), `fetch <pin>`, `release` (`bundle`, `sidecars`, `verify-macos`, `self-test`, `latest-json`, `notes`,
  `strip-keys`, `publish`), `host` (`prune`, `container`, `verify-installed`, `vm-smoke`, `cleanup`), `notices`,
  `window-weight`, `new-app`, `packages`, `check` (conformance), `icons`, `diagrams`, `images`, `build-info`, `i18n`.
- **Library crates in `crates/`** where a second consumer exists (§6): `paddock-repo-files` (bounded, `.gitignore`-aware
  walk and hashing), `paddock-repo-graph` (projects, edges, file-to-project mapping, affected closure, the `History`
  port), `paddock-repo-rules` (after a joint rules-model note with the Pacenotes design, §6), `paddock-drawio` (the
  draw.io emitter and theme tokens). CI-only logic stays in modules of `paddock-dev`.
- **Reuse, not new code:** `paddock-proc` for every external command (timeouts, output caps, tree kill, no console window
  on Windows); `paddock-download` and `paddock-archive` for pinned downloads; **`paddock-runtime`'s TOML catalogues** for
  every pin (Chrome for Testing, FFmpeg, llama.cpp, libduckdb, sccache), rewritten with `toml_edit` so comments survive;
  `paddock-env`; `paddock-chromium` for the test browser and screenshots; `paddock-testkit` for tests. Debrief's `xtask
  libduckdb` moves into `dev fetch`; `xtask gen-samples` stays in Debrief.

### 3.2 Placement and how developers run it

- **The binary has its own workspace, `tools/paddock-dev/`, with its own `Cargo.lock`.** A dependency change of the tool
  then never touches `crates/Cargo.lock`, which the planner treats as an input of every crate. The libraries stay in
  `crates/` (published, 99% coverage, hakari) and are path dependencies of the tool. The planner gets a `paddock-dev`
  project with its own three-OS leg (fmt, clippy, xwin clippy, nextest).
- **`./dev` at the repository root, and `dev.cmd` beside it** (part of the bootstrap that stays shell). `./dev` is about
  40 lines of POSIX shell. `dev.cmd` is a two-line delegate that runs `./dev` with Git for Windows' `bash.exe`, which every
  Windows machine of ours has (CI steps already run in Git Bash). "Works from any subfolder" means **root discovery
  only**: the wrapper finds the root with `git rev-parse --show-toplevel` and runs the tool there. The tool gets the
  caller's working folder and resolves relative path arguments against it. A `.cargo/config.toml` alias is not used,
  because its relative `--manifest-path` breaks in subfolders.
- **The tool's own workspace keeps its lockfile current.** When a path dependency in `crates/` gains or changes a
  dependency, `tools/paddock-dev/Cargo.lock` must change too. The tool's planner leg runs `cargo metadata --locked` for
  the tool whenever its input set (below) is affected, and fails with "run `./dev self lock`". The author of the
  `crates/` change regenerates the lockfile in the same PR. Renovate covers `tools/paddock-dev` as one more Cargo
  workspace, grouped with `crates/` (ADR 0014 §8).
- **First run per agent:** the binary cache is per user (`~/.paddock-dev/bin/<hash>/`), not per worktree, so a new agent
  worktree downloads the binary for `main`'s hash in about 1–2 s. Only an agent that changes the tool builds it, in its
  own `CARGO_TARGET_DIR`. Measured cold release builds with 2 jobs: 33 s for a probe with the tool's third-party
  dependencies, 41 s for `paddock-cli` + `paddock-proc` + `paddock-env` with the workspace-hack; seconds with a warm
  sccache.

### 3.3 How CI gets the binary: content-addressed, written only by `main`

**Floating `stable`, no toolchain pin (decided 2026-10-28).** The repository pins no Rust toolchain, and this ADR adds
none: no repository-wide `rust-toolchain.toml` and no pin in `tools/paddock-dev/`. `setup-rust` syncs to the `stable`
channel. On host runners it already does (`rustup toolchain install stable`). Its fast path for the CI image does not
today: when the image has the components, it uses the image's toolchain without comparing it with `stable`
(`.gitea/actions/setup-rust/action.yml`, lines 37–47). R0 changes the fast path: it runs `rustup check` with a 20 s
timeout and installs `stable` when an update is available. When the check itself fails (no network, timeout), the job
uses the image's toolchain and prints a `::notice::` naming its version. When the install fails, `setup-rust` stops with
a clear error that names the channel and the rustup output. Because the compiler is not
pinned, a binary built by one machine is matched to the compiler of another through the input hash below, which
includes the compiler actually used.

**Consequence: stable can move overnight.** A new stable release can break a workspace or the tool (a new lint under
`-D warnings`, a changed diagnostic in a golden file) without any commit in the repository. Such a break shows up as a
CI failure on `main` and is fixed forward in a normal PR, like any other red `main`. The ADR adds no machinery for it.
The tool's own binaries follow: the new compiler gives a new hash, so runs after the update miss and build until
`dev-tool.yml` has published for it. A new stable changes no path, so a push alone would not trigger that; the nightly
run of `dev-tool.yml` (below) publishes for the new compiler within a day.

- **Input hash, computed by git.** The inputs are:
  - the folder `tools/paddock-dev` (its `Cargo.toml`, its `Cargo.lock`, which is exactly the tool's resolved closure with
    registry checksums, and `src/`);
  - the folders of the tool's path dependencies, **normal and build dependencies only**. Dev-dependencies such as
    `paddock-testkit` are left out. They are listed in `tools/paddock-dev/inputs.txt`, and a test compares the list with
    `cargo metadata`'s resolve graph;
  - the build environment: every non-empty `RUSTFLAGS`, `CARGO_ENCODED_RUSTFLAGS`, `CARGO_BUILD_*`, `CARGO_PROFILE_*` and
    `CARGO_TARGET_*` variable as a `NAME=value` line in byte order (not `CARGO_TARGET_DIR`, `CARGO_BUILD_JOBS`,
    `CARGO_BUILD_TARGET_DIR` or `CARGO_BUILD_RUSTC`);
  - the cargo config files cargo reads for the tool: `config.toml` and `config` in every `.cargo` folder from
    `tools/paddock-dev` up to the filesystem root, the other `*.toml` files in the tool's own `.cargo` (what `include`
    pulls in, such as the git-ignored `ci.toml`), then `$CARGO_HOME`'s, closest first and without duplicates. Each file
    enters the hash as a line `config-file <git blob id of its text without carriage returns>`. The files are streamed
    into git and only the digest is held, because a config can hold registry tokens that a shell trace (`sh -x`) would
    print; a wrapper test checks that a dummy token never appears in a trace;
  - the compiler cargo will use: the `release:` and `commit-hash:` lines of its `-vV` output, run after `setup-rust`.
    The wrapper resolves that compiler as cargo does: `RUSTC`, then `CARGO_BUILD_RUSTC`, then `build.rustc` (also the dotted key) from
    those config files, else `rustc` run from
    `tools/paddock-dev`, so a rustup directory override applies. A `--config` on cargo's command line is not seen; the
    wrapper never passes one. The `host:` line is left out because it differs per machine. A stable update therefore
    gives a new hash and a rebuild.

  The hash is `git hash-object --stdin` over the output of `git ls-tree HEAD -- <inputs>` followed by those two lines and the build environment and `config-file` lines above (`./dev` and `dev self hash` build the
  same text; a test runs both).
  `rustc -vV` takes about 35 ms here (measured, through the rustup proxy). The consumer's target triple selects the file
  inside the version (`paddock-dev-<target>[.exe]`). When `git status --porcelain --untracked-files=all -- <inputs>` is
  not empty, the wrapper always builds: a changed tracked file or a new untracked file that `.gitignore` does not exclude
  (for example a new module under `src/`) is not in `HEAD`'s tree, so the hash cannot describe it.
  `tools/paddock-dev/target/` is ignored in `.gitignore` (R0), so a local build folder never counts as a change. The
  tool embeds the hash it was built from and prints it (`./dev self hash` prints both the wrapper's and the binary's),
  so the wrapper can be tested. `git rev-parse` takes 16 ms (measured). The tool's dev-dependencies change
  the hash only through `Cargo.lock`; that errs on the side of a rebuild.
- **Publishing.** `dev-tool.yml` runs on a push to `main` that changes the inputs, nightly on `main` at 02:35
  (`35 2 * * *`, a free slot) to catch a new stable, which changes no path, and on `workflow_dispatch`. Each runner computes the hash with the same
  command and its own compiler. When the package already has this runner's file under that hash, the job ends;
  otherwise it builds in a fresh target folder with `RUSTC_WRAPPER` unset:

  | Target | Runner |
  |---|---|
  | `x86_64-unknown-linux-gnu` | the CI image on a Linux runner |
  | `aarch64-unknown-linux-gnu` (agents' Linux containers on the Mac) | none yet: `ci-image.yml` builds `linux/amd64` only. Until it also builds `linux/arm64` (then: an arm64 container on the macOS host runner), arm64 Linux users build on a miss, as on any branch that changes the tool |
  | `aarch64-apple-darwin` | the macOS host runner |
  | `x86_64-pc-windows-msvc` | the Windows host runner. Not `cargo xwin`, whose output is linked differently from what the Windows runners build and test |
  | `aarch64-pc-windows-msvc` (the Windows 11 ARM test VM) | none: there is no Windows ARM runner, so it builds on a miss |

  It then uploads each binary and its `.sha256` to the Gitea generic package `paddock-dev`, version `<hash>`. Runners on
  the same compiler share one version. After a stable release, runners pick it up at different times (the image's check,
  a host's next `setup-rust`), so for a while they publish under different hashes, and a consumer finds a binary only
  when its compiler matches one runner's. A consumer whose `stable` lags every runner builds on a miss until
  `rustup update`; one whose `stable` is ahead of the runners (a developer who updated first) also misses until the
  runners catch up and the nightly run publishes. Only this
  workflow is given the publish token (`DEV_TOOL_PUBLISH_TOKEN`).
- **Using it in CI.**
  - When the hash is not published (a branch that changes the tool), one `dev-build` job per run builds the Linux
    binary once and uploads it as a run-scoped artifact (v3 artifact actions). The other Linux jobs `need` it and download
    it. When the hash is published, `dev-build` only checks and ends in seconds.
  - Linux container jobs otherwise download the package file for the hash with the job's read token and check its
    `.sha256`.
  - Host runners use `~/.paddock-ci/bin/<hash>/` if present. Otherwise they download it, or build it on a miss, copy it
    there with a write-then-rename, and run the copy. Windows cannot replace a running `.exe`, and parallel jobs share the
    folder.
- **Release and tag jobs bypass every cache.** They set `PADDOCK_DEV_FRESH=1`: the wrapper skips
  `~/.paddock-ci/bin`, the package and the run artifact, and builds in a fresh target folder with `RUSTC_WRAPPER` unset.
  So neither the package store, the host cache nor the shared sccache backend (Redis, writable by every job) can reach a
  signed artifact.
- **Retention.** A Gitea cleanup rule keeps the 30 newest versions of `paddock-dev`. Release jobs never need old
  versions, so an evicted hash only costs one build.
- **Fresh containers** that build on a miss also fetch the tool's crates. On this Mac, a cold `cargo fetch` of the
  probe's 82 crates into an empty `CARGO_HOME` took about 1 s (sparse index). The container network was not measured;
  allow 2–10 s.

**Trust model.** The tool runs in jobs that hold secrets, so whoever can write a binary under a hash can act with those
secrets.

- **The token is wider than the tool.** The publish token is a Gitea `write:package` token, and such tokens are scoped
  to a user, not to a package (read). It can publish to the cargo and npm registries too, and delete and re-upload
  package versions, so "a version, once written, stays" holds only against callers without it. That is the same class of
  token `publish.yml` already holds for the crates and npm packages; the ADR adds no new kind of privilege.
- **The real boundary.** Gitea passes no secrets to workflows from forks (read, not verified on our instance), and it
  does not scope a secret to one workflow (read, unverified). Any workflow run by a push to this repository could read
  the token. The real boundary is therefore "who may push workflows to this repository" (the user and the agents), the
  same boundary that protects the signing secrets today.
- **What the checks catch.** Content addressing makes sure a binary is only used for the inputs it was built from. The
  `.sha256` file and the embedded hash catch transport errors and mix-ups, not a malicious token holder.
- **Never trusted.** The `actions/cache` server and the sccache Redis backend are writable by every job, so they are
  never trusted for the tool or for release builds (research note §7), and release builds start from source.

### 3.4 Windows and macOS host runners

The YAML step stays a Git Bash one-liner (`./dev ci setup …`). The junction, path conversion, extraction and hashing
happen in Rust (`std::os::windows`, the `junction` crate, `paddock-archive`, `sha2`), not in `cygpath`, `cmd //c mklink`
or PowerShell. The host bootstrap (Rust, Git, Visual Studio build tools, Node for the UI) stays shell, merged into one
`tools/bootstrap.{sh,ps1}` per OS in phase 3.

### 3.5 Speed

Measured on the Mac mini (M4 Pro, warm file cache, inventory note):

| What | Today | Rust |
|---|---|---|
| Process start | `node -e 0` 32 ms | 10 ms (`bash -c true` 9 ms) |
| `affected.mjs`, 1 to 200 commits | 0.36–0.37 s: 15 serial `cargo metadata --no-deps` calls (23–32 ms each, including the timing wrapper, so about 20 ms) ≈ 0.3 s, plus git | est. 0.05–0.1 s with the calls in parallel |
| Conformance, CI subset (12 checks) | 2.9 s (C36 2.0 s, an O(files × crates) owner lookup; C30 1.1 s) | est. 0.2–0.4 s; an index in JS would also bring C36 down |
| Read, regex and SHA-256 of 2,691 `.rs` files | Node, one thread, 0.12 s | rayon 0.09 s (I/O-bound) |

In CI (142 recorded runs, 1,070 executed jobs) the tooling steps are short: plan 0.3 s, release facts 0.2 s, `.app`/DMG
verification 0.5–1.0 s, publish dry run 7.8 s, the conformance job 7.5 s. Plan, conformance and tools jobs run no `npm
ci` (Node is in the image). The 113 s "notices are up to date" step is cargo resolving licences.

**Net per CI run** (about 7.5 executed jobs): the tool saves about 3–4 s (planner, conformance) and costs a download of
about 3 MB, 1–2 s, per Linux container job that uses it, plus a few seconds for the `dev-build` check job. Host runners
pay nothing after their first copy. Net about +5 to +15 s per run. A run that changes the tool adds one Linux build in
`dev-build` (about 10 s with a warm sccache, 40–60 s cold), shared by the run's jobs, and one build per host runner. **Speed is neutral; it is not a reason for this
decision.**

### 3.6 Testing

- **Pure core, thin edges.** Each command is a plan (pure: inputs to actions or a result) and an executor (runs the
  actions through `paddock-proc`). Plans are unit-tested on every OS. OS commands (`codesign`, `hdiutil`, `xattr`, `lipo`,
  `otool`, `mklink`, `launchctl`) are scripted with `FakeRunner`.
- **Golden files from today's scripts.** Before a script is deleted, its outputs on fixture inputs become golden files
  (`paddock_testkit::golden`): the planner on the research replay's cases (an identical tree, a red run with one failed
  job, a CI-only change) and on synthetic diffs; `latest.json`; notices; release notes; conformance findings per check, as
  JSON, on `conformance/fixtures` and on the inputs of the 240 existing tests; draw.io XML after canonicalisation. The port must reproduce these exactly; any difference is reviewed, never
  accepted silently. Icons are compared by decoded pixels, not bytes: PNG encoders differ. A pixel may differ by at most
  2 per channel, and at most 0.1% of the pixels may differ at all. The `.icns` and `.ico` files are compared by
  structure: entry types, sizes and order.
- **Shadow week** for the planner, release, `latest.json` and conformance: on `main` the old and the new run side by
  side and a step compares their JSON; a difference is a warning with both outputs. Release commands also get a
  `workflow_dispatch` dry run of `app-release.yml` per app before the switch.
- **Real-OS checks** stay: macOS verification and `--self-test` of every sidecar from the built app (rule 23). Host
  commands have a dry-run mode and scratch roots (rule 7).
- Coverage 99% lines for the library crates and the tool's plan modules.

### 3.7 Coexistence, and the check that keeps the class closed (rule 40)

- **One command at a time.** A PR adds the command and its golden tests, switches the call sites (YAML,
  `paddock.ci.json`, `package.json`, docs) and deletes the script and its tests. No long-lived dual paths, except the shadow
  week.
- **Extend the WP-C6 check, do not add a new one.** WP-C6 adds a conformance check that fails when an app re-adds a copy
  of a kit script. This ADR extends it, from the start of phase 1:
  - (a) no new `.sh`, `.ps1`, `.py`, `.mjs`, `.js` or `.ts` tooling file in the tooling paths of §1, outside an
    allow-list (the bootstrap, the `dev` wrappers, the Playwright suites, the ESLint preset);
  - (b) a counted baseline of today's tooling files that may only shrink: each phase removes its entries;
  - (c) counted baselines of the 31 long and 21 OS-branching `run:` blocks that may only fall.

  A block is **long** at more than 10 non-comment lines. It **branches on the OS** when its shell tests `RUNNER_OS`,
  `runner.os`, `OSTYPE` or `uname`, or calls `cygpath`. A step-level `if: runner.os == '…'` is declarative and exempt.
  Counted waivers as in ADR 0021.

## 4. What stays non-Rust, and why

| Category (lines) | Rust alternative | What would be lost | Decision |
|---|---|---|---|
| Playwright screenshot and visual-QA suites (720) | `paddock-chromium` (`Page.captureScreenshot`, a small synchronous DevTools client) | Playwright's runner, locators and auto-waiting, the trace viewer, and **WebKit**: the suites run in Chromium and WebKit, `paddock-chromium` drives Chromium only | stay TS; they are UI test code, like the Vitest specs |
| TypeScript type-checker analysis of dialog commands (633) | none: `oxc` parses TypeScript but has no type checker, and the native TypeScript compiler is written in Go | the types of the calls; a name-based approximation would miss cases | stays until the dialog command contract is **generated from Rust** (ts-rs is already in the family; ADR 0009's declared contracts). Then the analysis is deleted, not ported (phase 6) |
| UI source lints (243) | oxlint (custom rules need its JS plugin API or a fork) | integration with the ESLint preset every app runs | become rules in `eslint/` (JavaScript by the nature of ESLint) |
| Bootstrap (about 570 with the wrappers) | none: it installs Rust | n/a | shell, one script per OS |

Everything else moves without real loss. For the icon pipeline, `@resvg/resvg-js` is a binding of the Rust `resvg`, the
`.icns`/`.ico` writers are hand-written already, and previews use `paddock-chromium`. The cost is design iteration speed
(a cargo build instead of a 30 ms Node start). The diagram generators are mostly data and become input files for
`paddock-drawio`. The npm glue becomes `./dev` calls from `package.json`.

## 5. Risks

| Risk | Mitigation |
|---|---|
| A port changes behaviour silently | golden files from the old script; shadow week; release dry runs |
| The binary in the package store is wrong or malicious | content-addressed by inputs; only `main` writes; releases build from source without shared caches (§3.3) |
| A tool change costs a build per runner type | own workspace and lockfile; one `dev-build` job per run shares the Linux build; sccache on non-release misses; host copies by hash |
| A new stable release breaks a workspace or the tool overnight | no pin by decision (§3.3): it shows as a CI failure on `main` and is fixed forward; the compiler is part of the input hash, so no binary built by an older compiler is reused |
| Contributors must change CI in Rust | small commands with a plan/executor split and golden tests; YAML stays the readable surface |
| The generators lose quick iteration | `cargo` incremental builds of one small crate take seconds; data files for diagrams need no build |
| The project stalls half-way, with two systems | each phase ends in deletions; per-phase acceptance and a checkpoint (§7) |
| Scope creep into a build system | the tool runs steps and checks; it never replaces cargo, npm, ng or nextest and has no result cache |
| `crates/` legs run more often | the tool has its own workspace; job-level last green (WP-A1) skips unchanged legs |

## 6. Reuse by Pacenotes (rule 42)

Pacenotes plans `pacenotes-index` (walk, ignore, hashing, secret denylist), `pacenotes-git` (gix behind a `History`
port), `pacenotes-rules`, `pacenotes-review`, a dependency graph from repositories down to symbols (proposal §23) and
`pacenotes-diagram` (§9.7, with the Paddock tools as its oracle).

| Piece | Shared crate | Fits Pacenotes | Constraints | CI-only |
|---|---|---|---|---|
| Walk and hashing | `paddock-repo-files`: bounded walk with `ignore`, `.gitignore`-aware, rayon, SHA-256 | yes; `pacenotes-index` adds its denylist and allow-list on top | its own crate, so the graph and rules crates do not drag each other in | none |
| Project graph: Cargo and npm workspaces, `tsconfig` paths, Tauri `frontendDist`, file to project, reverse closure | `paddock-repo-graph` | yes: §23's "repositories → packages and modules" level, edges with source and confidence | reads manifests with `toml`/`serde_json`. The `cargo metadata` adapter (exact, used by CI) **is never run inside Pacenotes**. Pacenotes opens hostile repositories and executes nothing from them (§14), and cargo reads the repository's `.cargo/config.toml`, which can name the `rustc` and wrapper programs. To enforce this, the adapter sits behind the crate feature `cargo-metadata`, off by default, and the extended WP-C6 check fails when any manifest under `apps/pacenotes/` enables it, directly or through `cargo tree -e features` in Pacenotes' leg | GLOBAL/DOCS rules, `kinds`, nextest packages, the CI matrix: `dev plan` |
| Git history | the `History` port in `paddock-repo-graph`: changed files between revisions, log with subjects, file at revision | yes: Pacenotes plugs in its gix adapter | the CI adapter runs `git` through `paddock-proc` | Gitea run and job records |
| Rules engine | `paddock-repo-rules` | yes, once both models agree | **The models differ today.** Conformance has `error`/`warn`, findings with a one-sentence fix, path-glob waivers with reason and expiry (counted waivers from ADR 0021), and job summaries. Pacenotes §24 has severities `blocker`…`info`, scoped suppressions (line, file, path, rule) with reason and expiry, baselines by fingerprint (rule, path, normalised snippet hash), fix diffs, SARIF and exit codes 0/1/2/3. A **joint rules-model note**, written with the Pacenotes design before the crate (phase 4, first WP), fixes one model: a severity superset, a `Finding` with optional fix text and diff, `Suppression` with scope, reason, expiry and count, fingerprint baselines, report writers (text, JSON, SARIF 2.1.0, job summary) and the exit-code mapping | the family checks themselves |
| draw.io emitter | `paddock-drawio` (model to canonical mxGraph XML, theme tokens from one source, UML/C4 shapes) | yes: `pacenotes-diagram` adds layout, round-trip merge and diffs on top | golden tests against today's `.mjs` output (the oracle §9.7 asked for) | the family's diagram data files |
| Gitea REST client | a module of `paddock-dev` | partly (auth, paging, errors) | extracted when `pacenotes-gitea` (PR comments) starts | releases, packages, run records |

## 7. Phases, work packages and acceptance

Phase 0 is the prerequisite and is not counted: WP-C1–C6 (consolidation) and WP-A1 (job-level last green, built in JS
first and ported in phase 3 with its fixtures). Estimates are agent-days for an implementer, with a researcher or designer
on the first package of each phase. "Deletes" is measured (scripts plus their tests); "adds" is estimated (Rust about 1.2
times the JS it replaces after duplicates collapse, shell about 2 times; tests about 1:1).

| Phase | WP | Content | Days | Deletes | Adds |
|---|---|---|---|---|---|
| **1 Foundation and runner set-up** | R0 | `tools/paddock-dev` skeleton, `paddock-repo-files`, `paddock-repo-graph` (graph, `History` port), `./dev` + `dev.cmd`, git input hash with the compiler cargo resolves, `setup-rust`'s image fast path checked against `stable` (notice on a failed check, clear error on a failed install), `self hash`, `inputs.txt` test, `dev-tool.yml` on the three host runners and the CI image, on push, nightly and `workflow_dispatch`, the `dev-build` job and run artifact, the generic package and its cleanup rule, `.gitignore` for `tools/paddock-dev/target/` | 3.5 | – | 1,300 + 900 tests |
| | R1 | composite actions' logic (sccache, stable workspace, build folder and env, host tools and cleanup, node caches, sccache stats); actions become one-liners; extend the WP-C6 check (§3.7) with today's baselines; delete the three one-off scripts | 2.5 | 334 shell lines (actions 815 → about 300 YAML), 72, 491 | 900 + 800 tests |
| | R10 | app workflows' long and OS-branching `run:` blocks into commands or `paddock.ci.json` (Debrief, Dyno, Spotter, Pitcrew, Pitwall) | 2 | about 450 inline shell lines | 500 + 400 tests |
| **2 Release and downloads** | R2 | release, bundles, sidecars, `verify-macos` with `FakeRunner`, `self-test`, `latest-json`, notes, strip-keys, publish; Debrief `build-sidecars`, macOS dev-signing, Dyno `check-bundle`, demo release | 3 | 1,271 + about 900 test lines | 1,600 + 1,200 tests |
| | R3 | `dev fetch` on `paddock-runtime` catalogues, `paddock-download`, `paddock-archive`: Chrome for Testing, libduckdb (from `xtask`), runtime pins; the Python updaters go | 2 | 1,017 + about 500 | 900 + 700 tests |
| **3 Planner, hosts, scaffolding** | R4 | `dev plan` (explain, CI matrix, job-level last green with the replay fixtures), `ci timings`, `coverage-gate`; shadow week; update rule 3's wording in `docs/rules.md` (line 127), root `AGENTS.md` (lines 24, 42, 53, 83) and `apps/debrief/AGENTS.md` (line 13) | 2.5 | 1,003 + 341 | 1,300 + 900 tests |
| | R5 | `dev host` (prune, container, verify-installed, vm-smoke) with per-app config instead of 24 scripts; one bootstrap per OS | 2.5 | 1,555 + 620 | 1,100 + 800 tests |
| | R6 | notices, window weight, startup bench, plugin and font builds | 1.5 | 976 + about 400 | 900 + 500 tests |
| | R7 | `new-app`, `packages publish` (with `cargo hakari publish`) | 2 | 990 + about 700 | 1,200 + 800 tests |
| **4 Rules** | R8a | joint rules-model note with the Pacenotes design (§6) | 1 | – | note |
| | R8 | `paddock-repo-rules`, `dev check`: checks C03–C37 and the six app checks against golden findings; shadow week; `conformance/` keeps only fixtures | 4.5 | 2,865 + 3,460 | 3,300 + 3,800 tests |
| **5 Generators and UI glue** | R11 | `paddock-drawio` and `dev diagrams` (generate, export through the draw.io CLI); per-app diagram sources become data files | 7 | 5,999 + 159 wrappers | 1,500 + 5,000 data + 1,200 tests |
| | R12 | `dev icons` (resvg, PNG, `.icns`, `.ico`, previews and sheets with `paddock-chromium`, install and `--check`); `cargo deny` on resvg's licence | 4.5 | 4,098 + 161 | 4,500 + 1,200 tests |
| | R13 | `dev images` (posters: templates and `paddock-chromium` screenshots) | 2 | 1,278 | 600 + 300 tests |
| | R14 | npm glue: `build-info`, `whats-new`, i18n locales, `tauri-before-build`, `ui-coverage-guard` as `./dev` commands | 1 | 485 + about 300 | 500 + 400 tests |
| **6 Lossy categories** | R15 | UI lints into the ESLint preset (moved, not deleted); dialog command contract generated from Rust, then the TS analysis deleted | 2.5 | 633 (243 moved) | 400 + 300 tests |
| | R16 | clean-up: allow-list down to its final form, docs, a lesson in `docs/lessons.md` | 0.5 | about 200 | 200 |
| | | **Total** | **about 45, plus about 1.5 for the six checkpoints** | **about 23,900 source and inline shell + about 7,400 test lines** | **about 20,700 Rust + 5,000 data + 14,400 test lines** |

The checkpoints add about 0.25 days per phase, about 1.5 in all.

**Per-phase acceptance** (all must hold before the next phase starts):

- Every script in the phase is deleted, along with its tests and call sites. The allow-list and the baselines of §3.7
  shrank by those entries.
- The golden files pass on Linux, macOS and Windows. The shadow week ended with no unexplained difference.
- Phase 1:
  - There is no `rust-toolchain.toml`; `setup-rust` installs `stable` and fails with a clear error when it cannot.
  - `setup-rust`'s image fast path, tested in a container whose toolchain is older than `stable`, installs `stable`;
    with the network cut, it uses the image's toolchain and prints the notice.
  - A test shows that two different `rustc -vV` release or commit-hash lines give two different hashes, and that a
    different `host:` line does not; that the wrapper hashes the compiler named by `RUSTC` and by `build.rustc`; and
    that a new untracked, non-ignored file under an input path makes the wrapper build.
  - `dev-tool.yml`'s publish decision: a `workflow_dispatch` run on `main` with a compiler whose hash is missing from the
    package publishes it, and a second run with the same compiler publishes nothing.
  - On one commit, the hash that `./dev self hash` prints on Linux, macOS and Windows equals the version `dev-tool.yml`
    published from that OS's runner, and equals the hash embedded in the downloaded binary.
  - `./dev` finds the root from a subfolder on all three OSes.
  - A new worktree downloads the binary for `main`'s hash; a branch that changes the tool builds it once in `dev-build`.
  - A job with `PADDOCK_DEV_FRESH=1` builds it fresh without sccache.
- Phase 2: a `workflow_dispatch` dry run of `app-release.yml` succeeds for every app. A published DMG still passes
  `codesign --verify --deep --strict`.
- Phase 3: rule 3's documents name `./dev plan`, and the planner's output equals the JS output on the replay fixtures.
- Phase 4: the joint model note is approved by the user. `dev check` reports the same findings as the JS checks on
  `main` and on the fixtures.
- Phase 5:
  - The icons match the JS output by decoded pixels within the tolerance of §3.6, and the `.icns`/`.ico` structure
    matches.
  - The icon sheets still get an independent review (rule 22).
  - The diagrams match after canonicalisation.

**Checkpoint after each phase.** Work on the next phase continues during the four-week checkpoint window. A failed
checkpoint can only reorder or pause later work, never undo the decision for all-Rust.

- *What is counted:* `fix:` commits with a tooling scope (§Context) whose diff touches the phase's paths. Not counted:
  the commits of the PR that created a command, and fixes made during its shadow week (reported separately as
  shake-out).
- *Baseline:* the same areas in the four weeks after WP-C6 merges (the consolidated kit, not the import fortnight).
- *Control:* in the same window, the tooling areas not yet ported, plus the workflows.
- *Window:* four weeks after the phase's last deletion.
- *Pass:* the ported areas' rate per 1,000 lines is at most half their baseline rate and no higher than the control's
  rate in the same window. When both counts are below 5, pass means "no more commits than the baseline". On a fail, later
  work packages pause or are reordered until the defects are root-caused (rule 40) and the cause is fixed in the tool's
  design.

**Line deltas:** about 23,900 lines of JS, shell, PowerShell, Python and inline shell go (measured), and 243 lint lines
move into the ESLint preset. About 20,700 Rust lines and 5,000 lines of diagram data come in (estimated). About 1,100
duplicated lines collapse. Left outside Rust: 1,353 TS lines (the Playwright suites, and the TS analysis until R15
removes it), 243 lines of ESLint rules, about 570 lines of bootstrap shell, and YAML.

## Consequences

- One implementation per tooling step, tested on Linux, macOS and Windows; composite actions and workflow steps become
  readable one-liners, kept that way by the extended WP-C6 check.
- The kit uses Rust, YAML and a small bootstrap; TypeScript remains only in the UI's own test and lint toolchain.
- Pacenotes starts with a walk, a project graph, a `History` port, an agreed rules engine and a draw.io emitter that CI
  and the docs already exercise.
- A tool change costs a build per runner type and a publish from `main`; everyday use costs a 1–2 s download per
  container job.
- Icon and diagram work moves from editing `.mjs` files to editing Rust and data files.

## Decisions taken

The user took these decisions on 2026-10-28:

1. **Floating `stable`, no toolchain pin** (§3.3): no repository-wide `rust-toolchain.toml` and no pin in
   `tools/paddock-dev/`. `setup-rust` syncs to `stable`, its image fast path included, and fails with a clear error. The compiler's release and
   commit hash are part of the binary's input hash, and `dev-tool.yml` also runs nightly so a new stable gets
   binaries. A break from a new stable release is a CI failure on `main`, fixed
   forward.
2. **Prebuilt binaries written only by `main`**, content-addressed, through the Gitea generic package, with the trust
   model in §3.3; release and tag jobs always build fresh.
3. **The checkpoint passes when the fix rate halves** (§7): a 50% drop against the post-WP-C6 baseline and no worse than
   the control; when both counts are below 5, pass means no more commits than the baseline. Phases continue during the
   checkpoint window.
4. **A joint rules-model note with Pacenotes comes before the rules crate** (R8a); the user approves it.
5. **The dialog command contract is generated from Rust** (R15), so the TypeScript analysis is deleted rather than kept.

## Amendment (2026-10-28): job-isolation slots in R0

R0 also holds `paddock-slots` and `dev ci slot acquire|release|status|hold`, which the review of the branch found had no home
in this ADR. They exist because a job that needs one of N machine-wide resources (a test port range, a build folder) needs a
claim that cannot go stale; they have no consumer inside R0, and the CI kit's job isolation (a separate ADR, on top of R0)
is the first.

- **Mechanism.** A slot is an exclusive OS file lock on `<root>/slots/sN.lock` (`File::try_lock`: `flock`, `LockFileEx`). The
  kernel drops it when the holding process ends however it ends, so there is no stale lock and no reclaim race. A lock lives
  as long as its process, so `acquire` starts a small detached holder (`dev ci slot hold`, the same binary) that takes the
  slot, prints `sN <token>` and keeps it. `release` asks the holder by a marker file carrying the token (no signals: the same
  on every OS); a holder honours only its own token.
- **A holder always ends.** It ends when (1) the job folder given as `--owner` is gone, (2) a release with its token is
  requested, or (3) `--max-hold-secs` has passed (default 6 hours, 0 for no limit): a job that never ends and never releases
  cannot keep a slot for ever. It leads its own process group (Unix) or one without a console (Windows) so a runner killing
  the step's group does not free the slot mid-job.
- **Where the files may live.** The tool binary and the CI home (`--root`, also the holder's working folder) must be outside
  the job folder: a process with its program or working folder inside it stops Windows deleting the folder at the end of the
  job. `acquire` refuses both and says what to pass instead.
- **`acquire` cannot hang.** It waits 30 seconds for the holder's first line (the slot, or `none`); after that it stops the
  holder and fails with a message, so a holder that starts late cannot take a slot nobody knows about.
- **Library hazard.** A lock file open in a process that forks a child is inherited until the child execs. Consumers that spawn
  children while slots are taken must serialise the two or spawn from a process that holds none (documented in the crate; the
  crate's own tests use a read-write gate).
- **Open item, to verify on the Windows runner.** `CreateProcessW` inherits every inheritable handle of the parent. `acquire`
  clears the inherit flag on its own three standard handles; handles beyond those that a runner marks inheritable are not
  covered by that. The lifetime cap above is the backstop; verifying this needs a Windows host and is a task of the CI-kit ADR.
- **R0b item: hash only the build-affecting keys.** R0 hashes whole config files and every `CARGO_BUILD_*`, `CARGO_PROFILE_*`
  and `CARGO_TARGET_*` variable, which is safe but too wide: a registry or credential edit, or a runner-local setting such as
  `rustc-wrapper` (sccache), `target-dir` or `jobs`, gives that runner its own hash, and then "download main's binary" never
  matches. Before the download path is switched on (R0b), the hash takes only the keys that change the compiled code (`rustflags`,
  `profile.*`, `linker`, `target`, `rustc`) from the files, parsed once with one rule for both the wrapper and the tool, and the
  environment and config exclusions are made the same list (`target-dir`, `jobs`, `rustc-wrapper`, `build-dir` are in neither).
  Until then the wrapper only builds locally, so the width costs cache hits, not correctness.
- **Tests.** Test holders are started with a short `--max-hold-secs`, so one left behind by a failing test ends by itself
  (ER-14).

## Amendment (2026-10-31, Proposed amendment): generic tools, extensibility and tool logging

- Status: **Proposed amendment**, not yet accepted: the user took its three decisions on 2026-10-31 (G.10); acceptance
  waits for an independent review. The rest of this ADR stands; where this section differs, it describes
  the change being proposed.
- Asked by the user on 2026-10-31: "start moving all to rust tooling", "consider which ones are generic and which are
  paddock", "perhaps rust tooling can be plugin based? and extensible?" ("but not sandboxing"), and the tools must log well
  enough for troubleshooting (rule 56, ER-27).
- Already decided: `wiki` (branch `tools/wiki`) and `heavy` (branch `tools/heavy`, replacing plock and engineering-rules
  `scripts/heavy.sh`) are standalone, generic Rust tools. `wiki` stays in Paddock `tools/` for now, is published to the
  `decryp7` registry, and moves to its own repository later.
- Related: ER-2, ER-4/5, ER-12, ER-14, ER-15, ER-22, ER-24, ER-27; rules 42/43, 55 (ADR 0034, `docs/known-copies.toml`),
  56, 57, 58; ADR 0001 (plugin platform), ADR 0025 (CI job isolation), ADR 0036 (logging for root cause, on
  `docs/adr-logging-root-cause`); the crates `paddock-cli`, `paddock-log`, `paddock-proc`, `paddock-packs`,
  `paddock-slots`, `paddock-testkit`.
- Evidence: a read of `origin/main` at `233c1339b` and of the branches `ci/diagnostics`, `ci/setup-tools-hardening`,
  `docs/rca-rules`, `docs/wiki-pilot`, `tools/wiki` (worktree), `tools/dev-*` and of `engineering-rules` at `791e941`.
  Line counts are `wc -l` of tracked files. The inventory is Appendix G.

### G.1 Why a second kind of tool

§3.1 puts every command in `paddock-dev`. That is right for what knows Paddock's layout (apps, `paddock.ci.json`, the
release kit, the family's checks). It is wrong for about a third of the scripts, which know nothing about Paddock and
already exist **twice or three times**, partly in `engineering-rules` for other projects:

| Mechanism | Copies today |
|---|---|
| wiki lint | engineering-rules `scripts/wiki-lint.mjs` and `templates/wiki/wiki-lint.mjs` (327 lines each), Paddock `docs/wiki-pilot` `scripts/wiki-lint.mjs` (244) |
| heavy-step semaphore | engineering-rules `scripts/heavy.sh` (217), plock |
| commit lint | `conformance/src/commit-lint.mjs` (72), `apps/debrief/scripts/commit-lint.mjs` (67) |
| CI timings | engineering-rules `scripts/ci-timings.mjs` (155), Paddock `scripts/ci-timings.mjs` (355) |
| secret whitespace | engineering-rules `scripts/strip-secret-whitespace.sh` (77), `conformance/release/strip-keys.mjs` (52) |
| Gitea release publish | `conformance/release/publish.mjs` (117), `apps/pacenotes/scripts/release/publish.mjs` and `apps/sighter/...` (93 each) |
| macOS bundle verify | `conformance/release/macos-verify-bundle.sh` (156), `apps/debrief/scripts/ci/macos-verify-bundle.sh` (85), `scripts/demo/verify-macos.sh` (83) |
| build-folder pruning | `tools/prune-build-folders.sh` (38) and `.ps1` (57), `tools/prune-ci-build-folder.sh` (22), `apps/debrief/scripts/clean-build-dirs.ps1` (56) |
| coverage gate | `scripts/coverage-gate.mjs` (49), `apps/pitwall/scripts/ci/ui-coverage-guard.mjs` (47) |

A port into `paddock-dev` would end the Paddock copies but leave the engineering-rules ones (ER-22: "build it shared
instead of copying"), and other projects could not install it without Paddock's layout. So: **generic tools are their own
binaries, Paddock-specific commands stay in `./dev`, and both stand on one shared runtime.**

### G.2 Classification rule

| Class | Test | Lands in |
|---|---|---|
| **Generic** | Useful to any project, or to any project that follows the engineering rules; reads no Paddock path, app id or file format (inputs come from flags or the project's config) | `devkit`, or a standalone tool (G.3) |
| **Paddock** | Knows Paddock's layout, apps, `paddock.ci.json`, the release kit, Tauri bundle layout of the family, or the family's style | `./dev` (`tools/paddock-dev`), as §3.1 says |
| **Split** | A generic engine plus Paddock-specific configuration or rules | engine in a generic tool or a `crates/` library; Paddock's part as a **data pack** (G.4) or as `./dev` code registered on the engine |
| **Keep** | Loses something real in Rust (§4), or is bootstrap, or is YAML glue better left as YAML, or is a one-off to delete | stays, or is deleted |

Counts (Appendix G, 84 rows covering every non-Rust tooling file; copies of one mechanism are one row): **Generic 22,
Paddock 34, Split 14, Keep 14** (two of them one-offs to delete). The generic rows hold about 4,800 lines (copies
included), the split rows about 7,000; the rest is Paddock or kept.

### G.3 Structure

**Generic tools: one multi-command binary `devkit`, plus standalone tools where the user decided so or a tool earns it.**

| | One `devkit` with subcommands (recommended) | A separate binary per tool |
|---|---|---|
| Install friction | one `cargo install --registry decryp7 paddock-devkit`; one download in CI | one install per tool; a project needs 10+ installs and 10+ version pins |
| Versioning | one version, one CHANGELOG, one tag `devkit-v*` | a tag, changelog and release per tool |
| Shared code | the runtime crate (below) compiled once (about 3 MB, as `paddock-dev`) | linked into each binary; same source |
| Testing and CI | one workspace, one leg (fmt, clippy, xwin clippy, nextest on 3 OS) | one leg per tool; each pays the cold build |
| Fit for small commands | most generic commands are 25 to 360 lines of script; a crate each is more boilerplate than code | good for large tools with their own audience |
| Coupling | a bad release of one command ships with all; mitigated by golden tests per command | none |

Recommended: **`devkit`** (package `paddock-devkit`, binary `devkit`, workspace `tools/devkit/`, own `Cargo.lock`, as
`tools/wiki/`) for the small generic commands. **`wiki` and `heavy` stay standalone** as decided: both are larger, have their
own audience (every agent runs `heavy`, wiki users need nothing else) and their own release rhythm. A command graduates to
its own binary only when it has a different audience or release rhythm, never for size alone. Through external
subcommands (G.4), `devkit wiki` and `devkit heavy` also work when the tools are installed, so the user meets one name.

**Paddock-specific tools** stay in `./dev` (`tools/paddock-dev`) exactly as §3.1 to §3.3 say. `./dev` uses the generic
libraries and the runtime; it does not wrap `devkit`. CI calls `devkit <cmd>` directly for a generic step.

**Shared runtime: `paddock-tool`, a small library crate in `crates/`** (rule of three is met: `wiki`, `heavy`, `devkit`
and `paddock-dev` need it). It adds no new mechanism where one exists (ER-22); it wires:

- `paddock-cli`: exit codes (`Exit`: 0 ok, 1 failure, 2 usage, 3 not running, 4 refused, 5 findings, 130 interrupted),
  `--json` (one object, errors included), `CliError` with a next step, `Progress`, `Table`;
- `paddock-log` (ADR 0036): one log per process, CLEF lines, redaction, `event::phase`, the error chain;
- `paddock-proc`: every spawned command (timeouts, output caps, tree kill), plus the new exit-code decoding of G.5;
- `paddock-packs`: data packs, parsed once with limits and unknown keys reported;
- `paddock-env`: the per-tool log folder;
- external subcommand discovery and the project config (G.4).

**Source location.** Paddock `tools/<tool>/` for now, each its own Cargo workspace and lockfile (ADR 0014's third kind),
path dependencies on `crates/`. Published to the `decryp7` registry from a tag `<tool>-vX.Y.Z` in CI (ER-15), never from a
workstation; the crates they use are already published by `publish.yml`. **Later move:** `wiki`, `heavy`, `devkit` and
`paddock-tool` move to one repository of their own (with the `paddock-cli`, `-log`, `-proc`, `-packs`, `-env` crates
consumed from the registry) when a second project installs them or the paddock crates reach v1, whichever is first. Until
then nothing in them may read a Paddock path; a conformance check (G.8) enforces that.

**Naming by concept, not project** (memory "broader naming"): binaries `devkit`, `wiki`, `heavy`; subcommands by what
they do (`devkit commit-lint`, `devkit run`, `devkit ci timings`, `devkit bisect-flaky`, `devkit macos verify-bundle`).
Packages keep the registry's `paddock-` prefix (as `paddock-wiki` does) until the move, where the prefix is decided once.

**How engineering-rules calls them.** Templates and skills name the binary and carry one fallback line:

```text
heavy cargo clippy --workspace -- -D warnings
# Not installed? cargo install --registry decryp7 paddock-heavy --locked
# (or from a Paddock checkout: cargo install --path tools/heavy --locked)
```

The shell and Node scripts in `engineering-rules/scripts/` stay for one overlap release after the Rust tool ships with
parity tests; then they are deleted and the templates (`lefthook.yml`, `BRIEF.md`, `checklists/*.md`, `skills/*/SKILL.md`)
name only the binary. `templates/wiki/wiki-lint.mjs` and its test go with `scripts/wiki-lint.mjs`.

### G.4 Extensibility: built-in registry, external subcommands, data packs

| Option | Verdict | Why |
|---|---|---|
| (a) External subcommands, git/cargo style: `devkit foo` runs `devkit-foo` | **Adopt** | language-agnostic, no ABI, a project extends without forking; the contract is argv, env, stdout JSON and exit codes, which `paddock-cli` already defines |
| (b) In-process registry: a `ToolCommand` trait and one list | **Adopt for built-ins** | open/closed for `devkit` and `./dev`: a command is a module plus one line; the runtime gives every command logging, phases, `--json` and exit codes the same way |
| (c) WASM plugins (wasmtime, extism) | **Dropped** | the user ruled out sandboxing for tool plugins; without it WASM only adds weight and a host API |
| (d) Dynamic libraries | **Rejected** | no stable Rust ABI, `unsafe` loading, per-compiler rebuilds |
| (e) Data packs (TOML) | **Adopt** | projects extend by configuration, not code (memory "avoid hard coding"): rule packs for the rules engine, prefixes for temp-leak checks, per-app vm-smoke and verify-installed config, runtime pin catalogues; `paddock-packs` already parses them with limits |

**Decided: no sandbox** (user, 2026-10-31). Tool plugins are trusted, local code a developer or CI runs, like a
`Makefile` target. The trust rule is simple: **a plugin comes only from the project's repository or from a registry
install; nothing is fetched from the network at run time.** A missing plugin is an error that names the install command;
the tool never installs it.

**Reuse ADR 0001's plugin platform?** No, it is overkill here. `paddock-plugin-api/-host/-package` serve long-lived,
out-of-process app plugins: an IPC protocol, a supervisor with restart and quarantine, permission grants with a sandbox
mapping, Ed25519-signed `.paddockplugin` archives and a trust store. A tool plugin is a one-shot process whose whole
protocol is argv, environment, one JSON object and an exit code. Its provenance is git (repository-local) or the cargo
registry's checksums (installed), so signing adds nothing. What is reused: `paddock-packs` for packs, `paddock-cli`'s
`Report`/`Exit`, and the **compatibility rule** of `paddock_ipc`'s protocol version (same major, plugin minor at most the
host's) for the contract version. Revisit only if a tool ever needs a long-lived plugin.

**The tool contract, version 1.0** (one page in `tools/devkit/CONTRACT.md`, the single source; a test runs a shell-script
plugin and a Rust plugin against it):

- **Discovery**, in order, first match wins: built-in commands; `[commands]` in the project config; an executable
  `devkit-<name>` on `PATH` (how registry installs land, `~/.cargo/bin`). Built-ins cannot be shadowed. `devkit help`
  lists all three kinds and where each came from.
- **Invocation**: `devkit foo <args>` runs the plugin with `<args>` unchanged and these variables set:
  `DEVKIT_CONTRACT=1.0`, `DEVKIT_RUN_ID` (correlation, G.5), `DEVKIT_LOG_LEVEL`, `DEVKIT_LOG_DIR`, `DEVKIT_JSON=0|1`,
  `DEVKIT_ROOT` (the project root), `DEVKIT_CONFIG` (the config file, if any), `DEVKIT_MASK` (names of variables whose
  values must never be printed). `CI` and the runner's variables pass through.
- **Output**: with `DEVKIT_JSON=1`, standard output is exactly one JSON object in `paddock-cli`'s `Report` shape (`ok`,
  data; on failure `ok: false`, `error`, `hint`, `exit`); notes and progress go to standard error.
- **Exit codes**: `paddock-cli`'s `Exit`. A plugin that ends with anything else is reported as a failure with its decoded
  exit code (G.5).
- **Versions**: a configured plugin declares `contract = "1.0"`; a different major is refused before it runs, with a
  message that says which side to update. A `PATH` plugin checks `DEVKIT_CONTRACT` itself (`paddock-tool` does it for a
  Rust plugin).
- **Standalone tools** (`wiki`, `heavy`, `./dev`) follow the same contract for their own commands, so a caller sees one
  behaviour everywhere.

**The project config**, `devkit.toml` at the repository root, found by walking up from the current folder to the git top
(as `wiki-lint.json` is found today). It is optional; without it every built-in works with flags alone.

```toml
contract = "1.0"

[commands]                       # project plugins: a path in this repository, or a registry-installed binary
release-steps = { path = "tools/bin/devkit-release-steps", contract = "1.0" }
drawio-fmt    = { bin = "drawio-fmt", version = ">=0.3, <0.4", contract = "1.0" }

[packs]                          # data packs per kind: files or folders of *.toml, relative to this file
rules      = ["conformance/packs"]
temp-leaks = ["tools/packs/temp-leaks.toml"]
vm-smoke   = ["apps/*/vm-smoke.toml"]
```

**How the generic tools stay generic while Paddock ships its part.** A generic tool reads only flags, `devkit.toml` and the
packs it names. Paddock's knowledge lives in Paddock's repository: its rule packs (for example the data-expressible parts of
checks such as Pitwall's `privacy-check.sh`, "`reqwest` only in `egress.rs`", or Dyno's `check-wording.mjs` word lists), its
temp-leak prefixes (`debrief-*`, `imt-*`), its per-app vm-smoke and verify-installed config, its pin catalogues, and its
code-level checks and release steps registered in `./dev` on the shared engines. A rule kind enters the generic engine only
when a pack needs it (YAGNI, rule of three); everything else stays code in `./dev`.

### G.5 Logging for troubleshooting (rule 56, ER-27), in the runtime

Every tool and every Rust plugin gets this from `paddock-tool`; it reuses `paddock-log` (ADR 0036) and `paddock-proc`
rather than a new logger (ER-22). It needs ADR 0036's WP-1 (paddock-log's layering and `event::phase`) and WP-3
(paddock-proc events); `paddock-tool` lands after them, or uses `paddock-log`'s current API and switches in the same
change as WP-1.

- **Log file per run of a tool**, CLEF lines, written at **debug** whatever the terminal shows, so a failure can be
  diagnosed from the file without a re-run. Default folder per tool from `paddock-env` (`<logs dir>/<tool>/`, for example
  `~/Library/Logs/devkit/` on macOS), `<tool>.<day>.log`, `<tool>-<plugin>.<day>.log` for a plugin, with paddock-log's day
  budget and kept days. `--log-file <path>` or `DEVKIT_LOG_DIR` overrides; `--verbose` (repeatable) and `--quiet` set the
  terminal level only.
- **First line**: tool, version, commit, OS and version, the arguments (masked), the working folder, the project root and
  config file, the contract version, `CI` or not.
- **A timed phase per step** (`event::phase`): `Phase started` / `finished` with `ElapsedMs` / `failed` with the error
  chain; on the terminal one line per phase (`devkit: plan: ok in 1.2 s`), as `ci-log.mjs`'s `phase()` prints today.
- **Every spawned command** goes through `paddock-proc`: the exact argv and working folder with masked values, the names
  (not values) of environment variables set for it, the duration, the exit code in decimal and hex, the **NTSTATUS name on
  Windows** (`0xC0000409` fast-fail, `0xC00000FD` stack overflow, `0xC0000374` heap corruption, ... ; the table moves from
  `run-native.mjs` into `paddock-proc`), the signal on Unix, and a bounded stderr tail (last 40 lines, scanned).
- **Masking**: paddock-log's redactor plus the values of every variable named in `DEVKIT_MASK` (generalising
  `ci-log.sh`'s `CI_MASK`); in CI the tool also asks the runner to mask them (`::add-mask::`).
- **In CI** (`CI` set): every failure prints **exactly one `::error::` line**, the cause then `Next step:` (from
  `CliError`'s hint); annotations with file and line for findings; a **run summary** (phases, results, durations, the
  commands that failed, the log path) to `$GITHUB_STEP_SUMMARY` when set and always to the log; on failure, the last
  200 lines of the run's log in a collapsed `::group::`, so the job log alone explains the failure.
- **Correlation**: `DEVKIT_RUN_ID` is reused when set (a plugin, or a CI step that sets it per job) and generated
  otherwise; it is logged as `OperationId` (ADR 0036 §2.1's name) on every line and passed to every plugin and child.
- **Run summary** at the end of every run on stderr: phases with results and times, failed commands, the log file's path.
  The error message of a failed run always names the log file.
- **Plugins inherit it through the contract**: a Rust plugin calls `paddock_tool::main` and gets all of the above under the
  same `OperationId`; a shell plugin gets the variables and is logged by the host as a spawned command (argv, exit decoded,
  stderr tail), so the minimum holds for any language.
- **Bash callers in YAML** use `devkit run -- <cmd>` (the exit decoding and stderr tail of `run-native.mjs`) and
  `devkit ci error|skip|summary` (what `ci-log.sh` defines), so `ci-log.sh`, `ci-log.mjs` and `run-native.mjs` are not
  ported a second time.

**`wiki` and `heavy` adopt it now**, so they need no retrofit: both depend on `paddock-tool` from their first merge
(behind a thin local `run()` until `paddock-tool` lands, then a one-line switch); `wiki` takes `paddock-cli`'s exit codes
(findings exit 5, not 1) while its callers are few (no backward compatibility before v1); `heavy` logs each slot wait,
take, reclaim and release as phases with the waited time and logs the command it runs as a spawned command. `heavy` keeps
`heavy.sh`'s on-disk slot format only for the rollout, so shell and Rust waiters share slots; after the overlap it moves to
`paddock-slots` (kernel file locks, no stale reclaim), which ER-22 asks for.

### G.6 Reclassified package list

The R-packages keep their numbers. New generic packages are **G0 to G12**. "Status" is what exists on 2026-10-31.

| WP | Content | Class | Status | Days |
|---|---|---|---|---|
| G0a | `wiki` (replaces three wiki-lint copies) | Generic, standalone | in progress (`tools/wiki`) | +0.5 to adopt G1 |
| G0b | `heavy` (replaces `heavy.sh`, plock) | Generic, standalone | in progress (`tools/heavy`) | +0.5 to adopt G1 |
| G1 | `paddock-tool` runtime: common flags, log set-up, phases, spawned-command logging, NTSTATUS decoding in `paddock-proc`, CI error/summary/mask, correlation id, `ToolCommand` registry, external subcommand discovery, `devkit.toml`, packs via `paddock-packs`, `CONTRACT.md` and its two-plugin test | Generic | new | 2.5 |
| G2 | `devkit` skeleton: workspace, own leg in the planner, `help`, tag release to the registry, the "no Paddock path" check (G.8) | Generic | new | 1.5 |
| G3 | `devkit run` and `devkit ci error|step|skip|summary` (from `ci-log.sh`, `ci-log.mjs`, `run-native.mjs` on their branches) | Generic | branches | 1.5 |
| G4 | `devkit commit-lint` (two copies; types and forbidden trailers from flags or `devkit.toml`) | Generic | – | 0.75 |
| G5 | `devkit bisect-flaky`, `devkit find-polluter` (rules 57, 58) | Generic | `docs/rca-rules` | 1 |
| G6 | `devkit temp-leaks` (prefixes from a pack) and `devkit hermetic -- <cmd>` (C14's dynamic part) | Generic | – | 1 |
| G7 | `devkit coverage gate` (lcov; Angular's guard is the same gate) | Generic | – | 0.5 |
| G8 | `devkit secrets strip` (two copies) | Generic | – | 0.5 |
| G9 | `devkit ci timings` (Gitea and GitHub; Paddock's repo list as config) | Generic | – | 1 |
| G10 | `devkit worktree add|list|clean|disk` and `devkit prune` (moves the prune command from `tools/dev-host-prune`) | Generic | prune on branch | 1.5 |
| G11 | `devkit gitea release` (three publish copies) | Generic | – | 1 |
| G12 | `devkit macos verify-bundle`, `devkit win crt-imports` (with R2) | Generic | – | 1.5 |
| R0 | skeleton, graph, slots, input hash | Paddock | merged | done |
| R0b | prebuilt download; **generalised to any `tools/<tool>/`** so `devkit`, `wiki` and `heavy` use the same content-addressed cache in Paddock CI | Paddock | – | 2 |
| R1 | composite actions' logic; generic parts (stable workspace, sccache stats, cache) as `devkit ci` where they need no Paddock path | Split | – | 2.5 |
| R10 | app workflows' long `run:` blocks | Paddock | – | 2 |
| R2 | release kit; G8, G11, G12 taken out | Paddock | – | 3 |
| R3 | `fetch`: the engine (pinned download from a TOML catalogue, verify, extract) is `devkit fetch`; Paddock's catalogues are packs | Split | partial on `tools/dev-fetch-r3` | 1.5 |
| R4 | `dev plan`; timings and coverage moved to G7, G9 | Paddock | `graph affected` merged | 2 |
| R5 | `dev host`: container runner and the vm-smoke engine (Tart, judge) generic in `devkit`, per-app config as packs; verify-installed per-app config; prune moved to G10 | Split | prune on branch; `tools/win-*` VM work on branches | 3 |
| R6 | notices (generic: cargo-about plus npm lock, allow-list from `deny.toml`) in `devkit`; window weight, startup bench, plugin builds in `./dev` | Split | notices slice on `tools/dev-notices` | 1.5 |
| R7 | `new-app`, `packages publish` | Paddock | – | 2 |
| R8a | joint rules-model note (unchanged), plus: which rule kinds are data | – | – | 1 |
| R8 | `paddock-repo-rules` engine (generic) and `devkit check` over packs; Paddock's code checks in `./dev check`; Appendix G's generic checks into a generic pack shipped with `devkit` | Split | – | 5.5 |
| R11 | `dev diagrams`; the draw.io export is generic | Split | branch ahead 9 | 1 |
| R12 | `dev icons` | Paddock | – | 4.5 |
| R13 | `dev images` | Paddock | – | 2 |
| R14 | npm glue | Paddock | – | 1 |
| R15 | lossy categories (unchanged) | Keep | – | 2.5 |
| R16 | clean-up, plus deleting engineering-rules' replaced scripts and templates after the overlap | – | – | 1 |

### G.7 Waves and ordering

**Release first (rule 1, ER-1).** Tooling uses spare capacity only: at most two tooling agents while a release is in
flight, none touching a release path (R1, R2, R10, G12) during an app's release window. The macOS host runner is the
bottleneck: tooling branches run their Linux leg on every push and the macOS and Windows legs only when OS-specific modules
change or on the final run before merge (rule 50). Every local build or test runs through `heavy` (today `heavy.sh`) with
`CARGO_BUILD_JOBS=2`.

| Wave | Packages | Agent-days | Parity tests (golden against the old script, on all three OS) | Retirement step |
|---|---|---|---|---|
| 0 (in progress) | G0a `wiki`, G0b `heavy` | +1 after G1 | `wiki lint --json` equals `wiki-lint.mjs`'s findings on the wiki fixtures and on `docs/wiki`; `heavy` and `heavy.sh` waiters sharing one slot folder, never more than N holders (a stress test with a fake clock), the home guard's refusals as goldens | `scripts/wiki-lint.mjs` (+test) and engineering-rules' two copies; `heavy.sh` after one overlap release, templates and skills name the binary |
| 1a | G1, G2 | 4 | the contract test (a bash plugin and a Rust plugin: env, one JSON object, exit codes, contract refusal, `OperationId` in both logs); a failure fixture whose log alone names the cause | – |
| 1b | G3 to G11 | 9 | per command: the old script's stdout, stderr's `::error::` lines and exit code on fixture inputs, as goldens (`paddock_testkit::golden`); `run` on fixtures that exit with 0xC0000409 (Windows), SIGSEGV (Unix); timings on recorded API replies; commit-lint on the existing 77-line test's messages; bisect and polluter on scripted fake test commands | each PR deletes the script and its test, switches YAML, `package.json`, lefthook and docs, removes the copy from `docs/known-copies.toml`, and shrinks §3.7's baseline |
| 2 | R0b, R1, R10, R2 + G12, R3, R4, R5, R6, R7, R8a, R8, R11, R12, R13, R14, R15, R16 (this order; R2 and R1 never inside a release window) | 38 | as §3.6 (goldens, shadow week for plan, release, `latest.json`, rules; release dry runs per app); for split engines also a generic fixture repo that is **not** Paddock-shaped | as §3.7; R16 also removes the engineering-rules scripts after their overlap |

Total about **52 agent-days**, against §7's 45 plus 1.5: about 4 more for the runtime, contract and logging (G1, G2), about 3
more to split engines from Paddock packs; R0 (3.5) is done.

### G.8 Checks that keep it so (rule 40)

- **No Paddock in generic tools.** A conformance check (extending WP-C6, §3.7) fails when a file under `tools/devkit`,
  `tools/wiki`, `tools/heavy` or `crates/paddock-tool` names `apps/`, `paddock.ci.json`, an app id, `PADDOCK_` variables or
  a Paddock-only crate; counted waivers as in ADR 0021.
- **Every tool logs.** ADR 0036's process-log check (C63 there) applies to tools: a `main` that does not start through
  `paddock_tool::main` (or `paddock_log::Logging::install`) fails.
- **One contract.** The contract test runs for `devkit`, `wiki`, `heavy` and `./dev`.

### G.9 Risks

| Risk | Mitigation |
|---|---|
| `devkit` couples unrelated commands' releases | golden tests per command; a regression is a patch release; a command with its own rhythm graduates (G.3) |
| A `PATH` plugin shadows or impersonates a command | built-ins cannot be shadowed; `help` shows each command's origin; the trust rule (repository or registry install) is the boundary, the same as any binary on `PATH` |
| `paddock-tool` waits for ADR 0036 WP-1/WP-3 | G1 starts on paddock-log's current API and switches in WP-1's change; WP-1 is small and shared |
| Generic tools drift back into Paddock knowledge | G.8's check; a fixture repository that is not Paddock-shaped in each generic tool's tests |
| Two systems during overlap (engineering-rules scripts and binaries) | one overlap release, then deletion in R16; templates name the binary from the first release |
| The move to an own repository later costs a history split | tools live in self-contained `tools/<tool>/` folders with their own lockfiles, so `git filter-repo --path` moves them |

### G.10 Decisions

Decided (user, 2026-10-31), before the amendment: `wiki` and `heavy` are standalone generic tools; `wiki` in Paddock
`tools/`, published to `decryp7`, its own repository later; **no sandbox for tool plugins**: plugins come only from the
repository or a registry install, never fetched at run time.

Decided (user, 2026-10-31), on this amendment, all three as recommended:

1. **One generic `devkit` with subcommands** for the small generic commands; `wiki` and `heavy` stay standalone (G.3).
2. **Conformance becomes a generic rules engine**: `paddock-repo-rules` with data rule kinds, `devkit check` with a
   generic pack, and Paddock's own checks in `./dev check` (G.4, R8; the R8a joint note still comes first).
3. **Extensibility is the built-in registry, external `devkit-<name>` subcommands and TOML packs**; ADR 0001's plugin
   platform is not reused, and there is no sandbox (G.4).

Defaults that stand (coordinator, 2026-10-31): the name `devkit`; `wiki` on `paddock-cli`'s exit codes (findings exit 5);
the project config file `devkit.toml`; a one-release overlap before engineering-rules' scripts are deleted.

The amendment stays **Proposed** until an independent review; nothing is left for the user to decide.

### Appendix G: inventory and classification (2026-10-31)

Every non-Rust tooling file of Paddock `origin/main` at `233c1339b`, the four named branches and engineering-rules at
`791e941`, without tests, fixtures, the UI's own Vitest and Playwright specs, and ESLint configs. Copies of one mechanism
are one row. Users: CI, Rel (release), Ag (agents), Dev, Hook (lefthook). R: the ADR 0022 package. Class: G generic, P
Paddock, S split, K keep.

| # | Path(s) | Lang | Lines | What it does | Users | R | Class | Target |
|---|---|---|---|---|---|---|---|---|
| 1 | er `scripts/heavy.sh` (+ plock) | sh | 217 | machine-wide slot semaphore for heavy steps, home guard | Ag, Dev, Hook | – | G | `heavy` (G0b) |
| 2 | er `scripts/wiki-lint.mjs`, er `templates/wiki/wiki-lint.mjs`, `docs/wiki-pilot` `scripts/wiki-lint.mjs` | js | 327+327+244 | wiki lint | Ag, CI | – | G | `wiki` (G0a) |
| 3 | er `scripts/worktree.sh` | sh | 125 | worktree per agent with build dir and port; list, clean, disk | Ag | – | G | `devkit worktree` (G10) |
| 4 | er `scripts/strip-secret-whitespace.sh`, `conformance/release/strip-keys.mjs` | sh, js | 77+52 | strip whitespace from secrets | Rel, Hook | R2 | G | `devkit secrets strip` (G8) |
| 5 | er `scripts/ci-timings.mjs`, `scripts/ci-timings.mjs` | js | 155+355 | CI run and job timings from the Actions API | Dev, Ag | R4 | G | `devkit ci timings` (G9) |
| 6 | `conformance/src/commit-lint.mjs`, `apps/debrief/scripts/commit-lint.mjs` | js | 72+67 | Conventional Commits, no attribution | CI, Hook | R8 | G | `devkit commit-lint` (G4) |
| 7 | `ci/diagnostics`: `conformance/ci/ci-log.sh`, `ci-log.mjs` | sh, js | 161+77 | `::error::` with next step, timed steps, summary, masking | CI | – | G | G1 runtime, `devkit ci` (G3) |
| 8 | `ci/setup-tools-hardening`: `conformance/ci/run-native.mjs` | js | 109 | run a native program, decode exit/NTSTATUS, stderr tail | CI | – | G | `devkit run` (G3), `paddock-proc` |
| 9 | `docs/rca-rules`: `scripts/bisect-flaky.sh` | sh | 45 | `git bisect run` payload for a flaky test | Ag, Dev | – | G | `devkit bisect-flaky` (G5) |
| 10 | `docs/rca-rules`: `scripts/find-polluter.mjs` | js | 101 | delta-debug the spec file that pollutes a victim | Ag, Dev | – | G | `devkit find-polluter` (G5) |
| 11 | `apps/debrief/scripts/ci/check-temp-leaks.mjs` | js | 133 | tests left files in the job temp folder | CI | – | G | `devkit temp-leaks` + pack (G6) |
| 12 | `conformance/src/hermetic.mjs` | js | 94 | run tests with HOME/TMP in scratch, check leftovers (C14) | CI | R8 | G | `devkit hermetic` (G6) |
| 13 | `scripts/coverage-gate.mjs`, `apps/pitwall/scripts/ci/ui-coverage-guard.mjs` | js | 49+47 | lcov line-coverage gate | CI | R4, R14 | G | `devkit coverage gate` (G7) |
| 14 | `conformance/release/publish.mjs`, `apps/pacenotes/.../publish.mjs`, `apps/sighter/.../publish.mjs` | js | 117+93+93 | Gitea release and asset upload, idempotent | Rel | R2 | G | `devkit gitea release` (G11) |
| 15 | `conformance/release/macos-verify-bundle.sh`, `apps/debrief/scripts/ci/macos-verify-bundle.sh`, `scripts/demo/verify-macos.sh` | sh | 156+85+83 | verify `.app` and DMG sealing and signatures | Rel | R2 | G | `devkit macos verify-bundle` (G12) |
| 16 | `conformance/release/windows-crt-imports.mjs` | js | 305 | PE imports of the VC runtime (ADR 0032) | Rel | – | G | `devkit win crt-imports` (G12) |
| 17 | `tools/prune-build-folders.sh`, `.ps1`, `tools/prune-ci-build-folder.sh`, `apps/debrief/scripts/clean-build-dirs.ps1` | sh, ps1 | 38+57+22+56 | cap build folders by size | CI, Dev | R5 | G | `devkit prune` (G10) |
| 18 | `apps/debrief/scripts/ci/check-quarantine.mjs` | js | 45 | quarantine entries carry a valid expiry (ER-12) | CI | R8 | G | generic rule pack |
| 19 | `apps/debrief/scripts/ci/scan-fixtures.mjs` | js | 124 | fixtures must not look like real logs (ER-14) | CI | R8 | G | generic rule pack |
| 20 | `ci/setup-tools-hardening`: `conformance/src/checks/C62-pipe-grep.mjs` | js | 140 | no `cmd \| grep -q` under pipefail | CI | R8 | G | generic rule (code) |
| 21 | `apps/debrief/scripts/macos/dev-signing.sh`, `build-app.sh` | sh | 58+17 | self-signed local signing identity keeps TCC grants | Dev | R2 | G | `devkit macos dev-identity` (wave 2) |
| 22 | `conformance/src/notices/*`, `scripts/notices.sh` | js, sh | 421+22 | THIRD_PARTY_NOTICES from cargo-about and npm lock | CI, Rel | R6 | G | `devkit notices` (allow-list from `deny.toml`) |
| 23 | `conformance/src/{cli,runner,report,context,finding,index}.mjs`, `util/*` | js | 844 | conformance engine: checks, waivers, reports | CI | R8 | S | `paddock-repo-rules` engine + `devkit check` |
| 24 | checks C14, C15, C16, C23, C24, C30, C31, C35, C36, C39, C53, C57 | js | 944 | rules that follow from ER/PRINCIPLES (hermetic, installed browser, hard-coded version, process, reviewed images, wall clocks, ADR numbers, untimed calls, parser hardening, freed port, pid kills, tracked symlinks) | CI | R8 | S | generic pack (data where expressible, else code in the engine) |
| 25 | checks C03, C04, C06, C07, C08, C13, C28, C29, C32, C33, C34, C37, C43, C45, C48, C50, C51, C54, C56 + `index.mjs` | js | 1,936 | family checks (icons, signing, release, presets, i18n, a11y, sidecars, dialogs, shell start, QoS, env names) | CI | R8 | P | `./dev check` code checks + Paddock pack |
| 26 | `conformance/src/sync/*` | js | 364 | sync baseline files and doc blocks from templates (C23, C28) | CI, Dev | R8 | S | generic "sync from a template repo" engine (also for engineering-rules' `rules-sync`) + Paddock manifest |
| 27 | `conformance/ci/install-test-browser.mjs`, `apps/spotter/scripts/install-test-browser.mjs`, `pin-chromium.mjs` | js | 183+55+72 | pinned Chrome for Testing: pin and install | CI | R3 | S | `devkit fetch` + catalogue pack |
| 28 | `conformance/ci/pin-runtime.mjs`, `runtime-catalogue.mjs`, `apps/debrief/app/scripts/libduckdb.mjs`, `apps/debrief/scripts/ci/libduckdb-env.sh`, `update-ffmpeg-runtimes.py`, `update-llama-runtimes.py` | js, sh, py | 127+118+86+59+147+149 | pinned runtime catalogues and their fetchers | CI, Dev | R3 | S | `devkit fetch` + Paddock catalogues |
| 29 | `tools/linux-container.sh`, `apps/dyno/scripts/linux-container.sh`, `linux-verify.sh`, `apps/pitwall/scripts/linux-container-verify.sh`, `-inner.sh`, `apps/debrief/scripts/ci/smoke-linux-docker.sh`, `e2e-linux-local.sh` | sh | 76+51+16+28+68+46+45 | run cargo or tests in a native Linux container | Dev, Ag | R5 | S | `devkit container` + per-app pack |
| 30 | `tools/macos-vm-smoke.sh`, `scripts/vm-smoke/*`, `tools/win-*` branches | sh, js | 186+735 (+branches) | VM smoke: Tart clone, install, judge | Dev, Rel | R5 | S | `devkit vm` engine + per-app JSON/TOML |
| 31 | `tools/host-cleanup.sh` | sh | 108 | leave nothing behind on a host runner | CI | R1, R5 | S | engine (processes by identity, ADR 0033) + Paddock pattern pack |
| 32 | `conformance/ci/job-isolation.sh`, `job-tmpdir.sh`, `build-jobs.sh`, `ci-home.sh`, `build-folder.sh` | sh | 445+120+166+28+38 | host-runner job isolation (ADR 0025) | CI | R1 | S | `devkit ci` on `paddock-slots`; Paddock variable names as config |
| 33 | `conformance/ci/check-index.mjs`, `apps/debrief/scripts/ci/check-index.mjs` | js | 25+71 | no inline style/script in the built index.html (CSP) | CI | R2, R8 | S | rule kind "built HTML" in the generic pack |
| 34 | `apps/pitwall/scripts/ci/privacy-check.sh` | sh | 22 | `reqwest` only in `egress.rs` | CI | R8 | S | rule kind "only in" + Pitwall pack line |
| 35 | `apps/dyno/scripts/check-capabilities.mjs`, `check-shipped.mjs`, `check-wording.mjs` | js | 94+82+145 | Tauri capabilities least privilege; 1.0 ships no helper; UI wording | CI | R8 | S | rule kinds + Dyno pack |
| 36 | `apps/debrief/scripts/export-diagrams.sh`, pacenotes, sighter, `diagrams/scripts/`, `templates/repo/scripts/` | sh | 5×41 | draw.io CLI export | Dev | R11 | S | export in `devkit diagrams export` later; `dev diagrams` now (branch) |
| 37 | `.gitea/actions/*/action.yml` (27 actions) | yaml + sh | 1,091 | composite CI actions | CI | R1 | S | YAML one-liners calling `devkit ci` or `./dev ci` |
| 38 | `.gitea/workflows/*` long and OS-branching `run:` blocks | yaml + sh | about 450 | per-app CI steps | CI | R10 | P | `./dev` commands, `paddock.ci.json` |
| 39 | `scripts/affected.mjs`, `conformance/ci/plan.mjs` | js | 1,067+38 | affected-projects planner, run gate | CI, Ag | R4 | P | `./dev plan` |
| 40 | `scripts/publish.mjs` | js | 436 | publish crates and npm packages | Rel | R7 | P | `./dev packages publish` |
| 41 | `scripts/new-app.mjs` | js | 580 | scaffold an app from a demo | Dev | R7 | P | `./dev new-app` |
| 42 | `scripts/demo-release.mjs`, `scripts/demo/bundle.sh` | js, sh | 197+52 | demo apps' release steps | Rel | R2 | P | `./dev release` |
| 43 | `conformance/ci/tauri-bundle.sh` | sh | 100 | `tauri build`, copy installers | Rel | R2 | P | `./dev release bundle` |
| 44 | `conformance/release/latest-json.mjs` | js | 118 | updater `latest.json` | Rel | R2 | P | `./dev release latest-json` |
| 45 | `conformance/ci/check-tag.mjs` | js | 36 | tag names the app version | Rel | R2 | P | `./dev release` |
| 46 | `conformance/ci/release-notes.mjs`, `first-release-notes.sh` | js, sh | 45+59 | release notes | Rel | R2 | P | `./dev release notes` |
| 47 | `conformance/ci/release-verdict.mjs` | js | 62 | what a release run proves (rule 54) | Rel | R2 | P | `./dev release verdict` |
| 48 | `conformance/ci/build-env.mjs` | js | 33 | build commit and date for About | CI | R14 | P | `./dev build-info` |
| 49 | `conformance/ci/run-commands.sh` | sh | 24 | run `paddock.ci.json` command lists | CI | R1 | P | `./dev ci run` |
| 50 | `conformance/src/sidecar/*`, `apps/pitcrew/scripts/sidecar.mjs`, `apps/pitwall/scripts/sidecar.mjs`, `apps/debrief/app/scripts/build-sidecars.mjs` | js | 170+25+26+123 | build Tauri sidecars | Rel, Dev | R2 | P | `./dev release sidecars` |
| 51 | `conformance/src/window-weight/*`, `apps/debrief/app/scripts/window-weight.mjs` | js | 152+107 | JS per window before first paint | CI | R6 | P | `./dev window-weight` |
| 52 | `apps/{debrief,pitwall,spotter}/scripts/ci/check-icons.mjs` | js | 3×121 | icon fills the canvas (now C03) | CI | WP-C6 | P | delete (C03 has it) |
| 53 | `apps/debrief/scripts/ci/check-smoke-dialogs.mjs` | js | 31 | smoke specs never look for a dialog in the page | CI | R8 | P | Debrief pack |
| 54 | `apps/debrief/scripts/ci/check-workflows.py` | py | 35 | workflow plan gate (local) | Dev | R8 | P | `./dev check` |
| 55 | `apps/debrief/scripts/ci/startup-bench.mjs` | js | 151 | start-up benchmark | CI | R6 | P | `./dev bench startup` |
| 56 | `apps/debrief/scripts/ci/macos-smoke.sh` | sh | 102 | built app starts its sidecars and quits | Rel | R2 | P | `./dev release smoke` |
| 57 | `apps/debrief/scripts/ci/windows-node-modules.sh` | sh | 53 | node_modules kept per slot on Windows | CI | R10 | P | `./dev ci` |
| 58 | `apps/debrief/scripts/build-plugins.sh` | sh | 43 | build Debrief's plugins binary | Rel | R6 | P | `./dev` |
| 59 | `apps/dyno/scripts/check-bundle.sh` | sh | 77 | Dyno bundle layout | Rel | R2 | P | `./dev release` |
| 60 | `tools/verify-installed.sh`, `apps/{pitcrew,pitwall}/scripts/{linux,macos}-verify.sh` | sh | 117+62+56+57+106 | check an installed app and its service | Dev, Rel | R5 | P | `./dev host verify-installed` + per-app config |
| 61 | `scripts/shell-demo/*` | sh | 287 | shell-demo service and desktop smokes | Rel | R5 | P | `./dev host` + vm engine |
| 62 | `scripts/dialog-commands.mjs`, `apps/debrief/app/scripts/check-dialog-commands.mjs` | js | 79+152 | dialog command contract checks | CI | R15 | P | `./dev check` |
| 63 | `apps/debrief/app/scripts/build-info.mjs`, `whats-new.mjs`, `i18n/*` | js | 55+81+284 | npm glue | Dev, CI | R14 | P | `./dev` from `package.json` |
| 64 | `apps/pitwall/scripts/tauri-before-build.mjs` | js | 14 | `beforeBuildCommand` | Dev, Rel | R14 | P | `./dev` |
| 65 | `icons/tools/*` | js | 4,259 | icon pipeline | Dev | R12 | P | `./dev icons` |
| 66 | `diagrams/tools/*` | js | 718 | family diagram generators | Dev | R11 | P | `./dev diagrams` (branch) |
| 67 | `docs/images/src/*` | js | 1,782 | posters | Dev | R13 | P | `./dev images` |
| 68 | `tools/paddock-dev/tests/golden/generate.mjs` | js | 57 | goldens from `affected.mjs` | Dev | R4 | P | delete with `affected.mjs` |
| 69 | `tools/test/helpers.mjs` | js | 101 | tests of `tools/*.sh` | CI | R5 | P | goes with the scripts |
| 70 | `apps/debrief/scripts/perf/*` | py, sh | 343 | Debrief import benchmark harness | Dev | – | P | Debrief `xtask` (beside `gen-samples`) |
| 71 | `apps/debrief/app/scripts/dialog-commands-analysis.mjs` | ts-js | 481 | TS type-checker analysis | CI | R15 | K | until the contract is generated from Rust (§4) |
| 72 | `apps/debrief/app/scripts/lint-effects.mjs`, `lint-styles.mjs` | js | 91+152 | UI source lints | CI | R15 | K | ESLint preset (§4) |
| 73 | `apps/debrief/app/scripts/screenshots/*`, `visual-qa/*` | ts | 732 | Playwright screenshots, visual QA | Dev | – | K | Playwright (§4) |
| 74 | `eslint/`, app `eslint.config.js`, `playwright.config.ts` files | js, ts | about 1,200 | Node/Angular toolchain config | CI | – | K | stays |
| 75 | `ui/scripts/build-css.mjs`, `build-e2e.mjs`, `stamp-dist.mjs` | js | 45 | Sass and tsc build glue of the UI packages | CI, Dev | R14 | K | tied to the npm toolchain |
| 76 | `tools/bootstrap.sh`, `scripts/setup-dev.sh`, `apps/{pitcrew,pitwall}/scripts/{linux,macos}-bootstrap.sh` | sh | 112+64+63+56+61 | machine bootstrap | Dev | R5 | K | one bootstrap per OS (§4) |
| 77 | `ci-image/verify.sh`, `ci-image/fetch-verified.sh` | sh | 94+35 | CI image build (before Rust tools exist) | CI | – | K | bootstrap |
| 78 | `scripts/perf/link-time/*` | sh, py | 107 | recorded linker measurements (rule 48 evidence) | Dev | – | K | evidence, not tooling |
| 79 | `spikes/dialog-hidden/*` | js | 164 | spike | – | – | K | stays with the spike |
| 80 | `apps/debrief/scripts/rename-to-debrief.mjs` | js | 336 | one-off rename | – | R1 | K | delete |
| 81 | `apps/pitcrew/scripts/windows-retire-legacy-runner.ps1` | ps1 | 142 | one-off runner retirement | – | R1 | K | delete |
| 82 | `conformance/bin/*.mjs` | js | 12 | entry points | CI | R8 | K | delete with the engine |
| 83 | `conformance/fixtures/pitwall-latest-json.mjs` | js | 118 | fixture | CI | R2 | K | becomes a golden |
| 84 | er `templates/ci/*.yml`, `templates/lefthook.yml` | yaml | – | templates that call 1, 4 | Ag | – | K | name the binaries (G.3) |


Summary: G 22 (rows 1–22), P 34 (rows 25 and 38–70), S 14 (rows 23, 24 and 26–37), K 14 (rows 71–84). Each
script's tests (`*.test.mjs`, `*.test.sh`, `tools/test/*`) go with it.

## Sources

- Research notes: [Nx, Turborepo or our own scripts](../research/2026-10-nx-vs-scripts.md);
  [Rust tooling inventory and measurements](../research/2026-10-rust-tooling-inventory.md)
- cargo xtask pattern: <https://github.com/matklad/cargo-xtask> (read)
- cargo-hakari publishing: <https://docs.rs/cargo-hakari/latest/cargo_hakari/publishing/index.html> (read)
- Gitea generic package registry (one upload per file name and version): <https://docs.gitea.com/usage/packages/generic> (read)
- Gitea Actions secrets: <https://docs.gitea.com/usage/actions/secrets> (read; workflow-level scoping not offered, unverified on our instance)
- resvg: <https://github.com/linebender/resvg> (read); `@resvg/resvg-js` is its Node binding: <https://github.com/thx/resvg-js> (read)
- oxc (parser and linter, no type checker): <https://oxc.rs/docs/guide/introduction.html> (read)
- The native TypeScript compiler is written in Go: <https://devblogs.microsoft.com/typescript/typescript-native-port/> (read)
- Playwright browsers: <https://playwright.dev/docs/browsers> (read)
- Node.js type stripping: <https://nodejs.org/api/typescript.html> (read)
- SARIF 2.1.0: <https://docs.oasis-open.org/sarif/sarif/v2.1.0/sarif-v2.1.0.html> (read)
