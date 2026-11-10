# 87. Windows code runs before it lands: `./dev test --windows` on the leased test VM, a detector, and positive controls

- Status: Proposed (2026-11-10), revision 2. Drafted at the user's request of 2026-11-10 (ER-21 redesigns). Revision 2
  answers the independent review of revision 1 (`fe314225c`, "changes": P1-1 to P1-5 and the P2s; see "Review answers").
  Draft only: nothing here is built. It needs the user's decisions below, D0 first, and a re-review.
- Date: 2026-11-10
- Related: ER-9 / rules 3, 4 and 50 (h) (fix locally first; "the Windows tests where paths are compared"; the slower local
  checks run beside CI and a local failure cancels the run), ER-11 / rule 5, ER-14 / rule 7, ER-21 / rule 40, ER-24 /
  rule 41 (load), ER-27, ER-31, ER-32 (`./dev wait`), ER-33 / rule 71 (positive controls), ER-36, rule 48; ADR 0021
  (portable tests; its line 38 records the user's 2026-10-27 decision "no Windows test host"), ADR 0022 (Rust tools in
  `./dev`), ADR 0025 and its 2026-11-10 amendment (build folders per worktree), ADR 0038 and its 2026-11-10 amendment
  (`./dev push-gate`, `./dev push-tests`; on `origin/docs/adr-redesigns-2026-11-10`), ADR 0040 (the Windows GUI session),
  ADR 0074 Part A (machine leases, `conformance/ci/machines.toml`), ADR 0080 (the landing run is full on every OS), ADR
  0086 (failing-first replay), ADR 0088 (ratchets; the enrolment list of §2 is one); the Rust tooling plan
  `docs/plans/rust-tooling-2026-11-10.md` §3 (package ownership; on `origin/docs/rust-tooling-plan`);
  `docs/testing/windows-test-vm.md`.
- Number: 0087 was free on every `origin` branch on 2026-11-10 (`git ls-tree -r --name-only` over every
  `refs/remotes/origin/*` after `git fetch`: family ADRs end at 0086, on `origin/docs/adr-redesigns-2026-11-10`).
- Evidence read at `origin/train/2026-11-09-3` (`67a633ef3`) and the branches named below. Nothing was run on the test VM
  for this draft: the 1-minute load was 55 to 77 while it was written, above `vm-win.sh`'s start limit of 25 and ER-24's
  hold level. Every cost figure below is therefore an estimate, and WP-0 measures it before anything else is built.

## Context: the incidents (2026-11-10)

Windows-only code was type-checked (`cargo xwin check`/`clippy`) and never run before CI, or before a reviewer read it.

| # | Where | What broke, or would have | Seen by | Would a local Windows test run have caught it? |
|---|---|---|---|---|
| 1 | `dyno-release.yml` run 7880, job 34520; fix `89a9f2352` | On the Windows host runner `bash` resolved to WSL because a bash step ran before the Git-bash step; the job died before checkout | CI | **No.** A workflow step order on a CI host. `89a9f2352` added the static check (`conformance/test/ci-actions.test.mjs`, "no Windows host job runs a bash step before Git bash is on PATH"). Out of scope (§8) |
| 2 | `origin/feat/rt1-ci-isolation` `7cf714872`, `69700f26d` | Git-bash spellings (`/c/x`, and `/c` alone) were not converted to drive paths for `--source` and `owns`; the code read `cfg!(windows)` directly, so no test on macOS or Linux reached it | review, then a fix that made the platform a parameter (`ctx.windows`) | **Yes**, for the real hand-off: Git bash converts arguments before a native program sees them, which only a run under Git for Windows shows. The parameter covers the pure part on every OS |
| 3 | `origin/feat/rt2-release-isolation` `ad9d85a5b` | A `needless_return` only `cargo xwin clippy` reported, and an untested junction branch (`mklink /J`, reparse points) | review | Clippy: **no**, that is ADR 0038 amendment step 5. Junction: **yes**, a junction exists only on NTFS |
| 4 | `origin/feat/rt4-hook` (`tools/paddock-dev/src/hook/install.rs:28`, `claude-pre-tool-use.sh:5`) | The installed hook binary is `paddock-dev.exe` on Windows and the shim falls back to `$b.exe`; neither name was ever run on Windows | review | **Yes**, with an install-and-run test on the VM |
| 5 | `origin/fix/liveness-broken-sites` `867add2de` | `pid_exists` treated a process as alive while an `OpenProcess` handle kept its object; the presence tests in `paddock-proc`'s tree module were `cfg(unix)` only | review | **Yes**, once the tests are not `cfg(unix)`; `867add2de` made them run on Windows. Nothing forces a Windows test to exist today (D7) |

Three and a half of five would have been caught by running the touched crates' tests on Windows before review (2, 4, 5
and the junction half of 3); half of 3 by `push-gate`'s xwin clippy (accepted 2026-11-10); 1 is a CI workflow class that
no local test run sees.

## Pattern

**Windows-only code reaches review and CI having only compiled.** Whenever a change touches code that runs only on
Windows (`cfg(windows)`, `cfg(not(unix))`, `cfg!(windows)`, `target_os = "windows"`, files under such a `mod`, Windows
paths and line ends), its first execution is a CI Windows job or a user's PC, because the developer Mac cannot run it and
no local step asks for it.

**Invariant.** A change that touches Windows-only code in an enrolled crate has run that crate's tests on Windows before
review, on the leased test VM, with runtime controls showing the run was on the expected Windows build and machine, that
the tests ran, and that a failure is seen; CI stays the final gate, not the first runner.

## Instances (counts at `origin/train/2026-11-09-3` `67a633ef3`)

- 323 tracked `.rs` files hold 629 Windows conditionals (`cfg(windows)`, `cfg(all|any(... windows ...))`,
  `cfg(target_os = "windows")`, `cfg!(windows)`): Debrief 95 files, Dyno 81, Pitcrew 27, Pitwall 15, Spotter 11, shared
  crates 74 (`paddock-service` 10, `paddock-proc` 10, `paddock-window` 5, `paddock-updater` 5, `paddock-ipc` 5, ...),
  `tools/` 8. 48 more `.rs` files are Windows by path (`windows/`, `win.rs`, `*_windows.rs`), plus 15 `.ps1` files.
- **Module tree.** 23 `mod x;` declarations sit under a Windows (or `not(unix)`) condition, which makes their whole
  child file Windows-only without any conditional in it (for example `paddock-ipc`'s `endpoint/windows_peer.rs` and
  `debrief-etw`'s `session.rs`); 5 files carry an inner `#![cfg(windows)]`; 54 files use `not(unix)`.
- **Landings:** of the 165 first-parent merges on the train since 2026-10-27, **23** changed a line carrying a Windows
  conditional or a Windows-only file, and **60** changed a `.rs` file that contains one anywhere. That is about 2 to 5
  landings per working day, with peaks (49 landings on 2026-11-09). The module tree raises the line-level count; WP-2's
  detector gives the real figure.
- **Host-baked test inputs** (feasibility, §1.3): 189 `.rs` files use `env!("CARGO_MANIFEST_DIR")` (11 of them in
  `tools/paddock-dev`), which bakes the **host's** absolute path into the test binary at compile time; 93 use
  `include_str!`/`include_bytes!`, which bake the host checkout's bytes (LF) into it. Several `paddock-dev` golden tests
  run `scripts/affected.mjs` as their oracle and so need Node and the repository's git history.
- Today's local Windows path: `scripts/vm-win.sh` (start, `put`/`get`, `ssh`, `stop`, `revert`), the interim lock
  `scripts/vm-lock.sh` / `vm-lock-run.sh` (ADR 0074 WP-0; it has no queue and no release-first order), and an unmerged
  Rust broker `./dev vm` on `origin/feat/dev-vm-broker` (`633cd1da2`, ADR 0054 V-1). Nothing builds a test archive for
  Windows or runs nextest there.
- The VM: Windows 11 Pro on Arm build 26300, 4 vCPU, 8 GB, Git for Windows with `core.autocrlf=true`, Defender on, tests
  as the standard user `paddock-test` in `C:\paddock-test`; never two Windows VMs at once (`windows-test-vm.md`). No Rust
  toolchain, no nextest, no Node in the guest; installing toolchains needs the user's OK (memory, 2026-10-27).

## Decision

### 0. Scope change that needs the user first (D0)

The user decided on 2026-10-27 that there is **no Windows test host** (ADR 0021, line 38: "Before a push, Windows is
checked on the Mac by `cargo xwin clippy`"), and on 2026-10-28 that VMs are for local install, service and update tests
before releases. This ADR reverses the first for unit and integration tests and widens the VM's purpose to a daily
pre-review runner. Nothing below is built before D0 is answered.

### 1. `./dev test --windows [<crate>...]`

A Rust command in `tools/paddock-dev/src/wintest/`, under the Rust tooling plan's ownership rules (§3): the package owns
that folder and its tests; `cli.rs` gets one enum variant and match arm, `lib.rs` one `mod` line, `Cargo.toml` only new
dependencies; it writes no `.sh` or `.mjs`. Without crate names it asks the detector (§3) for the touched enrolled crates
of `merge-base(origin/main, HEAD)..` plus the working tree.

#### 1.1 Steps (build outside the lease, so the one VM is held only for the part that needs it)

1. **Resolve.** Map crates to their Cargo workspace (`crates/`, `apps/<app>/...`, `tools/paddock-dev`; each has its own
   lockfile) and keep only enrolled crates (§2); a touched crate that is not enrolled is listed as "not enrolled", never
   silently skipped (ER-27). Crates whose tests are declared `vm: session` (they need a desktop: ADR 0040), `vm: admin`,
   `vm: node` or `vm: git` (§1.3) are listed with that reason.
2. **Build on the host,** through `heavy` with `CARGO_BUILD_JOBS=2` (ER-24), in the worktree's own build folder for
   that target (ADR 0025 amendment, `./dev target-dir`): the environment from `cargo xwin env --target
   aarch64-pc-windows-msvc` (cargo-xwin 0.23.1 has `env`; both arches are in its cache on the dev Mac), then
   `cargo nextest archive --target aarch64-pc-windows-msvc -p <crates> --locked --archive-file <tmp>/tests.tar.zst`.
   Runtime files (Debrief's DuckDB library and the like) come in through nextest's `profile.<name>.archive.include`.
   Nextest documents that an archive can be built on one OS and architecture and run on another, that the target needs
   nextest but not cargo, that sources are not in the archive, and that `--workspace-remap` points at them
   ([nextest, archiving](https://nexte.st/docs/ci-features/archiving/), read 2026-11-10).
   **This build shares nothing with `push-gate`'s `cargo xwin clippy`**, which checks `x86_64-pc-windows-msvc` and
   produces metadata only: the first run per worktree is a cold aarch64 build (§5).
3. **Sources with CI's line ends.** From a snapshot of the worktree (a temporary index, `git commit-tree`), write the
   touched workspaces' files with `git -c core.autocrlf=true checkout-index -a --prefix=<tmp>/src/` and tar them.
   Verified on a synthetic repository on the dev Mac (git 2.56.0): text files come out CRLF, a `binary` file per
   `.gitattributes` stays LF.
4. **Lease** `paddock-win11-test` and run the guest part as the lease's child, through one port, `MachineLease`:
   - interim adapter: `scripts/vm-lock-run.sh wintest -- <self> test --windows --guest <plan.json>` (ADR 0074 WP-0's
     lock), driving the VM only through `scripts/vm-win.sh`. **The interim lock has no queue and no release-first
     order**: a waiter polls (`PADDOCK_VM_LOCK_WAIT`), and a release run that starts while a `wintest` run holds the VM
     waits for it (at most one run). The coordinator does not start `wintest` runs while a release VM check is
     scheduled;
   - final adapter: ADR 0074 Part A's `./dev machine lease run paddock-win11-test --purpose "<branch> wintest" -- ...`,
     with `PADDOCK_LEASE_TOKEN` checked by every driver call and release tickets first. WP-6 swaps the adapter.
   Never `vmrun` directly, never another VM, never the CI Windows runner (ER-14; `vm-win/target.sh` refuses other
   folders).
5. **Guest part.** `vm-win.sh start` (it refuses at a 1-minute host load of 25 or more; §2's fallback), `put` the
   archive, the source tar and a pinned `cargo-nextest.exe` for aarch64 Windows (version and SHA-256 from ADR 0038 §4's
   `ci/tool-versions.toml`, checked on the host before the copy, D4) into `C:\paddock-test\wintest\<lease id>\`, extract
   the sources there (or at the host path, D10), then over SSH as `paddock-test`:
   `cargo-nextest nextest run --archive-file tests.tar.zst --workspace-remap src --profile wintest
   --test-threads <max_load>` with `PADDOCK_WINTEST_NONCE=<nonce>` in the SSH command's environment. `<max_load>` is
   ADR 0074's `max_load` of the machine in `conformance/ci/machines.toml` (3 for this VM: vCPUs minus one, so SSH keeps
   a core; lesson `9c738b1ad`, ADR 0074 incident I6). The `wintest` profile sets `fail-fast = false`, JUnit output, and
   no retries (ER-12). `get` the JUnit files and nextest's output, `stop`, and `revert` to the clean state ADR 0074
   declares (D5).
6. **Report.** One table per crate: tests run, passed, failed, skipped (declared), duration; failing tests with their
   output tail and the exact re-run command; `::error::` lines when `GITHUB_ACTIONS` is set (ER-27). Exit 0 only when the
   controls of §4 passed, every requested crate ran at least one test and none failed.
7. **Stamp.** A green run writes `<git-common-dir>/paddock/wintest/<key>.json`, where `<key>` hashes the touched crates'
   **inputs** (the planner's input set for those crates: their sources, manifests, lockfile entries and declared
   fixtures), not the whole tree: a later commit that touches only other crates keeps the stamp valid. The stamp holds
   the crates, counts, the guest's build and machine, the nonce, lease id and durations. It is a **nudge**, not
   enforcement (an agent can write a file); CI's Windows legs stay the enforcement.
8. **Waits.** Agents run it in the background and wait with `./dev wait --pid <pid> --max <m>` (ER-32), `--max` set from
   WP-0; a queued lease prints its place and the holder's purpose.

`--self-test` runs only the controls (§4). `--arch x64` builds `x86_64-pc-windows-msvc` and runs it under the guest's x64
emulation (Prism).

#### 1.2 x64-only code

The detector (§3) also marks code under `target_arch = "x86_64"` (with a Windows condition) and Windows machine
constants (`IMAGE_FILE_MACHINE_*`, `PROCESSOR_ARCHITECTURE_*`). When such code is touched, the run adds an `--arch x64`
pass for the crates concerned; when it cannot (the crate is declared `x64: no`), the report says "x64 paths not run" and
the review is told (D1).

#### 1.3 Tests that bake the host into the binary (feasibility)

`env!("CARGO_MANIFEST_DIR")` is fixed at **compile time on the host**, so a test that builds a fixture path from it looks
for `/Volumes/.../crates/x/tests/data` inside Windows, and `--workspace-remap` does not change that (it remaps what
nextest itself sets at run time, `CARGO_MANIFEST_DIR` in the environment). `include_str!`/`include_bytes!` bake the
host checkout's bytes (LF), so a CRLF test reading such a fixture tests LF. Choices (D10):

- (a) **extract at the host's path:** put the sources at `C:\<host path with / as \>` (for example
  `C:\Volumes\d7sst7\dev-targets\wt\x\...`) so a baked absolute path resolves on drive C:. Works without code changes
  for many tests; brittle for code that checks for a drive prefix, and the path is long (MAX_PATH);
- (b) **migrate fixtures to `std::env::var("CARGO_MANIFEST_DIR")`** (set by nextest at run time and remapped), crate by
  crate as each enrols (§2). 189 files today, mostly one line per fixture helper; `paddock-testkit`'s golden helper is
  the shared place for it.

Recommendation: (b) for every crate as it enrols, with (a) only as a stop-gap per crate during WP-0. Tests that need
Node (`paddock-dev`'s golden tests run `scripts/affected.mjs`) or the repository's git history are declared `vm: node` /
`vm: git` and skipped with that reason until the guest has Node (a D0 follow-up), and `include_*!` fixtures whose line
ends matter move to run-time reads. Cost: §5 and WP-5.

### 2. Enrolment, the rule, and the fallback

**Enrolment, a ratchet.** A crate is in `docs/wintest-crates.toml` (a list under ADR 0088's engine, growing only by
enrolment) only after one green `./dev test --windows -p <crate>` baseline run on the train tree, with its declared
skips. The detector marks only enrolled crates as needing the run; touched non-enrolled crates are reported. Order: the
crates of the incidents first (`paddock-proc`, `tools/paddock-dev`, `paddock-ipc`), then shared crates with Windows
code, then apps by their next release. An enrolled crate leaves only with a recorded reason.

**Rule (proposed; amends ER-9 and rule 50 (h); the coordinator numbers any new Paddock rule):**

> **Windows-only code runs on Windows before review.** A change that the detector marks as touching Windows-only code
> in an enrolled crate runs `./dev test --windows` for those crates, green, before it is handed to review; a fix round
> that touches such code again runs it again. Windows logic that can be pure takes the platform as a parameter (as
> `ctx.windows` in `ci_isolation`), so every OS tests it; the VM run covers what only Windows does. A touched enrolled
> crate in which no test runs on Windows fails the run (D7). The run may overlap CI (ER-9): a red Windows run cancels
> the branch's CI run.

**Fallback when the VM cannot run it.** When the run has not started within **N minutes** (D6; proposed 30) because the
host's load keeps `vm-win.sh` from starting the guest, or the lease is held (a release run, another agent), or the VM is
broken, the command dispatches the branch's **CI Windows leg** for the touched crates (the affected plan restricted to
the Windows job, rule 50: one run per head), prints that it did and why, and review may start; the branch lands only
after either run is green. This bounds the wait and never makes the VM a single point of failure.

**Priority.** A fix on a release branch (`release/**`, `<app>/x.y-rc`) gets the release priority of ADR 0074's queue (in
the interim: the coordinator holds other `wintest` runs while a release-branch fix waits).

**Where it is written.** The review brief template gets one line ("Windows run: stamp `<key>`, CI Windows leg `<run>`,
or not needed per `./dev plan --windows-touch`"), the integrator's landing checklist reads it, and ADR 0038 §3's Windows
row changes from "tests and bundles are `ci-only`" to "tests of touched enrolled Windows crates: `./dev test --windows`
(ADR 0087), CI fallback after N minutes; bundles stay `ci-only`".

### 3. The check: `./dev plan --windows-touch`

A detector in the planner (the T0 Rust planner; one new mode, its own module), JSON and text output: the crates whose
Windows-only code the range changed, each with the reason. A change counts when:

1. **A changed hunk lies inside a Windows span.** A `syn` pass over the old and new file finds items, `impl` blocks,
   statements, expressions and `mod`s under a **Windows predicate**: `windows`, `target_os = "windows"`,
   `target_family = "windows"`, `not(unix)`, `not(target_family = "unix")`, and any `all`/`any`/`not` that is true on
   Windows and false on at least one other target Paddock builds (macOS, Linux) when evaluated over those three;
   `cfg!(...)` with such a predicate marks the `if`/`match` arm it selects; `#[cfg_attr(<windows>, ...)]` marks the
   item. The same pass exists in shape for `#[cfg(test)]` (ADR 0086 §2).
2. **The module tree.** The crate's module tree is resolved from its roots (`lib.rs`, `main.rs`, `bin/*`, `tests/*`),
   following `mod x;` and `#[path = "..."] mod x;`: a `mod` under a Windows predicate (or a `#[path]` to a file that only
   a Windows `mod` reaches) marks the **whole child file and its own children**, as does an inner `#![cfg(<windows>)]`.
   23 such `mod`s and 5 inner attributes exist today.
3. **The path is Windows-only:** `**/windows/**`, `**/win.rs`, `**/*_windows.rs`, `**/*.ps1` inside a crate, and crates
   whose `Cargo.toml` changed a `[target.'cfg(windows)'.*]` table.
4. **It is declared:** `docs/windows-paths.toml` lists modules that are Windows-relevant without a conditional (path
   comparison, Git-bash conversion, CRLF parsing: ER-9's "where paths are compared"), each with a reason.
5. A file `syn` cannot parse counts as touched if it contains any Windows predicate (fail toward running).

`x86_64` marks (§1.2) are reported beside. Workflow files (`.gitea/**`) are never routed to the VM (§8).

**Fixtures** (one each, marked or not as stated): a hunk inside `#[cfg(windows)] fn`; inside `#[cfg(not(unix))]`;
inside `#[cfg(all(windows, feature = "x"))]`; under `#[cfg(any(windows, target_os = "macos"))]` (marked: true on Windows,
false on Linux); under `#[cfg(not(windows))]` (not marked); inside a `cfg!(windows)` arm and outside it; in a file reached
only through `#[cfg(windows)] mod x;`; in a grandchild of such a file; in a file named by `#[cfg(windows)] #[path =
"w.rs"] mod x;`; in a file with `#![cfg(windows)]`; under `#[cfg_attr(windows, ...)]`; in `win.rs`; in a
`[target.'cfg(windows)'.dependencies]` table; in a declared module; under `#[cfg(all(windows, target_arch = "x86_64"))]`
(marked, x64 pass); in a `cfg(unix)` item and a neutral file (not marked); an unparsable file with a Windows predicate
(marked).

### 4. Positive controls (ER-33)

A run counts only when its controls pass; otherwise it reports `check broken`, never green. The controls assert
**runtime** facts, not facts fixed when the binary was compiled.

| Control | How | What it shows |
|---|---|---|
| **Nonce** | The host draws a random nonce per run and passes it only through the SSH command's environment (`PADDOCK_WINTEST_NONCE`); the canary test prints it into its JUnit `system-out`; the host compares | The JUnit file comes from this run in the guest, not from a cached or host-side run |
| **Build** | The canary calls `RtlGetVersion` (not `GetVersionEx`, which reports an older version without a manifest) and prints major, minor and build; the host compares the build with the machine's declared build in `machines.toml` (26300 today) | The tests ran on the declared Windows guest |
| **Machine** | The canary calls `IsWow64Process2` and prints the process machine and the native machine; for `--arch aarch64` the native machine must be ARM64 and the process not emulated (process machine `IMAGE_FILE_MACHINE_UNKNOWN`), for `--arch x64` the binary is AMD64 code on an ARM64 native machine (WP-3 records what `IsWow64Process2` reports for an x64 process under Prism before the assertion is fixed) | The binary built for this run ran natively or under emulation as requested |
| **A failure is seen** | The canary runs as its **own nextest invocation** before the real run (`--profile wintest-canary`), with one test that fails on purpose; the host expects exactly one failure, of that test, and every other canary test passed | nextest's failures reach the host's report |
| **Line ends** | A canary test reads a checked-in text fixture **at run time** (`std::env::var("CARGO_MANIFEST_DIR")`) and asserts it saw `\r\n` | The sources arrived with CI's line ends |
| **Tests ran** | Every requested crate ran at least one test; the triple in the archive equals the requested one | A crate whose tests were all filtered or gated away is not "green" |
| **Replays** (WP-3; `--self-test`) | `paddock-proc`'s presence tests from `867add2de` against its parent's `src/` must be red; `from_git_bash("/c", true)` driven through a real Git-bash invocation of a test binary must yield `C:/` | The run catches the classes of incidents 2 and 5 |

The canary is a package of its own, `tools/wintest-canary` (its own tiny workspace and lockfile, `windows-sys` only), and
gets its own archive: a nextest archive covers one Cargo workspace, so the canary cannot sit inside an app workspace's
archive, and putting it there would add a member to every app. Its build is cached by input hash.

### 5. Parallelism and cost (rule 51)

**Not measured.** The only VM figure on record is a note, not a results row: the 2026-11-09 endings run says the VM was
used "139 s with the lock held" for start, copy, one single-threaded program, stop and revert
([benchmarks.md, 2026-11-09 endings block, "Notes"](../benchmarks.md#2026-11-09-how-a-connection-ends-on-each-os-unread-input-pipe-closehandle-listener-backlog-ordered-close-baseline-featipc-endings-m0));
it was one run with no stated load. No number below is evidence for a decision until WP-0 records a results block in
`docs/benchmarks.md` (rule 48). WP-0 measures:

- **Lease time per run:** start, copy, canary invocation, test run, get, stop, revert, for `paddock-proc` (small),
  `tools/paddock-dev` (11 `env!` fixture files, the D10 case) and **one Debrief crate with Windows code and runtime
  files** (`debrief-etw` or `debrief-store-duck`);
- **Archive size** and copy time per crate (scp over the NAT), the source tar size;
- **Defender's share**: real-time scanning of each extracted test binary on first run (Defender stays on, like CI);
- **Host build:** cold and warm aarch64 test build per crate (the first run per worktree is cold: nothing is shared with
  `push-gate`'s x64 clippy);
- **D5:** boot and revert as a share of the lease.

Estimates until then (not evidence): a few minutes of lease per run for a small shared crate, more for an app crate with
runtime files; host builds of minutes warm and tens of minutes cold.

**How many runs.** Per branch: one run before review, one per fix round that touches Windows code again, plus the
`push-tests` stage after a push of such code (A1b of ADR 0038's amendment) only when the stamp's key changed (a push
whose touched crates' inputs are unchanged reuses the stamp). **Trains:** an integrator's train push does not run it;
the merged branches' stamps count when their keys still match the train's inputs for those crates, and a crate whose key
changed in the merge (a resolution touched it) is run once on the train. With 2 to 5 Windows-touching landings a day
and about two runs each, demand is 4 to 10 runs a day; capacity follows from WP-0's lease time (one VM, one run at a
time). If WP-0 shows the queue would exceed about an hour on a normal day, enrolment (§2) slows instead of the queue
growing.

**More parallelism:** none on the guest side (one VM; a second Windows VM is excluded by its RAM budget and
`windows-test-vm.md`); `max_load` threads inside it; the host builds while another agent holds the lease.
**Cheaper:** build outside the lease; only touched enrolled crates; only their workspaces' sources; stamps keyed on
inputs; D5 (b) if boot dominates.

### 6. Relation to other ADRs

- **ADR 0074 (machine leases).** A lease client of `paddock-win11-test` with normal priority (release tickets first;
  release-branch fixes get release priority, §2), activity kept alive by `lease run`'s child, fencing by
  `PADDOCK_LEASE_TOKEN`, `clean_state` restored as `machines.toml` declares, `max_load` read from it. No new field.
  Until Part A lands, the interim lock (no queue, no priority).
- **ADR 0038 and its 2026-11-10 amendment (push gate).** Not in `push-gate`, which blocks every push and must stay fast;
  its xwin clippy (step 5) covers incident 3's lint. `./dev test --windows` is one of ER-9's slower local checks, a stage
  of `push-tests` (A1b, only when the stamp key changed) and of `./dev check --pre-push` (§3) when the detector marks the
  change.
- **ADR 0088 (ratchets).** `docs/wintest-crates.toml` is a list of that engine (an enrolled crate that no longer exists
  is stale).
- **ADR 0086 (failing-first).** A fix of Windows-only code names a test that runs on Windows; its replay runs through
  this command's guest part.
- **ADR 0040 (Windows GUI session).** Tests that need a desktop are `vm: session` and stay with that harness.
- **ADR 0080.** The landing run is full on every OS and stays the enforcement; this ADR moves the first Windows execution
  earlier (ER-36).

### 7. Release impact (ER-31)

- No shipped code changes; `tools/paddock-dev` and `tools/wintest-canary` mark the `plan` and `tools` jobs only. The
  detector's fixtures and the enrolment list are outside every app's dependency set.
- **Nothing here gates a tag** (D8): CI's Windows legs on the tagged commit remain the release gate. A missing stamp is
  never a release blocker; a red `./dev test --windows` on a release branch is a finding like a red local test.
- The VM is shared with the release VM checks (vm-smoke, rule 54); ADR 0074's queue puts release runs first and the
  fallback (§2) keeps branch work moving while a release holds the VM.
- Fixes on a release line get release priority (§2) and may use the CI fallback at once when a release run holds the VM.

### 8. Not covered

- Windows host steps of CI workflows (incident 1): static checks in the CI kit (RT-13) and ADR 0080's preflight.
- Installers, services, login items and updates on Windows: the vm-smoke harness (ADR 0042, rule 54).
- Differences between the Arm guest and the x86_64 CI runner beyond what `--arch x64` emulates (CPU count, Defender
  exclusions, the runner account): CI's Windows leg still sees those.

## Alternatives

- **The CI Windows runner for every branch run.** Rejected as the primary path (CI as the place failures are found,
  rule 50); kept as the bounded fallback (§2).
- **A Rust toolchain inside the guest, building there.** Rejected for now: 4 vCPUs and 8 GB; builds would hold the lease
  for tens of minutes; installing toolchains needs the user's OK; building on the host keeps signing material and
  caches off the VM (decision 2026-10-28).
- **Wine or a Linux container.** Rejected: no NTFS junctions, no real process handles, no SCM.
- **Only platform parameters (`ctx.windows`).** Kept as part of the rule, but alone it tests our model of Windows.
- **Blocking in `push-gate`.** Rejected: one VM would serialise every push.
- **Every crate at once.** Rejected (review P1-4): crates with host-baked fixtures or Node-dependent tests would be red
  on day one; enrolment after a green baseline instead.

## Consequences

- A Windows-touching branch in an enrolled crate costs one VM lease before review, and a cold aarch64 build the first
  time per worktree.
- Enrolling a crate means making its fixtures run-time reads (D10 (b)); that work is real and is estimated (WP-5).
- The VM becomes a daily dependency; the CI fallback bounds the damage when it is down or busy.

## Rollback

Delete `tools/paddock-dev/src/wintest/`, `tools/wintest-canary`, the detector mode and `docs/wintest-crates.toml`;
restore ADR 0038 §3's `ci-only` Windows row. The VM, its lock and the lease predate this ADR and stay.

## Decisions for the user

| # | Question | Options | Recommendation |
|---|---|---|---|
| D0 | Reverse the 2026-10-27 "no Windows test host" decision (ADR 0021:38) and widen the test VM's purpose from release checks to a daily pre-review runner | (a) yes, for enrolled crates, with the CI fallback; (b) no: keep xwin clippy only and rely on CI's Windows legs | **(a)**: three and a half of the five incidents of one day were run-time faults that clippy cannot see |
| D1 | Target architecture | (a) `aarch64` native by default, an automatic `--arch x64` pass for touched `target_arch = "x86_64"` and machine-constant code, "x64 paths not run" when a crate cannot; (b) always x64 under emulation; (c) both on every run | **(a)** |
| D2 | When it must run | (a) before review for marked changes in enrolled crates, re-run after Windows fix rounds; stamp as a nudge, CI's Windows legs as the enforcement; (b) blocking in `push-gate`; (c) only before landing | **(a)** |
| D3 | Detector precision | (a) `syn` spans with the module tree, `not(unix)`, inner attributes, paths, target tables and `docs/windows-paths.toml`; (b) any changed file that contains a Windows predicate | **(a)**, (b) as the fallback for unparsable files |
| D4 | nextest in the guest | (a) copy a pinned, hash-checked binary per run (no VM change); (b) install it into a new baseline snapshot (a VM change; needs your OK) | **(a)** |
| D5 | VM state between runs | (a) revert to the declared clean state after every lease; (b) keep the guest running between queued `wintest` runs | **(a)** now; (b) only if WP-0 shows boot and revert dominate |
| D6 | Fallback | (a) after N minutes without a start (load, lease, broken VM) dispatch the branch's CI Windows leg for the touched crates, review may start, landing waits for either green run; N = 30; (b) wait for the VM | **(a)**, N = 30, revisited after WP-0 |
| D7 | A touched enrolled crate where no test runs on Windows | (a) the run fails; (b) report only | **(a)** |
| D8 | Tag gating | (a) nothing new gates a tag; (b) a tag needs a green `--self-test` within N days | **(a)** |
| D9 | Engineering rule | (a) amend ER-9: "Windows-only code runs on a Windows test machine before review; pure Windows logic takes the platform as a parameter"; (b) a new ER | **(a)** |
| D10 | Host-baked fixtures (`env!("CARGO_MANIFEST_DIR")` in 189 files, `include_*!` in 93) | (a) extract sources at the host's absolute path on C:; (b) migrate each crate's fixtures to run-time `env::var` as it enrols; (c) both: (a) as a per-crate stop-gap, (b) as the rule | **(c)** |
| D11 | Enrolment | (a) crate by crate after a green baseline, incident crates first, a ratchet list under ADR 0088; (b) every crate with Windows code at once | **(a)** |

## Work packages (raw agent-days, one agent)

| WP | Content | Days |
|---|---|---|
| WP-0 | Measure by hand under the interim lock, at a load under 25: §5's list for `paddock-proc`, `tools/paddock-dev` (with its `env!` fixtures, trying D10 (a)) and one Debrief crate with runtime files; archive and tar sizes; Defender's share; cold and warm aarch64 builds; the `867add2de` replay. Results block in `docs/benchmarks.md` (rule 48); `--max` and N for D6 | 1.5 |
| WP-1 | `./dev test --windows`: resolve, `cargo xwin env` + `nextest archive`, snapshot and CRLF source tar, `MachineLease` port with the interim adapter, guest part through `vm-win.sh`, nonce, `wintest` profile (`fail-fast = false`), JUnit parse, table, `::error::`, input-keyed stamp, declared skips, the CI fallback dispatch; tests with `vm-win.sh`'s fake mode and a fake lease | 3 |
| WP-2 | `./dev plan --windows-touch`: `syn` spans, predicate evaluation, module-tree resolution with `#[path]`, inner attributes, `cfg_attr`, paths, target tables, `docs/windows-paths.toml`, x64 marks, JSON; the fixtures of §3 | 2 |
| WP-3 | Controls: `tools/wintest-canary` (nonce, `RtlGetVersion`, `IsWow64Process2`, the deliberate failure, CRLF read), its own archive and invocation, tests-ran and triple checks, `--self-test`, the two replays | 1.5 |
| WP-4 | Rule text (ER-9 amendment, rule 50 (h), AGENTS.md), review brief and landing checklist lines, ADR 0038 §3 row, the `push-tests` and `--pre-push` stages (declared lines) | 0.5 |
| WP-5 | Enrolment of the first three crates (`paddock-proc`, `tools/paddock-dev`, `paddock-ipc`): fixtures to run-time reads (D10 (b)), `vm: node` / `vm: git` declarations, runtime files, green baselines; `docs/wintest-crates.toml`. Later crates: about 0.25 to 1 day each by their fixture count, booked with the owning app's batches | 2 |
| WP-6 | Swap to ADR 0074 Part A's lease when it lands; delete the interim adapter | 0.5 |
| | **Total (first three crates enrolled)** | **11** |

First cut (closes incidents 2, 4 and 5 for the crates involved): WP-0, WP-1, WP-3 and WP-5 = **8 days**; WP-2 and WP-4
follow, so the rule starts with the detector in place.

## Review answers (revision 1, `fe314225c`)

| Finding | Answer in revision 2 |
|---|---|
| P1-1 detector misses the module tree, `not(unix)`, inner attributes | §3 items 1, 2 and 5; 23 gated `mod`s and 5 inner attributes counted; a fixture per case |
| P1-2 controls assert compile-time facts | §4: nonce through the SSH environment into JUnit, `RtlGetVersion` against the declared build, `IsWow64Process2`, the canary as its own invocation expecting exactly its one failure, `fail-fast = false`, the canary as its own package |
| P1-3 host-baked fixtures, git and Node | §1.3, D10, WP-0 runs `tools/paddock-dev`, WP-5 estimates the migration; `vm: node` / `vm: git` declarations |
| P1-4 gating, fallback, priority, ER-31 | §2 enrolment (D11), CI fallback after N minutes (D6), release-branch priority, §7 |
| P1-5 cost claims | the warm-reuse claim removed (§1.1 item 2); WP-0 measures a Debrief crate, archive size and Defender; run counts for branches, trains and `push-tests`; stamp keyed on the touched crates' inputs |
| P2 x64-only code | §1.2, D1 |
| P2 `max_load`, interim queue | §1.1 items 4 and 5, §6 |
| P2 D0 | D0 and §0 |
| P2 the 139 s figure (rule 48) | §5 quotes it as the block's note, not as a result, and WP-0 records a results block |
