# 21. Portable tests: one test kit for paths, text, fixtures, waits and tools, and a check over every test

- Status: Proposed
- Date: 2026-10-27
- Related: rules 3–5 (affected tests, fix locally, failing test first), rule 7 (tests never touch the machine), rule 8
  (lessons and rules live once), rule 40 (recurring bugs mean a missing design rule), rules 42/43 (share first, shared
  behaviour tested once); ADR 0014 (monorepo, affected CI); ADR 0019 (limits explain themselves); ADR 0020 (shared
  executor and `Clock`); checks C13 (clippy preset), C28 (repo baseline), C30 (wall clock in tests), C37 (test binaries);
  `crates/paddock-testkit`, `crates/paddock-proc`
- Evidence: CI runs 6419–6430; fixes on `fix/dyno-ci` (6598f4286, cb70d00cc, e304123cc, a964fb7a2, 3abdd303b),
  `fix/shared-spotter-ci` (507507a72), `fix/spotter-ci3` (654404cb3), and commit 1f701932b (clippy on Linux and
  Windows); the survey below (heuristic scans over `git ls-files` of `origin/integrate/monorepo` at a9851eaf9, no builds)

## Context

Development moved to the Mac (2026-10-22). Every app runs its Rust tests on Linux, macOS and Windows in CI
(`.gitea/workflows/{dyno,pitcrew,pitwall,spotter}.yml` matrix, Debrief and the shared crates through `ci.yml`), but
nobody runs them on Windows before pushing. In one week (runs 6419–6430) the same kinds of failure turned red only on
Windows, sometimes on Linux, and were fixed one at a time:

| # | Shape | Incident (fix) |
|---|---|---|
| 1 | A hard-coded Unix absolute path (`/tmp/r.md`, `/definitely/not/here`, `/private/var/folders/...`) is not absolute on Windows, so validation takes another branch | Dyno inventory and finder tests (6598f4286), new-program tests (a964fb7a2) |
| 2 | An assertion assumes `\n`; a CRLF checkout of a `.rs` file changes multi-line literals | 3abdd303b (`*.rs text eol=lf`) |
| 3 | A test creates tens of thousands of files to reach a production limit; NTFS plus Defender time it out | Spotter match cap, 50,001 files (507507a72); folder-scan tests (654404cb3) |
| 4 | A path pasted into JSON or TOML by hand; the backslashes of a Windows path break the document | Dyno CLI discovery file (cb70d00cc) |
| 5 | An OS-specific test or call not gated by `cfg` | `paddock-proc` macOS-only test mode (1f701932b) |
| 6 | A read-only handle used to change metadata (`File::open(p)?.set_modified(t)`); Windows denies it | Dyno watch and lookups tests (e304123cc) |
| 7 | A test waits a fixed time equal to a product interval (a 10 s refresh) instead of a condition | Dyno new-program tests (a964fb7a2) |
| 8 | A real external tool started without a timeout | (hangs in the same runs; the class is already a clippy rule for production code) |

Each fix picked its own idiom: `if cfg!(windows) { "C:\\tmp" } else { "/tmp" }`, `std::env::temp_dir().join(..)` (which
clippy forbids outside tests), a local `temp_exe()`, a local `dyno_argv()`. The survey finds **20 such hand-rolled
"absolute on this OS" helpers** and **31 hand-rolled `wait_until`/`eventually` helpers** already. `paddock-testkit`
exists (Scratch, EnvLock, golden files with CRLF normalisation, FakeRunner, never-panics, FakeClock), but **no app depends
on it yet**: 21 shared crates and the shell demo do.

The user decided (2026-10-27): no Windows test host. Before a push, Windows is checked on the Mac by **`cargo xwin clippy`**
(compile and lint only); everything that only shows at run time must be made impossible by the test kit or found by a check.

## The pattern

> Tests fail on Windows (or Linux) whenever a test bakes in a property of the author's OS — path syntax, line endings,
> file-system speed, quoting, tool availability, handle semantics or a product timing — instead of asking the test kit for
> it.

**Invariant:** a test's inputs and expectations are either OS-neutral or obtained from `paddock-testkit`; anything
OS-specific is `cfg`-gated; every wait is for a condition with a hang guard; every external process is bounded.

## Instances found

Heuristic scans (Python over `git ls-files`). For the survey, Rust test code was approximated as files under `tests/` or
`benches/` plus everything after the first `#[cfg(test)]` in a `src/` file; TypeScript = `*.spec.ts`/`*.test.ts`/`e2e/`.
That approximation misclassifies production code that follows a `#[cfg(test)]` import (for example
`apps/debrief/app/src-tauri/src/updater.rs:454`, the production auto-check delay) and does not see gating by a parent
`mod` or by the enclosing test fn, so **the counts include such misclassification and are upper bounds** unless "at risk"
is stated. C38 defines the scope precisely (§2). A few examples per pattern. Pacenotes, Sighter and the demo apps have no
Rust tests with findings. The TypeScript rows are **out of scope** for this ADR: their path literals are mock data that
never reach an OS, and their fixed waits belong to C30's TypeScript part.

| Pattern | Debrief | Dyno | Pitcrew | Pitwall | Spotter | Shared crates | Total |
|---|---:|---:|---:|---:|---:|---:|---:|
| 1. Unix absolute literals in Rust tests (all) | 291 | 356 | 79 | 79 | 71 | 283 | 1,159 in 325 files |
| 1. … at risk: missing-path sentinel, temp root or home root passed to a path/fs API on the same line | 28 | 28 | 18 | 2 | 9 | 38 | **123** |
| 1. hand-rolled `cfg!(windows)` absolute-path helpers | 7 | 7 | 2 | 1 | 0 | 3 | 20 |
| 1. `std::env::temp_dir()` in Rust tests (clippy-allowed) | 141 | 7 | 29 | 0 | 0 | 4 (+2 shell demo) | 183 |
| 2. test data checked out with `text=auto` and no `eol` (CRLF on Windows) | 46 | 0 | 0 | 0 | 0 | 13 (+1 conformance) | **60 files** |
| 2. asserts with a `\n` literal (mostly in-memory strings, fine) | 293 | 99 | 87 | 41 | 15 | 75 | 610 |
| 3. tests creating ≥ 10,000 files (nested bounds multiplied) | 1 | 0 | 0 | 0 | 1 → 0 on `fix/spotter-ci3` | 1 | **3 → 2** |
| 3. tests creating 1,000–9,999 files | 0 | 1 (2,100 symlinks, Linux) | 0 | 0 | 0 | 0 | 1 |
| 3. in-memory loops of ≥ 10,000 iterations (slow on loaded runners) | 1 | 0 | 3 | 1 | 0 | 4 | ~9 |
| 4. path interpolated into hand-written JSON/TOML in tests | 9 | 2 | 3 | 0 | 0 | 1 | **15** |
| 5. Unix tools or `/proc` in test code without a visible `cfg` | 2 | 9 | 4 | 0 | 2 | 1 | 18 (most gated by their parent `mod` or enclosing fn; a few need a look) |
| 6. `File::open(..)` then `set_modified`/`set_times` | 0 | 0 | 1 (in a `cfg(unix)` fn) | 0 | 0 | 1 (gated) | 0 open after e304123cc |
| 7. C30 today: wall-clock asserts / sleeps in threaded tests (warn) | 155 | 20 | 5 | 5 | 16 | 5 | **206** (96 / 110) |
| 7. `thread::sleep` ≥ 1 s in test code (incl. misclassified production) | 10 | 1 | 2 | 6 | 6 | 1 | 26 |
| 7. literal `recv_timeout`/`wait_timeout` < 10 s in tests | 16 | 2 | 1 | 4 | 7 | 12 | 42 |
| 7. spin waits (an empty loop of millions of iterations used as "work that takes time") | 0 | 0 | 0 | 0 | 0 | 1 | 1 |
| 8. `Command::new` in tests without a bounded runner | 7 (+2 own binary) | 0 (+1) | 3 | 1 (+1) | 4 | 0 (+1) | 15 (+5) |
| TypeScript specs: Unix absolute literals (out of scope) | 345 | 66 | 5 | 1 | 14 | 94 UI + 26 tooling | 553 |
| TypeScript specs: `waitForTimeout`/`setTimeout` ≥ 1 s (C30's TS part; out of scope) | 13 | 3 | 4 | 7 | 0 | 13 + 1 | 41 |

Examples:

1. `apps/debrief/crates/debrief-ai/src/runtimes/report.rs:181` `list_builds(Path::new("/no/such/worker"), ..)`;
   `apps/debrief/crates/debrief-ipc/tests/it/launch.rs:384` `format!("/tmp/debrief-sandbox-test-{}", ..)`;
   `apps/dyno/crates/dyno-agent/tests/lookups.rs:136` `"/definitely/not/here"`; already portable by hand:
   `apps/debrief/crates/debrief-core/tests/it/settings.rs:216` `if cfg!(windows) { "C:\\logs" } else { "/var/log" }`.
2. `apps/debrief/crates/debrief-core/parsers/samples/*.log` (16), `apps/debrief/crates/debrief-core/tests/it/snapshots`
   (17) and `debrief-plugins/tests/it/snapshots` (11), `crates/paddock-service/tests/golden` (11). The golden helper
   normalises CRLF (`crates/paddock-testkit/src/golden.rs:103`); whether insta does is not verified here. Some of these
   files are **deliberately CRLF in the repository** (`parsers/samples/billing.log`, `parsers/samples/u_ex261014.log`,
   `debrief-core/tests/fixtures/timestamps/log4net-csv.log`, `debrief-plugin-windows/tests/fixtures/eventlog/*`; 55 files
   under Debrief's `samples/` and `tests/fixtures/` contain CR bytes). `templates/repo/.gitattributes` and the seven app
   files lack `*.rs text eol=lf` (the monorepo root covers them today).
3. `apps/debrief/crates/debrief-core/src/watch.rs:604` writes `MAX_SCAN_ENTRIES + 1` = 50,001 files (the same class as
   Spotter's 507507a72); `crates/paddock-jobs/tests/it/walk.rs:282` 40 × 300 = 12,000 files in nested loops; in memory:
   `crates/paddock-packs/src/tauri/tests.rs:181` (2,000,000), `apps/pitwall/crates/pitwall-core/src/limits.rs:439`
   (100,001); `apps/dyno/crates/dyno-platform/src/linux/locks.rs:231` (2,100 symlinks).
4. `apps/debrief/crates/debrief-core/tests/it/plugins.rs:388` `file='{}'` (a TOML literal string: safe for backslashes,
   breaks on a `'`); `apps/debrief/crates/debrief-collect/tests/it/collect.rs:126` `"path":{:?}` (Rust `Debug` escaping is
   not JSON for non-ASCII); `apps/pitcrew/crates/pitcrew-service/tests/supervision.rs:87`.
5. `apps/debrief/crates/debrief-plugins/tests/it/evtx_memory.rs:195` `quiet_command("ps")`;
   `apps/pitcrew/crates/pitcrew-service/tests/supervision.rs:536` `Command::new("ps")`. (The scan's
   `debrief-plugin-windows/src/command.rs:194` `kill` is already gated: it is a false positive of the approximation.)
7. `crates/paddock-jobs/tests/it/parallel.rs:13` spins 50,000,000 iterations as "slow work" (a spin wait, not a
   fixture: use a gate or a `FakeClock`); `apps/debrief/app/src-tauri/src/jobs/service.rs:1127`
   `recv_timeout(500 ms).is_ok()` as the success condition.
8. `apps/debrief/app/src-tauri/src/video/tests.rs:562` (ffmpeg), `apps/debrief/crates/debrief-plugin-dotnet/tests/it/real_runtime.rs:24`
   (`dotnet --version`), `apps/pitwall/crates/pitwall-cli/tests/cli.rs:98` (own binary, no timeout).

## Decision

### 1. `paddock-testkit` grows one module per shape, `paddock-proc` gets the tool runner (the mechanism)

**Charter of the kit (restated):** pure helpers and scratch-bound file helpers; nothing touches the user's folders or
starts a process; nothing sleeps except `wait::eventually`, which sleeps only between polls of a condition and always
ends at its hang guard. Each module is tested once in the kit on all three OSes (rule 43).

| Module | API (sketch) | Replaces |
|---|---|---|
| `paths` | `abs(&["logs", "a.log"]) -> PathBuf` absolute on the host (`/logs/a.log`, `C:\logs\a.log`), never touched; `missing(&Scratch) -> PathBuf` an absolute path that is guaranteed absent; `temp_shaped(name)` / `home_shaped(name)` paths shaped like the host OS's temp or home folder for pure classification code (`/tmp/x`, `C:\Users\tester\AppData\Local\Temp\x`) | pattern 1, the 20 local helpers, `/definitely/not/here`, `temp_exe()` |
| `text` | `assert_text_eq!(actual, expected)` comparing with `\r\n` → `\n` (the golden normaliser, moved here and shared); `read_text(path)` | pattern 2 where a test reads a checked-out file |
| `quote` | `json_str(&Path) -> String` (a JSON string literal, via `serde_json`), `toml_str(&Path) -> String` (a TOML basic string, escaped) | pattern 4 |
| `fixtures` | `files(dir, n, name)` creates `n` small files and refuses `n > MAX_FIXTURE_FILES` with "make the limit a parameter"; `big_text(lines)` streamed into one file. `MAX_FIXTURE_FILES` (1,000) is **the one threshold** that C38b also uses (the check reads it from the kit's source) | pattern 3 |
| `wait` | `eventually(what, cond)` polls a condition with a backoff and fails after `HANG_GUARD` (60 s, one constant, never a product interval) naming `what`; product intervals are driven by the shared `Clock` (`FakeClock::advance`) or a subscription that shortens them, never waited out | pattern 7, the 31 local helpers |
| `fs` | `set_mtime(path, time)` opens a write handle (and the directory flag on Windows) on every OS | pattern 6 |

The **tool runner lives in `paddock-proc`** behind a `testing` feature, not in the kit (rule 42: `run_bounded` is
already there, and a kit → proc dependency would form a cycle with `paddock-proc`'s own tests):
`paddock_proc::testing::run_tool(cmd, TOOL_TIMEOUT)` on `run_bounded`, and `require_tool("ffmpeg") -> Option<PathBuf>`,
which prints why it skips and fails instead under `PADDOCK_REQUIRE_TOOLS=1` (set in the CI jobs that install the tool), so
nothing skips silently (rule 5). Replaces pattern 8.

Two written rules, in the kit's docs and as one new binding rule in `docs/rules.md` under **the next free rule number (46
today), taken at merge**:

- **Limits are parameters.** A production limit (match cap, scan entries, batch size, check interval) is a field or argument
  with the constant as its default. Tests prove the behaviour with a handful of items at a small value, and one cheap test
  pins that production passes the real constant (`match_cap()` in 654404cb3). A magic number a test depends on gets a name
  (`CHECK_EVERY`).
- **No wall-clock assertions in unit tests.** Speed belongs in the benchmarks (`paddock-bench`, `debrief-bench.yml`);
  tests count items or advance a `FakeClock`. C30 already finds them; it becomes strict for files without a waiver (below).

### 2. One conformance check over every test: C38 "Portable tests"

`conformance/src/checks/C38-portable-tests.mjs`, rule 40. It reuses C30's masking of comments and literals.

**Scope (exact):** Rust files under a crate's `tests/` or `benches/`, plus the bodies of items attributed
`#[cfg(test)]` or `#[cfg(all(test, …))]` in `src/` files, including module files reached from such an item
(`#[cfg(test)] mod tests;` → `tests.rs` or `tests/mod.rs`, resolved like rustc). **Gating:** a finding is gated (and not
reported) when the enclosing fn, any enclosing module, the module's declaration in its parent (`#[cfg(target_os =
"linux")] mod linux;`, resolved through the `mod` tree) or the file (`#![cfg(..)]`) carries `cfg(unix)`,
`cfg(windows)`, `cfg(target_os = ..)`, `cfg(not(windows))` or a combination of them.

Sub-checks, each with its own id so waivers name exactly one:

| Id | Finds | Fix it names |
|---|---|---|
| C38a | a Unix absolute literal passed directly to `Path::new`, `PathBuf::from`, `fs::*`, `File::*` or `.join`; any literal under `/tmp/`, `/private/var/folders/`, `/var/folders/`, `/Users/`, `/home/` | `testkit::paths` |
| C38b | a loop nest whose innermost body creates files (`fs::write`, `File::create`, `create_dir*`, `symlink`) where the **product of the nested bounds** exceeds `MAX_FIXTURE_FILES`, or a bound named like a limit (`MAX_*`, `*_LIMIT`, `*_CAP`) | make the limit a parameter; `testkit::fixtures` |
| C38c | `format!`/`write!` with a placeholder inside JSON/TOML quotes (or `{:?}` after a `"key":`) and a path argument | `testkit::quote`, or build with `serde_json`/`toml` |
| C38d | `Command::new`/`quiet_command`/`command(` in test code whose statement does not reach `run_bounded`/`run_tool` | `paddock_proc::testing::run_tool` |
| C38e | `File::open(..)` followed by `.set_modified`/`.set_times`/`.set_len`/`.set_permissions` | `testkit::fs::set_mtime` |
| C38f | a Unix-only tool name (`sh`, `/bin/sh`, `ps`, `kill`, `sleep`, `true`, `launchctl`, ...) **as the first argument** of `Command::new`, `run_bounded`/`run_tool`, a `run(` of a runner or an app's process helper, or a read of `/proc/`, in an ungated scope | gate with `#[cfg(unix)]` or the OS, or use `FakeRunner` |
| C38g | test-data folders (`tests/fixtures`, `fixtures`, `golden`, `snapshots`, `samples`, `testdata`) with a file whose `git check-attr text eol` is `text=auto` and no `eol` | add `-text` or `eol=lf` (§4) |

C30 gains one finding: a positive wait (`recv_timeout(..).unwrap()/.is_ok()`, `wait_timeout(..)`) with a literal under
10 s → `testkit::wait::eventually`. A negative wait ("nothing arrives within 100 ms") stays allowed.

**Waivers.**

- Inline: `// conformance: portable — <reason>` on the line or the line above (the same shape as C30's `hang-guard`),
  for string data that never reaches the OS (a Unix path parsed as log content, a mock path). A missing reason is a
  finding.
- Backlog: per-**file**, per-**sub-check** entries in `paddock.waivers.toml` (`check = "C38a"`, `path = "<one file>"`,
  `reason`, `expires = "2026-12-31"`) with an optional **`count`**: the number of findings the file had when the waiver was
  generated. The waiver applies only while the file has at most `count` findings of that sub-check, so **a new finding in
  an old file still fails**; a lower count after a fix is reported so the waiver is tightened. This needs a small
  extension of `conformance/src/util/waivers.mjs` (an integer `count` key), useful to every check. Never a glob over an
  app, so a new test file is checked from day one.

**Level and order (CI stays green at every step).** WP-2 generates per-file waivers for **every** current finding — the
apps **and the shared crates** (38 at-risk paths, the 12,000-file walk, the 50 M spin wait, 12 short positive waits) —
and for **all 206 C30 findings**, then switches C38 and C30 to strict. WP-5 and WP-7 remove waivers as they migrate. New
files and new findings fail from WP-2 on.

### 3. Clippy for what clippy can see, and xwin before push

- `clippy::disallowed_methods` cannot read string literals and fires in test code too (the lint configuration has no
  in-tests switch for it; [clippy lint configuration](https://doc.rust-lang.org/clippy/lint_configuration.html), read,
  not re-checked this session), so the path, loop and process checks live in C38. The preset (`lint/clippy.toml`,
  copied into the apps by C13) adds only `std::fs::File::set_modified` and `std::fs::File::set_times` (reason:
  `paddock-files` in production, `testkit::fs::set_mtime` in tests, a write handle on every OS); the 22 current call
  sites are migrated or allowed with a reason first. `Command::output`/`status` and `Child::wait` are **not** added:
  they would fire in every test (hundreds of hits to triage) for little gain over the existing `Command::new` entry plus
  C38d.
- **`scripts/xwin-clippy.sh`** wraps the Windows lint so nobody types it differently:
  `cargo xwin clippy --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings` for each affected Cargo
  workspace (`node scripts/affected.mjs --files` on the branch's changed files → `crates/` and `apps/<app>/...`), with its
  own target folder (`$CARGO_TARGET_DIR/xwin`), `CARGO_BUILD_JOBS=2`, through the machine-wide slot of
  `engineering-rules/scripts/heavy.sh`. It catches the compile half of pattern 5 (Unix-only APIs used ungated,
  `cfg(windows)` code that rotted, Windows-only clippy findings as in 1f701932b). It runs no tests and links nothing for
  the target (build scripts are built for the host), so the `rust-lld.exe` linker setting of `.cargo/config.toml` does
  not apply. A missing `cargo-xwin` is a loud message with the install line, never a silent pass.
- It is a documented pre-push step in `docs/dev-setup.md` and in every brief; optionally a `pre-push` command in
  `lefthook.yml` with `glob: "*.rs"` that passes `{push_files}` to the script, so only workspaces with pushed Rust changes
  are linted (decision for the user below: hook or manual).
- cargo-xwin (MIT/Apache-2.0, [rust-cross/cargo-xwin](https://github.com/rust-cross/cargo-xwin)) downloads the MSVC CRT
  and Windows SDK headers and libraries with `xwin` and builds C code with `clang-cl`. Installed on the Mac (0.23.1,
  verified with `cargo xwin --version`); not yet run against these workspaces (this ADR made no builds).

### 4. `.gitattributes` policy

- Source text: `* text=auto` plus `*.rs text eol=lf` (already at the root, 3abdd303b) also in `templates/repo/.gitattributes`
  so C28 carries it into every app's file and a future split.
- Test inputs are byte-exact: `**/tests/fixtures/** -text`, `**/fixtures/** -text`, `**/samples/** -text`,
  `**/testdata/** -text`. Expected output is LF: `**/golden/** text eol=lf`, `*.snap text eol=lf`. These lines go
  **before** the existing `*.cmd text eol=crlf` line (later lines win in Git), so a `.cmd` file stays CRLF everywhere,
  also inside a golden folder. C38g checks the result over every file, not a sample.
- **Renormalising** (`git add --renormalize`) is limited to the paths whose attributes changed to `eol=lf` (golden,
  snapshots) and must not touch the `-text` folders: Debrief's deliberately CRLF inputs (`parsers/samples/billing.log`,
  `u_ex261014.log`, `tests/fixtures/timestamps/log4net-csv.log`, the eventlog fixtures and the rest of the 55 files with
  CR bytes) keep their bytes. The WP verifies with `git diff --stat` that no `-text` file changed.
- Tests that read checked-out text still compare through `testkit::text`, so a missed attribute fails softly.

## Alternatives

- **A Windows test host or VM before every push.** Rejected by the user (cost, load, one more machine to keep healthy).
- **Run tests under Wine (`cargo xwin test`).** Runs some tests, but Wine's file system, process and handle semantics are
  not Windows' (patterns 3 and 6 would not reproduce); a green Wine run would be false confidence.
- **Only clippy.** Cannot see literals, loops or quoting (above); leaves five of eight shapes unchecked.
- **Fix instances as CI finds them.** What happened this week; 20 local helpers show it does not converge.
- **Per-app test helpers.** Rule 42: the helpers are generic and already copied 20 and 31 times.

## Consequences

- Every app takes `paddock-testkit` (and `paddock-proc[testing]` where it runs tools) as a dev-dependency; none does today.
  The migrated tests read shorter.
- C38 is heuristic: it will miss shapes it does not know and needs the waiver line for path-shaped data. False positives
  are expected mostly in parsers whose input is Unix paths (Debrief's pprof, container and pod-path tests); one waiver line
  each.
- `cargo xwin clippy` adds a few minutes per workspace on a cold cache; Debrief's bundled DuckDB and other `-sys` crates
  build C/C++ for the target with clang-cl and may need flags (risk below).
- Windows-only run-time behaviour (Defender, sharing violations, services) still shows first in CI; this ADR makes the
  known shapes impossible or checked, not all of Windows.

## Work packages (order)

| WP | Scope | Files | Role | Size |
|---|---|---|---|---|
| 1 | `paddock-testkit`: `paths`, `text` (move the golden normaliser), `quote`, `fs`, `fixtures` (`MAX_FIXTURE_FILES`), `wait` (`eventually`, `HANG_GUARD`); `paddock-proc` `testing` feature with `run_tool`/`require_tool`; ≥ 99 % lines; tests run on all three OSes in CI | `crates/paddock-testkit/`, `crates/paddock-proc/` | implementer | M (1 day) |
| 2 | C38a–g with fixtures and tests (scope and `mod` resolution, nested-bound product, first-argument tool match); C30 positive-wait finding; `count` in the waiver format; generate per-file, per-sub-check waivers with counts for **all** current C38 findings (apps and shared crates) and all 206 C30 findings; then C38 and C30 strict in CI | `conformance/src/checks/C38-portable-tests.mjs`, `C30-wall-clock.mjs`, `conformance/src/util/waivers.mjs`, `conformance/test/`, `paddock.waivers.toml` | implementer (reviewer for the heuristics) | M (1–1.5 days) |
| 3 | `.gitattributes` policy in root and `templates/repo` (golden and snapshot lines before `*.cmd`); renormalise only the `eol=lf` paths; C28 sync of the app files | `.gitattributes`, `templates/repo/.gitattributes`, `apps/*/.gitattributes` | implementer | S |
| 4 | `scripts/xwin-clippy.sh` (affected workspaces, own target dir, heavy slot), `docs/dev-setup.md`, optional lefthook `pre-push` with `{push_files}`; one cold run per workspace to record time and any `-sys` crate trouble | `scripts/`, `docs/dev-setup.md`, `lefthook.yml` | implementer | S–M |
| 5 | Migrate the shared crates (38 at-risk paths, 12,000-file walk, 50 M spin wait, 12 short positive waits, their C30 findings) and delete their waivers | `crates/*` | implementer | S |
| 6 | Clippy preset entries `File::set_modified`, `File::set_times` after migrating or allowing the 22 call sites | `lint/clippy.toml`, app `clippy.toml` | implementer | S |
| 7 | Per app, one branch each, in the release order: Debrief (largest: 28 at-risk paths, 50,001-file test, 141 `temp_dir`, 155 C30, 9 JSON/TOML), Dyno, Pitcrew, Pitwall, Spotter; delete the local helpers and the app's waivers | `apps/<app>/` | implementer | Debrief L, others S–M |
| 8 | The new rule under the next free number (46 today, taken at merge) into `docs/rules.md` and the root `AGENTS.md` only (rule 8: rules live once, no per-app copies); the lesson is already recorded | `docs/rules.md`, `AGENTS.md` | coordinator | S |

WP-1 to WP-4 can run in parallel (separate files). CI stays green throughout because WP-2 waives every current finding
before going strict. WP-5 and WP-7 need WP-1 and WP-2; WP-6 after WP-5.

## Risks

- **C38 noise** makes people waive by reflex. Mitigation: the waiver needs a reason and carries a count; the reviewer
  samples waivers per WP; the heuristics start narrow (the "at risk" 123, not the 1,159).
- **cargo-xwin and C/C++ `-sys` crates** (DuckDB bundled, zstd, sqlite, ring/aws-lc): a crate's build script may not
  cross-compile with clang-cl, or take long. Mitigation: WP-4 measures each workspace once; a workspace that cannot be
  linted is listed with the reason and falls back to `cargo check --target x86_64-pc-windows-msvc` for the crates that
  can (the existing AGENTS.md rule).
- **Microsoft licence:** xwin downloads the MSVC CRT and Windows SDK, whose licence the user accepts on first use
  (`--accept-license` / `XWIN_ACCEPT_LICENSE`); see the decision below.
- **`require_tool` skipping** hides a missing tool locally; CI must set `PADDOCK_REQUIRE_TOOLS=1` where tools exist, or the
  tool test never runs anywhere.
- **Renormalising** the wrong paths would rewrite Debrief's deliberately CRLF inputs and break their parser tests; WP-3
  limits and verifies it (§4).

## Migration and rollback

Migration is WP-5 then WP-7, one small commit per pattern group, with each removed waiver as the proof. Rollback: C38 can
be dropped from `checks/index.mjs` and its waivers deleted; the testkit modules and the `paddock-proc` feature are
additive; the `count` waiver key is optional; the `.gitattributes` change is reverted with a renormalise of the same
paths.

## Decisions for the user

1. Accept this ADR (status Accepted, date) and the new rule under the next free number (46 today, taken at merge):
   "tests ask the test kit for anything OS-specific; limits are parameters; no wall-clock assertions in unit tests".
2. xwin clippy as a lefthook `pre-push` command (automatic, minutes on a cold cache) or a manual documented step.
3. Accept the Microsoft CRT/SDK licence for `xwin` on the Mac (and in a CI job, if one is wanted later).
4. C38 level: strict after WP-2 with per-file counted waivers for every current finding (proposed) or warn first.
