# 100. Tests start programs through one process builder with a listed environment

- Status: Accepted 2026-11-13 (user: the redesign was approved for building, ER-21); built on `feat/test-env-builder`.
  Nothing else is open except the follow-up in D3.
- Date: 2026-11-13
- Related: ER-14 (tests never touch the user's machine), ER-21 / rule 40 (a recurring class gets a redesign), ER-22 / rule 42
  (share first), ER-24 (heavy slots), ER-37 / rule 76 (common practice), ER-41 (new checks start report-only); ADR 0077
  (tests never reach system services), ADR 0095 (testkit helpers), ADR 0099 (the same shape of mechanism for paths).
- Number: 0100. On 2026-11-13 the highest ADR on `origin/main` and on every `origin` branch was 0099 (`git ls-tree` of
  `docs/adr/` over all remote branches; 0098 and 0099 are on `origin/feat/adr-0098-member-gates` and `origin/docs/adr-path-asserts`).
- Check id: C123 (assigned centrally).

## Context: the class

A test that starts `./dev`, `cargo` or `node` with a plain `Command` hands the child the whole environment of the machine
it runs on. Every new environment lookup in the program under test then reaches real machine state, and nothing in the
test says so. Two instances in one day:

1. The pre-push integration tests (`tools/paddock-dev/tests/it/pre_push.rs`) ran the real pre-push heavy steps: the
   child inherited `PATH`, so `heavy.sh` and `heavy` were found and the fake `cargo` was only one of the programs run.
2. On `fix/precommit-heavy-slot` the new lookup of `heavy.sh` (`PADDOCK_HEAVY`, `PATH`, `$ENGINEERING_RULES`, then the
   `engineering-rules` checkout beside the main checkout) made the test helper `dev()` find the real
   `engineering-rules/scripts/heavy.sh` and take machine-wide slots in `~/.cache/heavy-slots` (ER-14). It was patched per
   test (`2ff5f5072`: a fixture `ENGINEERING_RULES`, a `HEAVY_LOCK_DIR` assertion). The next lookup added to any tool
   fails the same way in every test that was not patched, and nothing finds those tests.

The mechanism is the same both times: a test sets what it knows about, and everything else comes from the machine. A
deny-list (unset the variables we know) can never be complete, because the variable that causes the next incident is by
definition not known yet. An allow-list can: the child gets nothing it was not given.

## Decision

**One builder, `paddock_testkit::TestCommand`** (`crates/paddock-testkit/src/process.rs`). It is a `std::process::Command`
(it derefs to one: `args`, `current_dir`, `output`, `spawn` are the usual ones) that starts from an **empty environment**
(`env_clear`) plus a listed set:

- `HOME`, `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME`, `XDG_STATE_HOME`,
  `TMPDIR`, `TEMP`, `TMP` and `GIT_CONFIG_GLOBAL` point into a **private folder** made for the command and removed when it
  drops (`TestCommand::new_in` takes a folder the test owns). A slot tool that keeps its slots under `$HOME/.cache` then
  keeps them in the private folder, never in `~/.cache/heavy-slots`.
- `PATH` is **curated**: the folder of the cargo that runs the test (`CARGO`, so also `rustc`), the folders of `node` and
  `git` as the test process finds them, then the system folders (`/usr/bin`, `/bin`, `/usr/sbin`, `/sbin`, or `System32` and the Windows folder).
  `path_first(dir)` puts a test's stand-in folder first; `.env("PATH", ..)` replaces it whole. A bare program name is
  resolved once, at construction, to a full path.
- the toolchain: `RUSTUP_HOME`, `CARGO_HOME`, `RUSTUP_TOOLCHAIN` and `CARGO_BUILD_JOBS` (a bound, ER-24) as the test process has them, and, when unset, the real
  `~/.cargo` and `~/.rustup` named explicitly (a redirected `HOME` would otherwise hide the offline registry and the
  toolchains); on Windows `SystemRoot`, `ComSpec`, `PATHEXT` and a few more. These are `PASSED_THROUGH` and the only
  places a child reads the user's folders.
- fixed values: a git author and committer, `GIT_CONFIG_NOSYSTEM=1`, `NO_COLOR=1`.

`allowed_names()` lists every name a fresh command carries. Everything else a test needs is a **named addition**
(`.env("NAME", value)`), visible in the test. `into_parts()` hands the plain `Command` and a guard to a runner that takes a
`Command` by value (`paddock_proc::run_bounded`).

**One check, C123** (Rust host `tools/paddock-dev/src/check/c123.rs`, JavaScript port
`conformance/src/checks/C123-test-process-builder.mjs`, held equal by `tools/paddock-dev/tests/it/check_c123.rs`): in a Rust
test file, a constructor (`Command::new`, `paddock_proc::command`, `quiet_command`) whose first argument is the paddock-dev
binary, cargo, `node` or `dev` is a finding, and in `tools/paddock-dev` so is `sh` or `bash` (the wrapper script `./dev` and
the CI scripts run under a shell); so is any other use of `env!("CARGO_BIN_EXE_paddock-dev")` or of cargo's path in a function
that does not name `TestCommand`. A justified case carries `conformance: test-env — <why>`. It is report-only (ER-41: a
warning) until its first green run on main, recorded in `docs/gates.toml`.

**Every existing spawn site is migrated in the same change** (no baseline). The first run of the check listed 74 findings;
the migration (92 `TestCommand::new` sites in 36 files across 5 workspaces) is in the commit. Two checks keep the rule from decaying: the module's own test starts a real child
under a machine environment full of poisoned variables and compares its environment with `allowed_names()`; and the
pre-push test starts `./dev check --pre-push` with a machine that has a `heavy.sh` of its own (`ENGINEERING_RULES`,
`HEAVY_LOCK_DIR`) and fails when that script runs, a slot folder is made, or the fake cargo sees a name nobody listed.

One more product change falls out: a **named `ENGINEERING_RULES` is the only place** `sibling_heavy` looks. A folder
without `heavy.sh` means "none", not "try the checkout beside this repository", so a test can name an empty folder and the
machine's own checkout is never found (a test that did not, reached it through the repository layout, not the environment).

## What exists (ER-22: look before adding)

- `paddock_proc::clean_env(cmd, KEPT_ENV)` (`crates/paddock-proc/src/command.rs`): the production allow-list for the apps'
  children (`PATH`, `HOME`, `TMPDIR`, `LANG`, ...). It keeps the machine's values; a test needs private ones and a curated
  `PATH`, and `paddock-proc`'s own tests use the testkit, so the kit cannot depend on it (the testkit's existing rule).
  The two lists are short and the testkit one is the superset the tests need; if `KEPT_ENV` grows a name, the testkit
  tests do not notice, and that is accepted (production children and test children answer different questions).
- `EnvLock::hermetic` (`env.rs`): the same private home and temp folders for the **test process's own** environment. The
  builder is the child-process counterpart; both use the same folder names.
- `Scratch` and `canonical_tempdir`: where a test's files go. `TestCommand` makes its private folder with `tempfile` and does
  not need a `Scratch` (a leftover check on `tmp` would fail children that write there by design).

## ER-37: how comparable software does it

- **Cargo's own test support** (`crates/cargo-test-support/src/lib.rs`, read 2026-11-13): `TestEnvCommandExt::test_env`
  redirects `HOME`, `CARGO_HOME` and `USERPROFILE` into a private test root, sets `CARGO_INCREMENTAL=0` and
  `GIT_CONFIG_NOSYSTEM=1`, and **removes** every variable that starts with `CARGO_` and a list of named ones (`CI`,
  `RUSTFLAGS`, `RUSTC_WRAPPER`, `GIT_AUTHOR_*`, proxies, `SSH_AUTH_SOCK`, ...). It does not call `env_clear`: it is a
  deny-list. The brief for this ADR said cargo uses an empty environment; it does not, and this design is stricter on
  purpose, because a deny-list is the shape that failed here twice (the variable that bites next is not on it).
- **Bazel** (https://bazel.build/reference/test-encyclopedia, read 2026-11-13): a test runs with a defined initial
  environment (`TEST_TMPDIR`, a fixed `TZ`, locale variables unset) and "should not depend on the presence, absence, or
  value of any environment variable not listed"; `--test_env` forwards named ones. That is this design: a documented list,
  named additions, a private temp folder.
- **Nix** builds and **git's** own `t/test-lib.sh` (which unsets or fixes the variables git reads and points `HOME` at
  the trash directory) take the same position: the test owns `HOME`, `TMPDIR` and the git config.
- The OS vendors give no test-environment standard; for `HOME`/`XDG_*` the XDG Base Directory specification is the contract
  the redirected variables follow.

Common practice is therefore "private home, listed environment". The deny-list of Cargo is the cheaper, weaker option and
is offered as a decision below.

## Cost (ER-37)

About 1 agent-day, measured on this change: the builder and its tests (about 0.25), the check, its port and the parity
test (about 0.25), migrating 92 spawn sites in 36 files across 5 workspaces and fixing what the cleaner environment
exposed (about 0.4), this ADR and the gates (about 0.1). Running cost: each command makes two folders (a few hundred
microseconds); nothing else. The cheaper common-practice option (Cargo's deny-list: redirect `HOME`/`TMPDIR`, remove a
named list) is about 0.4 agent-day but does not stop the next lookup; it is not recommended.

## Decisions

- **D1 (decided): allow-list, not deny-list.** Empty environment plus `allowed_names()`.
- **D2 (decided): migrate every site now, no baseline** (the user: the foundation is built complete and right). The check
  lists no baseline file; `docs/gates.toml` carries the report-only entry (ER-41).
- **D3 (open, follow-up): JavaScript tests.** About 50 test files in `conformance/test`, `scripts/*.test.mjs` and the apps'
  `scripts/ci` start `node`, `cargo` or a script with `spawnSync` and inherit the environment the same way. They are not
  judged by C123 and have no builder yet; a JavaScript `testProcess()` in `conformance/test/helpers.mjs` and a C123 arm that
  reads `.mjs` would be about 0.5 agent-day. Not done here: the incidents were Rust tests, and a second builder needs the
  parity rule applied once more.
- **D4 (decided): git is not in C123.** `git` reads `HOME` and the git config, which `TestCommand` redirects when it is
  used; plain `Command::new("git")` fixtures (about 60) keep working. If a git-config incident appears, add it to the check.

## Consequences

- A new environment lookup in a tool shows up as a failing test that names the variable, instead of a quiet read of the
  machine. Tests that need a variable say so.
- `TestCommand` curates `PATH`: a test that needs a program outside the toolchain folders, `node`, `git` and the system
  folders adds its folder with `path_first` or sets `PATH` itself.
- `real` `~/.cargo` and `~/.rustup` stay readable (the offline registry, the toolchains). That is the one deliberate hole.
