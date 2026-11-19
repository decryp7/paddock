# 0109. One portable layer owns every OS touchpoint; the Mac and a CI-matched Linux container catch the rest before a push

- Status: Proposed 2026-11-20 (researcher, for the user's decision). Nothing is built.
- Date: 2026-11-20
- Related: research note [2026-11-20-os-only-failures](../research/2026-11-20-os-only-failures.md) (the evidence, the counts
  and the sources); ER-9 (fix locally first; Linux before a push; a different machine counts only against its own base);
  ER-14 (containers only through the helper); ER-21 (a recurring class gets a redesign); ER-22 (share first); ER-33 (positive
  controls); ER-37 (common practice and cost); ER-41 (new checks start report-only); ER-45 (checks have no side effects);
  ER-47 (children inherit the environment); ADR 0021 (portable tests, trimmed 2026-11-11: C38 dropped); ADR 0033 (reusable OS
  names); ADR 0099 (paths compared as paths, C122); ADR 0100 (one process builder for tests); ADR 0103 (bounded steps, reaped
  processes); ADR 0107 (a test's hermetic environment, proposed); ADR 0108 (bulk APIs and start budgets).
- Number: 0109. On 2026-11-20 the highest ADR on any `origin` branch was 0108 (`git ls-tree --name-only <branch> docs/adr/`
  over every remote branch).
- **Heads read** (ADR 0091): `origin/main` 9cbd77a6365b, `origin/train/2026-11-19-16` 937d565fe0e0,
  `origin/train/2026-11-19-17` e80df3371, `origin/fix/train15-ci-8327` 00a224e50, all on 2026-11-20 and re-read before the
  commit.

## Context

From 2026-10-18 to 2026-11-20, 275 commits fixed 259 failures that showed only on Windows (161), only on Linux (96) or on both
but not on the Mac (2). The 22 classes and their counts are in the research note, section 2. The same classes keep coming
back. Paths compared as text failed in seven runs, from 7485 to 8327. CRLF caused 13 failures, the Linux container's own
environment 16, Windows spawn cost 5, and code that only the other target compiles 29. Several mechanisms already exist and
work where they are used: `winpath` and C122, the process builder, `text_busy` and `write_program`, `paddock_jobs::time`, and
the xwin and Linux Clippy rows. But nothing makes them the only way. And the Linux container runs Clippy only, never tests.

The user asked (2026-11-20) for a fix "once and for all" so that running on the Mac is enough. The honest answer is that
about 72% can be caught or prevented on the Mac, and about 28% need the real OS. This ADR closes the 72% by construction or by
a check, and leaves the rest to the member checks on real runners and the staged CI, which are being built separately.

## Decision

### 1. One portable layer

Every OS touchpoint has one owner. Code outside these modules does not reach the OS directly. The layer is not a new crate:
it is the set of modules below, most of which exist, plus the missing parts.

| Touchpoint | Owner (exists) | Missing (this ADR) | Classes closed |
|---|---|---|---|
| Starting programs | `paddock_env::command::command` / `paddock_proc::command`; tests: `paddock_testkit` `TestCommand` (ADR 0100); clippy bans `Command::new` in `crates/`, `tools/paddock-dev`, `lint/` and each app | every child gets `MSYS2_ARG_CONV_EXCL=*`, `MSYS2_ENV_CONV_EXCL=*` and `MSYS_NO_PATHCONV=1`; `.cmd`/`.bat` resolved through PATHEXT (as `npm.cmd`, eba8b9335); a shebang script on Windows is an error that names the cause; the command line is measured and refused above 8,191 (cmd) or 32,767 characters on every OS; a per-test start budget (ADR 0108) | MSYS, exec, command line, spawn cost |
| Starting programs from Node | none (382 raw `spawn`/`execFile` sites in `*.mjs`/`*.ts`) | one helper in `conformance/src/util/` with the same environment and length rules; ESLint `no-restricted-imports` of `node:child_process` outside it | MSYS and exec in scripts |
| Ending processes, process trees, Windows job objects, handle inheritance | `paddock_proc::end`, `kill_tree`, `tree::group_exists`, `job.rs`, `ProcessId` (ADR 0033); `tools/paddock-dev/src/fds.rs` | nothing | signals, handles, zombies (`group_exists` reads `/proc`) |
| Writing programs a test runs | `paddock_testkit::exe::write_program`, `exe::spawn`; `paddock_env::text_busy::retry` (5e4d80098) | StandIn writes a `.cmd` twin on Windows; StandIn tokens are declared per OS (`{nice}` is Unix only, ded73e60f) | ETXTBSY, exec |
| Paths | `paddock_env::winpath` (`real`, `real_for_tools`, `real_verbatim`); `paddock_testkit::path` (`same_path`, `under`, `text`; ADR 0099); clippy bans `canonicalize` | stand-in output and captured child output compared through `paddock_testkit::path::text` (the 8327 gap); test paths only from the testkit (no Unix-absolute literals) | paths, case |
| Temp folders | `paddock_testkit::scratch`, `private_temp` | tests take temp folders only from the testkit (60 `env::temp_dir(` and 135 `"/tmp` sites, upper bounds) | paths, Windows TMP ACL in tests |
| Replacing and removing files | `paddock_env::atomic`; paddock-dev `safe_remove` | one `paddock_env::fs` module: opens with `FILE_SHARE_READ`, `FILE_SHARE_WRITE` and `FILE_SHARE_DELETE` on Windows, removes with POSIX semantics where the OS has them, retries a sharing violation within a bound, sets folder times with `FILE_FLAG_BACKUP_SEMANTICS`; a strict test mode on every OS that refuses to remove or rename a file the layer still holds open, so the Windows rule fails on the Mac | locks, folder times |
| Line endings | `.gitattributes` (`*.rs`, `*.sh`, fixtures, Linux packaging LF) | `* text=auto eol=lf` for all text, with `*.cmd` CRLF and byte-exact fixtures as named exceptions (Deno's rule); a `paddock_env::text::lines` reader for repository text that a test splits | CRLF |
| Environment | `paddock_env::vars`, `paddock_proc::clean_env` (ER-47); `paddock_testkit::EnvLock` (ADR 0100); ADR 0107 (proposed) | a named Windows keep-list in `clean_env` (`SystemRoot`, `PATHEXT`, `ComSpec`, Git's `usr/bin` for tests) | environment (Windows part) |
| Clocks | `paddock_jobs::time` (`Clock`, `SystemClock`, `FakeClock`, `Deadline`); C30 (report-only, allow list `conformance/c30-allow.json`) | the app copies (`debrief` jobs and agent, `dyno-agent` disk centre, `dyno-plugin-host`, `spotter-engine`) re-export the shared clock (rule 55 entries); C30 blocks | timing (the part a fake clock can model) |
| Signals versus Windows console events | `paddock_proc` `console.rs`, `end` | nothing | |
| Fonts in e2e | system fonts through `--pd-font-sans` | an e2e project on the Mac with a pinned wide fallback font, so text that overflows on Linux overflows on the Mac too; the e2e suites in the arm64 container (WebKit and Playwright's Chromium for Linux arm64; Chrome for Testing has no Linux arm64 build) | fonts, platform detection |
| Repeated Windows API shapes | `paddock-proc` console and job code | small helpers, each tested on Windows: a loopback connect to a refused port fails fast (the 2 s retry, five instances), a process list leaves out `conhost.exe` children, `set_len` makes a sparse file | Windows API (part) |

### 2. Enforcement

Each ban names the layer function in its reason text, as the existing clippy bans do. Each new check gets its id from
`./dev check new-id --reserve <branch>`, starts report-only, and blocks after its first green run on the real tree (ER-41).
Its allow list may only shrink (ER-46).

| Check | Bans, outside the layer | Today's sites (upper bound, section 7 of the note) | Positive control (ER-33) |
|---|---|---|---|
| clippy `disallowed-methods` (extend `crates/clippy.toml`, `lint/clippy.toml`, `tools/paddock-dev/clippy.toml`, each app's) | `std::env::temp_dir`, `std::fs::remove_file`, `remove_dir_all` and `rename` in test targets (via `clippy.toml` per tests crate or a test-only lint list), `std::fs::File::set_modified` on folders | 60 + 102 + 153 + 11 in test paths | a planted test file calling each banned function makes `cargo clippy` red; removing the ban makes it green |
| C122 extended | path text in expected stand-in output, Unix-absolute string literals (`"/srv`, `"/Users`, `"/tmp`) in Rust tests, `.replace('\\', "/")` | 135 `"/tmp` (24 in test paths) and others | the 8327 test at a720dd85d's parent is flagged; the fixed version is not |
| new: process environment | a child started anywhere without the MSYS variables; a Node `node:child_process` import outside the helper (ESLint) | 382 Node sites (265 in test paths) | a planted spawn without the helper is flagged; a unit test of the builder asserts the three variables are set |
| new: line endings | a text file in the index with CR (`git ls-files --eol` reports `i/crlf` outside the named exceptions) | to be measured at the renormalisation | a planted CRLF file in the index is flagged |
| C30 to blocking | wall-clock assertions in tests | the current allow list | the existing C30 tests |
| start budget (ADR 0108's counted budget, per test) | a test that starts more than N programs without a declared budget | to be measured on the tree | a planted test starting N+1 stand-ins fails on the Mac |

### 3. Tests

- **The layer runs on all three real OSes**, in the member check and in every CI run that reaches it. This is the only code
  where a Mac pass proves nothing about Windows or Linux. Its tests name the OS behaviour they pin (for example "a file the
  layer holds open can be renamed on Windows").
- **Code above the layer is portable by construction.** A Mac run covers its logic. The real runners still run it (the
  staged CI does not change), but a red result there means the layer has a gap, which is fixed in the layer with a failing
  test first.
- **Timing above the layer** uses `paddock_jobs::time::FakeClock`, so ordering races show on the Mac as well. A test that
  must use real time declares a hang guard (C30's existing comment).
- **The Linux test row in the push gate** (`test-linux`): the affected nextest set in the native arm64 CI image, with the CI
  job's measured environment (a `sleep` PID 1 with `docker exec`, root, CI's open-files limit, CI's CPU share, locale and TZ).
  It is compared against the base's result in the same container (ER-9): only new failures are findings, and a container
  that is not running or not built is `not measured`, never a pass. A `--stress` mode adds CPU spinners for tests that a
  change marks as timing-sensitive.

### 4. What cannot be closed by design

About 70 of the 259 past failures (27 to 28%):

- **Windows API behaviour** that differs by version or machine (the listener table, WebView2, UAC, the tray shell,
  Defender's first-run scan, XtaCache on Arm, WER dialogs): about 26 of 41.
- **Runner and machine images** (the VC++ runtime, installed apps, toolchain drift such as Rust 1.100 in 8027): 5, plus
  infrastructure failures.
- **Linux systemd, D-Bus and packaging; kernel features** (landlock, seccomp, sock_diag): 6.
- **Session 0 and the runner's TMP ACL** on Windows: 5.
- **Timing that depends on the slower amd64 and Windows hosts**: about 14.
- **Rendering in the real app** (WebKitGTK, GTK): about 5.
- **Others**: runner toolchain locks (rustup on Windows), shell differences in workflow steps (cmd, GNU tar), one Unicode case fold, two Linux API cases seen on a VM: about 8.

These are covered by the member checks on real runners (each train member on CI's machines; ER-9, rule 50 (k)), the staged
CI and the VM checks. This ADR removes load from them; it does not replace them.

### 5. Stages, cost and effect

The order is failures prevented per agent-day. Shares are of the 259 past failures; the research note, section 3, gives the
reasoning per class.

| Stage | Work packages | Prevents | Cumulative | Agent-days |
|---|---|---|---|---|
| S0 (exists) | `clippy-windows` and `clippy-linux` rows, if always run | other-target code 29 | 11% | 0 |
| S1 Linux parity | WP-1 print the Linux job environment in each job summary (0.5); WP-2 container helper parity flags (1); WP-3 `test-linux` row with base comparison (1.5); WP-4 positive controls: a root-only permission test, an open-files leak at CI's limit, a zombie under a `sleep` PID 1 (0.5); WP-5 load mode (0.5) | 52 | 31% | 4 |
| S2 LF everywhere | WP-6 `* text=auto eol=lf`, renormalise, the line-ending check, `text::lines` (1) | 13 | 36% | 1 |
| S3 process builder | WP-7 MSYS variables, `.cmd` resolution, length limit, Windows keep-list (2); WP-8 Node spawn helper and ESLint rule (1.5); WP-9 per-test start budget (0.5) | 18 | 43% | 4 |
| S4 paths and temp | WP-10 C122 over stand-in output and Unix-absolute literals (1.5); WP-11 clippy bans on temp and remove in tests (0.5); WP-12 migrate the flagged sites (3) | 26 | 53% | 5 |
| S5 e2e on Linux and fonts | WP-13 WebKit and Chromium e2e in the arm64 container (2); WP-14 the wide-font Mac e2e project (1) | 16 | 59% | 3 |
| S6 clocks | WP-15 merge the five app clocks into `paddock_jobs::time` (2); WP-16 C30 blocking, migrate its allow list (3) | 13 | 64% | 5 |
| S7 file layer | WP-17 `paddock_env::fs` with Windows sharing rules and a strict Mac mode (2.5); WP-18 migrate the test sites the ban flags (1) | 8 | 68% | 3.5 |
| S8 Windows API helpers | WP-19 refused-loopback, conhost and sparse helpers, each with a Windows test (2.5) | 11 | 72% | 2.5 |
| | | **186** | **72%** | **28** |

For Linux alone (98 failures), S0 and S1 reach 62% and S5 raises it to 79%.

### 6. Common practice (ER-37)

Rust tests every merge commit on all tier-1 OSes through bors and runs a Linux check with a Windows cross-compile on each PR.
VS Code runs Linux on PRs and Windows in the merge queue, and owns its path code (`path`, `extpath`, `URI`). Chromium requires
Linux, Windows and Mac builders in its commit queue, has a typed `FilePath`, and bans raw APIs in `PRESUBMIT.py`. Deno fixes
line endings with `* text=auto eol=lf`. Git gates its tests by probed prerequisites, and MSYS2 documents
`MSYS2_ARG_CONV_EXCL`. Sources are in the research note, section 5. None of them treats one OS as enough. Real-OS runs before
landing are the norm, which our member check already is. A typed path layer with bans is what large projects (Chromium, VS
Code) do; for a project of our size it goes beyond the minimum.

**The cheaper common-practice option** is S1 without the load mode, plus S2: **4.5 agent-days**, 36% of past failures with
S0, and 62% of the Linux ones. The rest is left to the real runners, as Rust and VS Code do. The full set is **28
agent-days**, 23.5 beyond the cheaper option, so it needs the user's yes (ER-37).

## Decisions for the user

- **D1. Scope.** (a) The cheaper option, S1 and S2: 4.5 agent-days, about 36%. (b) The middle option, S1 to S4: 14
  agent-days, about 53%. (c) All stages: 28 agent-days, about 72%. Recommended: (b) now, then S5 to S8 after the next app
  release. Reason: S1 to S4 hold most of the recurring classes (paths, CRLF, MSYS, the Linux container), and S6's clock
  migration is the largest and least certain item.
- **D2. The Linux test row's default.** (a) On for every push that touches Rust, `not measured` without Docker. (b) Opt-in.
  Recommended: (a), because Linux runs most of CI.
- **D3. LF for every text file** (S2) renormalises the index in one commit. (a) Yes. (b) Keep `text=auto` and add more
  per-type rules. Recommended: (a).
- **D4. Strict file mode on the Mac** (S7). (a) On by default in tests, which can make tests fail that pass today on the
  Mac but would fail on Windows. (b) Opt-in per test. Recommended: (a), behind ER-41's report-only start.

## Risks

- The container is not the runner (arch, kernel, CPU speed, ext4). Without a base comparison its results would be false
  findings, as with the emulated amd64 run (lessons, 2026-11-14). The comparison (WP-3) is required, not optional.
- Moving every touchpoint into one layer moves the risk into one place. The layer's tests must run on all three real OSes
  in every change that touches it, and a layer gap found on a runner is fixed with a failing test in the layer.
- The migration is large in places (section 7 of the note: up to 382 Node spawn sites, about 260 temp and remove sites in
  tests). Only the sites a check flags move, and allow lists start at today's sites and only shrink.
- The shares are a judgement per row (about ±10 points). About 56 rows rest on commit subjects only.
- CI's open-files limit, kernel, locale and TZ are not measured yet (WP-1 is first for this reason).

## Rule (proposed, to be numbered when accepted)

Code reaches the OS only through the portable layer (section 1). A test that needs another OS's behaviour on the Mac uses the
layer's strict mode or the Linux test row. A failure seen only on a real runner is fixed in the layer with a failing test
there, never only at the site.
