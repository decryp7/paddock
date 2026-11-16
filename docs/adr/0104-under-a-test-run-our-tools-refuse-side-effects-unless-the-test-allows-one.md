# 0104. Under a test run our tools refuse side effects, unless the test allows one

- Status: Proposed (2026-11-17). The user approved drafting this redesign on 2026-11-16 (relayed by the coordinator's brief),
  with a budget of about 1 agent-day for the ADR and its implementation plan. Building it is not approved: the options and their
  costs are in "Decisions for the user".
- Date: 2026-11-17
- Related: ER-14 / rule 7 (tests never touch the user's machine), ER-21 / rule 40 (a recurring class gets a redesign), ER-22 /
  rule 42 (share first), ER-27 (logs explain every failure), ER-37 / rule 76 (common practice first), ER-41 (new checks start
  report-only), ER-45 (checks have no side effects), ER-46 (lists only shrink), ER-47 (children inherit the environment).
  ADR 0077 (tests never reach system services: the same shape for product code; its test-run marker was never built, see
  "What exists"), ADR 0078 (one shared test scratch), ADR 0100 (tests start programs through one builder with a listed
  environment; held on a branch), ADR 0103 (reap every process a test starts).
- Number: 0104. On 2026-11-17 the highest ADR on `origin/main` and on every `origin` branch was 0103 (`git ls-tree --name-only
  <branch> docs/adr/` over `git for-each-ref refs/remotes/origin`; 0100 on `feat/test-env-builder` and others, 0101 to 0103 on the
  trains). Rule and check numbers are taken on acceptance (`./dev check new-id --reserve <branch>`).
- Evidence heads read (ADR 0091), re-read before the commit: `origin/train/2026-11-15-13` 3585bcab1 (this branch's base; every
  file:line below is read there unless a row names a branch), `origin/main` 23213a35e, `origin/train/2026-11-15-12` 098a17e2e,
  `origin/feat/local-sccache` 0568c5e72, `origin/feat/test-env-builder` 9ac185c24, `origin/fix/testkit-ambient-hardening`
  1c522a079, `origin/feat/dev-launch-rust` c00a0f271, `origin/feat/no-new-shell-check` a403f5df5. Shipped code: none of the
  commits cited below is in a tag (`git tag --contains` of 95ac496a9, 91db3611f, 872ab6743, 0568c5e72 and c7d27c7fc is empty).
  This ADR is about developer and CI tools, which ship in no app installer.

## Context: the class

**A tool run by a test does what it does for a person: it installs, downloads, starts a server or writes into the real home.**
Our tools (`./dev`, `paddock-dev`, `heavy`, the Node scripts) read the caller's environment and act on the machine. A test that
runs them gets the developer's or the runner's machine unless it remembered to turn each effect off. Every fix so far was one
more per-test switch.

The instances (ER-45 records all three; each row cites the commit that fixed it with a test that is red without the fix):

| # | When, where | What happened | The fix | State |
| --- | --- | --- | --- | --- |
| 1 | 2026-11-16, a reviewer's probe of `./dev` (ER-45, ER-14) | A probe ran a `rustup` proxy below `rust-toolchain.toml`; rustup 1.28.1 and later installs a missing pinned toolchain on any proxy run, so the probe installed a toolchain into the user's `~/.rustup`. | 95ac496a9 (`feat/dev-launch-rust`): `RUSTUP_AUTO_INSTALL=0` on every probe of stage 0, the launcher, `dev.cmd` and setup-rust. Red first: `a_probe_with_a_missing_pin_never_contacts_the_dist_server`, `the_launchers_probes_never_start_an_install_of_a_missing_pin`. | Per call site, on a branch |
| 2 | 2026-11-16, C129 review round 2 (`feat/no-new-shell-check`) | C129's positive control runs on an in-memory tree whose root is `""`, so its two `baseline::growth` calls ran `git` in the process's folder, the real repository, and compared the control with the real lists. C129 reported "check broken" once on main. | 91db3611f: `Ctx::root()` panics on an in-memory tree, so any control that would run git or read files fails loudly. Red first: a process standing in a repository whose `origin/main` lists differ. | Fixed for checks' controls, on a branch |
| 3 | 2026-11-16/17, review of `feat/local-sccache` 872ab6743 + 0568c5e72 | `paddock-dev`'s integration tests started a real sccache server. `generators::run::Real::new` (`tools/paddock-dev/src/generators/run.rs:114-121` on the branch) calls `local_sccache::prepare_and_say` eagerly; `merge check` builds a `Real::new` for its generator step (`merge_check/mod.rs:437`); `prepare` runs `sccache --start-server` (`local_sccache.rs:487`) with the caller's `SCCACHE_DIR` (`:95-97`), on sccache's default port 4226 unless `SCCACHE_SERVER_PORT` is set. The reviewer's run with `PADDOCK_SCCACHE` unset and a private port 4793 left a server listening after the tests (`/Volumes/vault-dev/dev-targets/scratch/reviewer-sccache/gates.log:50-55`: pid 70706, `127.0.0.1:4793 (LISTEN)`); on the default port that is the agents' shared server with the user's real cache. | Per-test `.env("PADDOCK_SCCACHE", "off")` in `tests/it/generators.rs:102`, `merge_check.rs:70`, `pre_push.rs:39` (the branch). Which test started the reviewer's server is not named in the notes; any `dev()` helper without the switch can. | Open, on a branch |

Earlier members of the same shape, each fixed by a per-test switch: ADR 0100's two `heavy` instances (`PADDOCK_HEAVY=off`, then
a fixture `ENGINEERING_RULES` in 2ff5f5072, so a test stopped taking machine-wide slots in `~/.cache/heavy-slots`), and
`paddock-secrets`' `PADDOCK_SECRETS_FORBID_REAL` (`crates/paddock-secrets/src/os/mod.rs:19-35`: opt-in per suite, with a reverse
switch `PADDOCK_SECRETS_REAL`).

**Why per-test switches cannot end it.** A switch exists only after its effect has hurt someone. The next tool that downloads,
starts a server or writes a cache adds its own switch, and every test that runs it without the switch reaches the machine. This
is the deny-list problem ADR 0100 names for environment variables, one level up: there the unknown is the next variable, here it
is the next effect.

**Instance 2 is a near relative, not the same effect.** It read the real repository (an input), it did not change the machine.
The guard below covers it only as far as "run `git` outside the test's own folders" counts as an effect; D4 decides that.

## What exists (cited by enforcing code, not by comments)

| Mechanism | Enforcing code | Constraint |
| --- | --- | --- |
| One place that starts programs | `paddock_proc::command` (`crates/paddock-proc/src/command.rs:41`); Clippy `disallowed-methods` forbids `Command::new` elsewhere | Starts a program; decides nothing about effects |
| The tools' wrapper for third-party programs | `ci_tool::ToolCall::run` (`tools/paddock-dev/src/ci_tool.rs:209`), child environment from `child_env` (`:138`) = `paddock_proc::KEPT_ENV` (`command.rs:21-36`) plus the names the caller passes | Clears the environment: a marker or a restriction variable not in `KEPT_ENV` is **stripped** from every child (the ER-47 hazard) |
| Hermetic environment in a test process | `paddock_testkit::EnvLock::hermetic` (`crates/paddock-testkit/src/env.rs:53`) | In-process only; not set on children unless the test passes it |
| Listed-environment builder for children | `paddock_testkit::TestCommand` (ADR 0100), `crates/paddock-testkit/src/process.rs` | **Not on any train**: built on `feat/test-env-builder` 9ac185c24 and `fix/testkit-ambient-hardening` 1c522a079; merged into train 12 and reverted (c7d27c7fc, on train 13). A prerequisite of option A and of WP-3 |
| Process reaper | `paddock_testkit_start::Reaper` (`crates/paddock-testkit-start/src/reap.rs:54`, token `PADDOCK_REAPER_ID` `:36`) | Ends what a test started through it; a daemon started by the tool under test (sccache forks its server) is outside its tree unless it carries the token (readable on Linux only; ADR 0103 measured that macOS shows no other process's environment) |
| C14 dynamic part | `conformance/src/hermetic.mjs` (`paddock-conformance run -- <command>`): HOME, XDG, APPDATA, TMPDIR into a scratch folder, then checks leftovers and live processes of the run | Redirects HOME, so it cannot see writes to an absolute path the profile set (`SCCACHE_DIR`), nor a daemon outside the process group |
| Home guard for build folders | `paddock_env::home_guard` (`crates/paddock-env/src/home_guard.rs`), used by `heavy` and `generators::run::Real::target_dir` (`run.rs:137` on train 13) | Build folders only |
| Product-side reach decision | ADR 0077 §1 (`paddock_env::system_reach`, marker `PADDOCK_TEST_RUN` or `NEXTEST_RUN_ID`) | **Not built.** Trimmed 2026-11-11 to WP-3 (the guards in `paddock-secrets` and `paddock-service`); `git grep PADDOCK_TEST_RUN` on train 13 finds nothing |
| Test-run variables nextest sets | nextest sets `NEXTEST=1` and `NEXTEST_RUN_ID` in every test process and honours cargo's `[env]` (nexte.st, "Environment variables") | Free; a plain `cargo test` sets neither |

## The side-effect paths that need the guard

Read on train 13 (3585bcab1) unless marked **(branch)**. "Tests" says how tests reach the path today.

| Effect | Path (file:line) | What it does | Tests today |
| --- | --- | --- | --- |
| Server start/stop | `tools/paddock-dev/src/local_sccache.rs:487` **(branch `feat/local-sccache`)** `prepare` | `sccache --start-server` on the default port with the caller's `SCCACHE_DIR` | Per-test `PADDOCK_SCCACHE=off` (instance 3) |
| Server start/stop | `generators/run.rs:118` and `pre_push/mod.rs:278` **(branch)** | Call `prepare_and_say` eagerly at construction | Same |
| Server start/stop | `ci_setup_rust.rs:543`, `:548`, `:559` | `sccache --stop-server` / `--start-server` (CI's setup-rust) | Stand-in `sccache` shims (`tests/it/setup_rust.rs:69`) |
| Server stop | `ci_host/sccache_stats.rs:41` | `sccache --stop-server` when `SCCACHE_SERVER_PORT` is set | Shims (`tests/it/host_steps.rs:536`) |
| Download | `ci_setup_rust.rs:365` (`download`, base `RELEASES` `:39`, override `PADDOCK_SCCACHE_RELEASES` `:381`) | curl of the pinned sccache from GitHub | Loopback fake server via the override |
| Download + install | `local_sccache.rs:336` (`install_from`), `:409` (`ensure_binary`) **(branch)** | Downloads and unpacks sccache under the tool cache | `Install::Never` in gates; tests with a fake base |
| Download | `ci_host/host_tools.rs:179` | curl of host tools in CI | Fake URLs |
| Network (Gitea API) | `wait.rs:482`, `:566` (`CurlHttp`) | Real HTTP for `--ci-run`, `ci runs`, `pr`, `plan --since-run` | `Http` fakes (`ci_runs/testutil.rs:104`, `pr/fake.rs:83`, ...) |
| Network + install | `locks.rs:108`, `:116` (`cargo update --workspace`, `npm install --package-lock-only`), run by `sync_all` `:258` | Registry access, writes lockfiles | `Runner` fakes |
| Network + install | `worktree/ops.rs:434` (`npm_ci`) | `npm ci` into a new worktree (registry, `~/.npm`) | A fake `npm` (`ops.rs:773`) |
| Writes to the real home | `tool_cache.rs:21` (`home`: `PADDOCK_DEV_HOME`, else `$HOME/.paddock-dev`), `hold` `:64` | Writes and links under `~/.paddock-dev` | `PADDOCK_DEV_HOME` set by hand per test |
| Build + install into the home | `pre_push/heavy_tool.rs:46` (`ensure`), `:113` (`cargo build --release`) | Builds `heavy` into the tool cache | `PADDOCK_HEAVY=off`, `PADDOCK_DEV_HOME` per test |
| Shared machine slots | `tools/heavy/src/config.rs:72-80` (slot root: `HEAVY_DIR`, else `$TMPDIR/heavy-lock`) | Machine-wide build slots | `HEAVY_DIR` per test |
| Containers | `pre_push/exec.rs:712`, `:841` (`DOCKER`, else `docker`) | The Linux container leg of the push gate | `DOCKER` stand-in per test |
| Removals outside the test | `ci_host/cleanup.rs:265` (`remove_all`), `safe_remove.rs:66` | Deletes under the runner's home or a root | Fixture roots |
| Toolchain changes | `dev:133` and `dev:270` (`rustup toolchain install`), `dev:276` (`uninstall`) | Installs a toolchain into `RUSTUP_HOME` | A fixture `RUSTUP_HOME` and `CARGO_HOME` per test (`tests/it/wrapper.rs:406`) |
| Build + download into the home | `dev:387` (`cargo build` of paddock-dev), `dev:445` (curl of the prebuilt, when `PADDOCK_DEV_DOWNLOAD=1`) | Writes `~/.paddock-dev/bin` | `PADDOCK_DEV_HOME`, loopback server |
| Toolchain probe | `tools/dev-launch/src/proc.rs`, `toolchain.rs` **(branch `feat/dev-launch-rust`)** | Rust launcher probes; 95ac496a9 sets `RUSTUP_AUTO_INSTALL=0` | Red-first tests in 95ac496a9 |
| Node: network | `scripts/publish.mjs:289`, `:354` (`npm publish`), `:367`; `scripts/demo-release.mjs:125-146`; `scripts/affected.mjs:991`; `scripts/vm-smoke/report.mjs:22` | Registry and Gitea API | `publish.test.mjs`, `demo-release.test.mjs` with their own fakes |
| Node: real-home paths | `scripts/stage-paddock-setup.mjs:126` (`~/.cargo`), `scripts/vm-smoke/run.mjs:141` (`~/.tart`), `conformance/ci/tauri-tools.mjs:150` | Reads or writes under the home | Per test |
| The testkit itself | `paddock_testkit::PrivateTemp` (`private_temp.rs:35`), `Scratch`; `TestCommand`'s link folder in the target folder (**branch**) | Writes only into temp and target folders | Its own tests |

19 paths in Rust and the `dev` launcher, 4 groups in Node. The testkit has no path that leaves the test's folders; it is where the
marker is set (below), not a client of the guard.

## Common practice (ER-37)

| Software | How its tests keep effects off the machine | Source |
| --- | --- | --- |
| cargo's own suite (`cargo-test-support`) | Every test gets a sandbox folder under `target/tmp/cit/<test>/` with its own `home`; `test_env()` sets `HOME` and `CARGO_HOME` into it and removes every `CARGO_*` variable plus `RUSTC_WRAPPER`, `RUSTC_WORKSPACE_WRAPPER`, proxies, `XDG_CONFIG_HOME`, `SSH_AUTH_SOCK` and others. "You should not write any tests that use the network." Tests that need the public network or Docker are off unless `CARGO_PUBLIC_NETWORK_TESTS=1` or `CARGO_CONTAINER_TESTS=1`. | doc.crates.io/contrib/tests/writing.html, .../running.html; `crates/cargo-test-support/src/lib.rs` (`TestEnvCommandExt::test_env`) |
| rustup's own suite | `clitools::Config::env` sets `RUSTUP_HOME`, `CARGO_HOME`, `HOME`, `USERPROFILE` into the test's folders, `RUSTUP_DIST_SERVER=file://<mock dist>` (or `no-such-distdir`), and `RUSTUP_AUTO_INSTALL` empty; `setup_test_state` removes `RUSTUP_TOOLCHAIN`, `RUSTUP_UPDATE_ROOT`, `XDG_CONFIG_HOME` | `src/test/clitools.rs` (rust-lang/rustup, master) |
| npm CLI's suite | `mock-npm` makes `home`, `cache`, `global` and `prefix` folders in `t.testdir`, clears every `npm_*` variable, points `HOME` there; `MockRegistry` uses nock and calls `Nock.disableNetConnect()`, so an unmocked request fails | `test/fixtures/mock-npm.js`, `mock-registry/lib/index.js` (npm/cli) |
| Bazel | `HOME` = `$TEST_TMPDIR`; "Tests should create files only within" `$TEST_TMPDIR` and the undeclared-outputs folder; the home "may not be writable. Tests must not attempt to write to it". `linux-sandbox` makes "the entire filesystem read-only except for the sandbox directory" and can block the network | bazel.build/reference/test-encyclopedia; bazel.build/docs/sandboxing |
| cargo-nextest | Sets `NEXTEST=1` and `NEXTEST_RUN_ID` in every test; honours cargo's `[env]`; setup scripts can export variables through `$NEXTEST_ENV` but are "Experimental: This feature is not yet stable" (ADR 0071 keeps them out) | nexte.st/docs/configuration/env-vars/, .../setup-scripts/ |
| Vitest | `test.env`: "Environment variables available on `process.env` and `import.meta.env` during tests"; sets `VITEST` | vitest.dev/config/env |
| Node.js | The permission model (`--permission`, `--allow-fs-write`, `--allow-child-process`, `--allow-net`; stable since 22.13 / 23.5) is a "seat belt" that "prevents trusted code from unintentionally changing files or using resources" | nodejs.org/api/permissions.html |

**The common pattern** is a sandbox built by the test harness around the command it starts: private home and tool homes, the
caller's tool variables removed, the network off by default with a named opt-in, and nothing in the product that knows it runs
under a test. Only the platform tools (Bazel's sandbox, Node's permission model) refuse at the OS or runtime level. None of the
surveyed projects puts a test-aware guard in the product. They do not need one: their test harness starts every program, and
their tools have one home folder each. Ours differ in two ways that the incidents show: our tools take absolute paths from the
profile (`SCCACHE_DIR`), and a shared resource is a port (4226), not a folder, so a private `HOME` does not move it.

## Decision

### 1. One marker, set by the harness, never stripped

A process runs **under a test run** when `PADDOCK_TEST_RUN` is set (any non-empty value, the run or job id) or `NEXTEST=1`
(nextest sets it in every test process, so `cargo nextest run`, the documented way to run tests locally and the only way in CI,
needs no setup). Who sets `PADDOCK_TEST_RUN`:

| Place | Through |
| --- | --- |
| Rust tests under nextest | `NEXTEST=1` from nextest; nothing to add, no setup script (ADR 0071) |
| Children a Rust test starts | `paddock_testkit::TestCommand` puts `PADDOCK_TEST_RUN` in its listed set (ADR 0100; once it lands); `EnvLock::hermetic` sets it in-process; `Reaper::spawn` passes it |
| Vitest | the shared preset's `test.env` (`ui/vitest.tokens.config.ts`, `apps/debrief/app/vitest-base.config.mts`, and every new config through the preset) |
| `node --test` (scripts, conformance) | a preload in the test command (`node --import ./scripts/test-marker.mjs --test ...`); WP-3 confirms that `node --test` forwards it to each file's process, else every test file imports `scripts/test-kit.mjs`, which sets it |
| Playwright | `@decryp7/paddock-e2e` `paddockConfig` (web server and launched apps' environment) |

**ER-47.** Children inherit the marker. `paddock_proc::KEPT_ENV` gains `PADDOCK_TEST_RUN`, `NEXTEST` and `PADDOCK_TEST_ALLOW`, so
`ToolCall`, `TestCommand` and every listed environment pass them. A tool never clears them; a check (WP-5) fails on code that
removes them. This is the rule's "never strip variables that restrict the child".

**Seat belt for programs we do not own.** Under the marker, `paddock_proc::command` also sets the restriction variables that
third-party programs understand, unless the caller set them: `RUSTUP_AUTO_INSTALL=0` (instance 1's class, as rustup's own suite
does), `npm_config_offline=true`, `PADDOCK_DEV_DOWNLOAD=0`. `CARGO_NET_OFFLINE` is left out: tests that build fixture crates
resolve from the registry cache and a cold cache would fail (D3).

### 2. One guard API, in `paddock-env`

`paddock-env` is already a dependency of `paddock-dev` and `heavy`, holds the home guard and is where ADR 0077 placed the reach
decision (its D6). A new module `paddock_env::effects`:

```text
pub enum Effect<'a> {
    Download { url: &'a str },                         // a real host; loopback is never an effect
    Install { what: &'a str, dest: &'a Path },         // a program or package placed for later use
    Server { name: &'a str, op: ServerOp, port: Option<u16>, state: Option<&'a Path> },   // start or stop a long-lived process
    Write { path: &'a Path },                          // create, change or remove outside the test's folders
    Toolchain { op: &'a str },                         // rustup install, update, uninstall, default, self
    Network { host: &'a str },                         // any other connection to a real host
}
pub struct Effects { marker: Option<String>, own: Vec<PathBuf>, allows: Vec<Allow> }  // read once at the edge
impl Effects {
    pub fn from_env(vars: &dyn Fn(&str) -> Option<String>) -> Effects;
    pub fn check(&self, e: &Effect) -> Result<(), Refused>;     // pure
}
pub struct Refused { pub effect: String, pub why: Why, pub next: &'static str }
```

- **Without the marker** `check` always passes: a person's run is unchanged and costs one environment read.
- **Under the marker** `check` refuses every effect unless (a) its target is the test's own (a path under the process's temp
  folder, or under a `HOME` that is not the account's home; ADR 0077's `account_home()` reads the account database, never `HOME`,
  and is part of WP-1 here), or (b) an allow matches (§3). Loopback connections are never effects.
- **Protected targets are refused even with an allow:** the account's real home and its tool homes (`~/.rustup`, `~/.cargo`,
  `~/.paddock-dev`), sccache's default port 4226 and any `SCCACHE_DIR` outside the test's folders, the machine-wide `heavy` slot
  root. Toolchain changes are always refused (ER-14: tests use stand-ins).
- **A refusal is a typed value, logged once per effect kind** (`effects: refused Server sccache start (test run <id>); next: allow
  it with TestCommand::allow or use a stand-in`), ER-27. A caller maps it to its existing "off" path where one exists (the sccache
  wrapper is off, no warning: a choice, as `local_sccache::declined` already treats `PADDOCK_SCCACHE=off`), or to an error naming
  the refusal.

**Where it is called.** At the two chokepoints, so a new path in the same category is covered without anyone remembering:

1. `ci_tool::ToolCall::run` classifies the call before it starts it: `curl`/`wget` with a non-loopback URL (Download),
   `sccache --start-server|--stop-server` (Server), `rustup toolchain|update|self|default` (Toolchain), `npm ci|install` without
   `--offline` and `cargo install|update|fetch` (Install), `docker|podman run|start|stop|rm` (Server). A classified call is checked;
   anything else runs as today. The classifier is a pure table with a unit test per row.
2. `paddock_dev::wait::CurlHttp` (the one real HTTP client) checks `Network`.

And at the named functions that do not start a program: `tool_cache::home` (Write of the default `~/.paddock-dev`),
`local_sccache::prepare` (Server, before it touches anything; the eager call in `Real::new` then costs nothing under a test),
`heavy`'s slot root (Write), `ci_host::cleanup::remove_all` and `safe_remove` (Write). The `dev` launcher (shell, and the Rust
launcher on `feat/dev-launch-rust`) checks the marker before its `rustup toolchain install`, its prebuilt download and a build into
the default `~/.paddock-dev`: the same rule in a few lines, listed as a known copy (rule 55) with the Rust launcher as its
replacement.

### 3. The allow: explicit, scoped to one command or one value

- **In-process:** a test builds the value it passes: `Effects::for_test().allow(Allow::Server { name: "sccache", port: 4793,
  state: scratch.join("cache") })`. Functions that act take `&Effects` (parse at the edge), so the allow reaches only the call it
  was given to. No global switch, no `set_var`.
- **Across a process boundary:** `PADDOCK_TEST_ALLOW` on **one** child, set only through `TestCommand::allow(Allow)` (the builder
  serialises it: `server:sccache@4793:<state>`, `write:<abs path>`, `download:<url prefix>`, `install:<abs dest>`). The child's
  guard parses it and matches each effect exactly (a path by prefix after `home_guard::abs_path`). A malformed entry is refused,
  not widened (as C129's exceptions are exact paths only, 91db3611f).
- **Protected targets stay refused** (§2). An allow names a test's own port or folder; it cannot name the user's.
- **Tests that need the real thing** (a release-download smoke) set no allow: they run outside the marker in a job that declares
  it, the way cargo gates `CARGO_PUBLIC_NETWORK_TESTS`.

### 4. The checks

1. **Static, report-only first (ER-41), ratchet that only shrinks (ER-46):** a `./dev check` pass (syn, number on acceptance) over
   `tools/` and the shared crates the tools use: a classified program literal (`curl`, `wget`, `sccache`, `rustup`, `npm`,
   `docker`, `podman`, `cargo` with `install|update|fetch`) may appear only in a `ToolCall` or in a function that calls
   `Effects::check`; `std::fs` writes and `remove_dir_all` under a path built from `tool_cache::home` or an env home variable only
   in a function that calls it; no code removes the marker variables. Today's unguarded sites start in the baseline with the WP
   that removes them. **Positive controls:** an in-memory fixture with an unguarded `ToolCall::new("curl")`, an unguarded
   `sccache --start-server` and an `env_remove("PADDOCK_TEST_RUN")` must each be found; the check prints how many files and sites
   it scanned and reports `check broken` on zero (ER-33). A Node twin for `scripts/` is the same table on the conformance Rust and shell
   lexers (`conformance/src/util/mask-rust.mjs`, `shell-lexer.mjs`).
2. **Run monitor, report-only, Linux CI only:** the `tools` job (a container that is the only user of its home and network
   namespace) snapshots, before and after its nextest run, the listening TCP sockets and the declared real-home locations
   (`$HOME/.paddock-dev`, `$HOME/.rustup/toolchains`, `$HOME/.cargo/bin`, `$SCCACHE_DIR`, the npm cache) by name and mtime, and
   fails on a difference with one `::error::` naming it. Not on the shared Mac: other agents change the same places, and macOS shows
   no other process's environment (ADR 0103's measurement), so a run cannot tell its own daemon from another agent's.
   **Positive control:** the monitor's own test starts a loopback listener and writes a file in a declared location, and must
   report both.

### 5. How it fits the rules

- **ER-14:** the guard is the mechanism for "tests never touch the user's machine" in our tools; the per-test switches go
  (`PADDOCK_SCCACHE=off` in three test files, `PADDOCK_HEAVY=off`, `PADDOCK_SECRETS_FORBID_REAL` once ADR 0077's WP-3 folds into
  this marker).
- **ER-45:** a probe or check started under the marker cannot install or write; a check's positive control on in-memory data
  keeps its `Ctx::root()` panic (91db3611f) as the input side. A person's manual probe (instance 1) is outside any test run; the
  launcher's own `RUSTUP_AUTO_INSTALL=0` (95ac496a9) stays for that.
- **ER-47:** children inherit by default; the marker, the allow and the seat-belt variables are in `KEPT_ENV`, so no listed
  environment can strip them.

### 6. Rule text (proposed; number on acceptance)

"Under a test run (`PADDOCK_TEST_RUN` or nextest's `NEXTEST`), our tools refuse installs and downloads, starting or stopping
long-lived servers, writes outside the test's own folders, toolchain changes and connections to real hosts, through
`paddock_env::effects`, unless the test allows one effect for one call or command; the user's home, tool homes and shared
ports are refused even then. Checked by the effects check and, in the Linux tools job, the run monitor." ER-45 gains a pointer.

## Alternatives rejected

- **Keep adding per-test switches** (today): three instances in two days, each fixed in the tests that were run.
- **A deny-list in the harness only** (option A below, if chosen alone): it fixes the incidents seen and needs a new list entry for
  each new effect, the same unknown-next-thing problem; kept as the cheaper option the user may choose.
- **nextest setup scripts or cargo `[env]` for the marker:** setup scripts are experimental (ADR 0071); cargo `[env]` also applies
  to `cargo run`, so a person running the tool from source would be refused.
- **OS sandboxing** (Bazel's `linux-sandbox`, `sandbox-exec` on macOS, Node's permission model): Linux-only or deprecated on
  macOS for the Rust side; Node's model would need every `node` test start changed and does not cover the Rust tools. Recorded as a
  later option for the Linux CI legs.
- **A fake chosen by the marker** inside the tools: the guard refuses, it never substitutes (ADR 0073's reasoning).

## Parallelism and cost (ER-20)

`check` is a pure match on a short list: no I/O, nothing to parallelise. The marker is read once per process. The run monitor adds
two snapshots per Linux tools job (a socket listing and a few `stat` calls; to be measured in WP-6 and recorded in
`docs/benchmarks.md`, rule 48).

## Work packages (raw agent-days, one agent)

| WP | Content | Days |
| --- | --- | --- |
| WP-1 | `paddock_env::effects` (marker, `Effect`, `Effects`, `Refused`, `Allow` parse and match, protected targets, `account_home` on three OS), its log line; unit tests on injected variables | 1 |
| WP-2 | `KEPT_ENV` gains the three names; seat-belt variables in `paddock_proc::command` under the marker; tests that a `ToolCall` child keeps them and that a seat-belt variable the caller set wins | 0.5 |
| WP-3 | Marker sources: `TestCommand` (needs ADR 0100 landed), `EnvLock::hermetic`, `Reaper::spawn`, the Vitest preset, the `node --test` preload, `paddockConfig` | 0.5 |
| WP-4 | Adoption in `paddock-dev`, `heavy` and the `dev` launcher: the `ToolCall::run` classifier, `CurlHttp`, `tool_cache::home`, `local_sccache::prepare` (with `feat/local-sccache`), heavy's slot root, the removals; failing test first for each of instance 3 and the `heavy` slots; remove the per-test switches | 1.5 |
| WP-5 | The static check, its positive controls and the shrink-only baseline; the Node twin for `scripts/` | 1 |
| WP-6 | The run monitor in the Linux tools job, its positive control, report-only, measurement recorded | 1 |
| WP-7 | Node guard `scripts/lib/effects.mjs` with the same marker and allow format, adopted in `publish.mjs`, `demo-release.mjs`, `vm-smoke` | 0.75 |
| WP-8 | Rule text, ER-45 pointer, lessons entry, ADR index, `crates/README.md`, CHANGELOGs | 0.25 |
| | **Total** | **6.5** |

## Cost against common practice (ER-37)

| Option | What | Raw agent-days | Beyond common practice |
| --- | --- | --- | --- |
| **A. Common practice** (cargo, rustup, npm) | The harness sandbox only: `TestCommand` (ADR 0100) gets a fixed "off" list (`PADDOCK_SCCACHE=off`, `PADDOCK_HEAVY=off`, `PADDOCK_DEV_DOWNLOAD=0`, `RUSTUP_AUTO_INSTALL=0`, `npm_config_offline=true`) and removes `RUSTC_WRAPPER`, `SCCACHE_*` and the profile's tool paths, as cargo's `test_env` does; every tool test helper (`dev()` and friends) goes through it; the per-test switches go. No product-side code. | 1 (after ADR 0100 lands) | 0 |
| **B. Marker and guard at the chokepoints** | A, plus WP-1, WP-2, WP-3, WP-4: the tools refuse by default under the marker; in-process tests and tests that bypass `TestCommand` are covered; a new download, server or install through `ToolCall` is refused without a new list entry | 3.5 | 2.5 |
| **C. Full** | B plus the static check (WP-5), the run monitor (WP-6), the Node guard (WP-7), docs (WP-8) | 6.5 | 5.5 |

What A misses, concretely: a library test that calls a constructor like `Real::new` in-process (instance 3's eager start runs in
the test process itself), a test that starts a tool with a plain `paddock_proc::command` instead of `TestCommand`, and the next
effect nobody has listed yet. What B adds over A is exactly those three. C adds detection of what B's guard misses (a shell
script, a new program outside the classifier) and the Node tools.

## Decisions for the user

| # | Question | Options | Recommendation |
| --- | --- | --- | --- |
| D1 | Scope | (a) A, common practice, 1 day. (b) B, 3.5 days (2.5 beyond A). (c) C, 6.5 days. (d) B now, WP-5 and WP-6 report-only after the next app tag. | (d): the class came back three times in two days and A alone leaves in-process tests and the next unknown effect open; the checks follow when there is room. A is the right choice if the user wants only common practice. |
| D2 | Marker | (a) `PADDOCK_TEST_RUN` or `NEXTEST=1`. (b) `PADDOCK_TEST_RUN` only (every nextest config would need a setup script, experimental). | (a). |
| D3 | `CARGO_NET_OFFLINE` in the seat belt | (a) Not set (fixture builds may need the registry). (b) Set, and pre-fetch fixtures in CI. | (a); revisit with a measurement of which tests fetch. |
| D4 | Repository reads (instance 2) | (a) Not an effect; keep `Ctx::root()`'s panic (91db3611f) for checks' controls. (b) `git` with a working folder in the real checkout is an effect under the marker, allowed for the live tests (`check::live_`, `golden::live_`, `contracts::`, ADR 0095's `RepoCopy`) by name. | (a): reads change nothing; (b) adds an allow to every live test for one instance that already has a structural fix. |
| D5 | Fold ADR 0077's `paddock-secrets` switches into this marker | (a) Yes, with WP-4 (removes `PADDOCK_SECRETS_FORBID_REAL` and `PADDOCK_SECRETS_REAL` and their users). (b) Later, with ADR 0077's next work. | (a): one marker for the family. |
| D6 | Release gating | (a) None: tools ship in no installer. (b) Gate the next train on WP-4. | (a); land through the normal train. |

## Consequences

- A test that forgets a switch no longer reaches the machine: it gets a logged refusal, and a tool with an "off" path (sccache,
  the prebuilt download) runs without the effect.
- A person's runs are unchanged: without the marker the guard passes everything.
- Each new effect in the classifier's categories is covered on the day it is written; a new category needs one row in the
  classifier and the static check's table (both tested).
- Tools take `&Effects` where they act: a small signature change in `paddock-dev` and `heavy`, landing with its adoption (rule 43).

## Rollback

Each WP is its own commit. Removing the classifier rows or the named calls returns to today's behaviour; the marker and the
`KEPT_ENV` names are harmless without readers.
