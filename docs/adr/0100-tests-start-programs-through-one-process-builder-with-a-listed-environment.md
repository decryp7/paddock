# 100. Tests start programs through one process builder with a listed environment

- Status: Accepted 2026-11-13 (user: the redesign was approved for building, ER-21); built on `feat/test-env-builder`, which
  merged train 11 at its head `49e1a97cc` (2026-11-15) and ADR 0103's `feat/test-timing-redesign` at `031a11c3d`. Nothing else is
  open except the follow-up in D3.
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

1. The pre-push integration tests (`tools/paddock-dev/tests/it/pre_push.rs`) ran the real pre-push heavy steps (found when
   `c25f3860a`, 2026-11-13, made the gate queue for the heavy slot and the tests then had to opt out with `PADDOCK_HEAVY=off`;
   a local discovery, no CI run): the
   child inherited `PATH`, so `heavy.sh` and `heavy` were found and the fake `cargo` was only one of the programs run.
2. On `fix/precommit-heavy-slot` the new lookup of `heavy.sh` (`PADDOCK_HEAVY`, `PATH`, `$ENGINEERING_RULES`, then the
   `engineering-rules` checkout beside the main checkout) made the test helper `dev()` find the real
   `engineering-rules/scripts/heavy.sh` and take machine-wide slots in `~/.cache/heavy-slots` (ER-14). It was patched per
   test (`2ff5f5072`, 2026-11-13, found locally while reworking the pre-commit hook of `78a536f94`; no CI run: a fixture `ENGINEERING_RULES`, a `HEAVY_LOCK_DIR` assertion). The next lookup added to any tool
   fails the same way in every test that was not patched, and nothing finds those tests.

The mechanism is the same both times: a test sets what it knows about, and everything else comes from the machine. A
deny-list (unset the variables we know) can never be complete, because the variable that causes the next incident is by
definition not known yet. An allow-list can: the child gets nothing it was not given.

## Decision

**One builder, `paddock_testkit::TestCommand`** (`crates/paddock-testkit/src/process.rs`). It wraps a `std::process::Command`
and exposes only the safe methods (`arg`, `args`, `current_dir`, `env`, `envs`, `env_remove`, `stdin`, `stdout`, `stderr`,
`output`, `status`, `spawn`); it derefs read-only (`get_program`, `get_envs`) and has **no `DerefMut`**, so no method can reach the
wrapped `Command` (review of f39597fbb, P1-1: `Command::env_clear` through the deref erased the listed set at about 17 sites).
Its own `env_clear` goes **back to the listed set**. It starts from an **empty environment** plus a listed set:

- `HOME`, `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME`, `XDG_STATE_HOME`,
  `TMPDIR`, `TEMP` and `TMP` point into a **private folder** made for the command and removed when it
  drops (`TestCommand::new_in` takes a folder the test owns). A slot tool that keeps its slots under `$HOME/.cache` then
  keeps them in the private folder, never in `~/.cache/heavy-slots`.
- `PATH` is **curated**: the folder of the cargo that runs the test (`CARGO`, so also `rustc`), the folders of `node` and
  `git` as the test process finds them, then (Windows) Git for Windows' `usr/bin` and `bin`, then the system folders (`/usr/bin`, `/bin`, `/usr/sbin`, `/sbin`, or `System32` and the Windows folder). The folder of `node` is added whole, so on a Homebrew Mac it exposes `/opt/homebrew/bin` (other Homebrew tools, not the slot tool); a narrower PATH would need a private folder of links to `node` and `git`, which Windows cannot make cheaply, so it is accepted and named here.
  `path_first(dir)` puts a test's stand-in folder first; `.env("PATH", ..)` replaces it whole. A bare program name is
  resolved once, at construction, against the **test process's** `PATH`: `path_first` and `env("PATH", ..)` change only the
  child's own lookups, never which program starts (a test that starts a stand-in names it by its path).
- the toolchain: `RUSTUP_HOME`, `CARGO_HOME`, `RUSTUP_TOOLCHAIN` and `CARGO_BUILD_JOBS` (a bound, ER-24) as the test process has them, and, when unset, the real
  `~/.cargo` and `~/.rustup` named explicitly (a redirected `HOME` would otherwise hide the offline registry and the
  toolchains); on Windows `SystemRoot`, `ComSpec`, `PATHEXT` and a few more. These are `PASSED_THROUGH` and the only
  places a child reads the user's folders.
- a private global git config, `.gitconfig` in the private home, found through `HOME` (no `GIT_CONFIG_GLOBAL`: with it set, a test
  that names another home lost that home's `.gitconfig`, the P1 of fix/linux-container-gate 88c06e902): `maintenance.auto=false`,
  `gc.auto=0`, a test identity and `safe.directory=*` (`GIT_CONFIG_NOSYSTEM` hides the system config, where the CI image and the
  local Linux container trust a checkout another user owns), so no test's git starts background maintenance (a detached `git maintenance` held `objects/maintenance.lock` and a bare clone made
  meanwhile copied it: the kept-clone fixture hit that 104 times in 1200 runs under load). It is written only when the home has
  none, through a temporary file linked into place (`persist_noclobber`), so a `new_in` home the test prepared keeps its own and
  children sharing a home never see half a file. Because every folder is trusted, `GIT_CEILING_DIRECTORIES` names the temp root:
  git never looks for a repository above it (CVE-2022-24765, a `.git` planted in a shared `/tmp`), while a repository a test makes
  below it is still found;
- fixed values: a git author and committer, `GIT_CONFIG_NOSYSTEM=1`, `NO_COLOR=1`.

`allowed_names()` lists every name a fresh command carries. Everything else a test needs is a **named addition**
(`.env("NAME", value)`), visible in the test. `into_parts()` hands the plain `Command` and a guard to a runner that takes a
`Command` by value (`paddock_proc::run_bounded`).

**One check, C123** (Rust host `tools/paddock-dev/src/check/c123.rs`, JavaScript port
`conformance/src/checks/C123-test-process-builder.mjs`, held equal by `tools/paddock-dev/tests/it/check_c123.rs`): in a Rust
test file, a constructor (`Command::new`, `paddock_proc::command`, `quiet_command`) whose first argument is a built binary
(`CARGO_BIN_EXE_*`, or a name bound to one: a `const`, `static`, `let` or a function returning one, in the file or `pub` in another
test file of the same crate), cargo, rustc, rustup, `node`, `npm`, `npx`, `dev` or a path joined to `dev` (also spelled with `.exe`
or `.cmd`) is a finding, and in `tools/paddock-dev` so is `sh` or `bash` (the wrapper script `./dev` and the CI scripts run under a
shell); so is any other use of `env!("CARGO_BIN_EXE_paddock-dev")` or of cargo's path in a function whose code (comments left out)
does not name `TestCommand`. In a file under `src/` the test part starts at the first `#[cfg(..)]` that names `test` (not
`not(test)`). A justified case carries `conformance: test-env — <why>`. It is report-only (ER-41: a warning) until its first
green run on main, recorded in `docs/gates.toml`.

Known false negatives (accepted; a text check cannot see them without a Rust parser): a program held in a variable
(`let p = "cargo"; Command::new(p)`), an alias (`use std::process::Command as Cmd`), a block comment that names `TestCommand` (only
line comments are left out), and a spawn in a non-test helper module that tests call.

**Every existing spawn site is migrated in the same change** (no baseline). The rule of 2026-11-13, run on the tree at `adf56b5e7`
(`git archive`, `node conformance/bin/paddock-conformance.mjs --app paddock --root <archive> --only C123`), listed 118 findings in 49
files (the first, narrower version of the rule listed 74 on the same tree; a reviewer's count of 101 in 36 files used another reading
of "spawn"); 110 of them were spawn sites migrated in 50 files across 8 workspaces (the other 8 were uses of the binary in functions
that already built a `TestCommand` once the rule read whole functions), and the rule ends at 0 there. Merging train 11 (`49e1a97cc`)
brought 20 new sites in 9 files of `tools/paddock-dev/tests/it`, and the widened rule of the review of `a8fca0ebd` 8 more (6 in
`crates/paddock-ipc` through its shared `child_program()`, `rustc` in `generators.rs` and `inputs.rs`); all are migrated, and the
check counts 0 findings and 0 waivers on the branch. The migration is in the commits. Two checks keep the rule from decaying: the module's own test starts a real child
under a machine environment full of poisoned variables and compares its environment with `allowed_names()`; and the
pre-push test starts `./dev check --pre-push` with a machine that has a `heavy.sh` of its own (`ENGINEERING_RULES`,
`HEAVY_LOCK_DIR`) and fails when that script runs, a slot folder is made, or the fake cargo sees a name nobody listed.

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
- **Nix** builds start from an empty environment plus what the derivation names, with a private `HOME` (`/homeless-shelter`)
  and `TMPDIR`: the allow-list shape.
- **git's** own `t/test-lib.sh` points `HOME` at the trash directory and fixes a few values (`TZ`, the author and committer),
  but it keeps the environment it was started with and **unsets a list** of the `GIT_*` and locale variables it knows: a
  deny-list, like Cargo's. Both own `HOME` and the git config.
- The OS vendors give no test-environment standard; for `HOME`/`XDG_*` the XDG Base Directory specification is the contract
  the redirected variables follow.

Common practice is therefore split: Bazel and Nix list the environment, Cargo and git keep it and remove what they know;
all of them give the test a private home. The deny-list of Cargo and git is the cheaper, weaker option and is offered as the
common-practice decision below (D1).

## Cost (ER-37)

About 1.5 agent-days, measured on this change (1 for the first build, 0.5 for the review round: the safe-method wrapper, `PATH` and `CARGO_BIN_EXE_*` coverage): the builder and its tests (about 0.25), the check, its port and the parity
test (about 0.25), migrating 110 spawn sites in 50 files across 8 workspaces and fixing what the cleaner environment
exposed (about 0.4), this ADR and the gates (about 0.1). Running cost: each command makes two folders (a few hundred
microseconds); nothing else. The cheaper common-practice option (Cargo's deny-list: redirect `HOME`/`TMPDIR`, remove a
named list) is about 0.4 agent-day but does not stop the next lookup; it is not recommended.

## Decisions

- **D1 (decided): allow-list, not deny-list.** Empty environment plus `allowed_names()`. The common-practice option is
  Cargo's and git's deny-list (keep the environment, redirect `HOME`/`TMPDIR`, remove a named list): about 0.4 agent-day, but
  it does not stop the next lookup, which is the class this ADR exists for; not chosen.
- **D2 (decided): migrate every site now, no baseline** (the user: the foundation is built complete and right). The check
  lists no baseline file; `docs/gates.toml` carries the report-only entry (ER-41).
- **D3 (open, follow-up): JavaScript tests.** About 50 test files in `conformance/test`, `scripts/*.test.mjs` and the apps'
  `scripts/ci` start `node`, `cargo` or a script with `spawnSync` and inherit the environment the same way. They are not
  judged by C123 and have no builder yet; a JavaScript `testProcess()` in `conformance/test/helpers.mjs` and a C123 arm that
  reads `.mjs` would be about 0.5 agent-day. Not done here: the incidents were Rust tests, and a second builder needs the
  parity rule applied once more.
- **D4 (decided): git is not in C123.** `git` reads `HOME` and the git config, which `TestCommand` redirects when it is
  used; plain `Command::new("git")` fixtures (about 60) keep working. If a git-config incident appears, add it to the check.
- **D5 (decided, review of f39597fbb P1-3): every `CARGO_BIN_EXE_*` binary a test starts itself.** The check judges a constructor
  whose argument is `env!("CARGO_BIN_EXE_<any>")` or a name bound to one (see the check above), so the slot tool (`tools/heavy`, whose
  tests removed a deny-list of `HEAVY_*` variables by hand), the apps' programs and the wiki tool are covered. A binary handed to the
  code under test, which starts it through its own spawn API (`SpawnSpec`, `WorkerConfig`, `Builtins`), is the product's spawn and is
  not judged. Cost: about 0.3 agent-day (`tools/heavy` and `debrief-collect` gain a dev-dependency on the testkit; the heavy
  lockfile grows by about 285 lines of testkit dependencies).
- **D6 (decided, same review): the machine's `PATH` is never spliced back.** `path_first(dir)` with an explicit folder replaces
  `format!("{}:{}", dir, env::var("PATH"))`; check C123 flags `env::var("PATH")` in a function that builds a `TestCommand`.
- **D7 (decided, same review): a test that builds a program** calls `cargo_build()`, which adds `CARGO_TARGET_DIR` and the variables
  that decide how the child compiles (`BUILD_VARS`: `RUSTFLAGS`, `CARGO_ENCODED_RUSTFLAGS`, `RUSTC_WRAPPER`, `RUSTC_WORKSPACE_WRAPPER`,
  `LLVM_PROFILE_FILE`, the `CARGO_LLVM_COV*` variables, `CARGO_INCREMENTAL`), so a program built under coverage is the one the
  run measures. Used by Debrief's store-worker build and Pitcrew's fake runner; the Dyno tests only run `cargo tree` and
  `cargo metadata`, which compile nothing.
- **D8 (decided, last review; amended after the review of `a8fca0ebd`): `spawn()` returns a `TestChild`.** It derefs to the
  `Child`, holds the command's private folders until it is waited on or dropped (a child that writes to its `HOME` after the builder
  dropped used to find it deleted), and kills and reaps a child still running when it drops, through the handle it owns (ADR 0033:
  never a bare pid). **Descendants** are ADR 0103's: `paddock_testkit_start::Reaper::spawn_test(cmd)` starts a `TestCommand` through
  the reaper (its token, its groups, never a new group while the test leads one under nextest) and hands back the `TestChild`
  (`TestChild::from_parts`), so the child ends with its handle and everything below it with the reaper (rule 55: one
  mechanism). `TestChild` does not end descendants itself because the testkit cannot use `paddock_proc` (that crate's tests use the
  testkit; the dependency made a publish cycle, ci.yml 7989): a test whose child starts its own children uses `spawn_test`. Both
  are tested by identity in `paddock-testkit-start` (`reap::tests`).

## Consequences

- A new environment lookup in a tool shows up as a failing test that names the variable, instead of a quiet read of the
  machine. Tests that need a variable say so.
- `TestCommand` curates `PATH`: a test that needs a program outside the toolchain folders, `node`, `git` and the system
  folders adds its folder with `path_first` or sets `PATH` itself.
- `real` `~/.cargo` and `~/.rustup` stay readable (the offline registry, the toolchains). That is the one deliberate hole.
