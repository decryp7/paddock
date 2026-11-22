# 107. A test's hermetic environment is made once, never per child

- Status: Accepted 2026-11-22 (user), option A only, closed: option A is built (`crates/paddock-testkit/src/ambient.rs`, `tools/paddock-dev/src/ci_isolation/home.rs`; ci run 8247 measured it) and option B is not wanted. Was: Proposed 2026-11-18. The user chose to fix forward: TestCommand stays on train 14, and this ADR is the redesign in
  the same round (ER-21, ER-44). It amends ADR 0100. The choice between options A and B below is for the user.
- Date: 2026-11-18
- Related: ADR 0100 (one process builder with a listed environment), ADR 0025 (CI job isolation, kept folders), ER-9 (a gate
  counts on the real runner), ER-12, ER-21, ER-27, ER-28, ER-33, ER-37 / rule 76.
- Number: 0107. On 2026-11-18 the highest ADR on `origin/main` and every `origin` branch was 0106 (`git ls-tree docs/adr/`
  over every remote branch: 0104 on `origin/docs/adr-safe-by-default-under-tests`, 0105 and 0106 on `origin/feat/audit-on-main`).
- Evidence heads (ADR 0091), read 2026-11-18: `origin/train/2026-11-17-14` = `5484f36eb`, `origin/main` = `f5f08f71e`,
  `origin/exp/testcommand-runner-ab` = `a80479dd7` (its trial merge `trial/exp/testcommand-runner-ab` = `2ccf8dc00`).
- Numbers: docs/benchmarks.md, "TestCommand on the Windows CI runner: what one start costs (2026-11-18)".

## Context: the class

On the Windows CI runner, every run with `TestCommand` (ADR 0100) ran the tests that start many children several times
slower than a plain `Command` did. The Windows test VM and the Mac never reproduced it.

| Run | Tree | What slowed | Source |
|---|---|---|---|
| ci.yml 8152, job 39497 | train 12, `feat/test-env-builder` | merge_check median 5.8 to 44 s | docs/benchmarks.md "TestCommand construction" (read, not re-checked) |
| ci.yml 8161, job 39640 | train 13 | merge_check median 26.9 s against 4.54 s, 9 timeouts | docs/benchmarks.md "A child's git on Windows" (read, not re-checked) |
| ci.yml 8245, job 41212 | train 14, `5484f36eb` | 78 merge_check tests over 60 s on both tries; merge_check 19x, slots 33x | job log (checked): `merge_check::a_commit_that_is_not_a_merge_is_a_usage_error` 0.65 to 26.4 s with five starts; tests that start nothing 0.05 to 0.2 s at the same moment |

Each of the two fixes before this one removed a real cost that the VM showed. Neither found the main cost, because the VM
does not have it (ER-9).

## The cause, measured on the runner

`measure_test_command --ab 20` ran on windows-latest in ci run 8246 (job 41213, success, `testcommand-ab.yml` on the trial
ref). It was set up as the crate job: job isolation, the kept clone, the crate job's kept build folder `target-paddock-crate-s1`,
and the program running from the profile's `deps` folder, as a test program does. Each round ran every variant once, in an
order rotated per round. Each cell is the time for the merge_check fixture's four git starts (`init`, `add`, `commit`,
`rev-parse`) in a new repository.

| Variant (one change against main's fixture start) | median ms | p90 ms | / plain |
|---|---:|---:|---:|
| plain: `Command::new("git")`, inherited environment, hermetic git variables | 190.7 | 212.1 | 1.00 |
| plain, started by the full path TestCommand resolves | 185.9 | 199.7 | 0.98 |
| plain with `CREATE_NO_WINDOW` | 240.6 | 284.1 | 1.26 |
| plain with TestCommand's cleared environment (no private folder) | 180.3 | 201.4 | 0.95 |
| the cleared environment without `GIT_CEILING_DIRECTORIES` / with the machine's `PATH` / with the machine's folders | 185.6 / 188.8 / 181.8 | 208.8 / 204.0 / 207.4 | 0.95 to 0.99 |
| plain with `GIT_CEILING_DIRECTORIES` alone | 191.3 | 220.1 | 1.00 |
| `TestCommand::new` per start (the train's fixture) | 485.3 | 571.8 | 2.55 |
| `TestCommand::new_in`, one home for the sequence | 443.1 | 470.2 | 2.32 |
| `TestCommand::new` with the creation flags put back to none | 431.3 | 452.3 | 2.26 |
| `TestCommand::new` with the inherited environment put back | 482.6 | 524.5 | 2.53 |
| four `TestCommand::new` and drops, **nothing started** | 219.7 | 234.1 | 1.15 |

What each row rules in or out (ER-28):

- **The environment: out.** The cleared environment costs nothing (0.95). Putting the inherited environment back inside a
  TestCommand saves nothing either (2.53). None of `GIT_CEILING_DIRECTORIES`, the curated `PATH` or the private folders costs
  anything here.
- **Which git starts: out.** TestCommand resolves `C:\Program Files\Git\mingw64\bin\git.EXE`, the real program (0.98).
- **`CREATE_NO_WINDOW`: a small part.** It costs about 12 ms per start (1.26). Main's tools already pay it for their own git
  calls, and so do the product's children.
- **The builder: in.** About 55 ms per command, with nothing started. That is as much as the git start itself, and it is about
  three quarters of the gap between plain and `TestCommand::new` (295 ms per four starts).
- **One home per test instead of per command: a small part** (443 against 485 ms). Every `new_in` command still builds the rest.

The job log shows why the builder is slow. The child's `PATH` starts with
`...\tmp\paddock-test-cmd-JLtjB9\toolchain\0741e9b7bbf772ab`, a link folder **inside the command's own private folder**. The
shared folder `<target>/paddock-test-toolchain` was not used. No `TestCommand: cannot make the toolchain link folder` line
was printed, so `ambient::link_root()` returned `None` rather than failing. For a program in `<target>/debug/deps`, `None`
means one thing: the target folder has no `CACHEDIR.TAG`.

The kept build folders on host runners are made by `./dev ci isolation path target <name>`
(`tools/paddock-dev/src/ci_isolation/home.rs`, `create_dir_all(&target)` before cargo runs). Cargo writes `CACHEDIR.TAG` only
into a target folder it creates itself (from memory of cargo's `create_dir_all_excluded_from_backups_atomic`, not re-read;
the runtime evidence above does not depend on it). So on CI hosts every `TestCommand` links the toolchain's programs
(`cargo`, `rustc`, `rustup` and their rustup proxies) into its own folder, then deletes them when it drops. That is the
per-command version that run 8152 measured 6 times slower, and that ADR 0100's shared folder was meant to remove. It was
never active on the CI hosts.

On the VM, `scripts/member-gate-windows.sh` lets cargo make its own target folder, which gets the tag. That is why the VM
was fast. Under 8 parallel tests, every link points at the one `rustup.exe`, so the links pile up toward NTFS's 1024-link
limit. Past it, each command falls back to copying 15 MB programs, and Defender scans the new program files. That fits
run 8245's several seconds per start, but it is not measured.

**Confirmed both ways on the Mac.** The same program was run from a copied `debug/deps` folder with no `CACHEDIR.TAG`, then
with the tag written:

- without the tag: link folder in the command's private folder, 3.7 ms per build;
- with the tag: the shared `paddock-test-toolchain`, 1.0 ms per build.

Measured on the Windows runner with the tag present (ci run 8247, option A at `f09a919a9`, docs/benchmarks.md "...with
CACHEDIR.TAG present"): the child's `PATH` uses the shared `paddock-test-toolchain`, the builder fell from 55 to 5.9 ms per
command, and `TestCommand::new` per start is 1.28 times plain (2.55 before). The fault was present in run 8246 and removed in
run 8247, on the same runner and kept folder.

## Common practice (ER-37)

- **git's test suite** (`t/test-lib.sh`, https://github.com/git/git/blob/master/t/test-lib.sh, read 2026-11-18). Once per test
  script it:
  - unsets `VISUAL`, `EMAIL`, `LANGUAGE` and every inherited `GIT_*` variable except the trace and test switches;
  - sets `HOME` to the trash directory, `GIT_CONFIG_NOSYSTEM=1`, the identities and `GIT_CEILING_DIRECTORIES`;
  - exports them.

  Every git it starts afterwards inherits them. Nothing is made per command.
- **cargo's test support** (`TestEnvCommandExt::test_env` in
  https://github.com/rust-lang/cargo/blob/master/crates/cargo-test-support/src/lib.rs, read 2026-11-18). For every command it:
  - removes the inherited `CARGO_*` variables and a fixed list (`CI`, `RUSTFLAGS`, `RUSTC_WRAPPER`, `USER`, `XDG_CONFIG_HOME`,
    proxies, git identity, `MSYSTEM`, ...);
  - sets `HOME` (and `USERPROFILE` on Windows) to the test's `paths::home()`, plus `CARGO_HOME` and `GIT_CONFIG_NOSYSTEM`.

  It inherits the rest and makes no folders or links per command. That `paths::home()` is one folder per test, shared by its
  commands, is from memory and not re-read.

Both make the environment once and never write files per command. Paddock's allow-list (ADR 0100) is stricter than either,
on purpose. The cost found here comes from the per-command file system work, not from the allow-list.

## Decision

D1. **A kept build folder is a cargo target folder.** `./dev ci isolation path target` writes cargo's standard `CACHEDIR.TAG`
(`Signature: 8a477f597d28d172789f06886806bc55`, https://bford.info/cachedir/) when the folder lacks one. So does any other
place that makes a target folder before cargo does. `TestCommand` then shares one link folder per toolchain on CI hosts, as it
does on every developer machine.

D2. **A missing link root is visible (ER-27).** When a test program runs from a `deps` or `build/.../out` folder whose target
folder has no tag, the kit prints once per process:
`TestCommand: <target> has no CACHEDIR.TAG, so each command links the toolchain into its own folder (slow on Windows); next
step: let cargo create the folder or write the tag`.

D3. **Guards that fail on any OS.** Two deterministic tests:

- `ci_isolation` (paddock-dev): "a kept target folder made by `path target` carries cargo's CACHEDIR.TAG". It is red on every
  OS today.
- `paddock-testkit`: "a test program in a cargo target folder gets the shared link root". `ambient().link_root` is `Some`, and a
  command's `PATH` starts under `<target>/paddock-test-toolchain`. It is red on the CI hosts today.

Their positive control (ER-33) is the existing unit test `the_link_root_is_only_in_a_folder_cargo_tagged_as_its_target`, which
shows `None` without the tag.

D4. **Option B only: one environment per test.** `paddock_testkit::TestEnv` makes the private folders, the `.gitconfig`,
the resolved programs and the curated `PATH` once. `TestEnv::command(p)` returns a `TestCommand` that shares them, with no file
system work per child. Fixtures that start many children (merge_check's `Repo`, generators, pre_push, golden, slots) hold one.

D5. **Windows changes to the kit are measured on the runner.** A change to `crates/paddock-testkit/src/process.rs` or
`ambient.rs`, or to how CI makes build folders, runs `testcommand-ab.yml` through a trial before it joins a train. A VM
number is `not measured` (ER-9).

## Options and cost

| Option | What | Agent-days |
|---|---|---:|
| A (common practice, cheaper; recommended first) | D1, D2, D3, D5: the measured cause removed, made visible and guarded | 0.5 |
| B (the full redesign) | A plus D4, the environment made once per test as git and cargo do | 1.5 |

A removes about three quarters of the measured gap and follows git's and cargo's practice of no per-command files. B removes
the rest of the builder's per-command work: the private folder, the `.gitconfig` and the `PATH` assembly, about 1 to 4 ms per
command on the Mac. B is worth its extra day only if the runner A/B after A still shows `TestCommand::new` above 1.5 times
plain. Decision for the user: A now and B on that measurement (recommended), or both now.

`CREATE_NO_WINDOW` stays. It is the product's start flag, and a test's child should start as the product's does (ADR 0100).

## Work packages

- WP-1 (0.25 d, option A): D1 in `tools/paddock-dev/src/ci_isolation/home.rs`, and D3's paddock-dev test written first.
- WP-2 (0.25 d, option A): D2 in `crates/paddock-testkit/src/ambient.rs`, D3's testkit test, and D5 as a line in the train
  checklist. Then rerun `testcommand-ab.yml` on the trial. Expected: `tc-build-only` under 20 ms per four commands, and
  `tc-per-command` within 1.5 times plain.
- WP-3 (1 d, option B only): D4, `TestEnv` and the fixtures moved to it.
- Check: the train's next Windows paddock-dev job against run 8241 (main): merge_check median within 1.5 times.

## Risks

- One runner run of 20 rounds. WP-2's rerun is the second measurement, with the fault removed (ER-28).
- Kept target folders that already exist get the tag on the job's next `path target`. Until then, a job that skips that step
  stays slow, and D2 prints why.
- The link-limit and copy explanation for run 8245's several seconds per start is not measured. If WP-2's rerun under the
  real test load is still slow, the next suspect is Defender scanning the per-command links.
