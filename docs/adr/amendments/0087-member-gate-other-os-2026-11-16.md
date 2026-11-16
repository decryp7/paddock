# Amendment to ADR 0087 (2026-11-16, draft): the other-OS member gate, and a pinned toolchain for the test VM

- Status: Proposed 2026-11-16. The gate (§1) is built and report-only. §2 was decided by the user on 2026-11-16: option (b) plus the MSVC Build Tools in the image; it is built and measured below.
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

**Windows** (`scripts/member-gate-windows.sh`): under the test machines lock (`vm-lock-run.sh`; busy is exit 75), the guest is reverted
to the snapshot `buildtools-baseline` (clean-baseline plus the Build Tools, §2), started, and `vm-win.sh run --ci-like
--tools-provided` copies in the Rust toolchain of `rust-toolchain.toml`, the pinned Arm `cargo-nextest.exe` and the pinned Node,
then builds and runs the specs **natively**: `cargo nextest run -p <package> --profile ci --locked --no-fail-fast`. Tests that run
cargo (all of `paddock-dev`'s) therefore work. The guest command prints `other-os-control: os=windows tree=<tree> autocrlf=true
... target=x86_64-pc-windows-msvc vm-arch=aarch64 runner-arch=x86_64` from its own checkout and its verdict as an
`other-os-result: failed=<n>` line (the ci-like wrapper turns a guest exit 100 into 1, so an exit code cannot carry it).

**Architecture (decided 2026-11-16 on the coordinator's note).** The guest is Arm; the CI runner is x64. The gate builds
**x86_64-pc-windows-msvc by default**: the native Arm rustc cross-compiles (the image has the x64 MSVC libraries and a
`Hostarm64\x64\link.exe`; the extra `rust-std-x86_64-pc-windows-msvc` is copied with the toolchain) and the x64 test programs run
under Prism emulation, so a member is tested in the runner's architecture. Reason: native aarch64 does not build every crate yet
(Dyno's `dyno-platform` tests do not compile for aarch64: the `windows` crate lacks `RegisterClassW` in `mount_events.rs`; that gap
is recorded as its own item, not fixed here), and x64 programs built with the repo's `crt-static` rustflags start in the guest.
`cargo-nextest.exe` itself is the **native Arm** release: the pinned x64 one does not start (it is not statically linked and the
guest has no x64 VC runtime), so running the x64 nextest under emulation is not worth it: it would need the x64 VC++
redistributable in the image for no gain, since nextest only starts the test processes.
`PADDOCK_MEMBER_GATE_WIN_TARGET=aarch64-pc-windows-msvc` builds natively instead. Every Windows row says so (`note` in the JSON,
`[x64 test programs ... Arm64 test VM ...]` in the text).

**The row** (ER-33): `passed` needs exit 0, the control line for this merge's tree and nextest's summary with tests run for every
nextest target; `failed` is exit 100 with the control line; everything else (Docker down, the VM busy, a build failure, a missing
control, a run ended by its bound, a package that needs the toolchain, below) is `not measured` with the reason. A path set that
reaches neither OS is `not run (<why>)`. **Report-only (ER-41)**: `ENFORCED` is false in `other_os.rs`, the exit code is
unchanged, the text says `REPORT-ONLY` and what would happen. The first green run on a real train is recorded below by the change
that flips it; a const assertion in the tests fails until that is decided.

First report-only run: see "Measurements" at the end.

A package whose tests run `cargo` (all of `paddock-dev`) cannot run on Windows (§2): its row is `not measured`, listed by name.

## 2. The toolchain for the Windows guest (decided: (b) plus Build Tools)

The first version cross-built on the Mac and could not run tests that call `cargo` (all of `paddock-dev`'s: they read
`env!("CARGO")`, a Mac path in a cross-built program, or run `cargo metadata`). That hole is the class of run 8034 (a `/c/Users/...`
checkout path read by `queue::tests`). The options were (a) a pinned toolchain in the image, (b) the toolchain per run from a
host-side cache, (c) cross-built programs only. The user chose (b) **plus** the MSVC Build Tools in the image, so the guest builds
natively. (b) follows `rust-toolchain.toml` by itself; an image copy (a) would need a re-provision on every Rust bump.

What was built:

- **Build Tools (once, in the image).** `vm-win.sh toolchain-setup` (under the lock, bounded, with a cleanup trap that reverts to
  `clean-baseline` on any early exit): reverts to `clean-baseline`, takes the rollback snapshot **`pre-buildtools-2026-11-16`**
  (a copy of `clean-baseline`'s state before the change), installs **Visual Studio Build Tools 2022 17.12.37502.7** (LTSC 17.12,
  components `VC.Tools.x86.x64`, `VC.Tools.ARM64`, `Windows11SDK.22621`) as `paddock-admin`, shuts down cleanly and takes
  **`buildtools-baseline`**. `clean-baseline` is unchanged; the way back is `vm-win.sh revert` to it (or to the rollback snapshot).
  The installer is Microsoft's fixed LTSC 17.12 bootstrapper (`https://aka.ms/vs/17/release.ltsc.17.12/vs_buildtools.exe`, which
  redirects to `download.visualstudio.microsoft.com/download/pr/0b774c01-.../7ab0e750...e3e/vs_BuildTools.exe`); the SHA-256 in
  that path, `7ab0e75003c995688999833d01fce07ac60b92f53fc9439494e4dd8521bf4e3e`, is checked on the host and again in the guest,
  and the Authenticode signature must be valid and Microsoft's. The components come from Microsoft's CDN through that installer.
  No token, no account (ER-26).
- **Measured.** The installer ran 540 s (about 13 minutes from the VM start to the snapshot); about 8.3 GB used on the guest's C:,
  which is the size the new snapshot adds to the VM folder (the folder was 118 GB before; other agents' snapshots grew it too).
  Two first attempts failed and were fixed: an SSH session over the installer was dropped by its keep-alive while the guest was
  starved, and a `Start-Process` launch over SSH never started (the script now creates a scheduled task as SYSTEM, which survives
  the session). The poll loop prints a live progress line (ER-18; the first version printed only "still working" and kept the
  lock for 73 minutes while nothing ran). The install needs the VM exclusively, so it cannot release the lock in the middle, but
  it now lasts about 13 minutes.
- **Toolchain per run (host-side cache, `scripts/vm-win/member-gate-tools.lock`).** The cache folder holds the toolchain of
  `rust-toolchain.toml` (an exact version is required) for `aarch64-pc-windows-msvc` plus the x86_64 std, checked against the
  `.sha256` that static.rust-lang.org publishes, packed as one archive of about 160 MB; the pinned aarch64 `cargo-nextest` (the
  version of `host_tools.rs`' pin; a test holds the lock file equal to it; its SHA-256 comes from the GitHub release, which
  publishes none for this archive: the first trust is TLS) and Node 24.22.3 (`.nvmrc`'s major; SHA-256 from nodejs.org's
  SHASUMS256.txt). They are copied in with `--with` each run and unpacked in the guest (4 to 5 s). No rustup runs in the guest, so
  nothing can auto-install a toolchain (`RUSTUP_AUTO_INSTALL=0` is set too); the host never touches `~/.rustup` or `~/.cargo`.
- **Registries.** No token: the Gitea registry is read anonymously (`.cargo/config.toml`: "no token needed to read") and
  crates.io too. Nothing secret is written into the image or a log (ER-27); a test in `scripts/member-gate.test.mjs` fails if
  the script names a token.

## 2b. Measurements of the native guest build (2026-11-16, the VM under load from other lanes: upper bounds)

| What | Time |
| --- | --- |
| VM start from `buildtools-baseline` | 23 to 56 s |
| Toolchain, Node and nextest unpack in the guest | 4 to 5 s |
| `paddock-testkit-start`: cold build in the guest, then 23 tests (all passed, green on a real member) | 490 s for the whole run including the 400 MB bundle copy and the checkout |
| `paddock-dev`: cold build in the guest, then 1444 tests (first run, before Node was copied in) | 554 s of nextest, 808 s for the guest run; 1393 passed, 50 failed, 1 timed out (84 of the panics say `node must be installed` or `the host must run`, which Node in the guest fixes) |

The guest is reverted for every run, so every run is a cold build; a warm build exists only inside one session. The cost per run
of a `paddock-dev` change is therefore about 8 to 13 minutes. A second run with Node in the guest ran for over 35 minutes and was
ended by me when the coordinator asked the VM to be freed (the guest had stopped answering SSH under load), so the post-Node
failure list is **not measured yet**.

Failures seen in the first run that are not about Node and are for the review to triage (they are what this gate is for):
`/srv/dev-targets`-style paths that are not absolute on Windows (`places`, `wrapper`, `check_engine`: "The dev-targets folder
/srv/... is not absolute"), `grep: command not found` in a copy of the repo's `dev` script (the runner-style PATH has Git's `bin`
but not `usr/bin`), and a `TotalPhysicalMemory` a standard user cannot read.

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
