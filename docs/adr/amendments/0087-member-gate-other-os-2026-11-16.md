# Amendment to ADR 0087 (2026-11-16, draft): the other-OS member gate, and a pinned toolchain for the test VM

- Status: Proposed 2026-11-16. The gate (§1) is built and report-only; the toolchain choice (§2) waits for a decision.
- Trigger: train 12 spent about 17 hours in three red CI rounds, and every failure was Linux- or Windows-only: run 8138 (23
  failures: a hidden PATH, `/srv` paths that are not absolute on Windows, a lost Git-bash PATH, slow process starts, ext4 folder
  blocks), run 8147 (a `#!/bin/sh` fixture Windows cannot run), run 8152 (TestCommand 6x slower on Windows). All of them passed the
  Mac member gates. A review also found a VM-test bug that only shows in a fresh clone without repo-local git config.
- Related: ER-9, ER-33 (a row says why it did not look), ER-36, ER-41 (report-only first), ER-42 (read declared data through its
  accessor), ER-37 (common practice and cost), ADR 0087 and its amendment of 2026-11-13 (`run --ci-like`), ADR 0098 (member
  gates), ADR 0103.

## 1. The gate: `other-os`, a row of `./dev merge check --gates`

After the fast gates, the step reads the paths of the member's own change (`<merge>^1` to the merge) and decides what runs where
(`tools/paddock-dev/src/merge_check/other_os.rs`, `select`):

| A changed path | Linux (CI image) | Windows (test VM) |
| --- | --- | --- |
| `tools/paddock-dev/` | nextest `paddock-dev` | nextest `paddock-dev` |
| `crates/paddock-testkit/`, `crates/paddock-testkit-start/` | nextest of the crate | nextest of the crate |
| `conformance/` (and the CI kit under it) | `npm test` in `conformance` | none: its checks are Node and the runner's Node is not the question |
| `scripts/vm-*` | `node --test scripts/vm-*.test.mjs` | none (Mac-only scripts) |
| any other Rust crate whose tests start programs (`Command::new`, `TestCommand`, `paddock_proc`, `.spawn(`, `assert_cmd` in its sources) | nextest of that package | nextest of that package |
| Markdown, `docs/`, everything else | nothing | nothing |

The plan (`./dev plan`) is not the selector on purpose: it answers "which projects does CI test", which is every project in the
dependency cone, while this gate answers "which changes can differ by OS", which is a property of the files. The plan's own
documentation-only rule is the same as `docs_only` here.

**Linux** (`scripts/member-gate-linux.sh`, through `tools/linux-container.sh --ci-image --loop-mount`): the CI image
(`linux/amd64`, the image CI runs; native arm64 gave a false failure in conformance, "no pinned Chromium for linux/arm64"), as
root, with `TMPDIR` on an ext4 loop mount (the only addition to the container: `CAP_SYS_ADMIN`, the loop devices; not
`--privileged`, no host path mounted), in a fresh shallow clone of exactly the merge commit (`git init` + `fetch --depth 1`, as
`actions/checkout` leaves it, so no repo-local config), with `CI=true`, `GITHUB_ACTIONS=true`, `ulimit -n 1024` and nextest's
`ci` profile. The script prints `other-os-control: os=linux tree=<tree> user=root tmp=ext4 clone=fresh-shallow gitconfig=none`
only after it checked each of those.

**Windows** (`scripts/member-gate-windows.sh`): under the test machines lock (`vm-lock-run.sh`; busy is exit 75), the test
programs are built on the Mac (`cargo xwin env` + `cargo test --no-run`, target `aarch64-pc-windows-msvc`, with
`-C target-feature=+crt-static`), the guest is reverted to `clean-baseline`, started, and `vm-win.sh run --ci-like` runs each
test in its own process, three at a time (`<exe> --exact <test>`, nextest's model). The guest command prints
`other-os-control: os=windows tree=<tree> autocrlf=true ...` from its own checkout and its verdict as an
`other-os-result: failed=<n>` line (the ci-like wrapper turns a guest exit 100 into 1, so an exit code cannot carry it).
Measured 2026-11-16 on the guest: an x64 program does not start (exit 0xC0000135, a missing DLL), which includes the pinned
`cargo-nextest.exe` (the x64 release is the only Windows one it pins), and an Arm program needs the static CRT for the same
reason. So nextest itself does not run there; its test groups, retries and per-test timeouts (`.config/nextest.toml`) are not
mirrored, and the guest's architecture is the Arm one (ADR 0087 amendment of 2026-11-13, S1), not the runner's.

**The row** (ER-33): `passed` needs exit 0, the control line for this merge's tree and nextest's summary with tests run for every
nextest target; `failed` is exit 100 with the control line; everything else (Docker down, the VM busy, a build failure, a missing
control, a run ended by its bound, a package that needs the toolchain, below) is `not measured` with the reason. A path set that
reaches neither OS is `not run (<why>)`. **Report-only (ER-41)**: `ENFORCED` is false in `other_os.rs`, the exit code is
unchanged, the text says `REPORT-ONLY` and what would happen. The first green run on a real train is recorded below by the change
that flips it; a const assertion in the tests fails until that is decided.

First report-only run: see "Measurements" at the end.

A package whose tests run `cargo` (all of `paddock-dev`) cannot run on Windows (§2): its row is `not measured`, listed by name.

## 2. DECISION NEEDED: a Rust toolchain for the Windows guest

The guest has no Rust. Today the gate cross-builds test programs on the Mac, which covers every test that only runs the program
under test, and none that runs `cargo`. In `paddock-dev` that is `inputs.rs`, `generators.rs`, `golden.rs`, `locks.rs`,
`contracts.rs` and `src/queue`, `src/generators`, `src/check/c121.rs` and others: they call `env!("CARGO")`, which the
cross-build bakes in as a Mac path, or run `cargo metadata` over the repository. So with only the cross-built archive the Windows
row for `paddock-dev` is `not measured: its tests run cargo`, and the other packages run. That hole is exactly the class of run
8034 (a `/c/Users/...` checkout path read by `queue::tests`).

A toolchain in the guest is therefore not enough by itself for an archive: `env!("CARGO")` is fixed at compile time. A run that
uses it must build in the guest, which also needs a linker (the MSVC Build Tools, a few GB) and the registry sources
(`cargo fetch --locked` needs the private registry token). I measured or looked up what I could without touching the image:

| | (a) pinned toolchain in the VM image, once | (b) toolchain per run from a host-side cache | (c) cross-built programs only, no cargo-running tests |
| --- | --- | --- | --- |
| What it is | a provisioning step (`vm-win.sh toolchain-setup`, like `admin-setup`): from `clean-baseline`, installs the channel of `rust-toolchain.toml` (rustup-init with `--default-toolchain 1.100.0 --profile minimal`, SHA-256 from the channel manifest), takes the snapshot `toolchain-baseline`, reverts. Gate runs start from that snapshot. | the Mac keeps the unpacked toolchain in a cache folder (the `rust-1.100.0-x86_64-pc-windows-msvc.tar.xz` dist is 207 MB; minimal profile about 300 MB unpacked), the gate packs it as one archive, copies it with `--with` and unpacks it in the guest before the command (`vm-win/ci-like.ps1` already puts `%USERPROFILE%\\.cargo\\bin` first on PATH) | nothing to add: today's script |
| Changes the user's VM image | yes (one more snapshot, about 0.5 to 1 GB; `clean-baseline` stays untouched) | no | no |
| Build cost | about 1.0 agent-day: the subcommand, the checksum check, a check that fails when the snapshot's `rustc --version` differs from `rust-toolchain.toml`, a fake-VM test, the doc | about 0.7 agent-day: the fetch and verify, the pack, the unpack step in `ci-like.ps1`, tests, the doc | 0 (done); 0.2 agent-day to make the `paddock-dev` tests say what they need (a `PADDOCK_TEST_NEEDS_CARGO` skip) |
| Run time added | 0 s | about 30 to 60 s per run (a 100 MB packed copy over SSH plus unpack of about 10k files; to be measured when built) | 0 s |
| Upkeep | re-provision on every Rust bump (about 10 minutes of machine time, one command); a stale snapshot is refused by the check | none: it follows `rust-toolchain.toml` | none |
| Covers cargo-running tests | only with a linker and the registry added to the image (about +2 days and +3 to 6 GB for the Build Tools; the registry token must be put in the guest) | the same extra needs, every run | no |
| Risk | the snapshot drifts from the pin; a bigger image on d8 | slower runs; the cache must be verified | the biggest hole stays |

Recommendation: **(c) now, then (b) only if a test that needs cargo on Windows has failed in CI again**, and not (a).
Reasons: (1) a toolchain alone does not unlock the tests that matter, because they bake in `env!("CARGO")` (so the real price of
covering them is the Build Tools and the registry, beyond either option: about +2 agent-days); (2) (b) costs less to build than
(a), leaves the user's image alone and follows the Rust pin by itself, and its 30 to 60 s per run is small against the VM's own
start and revert; (3) the failures of train 12 that were Windows-only were in paddock-testkit, paddock-proc and fixtures
(`#!/bin/sh`, `/srv`, PATH), which (c) already runs. The honest limit of (c) is the `not measured` row for `paddock-dev`; it is
visible in every report. I stopped here for the decision; the gate is built so that either option changes only
`scripts/member-gate-windows.sh` (the `needs_toolchain` list) and `ci-like.ps1`.

## 3. Common practice and cost (ER-37)

GitHub Actions and the Gitea runner here use one runner per OS, so CI itself is the common practice; the cost of "a runner per OS
before merge" is the point of this gate, and the local VM is the user's own decision (ADR 0087). The gate costs about 1.5
agent-days beyond a docs-only change (module, scripts, tests, this note); the cheaper option, "run the other OSes only in CI",
is what train 12 cost 17 hours for.

## Measurements (2026-11-16, report-only; the machine was shared and often at 0% idle, so the times are upper bounds)

A planted merge (the member adds `crates/paddock-testkit-start/tests/planted.rs`: a `#[cfg(unix)]` test that needs the macOS-only
`/Applications`, and a `#[cfg(windows)]` test that starts a `#!/bin/sh` fixture) through `./dev merge check --gates`:

| Row | Result | Time |
| --- | --- | --- |
| Linux | `failed`: `planted_linux_only_failure_a_macos_only_path_is_expected`; 34 other tests passed; control line `os=linux user=root tmp=ext4 clone=fresh-shallow gitconfig=none arch=x86_64` | 31 s (warm build volume; a cold conformance run was 203 s under emulation) |
| Windows | `failed`: `planted_windows_only_failure_a_sh_shebang_fixture_cannot_run` (os error 193); 23 other tests passed | 90 to 120 s per run with the build warm; the bundle of the tree is 400 MB |

The same Linux mode ran `wrapper::the_cached_tool_holds_itself_for_its_run_and_completes_when_its_cache_entry_is_pruned_meanwhile`
(it fails 5 of 5 under `linux-container.sh`'s read-only `.git` mount) and it passed, because the run builds in its own
writable fresh clone, as CI does: 71 s including the build, `1 test run: 1 passed`.
