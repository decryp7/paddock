# 87. Windows code runs before it lands: `./dev test --windows` on the leased test VM, a detector, and positive controls

- Status: Proposed (2026-11-10). Drafted at the user's request of 2026-11-10 (ER-21 redesigns). Draft only: nothing here is
  built. Needs the user's decisions below and an independent review before any work package starts.
- Date: 2026-11-10
- Related: ER-9 / rules 3, 4 and 50 (h) (fix locally first; "the Windows tests where paths are compared"; the slower local
  checks run beside CI and a local failure cancels the run), ER-11 / rule 5, ER-14 / rule 7, ER-21 / rule 40, ER-24 /
  rule 41 (load), ER-27, ER-32 (`./dev wait`), ER-33 / rule 71 (positive controls), ER-36, rule 48 (benchmarks recorded);
  ADR 0022 (Rust tools in `./dev`), ADR 0025 and its 2026-11-10 amendment (build folders per worktree), ADR 0038 and its
  2026-11-10 amendment (`./dev push-gate`, `./dev push-tests`; on `origin/docs/adr-redesigns-2026-11-10`), ADR 0040 (the
  Windows GUI session), ADR 0074 Part A (machine leases), ADR 0080 (CI critical path; the landing run is full on every
  OS), ADR 0086 (failing-first replay); the Rust tooling plan `docs/plans/rust-tooling-2026-11-10.md` §3 (package
  ownership; on `origin/docs/rust-tooling-plan`); `docs/testing/windows-test-vm.md`.
- Number: 0087 was free on every `origin` branch on 2026-11-10 (`git ls-tree -r --name-only` over every
  `refs/remotes/origin/*` after `git fetch`: family ADRs end at 0086, on `origin/docs/adr-redesigns-2026-11-10`).
- Evidence read at `origin/train/2026-11-09-3` (`67a633ef3`) and the branches named below. Nothing was run on the test VM
  for this draft: the 1-minute load was 55 to 77 while it was written, above `vm-win.sh`'s limit of 25 and ER-24's hold
  level, so the cost figures below are estimates built on one recorded VM run, and WP-0 measures them.

## Context: the incidents (2026-11-10)

Windows-only code was type-checked (`cargo xwin check`/`clippy`) and never run before CI, or before a reviewer read it.

| # | Where | What broke, or would have | Seen by | Would a local Windows test run have caught it? |
|---|---|---|---|---|
| 1 | `dyno-release.yml` run 7880, job 34520; fix `89a9f2352` | On the Windows host runner `bash` resolved to WSL because a bash step ran before the Git-bash step; the job died before checkout | CI | **No.** It is a workflow step order on a CI host. `89a9f2352` added the static check (`conformance/test/ci-actions.test.mjs`, "no Windows host job runs a bash step before Git bash is on PATH"). Out of this ADR's scope (see "Not covered") |
| 2 | `origin/feat/rt1-ci-isolation` `7cf714872`, `69700f26d` | Git-bash spellings (`/c/x`, and `/c` alone) were not converted to drive paths for `--source` and `owns`; the code read `cfg!(windows)` directly, so no test on macOS or Linux could reach it | review, then a fix that made the platform a parameter (`ctx.windows`) | **Yes**, for the real hand-off: Git bash converts arguments before a native program sees them, which only a run under Git for Windows shows. The parameter fix covers the pure part on every OS |
| 3 | `origin/feat/rt2-release-isolation` `ad9d85a5b` | A `needless_return` that only `cargo xwin clippy` reported, and an untested junction branch (`mklink /J`, reparse points) | review | Clippy: **no**, that is ADR 0038 amendment step 5 (xwin clippy in `push-gate`). Junction: **yes**, a junction exists only on NTFS |
| 4 | `origin/feat/rt4-hook` (`tools/paddock-dev/src/hook/install.rs:28`, `claude-pre-tool-use.sh:5`) | The installed hook binary is `paddock-dev.exe` on Windows and the shim falls back to `$b.exe`; neither name was ever run on Windows | review | **Yes**, with an install-and-run test on the VM |
| 5 | `origin/fix/liveness-broken-sites` `867add2de` | `pid_exists` treated a process as alive while an `OpenProcess` handle kept its object; the presence tests in `paddock-proc`'s tree module were `cfg(unix)` only, so Windows had none | review | **Yes**, once the tests are not `cfg(unix)`; `867add2de` made them run on Windows (pid 4 control, exited child). Today nothing forces a Windows test to exist (D7) |

Three of five would have been caught by running the touched crates' tests on Windows before review; one by `push-gate`'s
xwin clippy (accepted 2026-11-10); one is a CI workflow class that no local Windows test run sees.

ER-9 already asks for "the Windows tests where paths are compared". Rule 50 (h) and ADR 0038 §3 then declare Windows tests
`ci-only: "no Windows test host (2026-10-27)"`. That declaration is out of date: since 2026-10-29 Paddock has a Windows 11
Arm test VM (`docs/testing/windows-test-vm.md`), already used for a cross-built Rust program under the lock
([benchmarks: how a connection ends on each OS](../benchmarks.md#2026-11-09-how-a-connection-ends-on-each-os-unread-input-pipe-closehandle-listener-backlog-ordered-close-baseline-featipc-endings-m0)).
What is missing is one command that makes that run cheap, a check that says when it is needed, and a control that shows
it really ran on Windows.

## Pattern

**Windows-only code reaches review and CI having only compiled.** Whenever a change touches code that runs only on
Windows (`cfg(windows)`, `cfg!(windows)`, `target_os = "windows"`, Windows-only files, Windows paths and line ends), its
first execution is a CI Windows job or a user's PC, because the developer Mac cannot run it and no local step asks for it.

**Invariant.** A change that touches Windows-only code has run the tests of the touched crates on Windows before review,
on the leased test VM, with a positive control showing that the run was on Windows and that tests ran; CI stays the
final gate, not the first runner.

## Instances (counts at `origin/train/2026-11-09-3` `67a633ef3`)

- 323 tracked `.rs` files hold 629 Windows conditionals (`cfg(windows)`, `cfg(all|any(... windows ...))`,
  `cfg(target_os = "windows")`, `cfg!(windows)`): Debrief 95 files, Dyno 81, Pitcrew 27, Pitwall 15, Spotter 11, shared
  crates 74 (`paddock-service` 10, `paddock-proc` 10, `paddock-window` 5, `paddock-updater` 5, `paddock-ipc` 5, ...),
  `tools/` 8. Another 48 `.rs` files are Windows by path (`windows/`, `win.rs`, `*_windows.rs`), plus 15 `.ps1` files.
  (`git grep -lE 'cfg\((all\(|any\()?(windows|target_os *= *"windows"...)|cfg!\(windows' -- '*.rs'`.)
- Landings: of the 165 first-parent merges on the train since 2026-10-27, **23** changed a line carrying a Windows
  conditional or a Windows-only file, and **60** changed a `.rs` file that contains one anywhere. The truth for "changed
  Windows code" lies between: a hunk inside a `cfg(windows)` block does not carry the attribute on its own line. That is
  about 2 to 5 landings per working day, with peaks (49 landings on 2026-11-09).
- Today's local Windows path: `scripts/vm-win.sh` (start, `put`/`get`, `ssh`, `stop`, `revert`), the interim lock
  `scripts/vm-lock.sh` / `vm-lock-run.sh` (ADR 0074 WP-0), and an unmerged Rust broker `./dev vm` on
  `origin/feat/dev-vm-broker` (`633cd1da2`, ADR 0054 V-1). Nothing builds a test archive for Windows or runs nextest there.
- The VM: Windows 11 Pro on Arm build 26300, 4 vCPU, 8 GB, Git for Windows with `core.autocrlf=true`, Defender on, tests as
  the standard user `paddock-test` in `C:\paddock-test`; never two Windows VMs at once (`windows-test-vm.md`). No Rust
  toolchain and no nextest in the guest; installing toolchains there needs the user's OK (memory, 2026-10-27).

## Decision

### 1. `./dev test --windows [<crate>...]`

A Rust command in `tools/paddock-dev/src/wintest/`, under the Rust tooling plan's ownership rules (§3): the package owns
that folder and its tests; `cli.rs` gets one enum variant and match arm, `lib.rs` one `mod` line, `Cargo.toml` only new
dependencies; it writes no `.sh` or `.mjs`. Without crate names it asks the detector (§3) for the touched crates of
`merge-base(origin/main, HEAD)..` plus the working tree.

Steps (build outside the lease, so the one VM is held only for the part that needs it):

1. **Resolve.** Map crates to their Cargo workspace (`crates/`, `apps/<app>/...`, `tools/paddock-dev`; each has its own
   lockfile). A crate whose tests are declared `vm: session` (they need a desktop: ADR 0040) or `vm: admin` is listed as
   skipped with its reason, never silently (ER-27).
2. **Build on the host,** through `heavy` with `CARGO_BUILD_JOBS=2` (ER-24), in the worktree's own build folder (ADR 0025
   amendment, `./dev target-dir`, one folder per target): the environment from `cargo xwin env --target
   aarch64-pc-windows-msvc` (cargo-xwin 0.23.1 has `env`; both arches are in its cache on the dev Mac), then
   `cargo nextest archive --target aarch64-pc-windows-msvc -p <crates> --locked --archive-file <tmp>/tests.tar.zst`.
   Extra runtime files (Debrief's DuckDB library and the like) come in through nextest's
   `profile.<name>.archive.include`. Nextest documents that archives may be built on one OS and architecture and run on
   another, that the target needs nextest but not cargo, that sources are not in the archive, and that
   `--workspace-remap` points at them ([nextest, archiving](https://nexte.st/docs/ci-features/archiving/), read
   2026-11-10).
3. **Sources with CI's line ends.** From a snapshot of the worktree (a temporary index, `git commit-tree`, as planned for
   `scripts/vm-test.sh` on 2026-10-27), write the touched workspaces' files with
   `git -c core.autocrlf=true checkout-index -a --prefix=<tmp>/src/` into a temporary folder and tar it. Verified on a
   synthetic repository on the dev Mac (git 2.56.0): text files come out CRLF, a `binary` file per `.gitattributes`
   stays LF, so the guest sees what the CI Windows runner's checkout sees.
4. **Lease** `paddock-win11-test` and run the guest part as the lease's child, through one port, `MachineLease`:
   - interim adapter: `scripts/vm-lock-run.sh wintest -- <self> test --windows --guest <plan.json>` (the interim lock of
     ADR 0074 WP-0), driving the VM only through `scripts/vm-win.sh`;
   - final adapter: ADR 0074 Part A's `./dev machine lease run paddock-win11-test --purpose "<branch> wintest" -- ...`,
     with `PADDOCK_LEASE_TOKEN` checked by every driver call. WP-6 swaps the adapter and deletes the interim one.
   Never `vmrun` directly, never another VM, never the CI Windows runner (ER-14; `vm-win/target.sh` refuses other
   folders).
5. **Guest part.** `vm-win.sh start` (it refuses at a 1-minute load of 25 or more), `put` the archive, the source tar and
   a pinned `cargo-nextest.exe` for aarch64 Windows (version and SHA-256 from ADR 0038 §4's `ci/tool-versions.toml`,
   checked on the host before the copy, D4) into `C:\paddock-test\wintest\<lease id>\`, then over SSH as `paddock-test`:
   `cargo-nextest nextest run --archive-file tests.tar.zst --workspace-remap src --profile wintest --test-threads 3`
   with JUnit output. `--test-threads 3` leaves one of the 4 vCPUs to SSH: loading all 4 from an SSH session lost the
   session once (lesson `9c738b1ad`, ADR 0074 incident I6). `get` the JUnit file and nextest's output, `stop`, and
   `revert` to the clean state ADR 0074 declares for the machine (D5).
6. **Report.** One table per crate: tests run, passed, failed, skipped (declared), duration; failing tests with their
   output tail and the exact re-run command (`./dev test --windows -p <crate> -- <filter>`); `::error::` lines when
   `GITHUB_ACTIONS` is set (ER-27). Exit 0 only when every requested crate ran at least one test and none failed.
   A green run writes a stamp `<git-common-dir>/paddock/wintest/<tree hash>.json` (tree, crates, counts, guest build,
   lease id, durations). Like ADR 0038's push-gate stamp it is a **nudge**, not enforcement (an agent can write a file);
   CI's Windows legs stay the enforcement.
7. **Time and waits.** Agents run it in the background and wait with `./dev wait --pid <pid> --max <m>` (ER-32), `--max`
   set from WP-0's measurement; a queued lease prints its place in the queue and the holder's purpose.

`--self-test` runs only the positive controls (§4). `--arch x64` builds `x86_64-pc-windows-msvc` instead and runs it
under the guest's x64 emulation (Prism) for code that depends on the architecture (D1).

### 2. Rule (proposed; amends ER-9 and rule 50 (h); the coordinator numbers any new Paddock rule)

> **Windows-only code runs on Windows before review.** A change that the detector marks as touching Windows-only code runs
> `./dev test --windows` for the touched crates, green, before it is handed to review; a fix round that touches such
> code again runs it again. Windows logic that can be pure takes the platform as a parameter (as `ctx.windows` in
> `ci_isolation`), so every OS tests it; the VM run covers what only Windows does (paths from Git bash, junctions,
> handles, services, line ends). A touched crate in which no test runs on Windows fails the run (D7). The run may overlap
> CI (ER-9): a red Windows run cancels the branch's CI run. When the VM is held by a release run, review may start with
> "Windows run queued"; the branch does not land until the run is green.

The review brief template gets one line ("Windows run: stamp `<tree>` or not needed per `./dev plan --windows-touch`"),
and the integrator's landing checklist reads the stamp. ADR 0038 §3's Windows row changes from "tests and bundles are
`ci-only`" to "tests of touched Windows crates: `./dev test --windows` (this ADR); bundles stay `ci-only`".

### 3. The check: `./dev plan --windows-touch`

A detector in the planner (the T0 Rust planner; one new mode, its own module), JSON and text output: the crates whose
Windows-only code the range changed, each with the reason. A change counts when:

1. **A changed hunk lies inside a Windows item:** a `syn` pass over the old and new file finds items, `impl` blocks,
   statements and `mod`s under `cfg(windows)`, `cfg(target_os = "windows")`, `cfg(target_family = "windows")` or an
   `all`/`any` that requires one of them, and expressions guarded by `cfg!(windows)` (the `if`/`match` arm it selects);
   a hunk overlapping such a span counts. The same pass already exists in shape for `#[cfg(test)]` (ADR 0086 §2).
   A file `syn` cannot parse counts as touched if it contains any Windows conditional (fail toward running).
2. **The path is Windows-only:** `**/windows/**`, `**/win.rs`, `**/*_windows.rs`, `**/*.ps1` inside a crate, and crates
   whose `Cargo.toml` changed a `[target.'cfg(windows)'.*]` table.
3. **It is declared:** `docs/windows-paths.toml` lists modules that are Windows-relevant without a conditional (path
   comparison, Git-bash conversion, CRLF parsing: ER-9's "where paths are compared"), each with a reason. A module
   enters the list when a Windows-only failure is found in it.

Workflow files (`.gitea/**`) are never routed to the VM: their Windows steps are the CI kit's class (incident 1),
checked statically in `ci-actions.test.mjs` and ADR 0080's preflight.

Fixtures for the detector: a hunk inside `#[cfg(windows)] fn`, inside a `cfg!(windows)` arm, in `win.rs`, in a
`[target.'cfg(windows)'.dependencies]` table, in a declared module (all marked); a hunk in a `cfg(unix)` item and in a
neutral file (not marked); an unparsable file with a Windows conditional (marked).

### 4. Positive controls (ER-33)

A run counts only when its controls pass; otherwise it reports `check broken`, never green.

| Control | What it shows |
|---|---|
| A canary crate `tools/paddock-dev/tests/fixtures/wintest-canary` is always added to the archive: one test that passes only when `std::env::consts::OS == "windows"` and prints the guest build, one `#[cfg(windows)]` test that must be **reported failed** (it asserts false), and one that reads a checked-in text fixture and asserts it saw `\r\n` | The tests ran on Windows (not on the host), nextest reports failures (a red test is seen), and the sources arrived with CI's line ends |
| Every requested crate ran at least one test | A crate whose tests were all filtered or gated away is not "green" |
| The archive's target triple equals the requested one, and the guest's `PROCESSOR_ARCHITECTURE` matches `--arch` | The binaries are the ones built for this run |
| Replay controls (WP-3, kept as fixtures, run by `--self-test`): `paddock-proc`'s presence tests from `867add2de` against its parent's `src/` must be red; `from_git_bash("/c", true)` driven through a real Git-bash invocation of a test binary must yield `C:/` | The run catches the classes of incidents 2 and 5 |

### 5. Parallelism and cost (rule 51)

Not measured for this draft (load 55 to 77). Estimates, which WP-0 replaces with a results block in
`docs/benchmarks.md`:

- **Lease time per run**, the scarce part: the recorded VM run of 2026-11-09 held the lock **139 s** for start, copy, one
  single-threaded program, stop and revert to `clean-baseline`
  ([record](../benchmarks.md#2026-11-09-how-a-connection-ends-on-each-os-unread-input-pipe-closehandle-listener-backlog-ordered-close-baseline-featipc-endings-m0)).
  Adding a test run of one to a few crates: **estimated 3 to 6 minutes** of lease per run.
- **Host time**, outside the lease: a warm cross-build of the touched crates' tests is close to the `cargo xwin clippy`
  that `push-gate` already runs (same graph, plus codegen and link), estimated **2 to 10 minutes warm**; a cold build
  folder for an app workspace is tens of minutes the first time (ADR 0025's kept folder per worktree and target).
- **Capacity.** One VM, one run at a time: at 5 minutes a run, about 12 runs an hour. At 2 to 5 Windows-touching
  landings a day and about two runs per landing (first run and one fix round), the daily demand is about 1 hour, and
  about 2.5 hours on a peak day; queue waits stay short except while a release run holds the VM for hours, which ADR
  0074's queue puts first (rule above: review may start, landing waits).
- **More parallelism:** none on the guest side (one VM; a second Windows VM is excluded by the VM's RAM budget and
  `windows-test-vm.md`). Inside the guest, `--test-threads 3`. On the host, the build runs while another agent holds
  the lease.
- **Cheaper:** build outside the lease; run only touched crates; copy only touched workspaces' sources; D5 (b) can skip
  boot and revert between queued runs once WP-0 shows boot's share.

### 6. Relation to other ADRs

- **ADR 0074 (machine leases).** This command is a lease client of `paddock-win11-test` with normal priority (release
  tickets first), activity kept alive by `lease run`'s child, fencing by `PADDOCK_LEASE_TOKEN`, and `clean_state`
  restored as the inventory declares. It adds one inventory field, `test_threads = 3`, read from `conformance/ci/hosts.toml`
  (Part B) instead of a constant once Part B exists. Until Part A lands it uses the interim lock (WP-0 of 0074).
- **ADR 0038 and its 2026-11-10 amendment (push gate).** Not part of `push-gate`: that gate blocks every push and must
  stay fast; xwin clippy (step 5) is already there and covers incident 3's lint. `./dev test --windows` is one of ER-9's
  "slower local checks" and becomes a stage of `push-tests` (A1b) and of `./dev check --pre-push` (§3) when the detector
  marks the change. Its guarded CI-cancel is `push-tests`' own.
- **ADR 0086 (failing-first).** A fix of Windows-only code names a test that runs on Windows; the replay for such a fix
  runs through this command's guest part.
- **ADR 0040 (Windows GUI session).** Tests that need a desktop session are declared `vm: session` and stay with that
  harness; this ADR runs headless tests only.
- **ADR 0080.** The landing run is full on every OS; it stays the enforcement. This ADR moves the first Windows execution
  earlier (ER-36), it does not replace a CI leg.

### 7. Not covered

- Windows host steps of CI workflows (incident 1): a static check per class, in the CI kit (RT-13) and ADR 0080's
  preflight.
- Installers, services, login items and updates on Windows: the vm-smoke harness (ADR 0042, rule 54).
- Differences between the Arm guest and the x86_64 CI runner beyond the architecture (CPU count, Defender exclusions,
  runner account): CI's Windows leg still sees those.

## Alternatives

- **The CI Windows runner for branch runs.** Rejected by the brief and by rule 50: it is CI as the place where failures
  are found, the runner is shared and slow, and agents must never drive it.
- **A Rust toolchain inside the guest, building there.** Rejected for now: the guest has 4 vCPUs and 8 GB, builds would
  hold the lease for tens of minutes, and installing toolchains needs the user's OK. Building on the host keeps signing
  material and caches off the VM (decision of 2026-10-28, "build on host, test in VM").
- **Wine or a Linux container.** Rejected: no NTFS junctions, no real process handles, no SCM; the incidents are exactly
  the OS semantics such layers fake.
- **Only platform parameters (`ctx.windows`) everywhere.** Kept as part of the rule, but alone it tests our model of
  Windows, not Windows (incident 2 needed both).
- **Make `./dev test --windows` blocking in `push-gate`.** Rejected: one VM would serialise every push of every agent.

## Consequences

- A Windows-touching branch costs one VM lease of a few minutes before review, and a host build that `push-gate`'s xwin
  clippy has partly warmed.
- The VM becomes a daily dependency: its health shows in `--self-test`, and a broken VM blocks review of Windows code
  (fallback: the coordinator records "VM down" and the branch's CI Windows leg is dispatched before review, ER-9).
- Tests that were `cfg(unix)` around Windows code surface as "no test ran on Windows" (D7) and need writing.

## Rollback

Delete `tools/paddock-dev/src/wintest/` and the detector mode; restore ADR 0038 §3's `ci-only` Windows row. The VM, its
lock and the lease are untouched (they predate this ADR).

## Decisions for the user

| # | Question | Options | Recommendation |
|---|---|---|---|
| D1 | Target architecture in the VM | (a) `aarch64-pc-windows-msvc`, native in the Arm guest; (b) `x86_64-pc-windows-msvc` under the guest's x64 emulation, the CI runner's triple; (c) both on every run | **(a)** by default, `--arch x64` for code that depends on the architecture; the OS semantics the incidents hit are the same, and emulation slows every run |
| D2 | When it must run | (a) before review for every change the detector marks, re-run after fix rounds that touch Windows code; stamp as a nudge, CI's Windows legs as the enforcement; (b) blocking in `push-gate`; (c) only before landing | **(a)**: it moves the first Windows run to before review without serialising pushes |
| D3 | Detector precision | (a) `syn` spans of Windows items and `cfg!` arms, plus paths, target tables and `docs/windows-paths.toml`; (b) any changed file that contains a Windows conditional (60 of 165 landings instead of about 23 to 60) | **(a)**, with (b) as the fallback for files `syn` cannot parse |
| D4 | nextest in the guest | (a) copy a pinned, hash-checked `cargo-nextest.exe` per run (no change to the VM); (b) install it into a new baseline snapshot (a VM change; needs your OK per the 2026-10-27 decision) | **(a)**; revisit after WP-0 if the copy is a measurable share |
| D5 | VM state between runs | (a) revert to the declared clean state after every lease; (b) when the next queued holder is another `wintest` run and this one released cleanly, keep the guest running and only delete `C:\paddock-test\wintest\<lease id>` | **(a)** now; (b) only if WP-0 shows boot and revert are most of the lease |
| D6 | VM held by a release run | (a) review may start with "Windows run queued"; landing waits for green; (b) review waits | **(a)** (ER-24: tests never crowd out a release, and reviews need not idle) |
| D7 | A touched crate where no test runs on Windows | (a) the run fails ("no test ran on Windows for <crate>"); (b) report only | **(a)**: incident 5's tests were `cfg(unix)` only; the fix is a test, which ER-11 asks for anyway |
| D8 | Tag gating | (a) nothing new gates a tag: CI's full Windows legs on the tagged commit already do; (b) a tag needs a green `--self-test` within N days | **(a)** |
| D9 | Engineering rule | (a) amend ER-9: "Windows-only code runs on a Windows test machine before review; pure Windows logic takes the platform as a parameter"; (b) a new ER | **(a)**: it is ER-9's own sentence about Windows tests, made concrete |

## Work packages (raw agent-days, one agent)

| WP | Content | Days |
|---|---|---|
| WP-0 | Measure by hand under the interim lock, at a load under 25: warm and cold host cross-build of the test archive for `paddock-proc` and `tools/paddock-dev`; the guest part (start, copy, nextest with 3 threads, get, stop, revert); the D5 split; confirm the CRLF tar and `--workspace-remap`; the `867add2de` replay red and green. Results block in `docs/benchmarks.md` (rule 48); `--max` for the wait | 1 |
| WP-1 | `./dev test --windows`: resolve, `cargo xwin env` + `nextest archive`, snapshot and CRLF source tar, `MachineLease` port with the interim adapter, guest part through `vm-win.sh`, JUnit parse, table, `::error::`, stamp, skips declared with reasons; tests with `vm-win.sh`'s fake mode and a fake lease | 2.5 |
| WP-2 | `./dev plan --windows-touch`: the `syn` span pass, paths, target tables, `docs/windows-paths.toml`, JSON; the fixtures of §3 | 1 |
| WP-3 | Positive controls: the canary crate, CRLF fixture, tests-ran and triple checks, `--self-test`, the two replay fixtures | 1 |
| WP-4 | Rule text (ER-9 amendment in engineering-rules, rule 50 (h) and AGENTS.md lines), the review brief and landing checklist lines, ADR 0038 §3 Windows row, the `push-tests` and `--pre-push` stage hook-up (a declared line each) | 0.5 |
| WP-5 | Runtime files per crate (`archive.include`), `vm: session` / `vm: admin` declarations, the first run over every crate with Windows code to list what does not pass yet (report to the coordinator, no fixes) | 1 |
| WP-6 | Swap to ADR 0074 Part A's lease when it lands; delete the interim adapter | 0.5 |
| | **Total** | **7.5** |

First cut (closes incidents 2, 4 and 5 for new changes): WP-0, WP-1 and WP-3 = **4.5 days**; WP-2 and WP-4 follow, so the
rule starts with the detector in place.

## Release impact (ER-31)

No shipped code changes. `tools/paddock-dev` marks the `plan` and `tools` jobs only; it can land during any release. No
tag is gated by this ADR (D8).
