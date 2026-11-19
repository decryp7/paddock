# Why tests fail on Linux and Windows but pass on the Mac (2026-11-20)

Research note. Questions from the user (2026-11-20): why do tests fail on other OSes but pass on macOS, can running on the Mac
be enough, and (widened the same day) how do we resolve all OS-related failures once and for all. A follow-up said that the
Linux runner runs almost everything in CI, so Linux-only failures matter most, and asked how closely the local Linux container
on the Mac can match the amd64 Linux runner. The design that follows from this note is
[ADR 0109](../adr/0109-os-portability.md) (Proposed). No code is changed here.

**Heads read** (ADR 0091): `origin/main` 9cbd77a6365b, `origin/train/2026-11-19-16` 937d565fe0e0 (this branch's base),
`origin/train/2026-11-19-17` e80df3371, `origin/fix/train15-ci-8327` 00a224e50, all on 2026-11-20 and re-read before the
commit. Code citations are at the train-16 head. The run 8327 fixes (a720dd85d, ded73e60f, ccba3e2a5) are only on
`origin/fix/train15-ci-8327` (`git branch -r --contains a720dd85d`); they are on no tag and not on main.

## Short answer

- The Mac misses these failures for three reasons. Code under `cfg(windows)` or `cfg(target_os = "linux")` is never compiled
  there. Tests carry the Mac's own assumptions (`/` paths, LF text, a non-root user, fast process starts, macOS fonts). And
  CI's Linux job container differs from the Mac's environment: it runs as root, its PID 1 never reaps zombies, it has GNU
  tools, ext4, other fonts and a slower CPU.
- Over 259 OS-only failures since 2026-10-18, about 72% could be caught on the Mac (or prevented by construction) once the
  mechanisms of ADR 0109 exist. About 28% need the real OS: Windows API behaviour, runner images, systemd and D-Bus, kernel
  features, session 0, and part of the timing class. The member checks on real runners still have to run.
- For Linux, which runs most of CI, a test row in the native arm64 CI image, with CI's environment copied (root, a `sleep`
  PID 1, CI's open-files limit, CI's CPU share), would have caught about 62% of the 98 Linux-only failures. With the e2e
  suites also run in that container, the figure is about 79%. Today the local Linux row runs Clippy only, which covers about 9%.

## 1. The failures

Two independent sources were read. Each failure was counted once per fix.

**A. Git history** (`origin/train/2026-11-19-16`; `git log --no-merges -i -E --grep` over OS words, limited to `fix`/`test`
subjects): 767 candidates. 275 commits fix **259 failures** that showed only on Windows (161), only on Linux (96) or on both
but not the Mac (2). The other 492 were rejected: the word only, the Mac failed too, predicted by review with no failing run,
VM scripts written for one OS, or hardening. About 56 rows rest on the subject line alone (mostly October, commits without a
body), so they are weaker evidence. The full table (sha, date, OS, area, class, cause) was built in the session scratch folder
and is summarised below. It is not committed because it is a working list.

**B. CI runs** (`./dev ci triage <run> --no-history` plus the failed jobs' full logs) for runs 7485, 7486, 7494, 7501, 7510,
8027, 8028, 8110, 8118, 8138, 8147, 8152, 8154, 8174, 8214, 8241, 8251 to 8257, 8271, 8297, 8300, 8315 and 8327. About 107
Windows-only and 38 Linux-only failed tests, plus five job-level failures. 8027 alone has 47 Windows tests (the Rust 1.100
runner drift and the first Windows run of paddock-dev's host code). On Linux, 13 of the 38 are e2e timing or rendering.

Run 8327 (train 15, 2026-11-20), checked against the job logs:

| OS | Test | Root cause | Fix |
|---|---|---|---|
| Windows | paddock-dev `pre_push::one_container_run_covers_every_touched_workspace` | MSYS path conversion: Git bash turned `CARGO_TARGET_DIR=/target/v7/...` into `C:/Program Files/Git/target/v7/...` | a720dd85d |
| Windows | paddock-dev `queue::the_lockfile_loop_of_the_train_gates` | a StandIn's `{cwd}` is native `C:\...\crates`; the test looked for `/crates` | a720dd85d |
| Windows | paddock-testkit `stand_in::the_nice_token_is_a_number` | `{nice}` exists only on Unix; the stand-in printed nothing | ded73e60f |
| Linux and Windows | paddock-proc `session::tests::many_requests_go_to_one_process` | a race in a test-only probe (`queued` went below 0, then overflowed at `session.rs:227`). Not OS code: the slower runners only made the order likely | ccba3e2a5 |
| Linux | paddock-dev `wrapper::two_wrappers_with_the_same_hash` (60 s) | a stand-in install raced (rm, write, chmod). Not OS-specific: 6 of 300 failed on the Mac too | ded73e60f, 00a224e50 |

Lessons that name the same causes: "land-f's first CI run: eight causes, none visible on the Mac" (runs 7485/7486),
"Windows-only assumptions in tests that came in with land-f" (7494), "A zombie answers kill(pid, 0)" (8110), "A git-fixture
property test timed out on Windows" (8138), "Two shared-crate tests failed once each in run 8174 ... ETXTBSY", "Goldens
captured on one machine failed on another", "Linux-only Rust was never linted before a push", and "The local Linux gate could
not judge a worktree, and amd64 emulation hid failed execs" (all in `docs/lessons.md`).

## 2. Classes

The counts come from source A (259 failures). The "Mac after" column is the projected share once ADR 0109's stages exist. It
is a judgement made per row and is explained in section 3.

| Class | All | Win | Linux | Typical cause | Example fixes | Mac after |
|---|---|---|---|---|---|---|
| Windows API semantics | 41 | 41 | 0 | the listener table, a refused loopback connect costing about 2 s, a pending recv that shutdown does not wake, conhost children, console windows, folder times, WebView2, tray | 059b7818d, 9ca3c97d3, 50772feb8 | 15 (layer) |
| Timing on a slower or loaded runner | 35 | 20 | 15 | wall-clock limits, thread order, a runner 1.7 to 1.9 times slower | fa9358cc9, 8a2e859a1 | 21 |
| Code only the other target compiles (lint, dead code) | 29 | 20 | 8 (+1 both) | `cfg(windows)` and `cfg(target_os="linux")` code | f23af17e9, 5397e488c, 99c4e7881 | 29 (rows exist) |
| Paths: separators, drive letters, verbatim `\\?\`, 8.3 names | 25 | 25 | 0 | `/srv` is not absolute on Windows; text comparison with `/` | 2a791297d, 0e996a61c, 5add985d8 | 25 |
| Environment | 25 | 9 | 16 | root in the container (7), no reaping PID 1 (5), missing kernel features (3), cleared `SystemRoot`/`RUSTUP_HOME`, runner TMP ACL (3), session 0 (2) | c9da43c18, 282c6e447, 6445233db | 17 |
| Fonts and rendering (Linux WebKit, GTK) | 17 | 0 | 17 | wider Linux fonts clip text; a 1 px WebKit difference | 6089a27dc, 1ee5d2be6 | 12 |
| CRLF | 13 | 13 | 0 | `* text=auto` gives CRLF on Windows; a split on `\n` | 1589c46bb, 4d7c69bb2 | 13 |
| ETXTBSY and file locking | 11 | 7 | 4 | Linux text-busy after a write; Windows cannot rename or delete an open file | 5e4d80098, e7af795a8 | 8 |
| Linux kernel and API semantics | 11 | 0 | 11 | threads listed as processes, `comm` cut at 15 bytes, seccomp/landlock order | bea504150, a9efd20bf | 9 |
| Tool differences (GNU and BSD, bash 5, cmd, GNU tar) | 10 | 3 | 7 | bash 5.2 `patsub_replacement`, `ps` flags, `du` sizes | 10e8b9a55, 723095a55 | 8 |
| Platform detection in tests (user agent, Mod key, host arch) | 6 | 0 | 6 | WebKit on Linux emulates the Mac; aarch64 means macOS | c29508d9d, c6c757208 | 6 |
| Process spawn cost (Windows) | 5 | 5 | 0 | about 100 ms per process; 1,651 git processes in one test | 498f9b03d, 48f3887b7 | 5 |
| Runner image | 5 | 5 | 0 | VC++ runtime only on the runner; Pitcrew installed on the runner | ff40d2a9f, 566f0470c | 0 |
| Linux memory accounting | 5 | 0 | 5 | jemalloc keeps freed pages; `RLIMIT_DATA` counts arenas | 17f2f1cf6, 565f04e09 | 5 |
| MSYS (Git bash) conversion | 4 | 4 | 0 | `/c/Users` read as `C:\c\Users`; backslash pairs rewritten | 3530d0643, 76e2d2d80, a720dd85d | 4 |
| Case | 3 | 2 | 1 | PATHEXT gives `rustc.EXE`; final sigma folding | b86f81230, 941a84f68 | 2 |
| Exec bits, shebangs, `.cmd` launchers | 3 | 3 | 0 | `npm` is `npm.cmd`; a `#!/bin/sh` wrapper does not start on Windows | eba8b9335, 42619e558 | 3 |
| Signals and Windows termination, handles, pid reuse | 3 | 2 | 1 | an inherited step-pipe handle; a pid reused at once | bcd97bf9d, 41e1d7900 | 3 |
| Linux packaging, systemd, D-Bus | 3 | 0 | 3 | `.deb` try-restart; zbus drop | 523563a64, 662bb7d8a | 0 |
| Command-line length | 2 | 2 | 0 | cmd.exe 8,191 and CreateProcess 32,767 characters | b9f9e422d, 4c581a781 | 2 |
| Open-files limit | 2 | 0 | 2 | a leak at a 1,024 limit; `accept4` takes a descriptor first | 6d70a2005, 25b559213 | 2 |
| Other (Tauri config per target) | 1 | 0 | 0 (1 both) | | 596836729 | 1 |
| Symlink privilege | 0 | 0 | 0 | no instance found | | |
| **Total** | **259** | **161** | **96 (+2 both)** | | | **~186 to 190 (about 72%)** |

## 3. Can the Mac catch each class?

Legend: (a) by design, one shared abstraction makes the class impossible above it; (b) by a static check on any OS; (c) by
emulating the other OS on the Mac; (d) only on the real OS.

| Class | How | What exists (cited code) | What is missing |
|---|---|---|---|
| Other-target code | (b) | the push gate's `clippy-windows` (xwin) and `clippy-linux` rows (`tools/paddock-dev/src/pre_push/gates.rs`, `PER_WORKSPACE_PREFIXES`; the push gate landed in bc74adf60 on 2026-11-12; the Linux row became a default for touched workspaces in 6d186d9fc on 2026-11-16 and later commits) | nothing new. Two instances came after the rows landed (1821ef544, da0289be2), so the rows were not run, or ran too late |
| Paths | (a)+(b) | `paddock_env::winpath`; `paddock_testkit::path` and check C122 (ADR 0099, `tools/paddock-dev/src/check/c122.rs`); clippy bans `canonicalize` (`crates/clippy.toml`, `tools/paddock-dev/clippy.toml`) | stand-in output (`{cwd}`) and Unix-absolute literals in tests (`"/tmp`: 135 sites, `env::temp_dir(`: 60) are not covered. 8327 shows the gap |
| CRLF | (a) | `.gitattributes` keeps `*.rs`, `*.sh`, fixtures and Linux packaging LF; the rest is `* text=auto` | `* text=auto eol=lf` for every text file, as Deno does, with named CRLF exceptions (`*.cmd`) |
| MSYS conversion | (a)+(b) | none. `git grep MSYS2_ARG_CONV_EXCL` finds nothing | the one process builder (ADR 0100) sets `MSYS2_ARG_CONV_EXCL=*` and `MSYS_NO_PATHCONV=1` for every child; workflow `bash` steps on Windows set them too; a check bans Git-bash-only path forms in tests |
| Exec bits, `.cmd` | (a) | `paddock_testkit::exe::write_program`, `paddock_testkit::stand_in` | the process builder resolves `.cmd`/`.bat` through PATHEXT and refuses a shebang script on Windows with a clear error; StandIn writes a `.cmd` twin |
| Command-line length | (a)+(b) | none | the process builder measures the command line and fails at 8,191 (cmd) or 32,767 characters on every OS |
| Spawn cost | (b) on the Mac | `paddock_env::starts` counts starts (`crates/clippy.toml` reason text); ADR 0108 budgets | a per-test start budget that fails on the Mac when a test starts more than N programs (the Windows cost is about 100 ms each) |
| ETXTBSY and locks | (a)+(c) | `paddock_env::text_busy::retry`, `exe::write_program` (5e4d80098); `paddock_env::atomic` | one file layer for replace and remove with Windows sharing rules (open with `FILE_SHARE_DELETE`; retry a sharing violation), plus a strict test mode on the Mac that refuses to remove or rename a file the layer still holds open |
| Signals, handles | (a) | `paddock_proc::end`, `kill_tree`, `ProcessId` (ADR 0033); clippy bans `libc::kill` and friends; `tools/paddock-dev/src/fds.rs` clears inherited handles | nothing new |
| Environment, Linux container | (c) | `tools/linux-container.sh --ci-image` runs as root (measured below) | a `sleep` PID 1, CI's open-files limit, CI's CPU share, a test row (section 4) |
| Environment, Windows | (a) part, (d) part | `paddock_proc::clean_env` keeps restricting variables (ER-47) | a named Windows keep-list (`SystemRoot`, `PATHEXT`, Git `usr/bin`); TMP ACL and session 0 stay (d) |
| Timing | (a) part, (c) part | `paddock_jobs::time` (`Clock`, `FakeClock`); check C30 (`conformance/src/checks/C30-wall-clock.mjs`, allow list `conformance/c30-allow.json`) | C30 blocking; the app `Clock` copies (`apps/debrief/.../jobs/service.rs`, `debrief-agent/src/clock.rs`, `dyno-agent/src/disk/centre.rs`, `dyno-plugin-host/src/clock.rs`, `spotter-engine/src/clock.rs`) merged into the shared one; a load mode for the container row |
| Fonts | (c) | e2e uses system fonts (`--pd-font-sans` in `ui/projects/paddock-ui/scss/_tokens.scss`) | e2e in the arm64 container, or a pinned wide fallback font in a Mac e2e project to show overflow |
| Linux kernel, memory, tools, case | (c) | the container is a real Linux kernel with GNU tools and case-sensitive file systems | the test row |
| Windows API semantics | (a) part, (d) part | `paddock-proc` console and job code | helpers for the repeated shapes (a refused loopback connect, conhost children, folder times, sparse files), each tested on Windows once. The rest stays (d) |
| Runner image, packaging, session 0, kernel features | (d) | the member checks on real runners, VM checks | nothing on the Mac can show them |

## 4. The local Linux container against the CI Linux runner

Measured on 2026-11-20 on the Mac (the session's container helper, the native arm64 CI image `paddock-ci-local:v3-arm64-32c391d5d840`,
`--cpus 4 --memory 12g`, a load average of 23 at the time):

| Property | Local container (measured) | CI Linux job (source) | Effect |
|---|---|---|---|
| Arch | aarch64 | amd64 (`ci-image/VERSION` from the registry) | `c_char` is `u8` on aarch64 Linux and `i8` on x86_64; atomics are weaker on arm64 (races show more often, which helps); a few tests assumed "aarch64 means macOS" (c6c757208, 209d41ba4) |
| Emulation | none (native) | none | the amd64-emulated container gave false failures (qemu-user hides failed execs; lessons 2026-11-14). The native image does not have this problem |
| User | uid 0 | root (lessons, land-f, 7485) | same |
| PID 1 | the test command | `sleep 21600`, which never reaps (lessons, run 8110) | differs. Add a `sleep` PID 1 and run the tests with `docker exec` |
| Open files | soft 20,480, hard 1,048,576 | **not measured** (lessons, "A polling test ran its in-process agent out of file descriptors": "the CI container's own open-files limit" is not measured; `docs/rules.md` line 247 asks for `ulimit -n 1024` locally) | differs. Measure CI first, then pass `--ulimit nofile=<CI>` |
| CPUs | `nproc` 6 with a 4-CPU quota | a 16-core host with 3 job slots (`conformance/ci/runner-slots.json`; docs/benchmarks.md, run 7721) | `available_parallelism` sees 6 and gets 4. CI's share is about 5. Timing races show less on the faster M4 Pro: run 8300 measured the runner at 1.7 to 1.9 times slower |
| Kernel | OrbStack 7.2.12 | not recorded | landlock, seccomp and sock_diag can differ (3 failures) |
| Build file system | btrfs (`/target`) | ext4 (lessons, prune 4 KiB blocks, ffe91b1ac) | folder sizes differ; one failure |
| Locale, TZ | unset | not recorded | measure |
| Fonts | 58 from the same Dockerfile | the same Dockerfile on amd64 | same font files; glyph metrics should match (not measured) |
| Browsers | no Chrome for Testing, no draw.io (no Linux arm64 build; `tools/linux-container.sh` header) | Chrome for Testing, WebKit | the e2e suites cannot run locally today |

**What the local Linux row runs today:** Clippy only (`gates.rs` `linux_clippy`), with the Tauri app crates left out
(`linux_inputs::tauri_crates`). No test runs in the container before a push.

**What it would take.** Four changes: (1) measure the CI job's environment once and print it in every Linux job summary (uid,
`ulimit -n`/`-Hn`, `nproc`, the cgroup CPU quota, the kernel, the PID 1 command, locale, TZ, a hash of `fc-list`); (2) pass
those values to the container helper (a `sleep` PID 1 with `docker exec`, `--ulimit nofile`, `--cpus` equal to CI's share,
the same locale and TZ); (3) add a `test-linux` row that runs the affected nextest set (the same plan as CI) in that container;
(4) add an optional load mode (`--stress N` CPU spinners) for tests a change marks timing-sensitive. ER-9 still applies: a
result from another machine counts only when it is compared with the base's own result on the same machine. The row therefore
runs the base once per CI image version and reports only new failures as findings.

**Projected share of the 98 Linux-only failures** (per class from section 2):

| Linux class | Count | Caught by the test row | Why not all |
|---|---|---|---|
| Environment | 16 | 13 | 3 are kernel features that OrbStack's kernel may lack or have |
| Fonts and rendering | 17 | 0 today, 12 with e2e in the container | 5 are WebKitGTK and GTK in the real app (VM) |
| Timing | 15 | 8 | under load, several reproduced in the CI image (8a2e859a1: 4 of 300; c3b8c0e27: 27 of 40); the rest depend on the slower amd64 host |
| Other-target lint | 9 | 9 (the existing Clippy row) | |
| Linux kernel and API | 11 | 9 | `nice` EPERM needs a non-root user; one was a VM-only finding |
| Tools | 7 | 7 | the same GNU tools and bash |
| Platform detection | 6 | 2 (arch), 4 more with e2e | |
| Memory | 5 | 5 | a real Linux kernel |
| ETXTBSY | 4 | 4 | a real Linux kernel, under load |
| Packaging, systemd, D-Bus | 3 | 0 | needs systemd (VM) |
| Open files | 2 | 2 | with CI's limit |
| Case, signals | 2 | 2 | |
| **Total** | **98** | **61 (62%)**, **77 (79%)** with e2e | |

## 5. Common practice (ER-37)

From the web research for this note (pages opened on 2026-11-20 unless marked):

- **Rust:** a short Linux-only check on every PR, which includes a Windows (mingw) cross-compile, and a bors "auto" merge
  that tests every merge commit on all tier-1 OSes before it lands (https://rustc-dev-guide.rust-lang.org/tests/ci.html).
- **Cargo:** tests write expected output with `/` and patterns such as `[ROOT]` and `[EXE]`, and the comparison normalises
  separators and CRs (https://doc.rust-lang.org/nightly/nightly-rustc/cargo_test_support/compare/index.html). The guide
  warns not to rename, delete or overwrite a binary right after running it on Windows (https://doc.crates.io/contrib/tests/writing.html).
- **Deno:** one CI matrix over three OSes and two architectures. Some heavy Windows and macOS jobs are skipped on PRs
  without a label. `.gitattributes` is `* text=auto eol=lf` with named exceptions
  (https://raw.githubusercontent.com/denoland/deno/main/.github/workflows/ci.ts, https://raw.githubusercontent.com/denoland/deno/main/.gitattributes).
- **VS Code:** Linux tests on PRs; Windows tests in the merge queue
  (https://raw.githubusercontent.com/microsoft/vscode/main/.github/workflows/pr.yml). All path handling goes through its
  own `path` (win32 and posix), `extpath` and `URI.fsPath`
  (https://raw.githubusercontent.com/microsoft/vscode/main/src/vs/base/common/extpath.ts).
- **Chromium:** 32 required commit-queue builders, among them 6 Linux, 6 Windows and 3 Mac
  (https://chromium.googlesource.com/chromium/src/+/main/infra/config/generated/cq-builders.md); a typed `base::FilePath`
  (https://chromium.googlesource.com/chromium/src/+/main/base/files/file_path.h); `PRESUBMIT.py` ban rules against raw APIs
  (https://chromium.googlesource.com/chromium/src/+/main/PRESUBMIT.py).
- **Git and Git for Windows:** tests are gated by prerequisites probed at run time (`MINGW`, `SYMLINKS`,
  `CASE_INSENSITIVE_FS`, `POSIXPERM`, `SANITY` false as root) (https://raw.githubusercontent.com/git/git/master/t/test-lib.sh).
  MSYS2 path conversion is turned off with `MSYS2_ARG_CONV_EXCL` and `MSYS2_ENV_CONV_EXCL`
  (https://www.msys2.org/docs/filesystem-paths/). Git for Windows' own `MSYS_NO_PATHCONV=1` is documented in its release
  notes (unverified: seen only in mirrors).
- **Platform documentation:** Windows refuses to delete a file with an open handle unless it was opened with
  `FILE_SHARE_DELETE` (https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-deletefilew).
  `FILE_DISPOSITION_POSIX_SEMANTICS` gives POSIX delete
  (https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntddk/ns-ntddk-_file_disposition_information_ex). The
  command line is limited to 32,767 characters
  (https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessw).
  `fs::canonicalize` returns `\\?\` paths (https://doc.rust-lang.org/std/fs/fn.canonicalize.html). On Linux, `execve`
  returns ETXTBSY while a file is open for writing (https://man7.org/linux/man-pages/man2/execve.2.html). The race is still
  open in Rust (https://github.com/rust-lang/rust/issues/114554) and Go (https://github.com/golang/go/issues/22315).
  Writing the program in another process, as `exe::write_program` does, is one of the proposed fixes.
- **Data:** a study of 500 Python projects found OS-dependent test failures in 11.2%. File and directory operations and
  process management were the largest categories (https://arxiv.org/abs/2609.25531, abstract only). No published figure was
  found for how many cross-OS failures one dev OS can catch.

**In one line:** well-known projects do not make one OS enough. They run every OS before landing (merge queues and trains),
use cross-compiling and Linux PR checks as cheap early filters, and route paths and processes through one typed layer. Our
member check on real runners is that common practice. ADR 0109's layer is the Chromium and VS Code part of it, sized for us.

## 6. Recommendation

Ranked by failures prevented per agent-day. Each stage, its classes, its cost and its positive control are in ADR 0109; this
is the summary.

| Rank | Stage | Closes | Past failures prevented | Agent-days |
|---|---|---|---|---|
| 1 | S1 Linux parity test row (measure CI, `sleep` PID 1, CI's open-files limit and CPU share, affected nextest, base comparison, load mode) | most Linux environment, kernel, tools, memory, ETXTBSY, open files | 52 (20%); Linux 61 of 98 with the existing Clippy row | 4 |
| 2 | S2 LF for every text file | CRLF | 13 (5%) | 1 |
| 3 | S3 the process builder finishes the job (MSYS off, `.cmd`, length, start budget, Windows keep-list, one Node spawn helper) | MSYS, exec, command line, spawn cost, Windows environment | 18 (7%) | 4 |
| 4 | S4 paths and temp folders complete (C122 over stand-in output, Unix-absolute literals and `/tmp` banned in tests) | paths, case | 26 (10%) | 5 |
| 5 | S5 e2e in the arm64 container, and a wide-font Mac project | fonts, platform detection | 16 (6%) | 3 |
| 6 | S6 clocks: C30 blocking, app clocks merged, a fake clock where tests time things | timing (part) | 13 (5%) | 5 |
| 7 | S7 one file layer with Windows sharing rules and a strict Mac mode | locks, folder times | 8 (3%) | 3.5 |
| 8 | S8 Windows API helpers for repeated shapes | Windows API (part) | 11 (4%) | 2.5 |
| | existing rows (`clippy-windows`, `clippy-linux`), if always run | other-target code | 29 (11%) | 0 |
| | **all** | | **~186 of 259 (72%)** | **28** |

**Cheaper common-practice option (offered as a decision):** S1 without the load mode, plus S2: 4.5 agent-days. It prevents
65 failures beyond today's rows (94 of 259 with them, 36%) and 62% of the Linux ones. The rest stays with the member checks
on real runners, as in Rust and VS Code. A middle option adds S3 and S4 (14 agent-days, about 53%).

**Load removed from the real runners.** These are failures that the member checks and the staged CI would no longer find,
because the push gate finds them first. The cheaper option removes about a third of past OS-only red results, nearly all on
Linux, which is the runner most jobs use. The full set removes about seven in ten. Every red member costs a pull and a new
round (ER-9 cites train 12: about 17 hours in red rounds, all Linux- or Windows-only). The remaining 28% are real Windows API
behaviour, runner images, systemd and D-Bus, session 0 and part of the timing class. They will always need the real runners
and the VMs.

**Risks.** The container is not the runner (arch, kernel, CPU speed). Without a base comparison, its results would be the
same kind of false finding as the emulated amd64 run. The layer moves risk into one place, so the layer's own tests must run
on all three real OSes in every change that touches it. Migrating call sites is mechanical but large (section 7). The
counts are a judgement per row, about ±10 percentage points, and about 56 rows rest on the subject line only.

## 7. Size of the migration (raw `git grep -c -E` over tracked files at the train-16 head)

These are upper bounds. Justified `#[expect]` sites, the layer's own code and comments are not filtered out.

| Pattern | Sites | Files | In test paths |
|---|---|---|---|
| `Command::new(` (excluding `TestCommand::new`) | 115 | 55 | 74 |
| `"/tmp` (Rust) | 135 | 70 | 24 |
| `env::temp_dir(` | 60 | 52 | 17 |
| `fs::remove_file(` / `remove_dir_all(` / `rename(` | 416 / 512 / 157 | 232 / 203 / 111 | 102 / 153 / 11 |
| `canonicalize(` (banned; mostly the winpath layer and `#[expect]`) | 180 | 114 | 55 |
| `Instant::now(` / `SystemTime::now(` | 1,222 / 187 | 437 / 141 | 358 / 42 |
| `format!("{..}/{..}")` path text | 111 | 76 | 8 |
| Node `spawn`/`execFile`/`spawnSync`/`execSync` (`*.mjs` + `*.ts`) | 382 | 143 | 265 |
| `split('\n')` in `*.mjs` + `*.ts` | 537 | 215 | 134 |

Test-path counts match on folder and file names only (`tests/`, `it/`, `.spec.`, `.test.`, `testkit`), so inline
`#[cfg(test)]` modules count as product code. Only the sites that a stage's check flags have to move. For example S4 moves the
`"/tmp` and `temp_dir` sites in tests, not every `remove_file`.

Recurring: yes. Windows paths compared as text (7485, 7494, 8027, 8110, 8138, 8154, 8327; ADR 0099 exists, and 8327 is its
stand-in gap). CRLF (13: 1589c46bb, 4d7c69bb2, a720dd85d ...). The Linux CI container differing from the dev host (root, no
reaping PID 1, ext4, ETXTBSY, localhost: 7485, 8110, 8138, 8174, 8256). Windows spawn cost against the 60 s limit (8138, 8152,
8271; ADR 0108). Other-target code linted late (29). The 2 s refused loopback connect on Windows (50772feb8, 76399b11a,
8f38b5f75, 041c4c98f, 1997b9d91). The redesign for all of them is ADR 0109.
