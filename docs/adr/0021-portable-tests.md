# 21. Portable tests: one test kit for paths, text, fixtures, waits and tools, and a check over every test

- Status: Proposed
- Date: 2026-10-27
- Related: rules 3–5 (affected tests, fix locally, failing test first), rule 7 (tests never touch the machine), rule 40
  (recurring bugs mean a missing design rule), rule 42/43 (share first, shared behaviour tested once); ADR 0014 (monorepo,
  affected CI); ADR 0019 (limits explain themselves); ADR 0020 (shared executor and `Clock`); checks C13 (clippy preset),
  C28 (repo baseline), C30 (wall clock in tests), C37 (test binaries); `crates/paddock-testkit`
- Evidence: CI runs 6419–6430; fixes on `fix/dyno-ci` (6598f4286, cb70d00cc, e304123cc, a964fb7a2, 3abdd303b),
  `fix/shared-spotter-ci` (507507a72), `fix/spotter-ci3` (654404cb3), `1f701932b` (clippy on Linux and Windows); the
  survey below (heuristic scans over `git ls-files` of `origin/integrate/monorepo` at a9851eaf9, no builds)

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
on it yet**: 22 shared crates and the shell demo do.

The user decided (2026-10-27): no Windows test host. Before a push, Windows is checked on the Mac by **`cargo xwin clippy`**
(compile and lint only); everything that only shows at run time must be made impossible by the test kit or found by a check.

## The pattern

> Tests fail on Windows (or Linux) whenever a test bakes in a property of the author's OS — path syntax, line endings,
> file-system speed, quoting, tool availability, handle semantics or a product timing — instead of asking the test kit for
> it.

**Invariant:** a test's inputs and expectations are either OS-neutral or obtained from `paddock-testkit`; anything
OS-specific is `cfg`-gated; every wait is for a condition with a hang guard; every external process is bounded.

## Instances found

Heuristic scans (Python over `git ls-files`, Rust test code = files under `tests/` or `benches/` plus everything after the
first `#[cfg(test)]` in a `src/` file; TypeScript = `*.spec.ts`/`*.test.ts`/`e2e/`). Numbers are upper bounds unless
"at risk" is stated; a few examples per pattern. Pacenotes, Sighter and the demo apps have no Rust tests with findings.

| Pattern | Debrief | Dyno | Pitcrew | Pitwall | Spotter | Shared crates | Total |
|---|---:|---:|---:|---:|---:|---:|---:|
| 1. Unix absolute literals in Rust tests (all) | 291 | 356 | 79 | 79 | 71 | 283 | 1,159 in 325 files |
| 1. … at risk: missing-path sentinel, temp root or home root passed to a path/fs API on the same line | 28 | 28 | 18 | 2 | 9 | 38 | **123** |
| 1. hand-rolled `cfg!(windows)` absolute-path helpers | 7 | 7 | 2 | 1 | 0 | 3 | 20 |
| 1. `std::env::temp_dir()` in Rust tests (clippy-allowed) | 141 | 7 | 29 | 0 | 0 | 4 (+2 shell demo) | 183 |
| 2. test data checked out with `text=auto` and no `eol` (CRLF on Windows) | 46 | 0 | 0 | 0 | 0 | 13 (+1 conformance) | **60 files** |
| 2. asserts with a `\n` literal (mostly in-memory strings, fine) | 293 | 99 | 87 | 41 | 15 | 75 | 610 |
| 3. tests creating ≥ 10,000 files | 1 | 0 | 0 | 0 | 1 → 0 on `fix/spotter-ci3` | 1 | **3 → 2** |
| 3. in-memory loops of ≥ 10,000 iterations (slow on loaded runners) | 1 | 1 (2,100 symlinks, Linux) | 3 | 1 | 0 | 5 | ~11 |
| 4. path interpolated into hand-written JSON/TOML in tests | 9 | 2 | 3 | 0 | 0 | 1 | **15** |
| 5. Unix tools or `/proc` in test code without a visible `cfg` | 2 | 9 | 4 | 0 | 2 | 1 | 18 (most gated by their parent `mod`; ~4 need a look) |
| 6. `File::open(..)` then `set_modified`/`set_times` | 0 | 0 | 1 (in a `cfg(unix)` fn) | 0 | 0 | 1 (gated) | 0 open after e304123cc |
| 7. C30 today: wall-clock asserts / sleeps in threaded tests (warn) | 155 | 20 | 5 | 5 | 16 | 5 | **206** (96 / 110) |
| 7. `thread::sleep` ≥ 1 s in tests | 10 | 1 | 2 | 6 | 6 | 1 | 26 |
| 7. literal `recv_timeout`/`wait_timeout` < 10 s in tests | 16 | 2 | 1 | 4 | 7 | 12 | 42 |
| 8. `Command::new` in tests without a bounded runner | 7 (+2 own binary) | 0 (+1) | 3 | 1 (+1) | 4 | 0 (+1) | 15 (+5) |
| TypeScript specs: Unix absolute literals (mock data, low risk) | 345 | 66 | 5 | 1 | 14 | 94 UI + 26 tooling | 553 |
| TypeScript specs: `waitForTimeout`/`setTimeout` ≥ 1 s | 13 | 3 | 4 | 7 | 0 | 13 + 1 | 41 |

Examples:

1. `apps/debrief/crates/debrief-ai/src/runtimes/report.rs:181` `list_builds(Path::new("/no/such/worker"), ..)`;
   `apps/debrief/crates/debrief-ipc/tests/it/launch.rs:384` `format!("/tmp/debrief-sandbox-test-{}", ..)`;
   `apps/dyno/crates/dyno-agent/tests/lookups.rs:136` `"/definitely/not/here"`; already portable by hand:
   `apps/debrief/crates/debrief-core/tests/it/settings.rs:216` `if cfg!(windows) { "C:\\logs" } else { "/var/log" }`.
2. `apps/debrief/crates/debrief-core/parsers/samples/*.log` (16), `apps/debrief/crates/debrief-core/tests/it/snapshots`
   (17) and `debrief-plugins/tests/it/snapshots` (11), `crates/paddock-service/tests/golden` (11). The golden helper
   normalises CRLF (`crates/paddock-testkit/src/golden.rs:103`); whether insta does is not verified here.
   `templates/repo/.gitattributes` and the seven app files lack `*.rs text eol=lf` (the monorepo root covers them today).
3. `apps/debrief/crates/debrief-core/src/watch.rs:604` writes `MAX_SCAN_ENTRIES + 1` = 50,001 files (the same class as
   Spotter's 507507a72); `crates/paddock-jobs/tests/it/walk.rs:282` 12,000 files; in memory:
   `crates/paddock-jobs/tests/it/parallel.rs:13` (50,000,000), `crates/paddock-packs/src/tauri/tests.rs:181` (2,000,000),
   `apps/pitwall/crates/pitwall-core/src/limits.rs:439` (100,001), `apps/dyno/crates/dyno-platform/src/linux/locks.rs:231`
   (2,100 symlinks).
4. `apps/debrief/crates/debrief-core/tests/it/plugins.rs:388` `file='{}'` (a TOML literal string: safe for backslashes,
   breaks on a `'`); `apps/debrief/crates/debrief-collect/tests/it/collect.rs:126` `"path":{:?}` (Rust `Debug` escaping is
   not JSON for non-ASCII); `apps/pitcrew/crates/pitcrew-service/tests/supervision.rs:87`.
5. `apps/debrief/crates/debrief-plugin-windows/src/command.rs:194` `Command::new("kill")`;
   `apps/debrief/crates/debrief-plugins/tests/it/evtx_memory.rs:195` `quiet_command("ps")`;
   `apps/pitcrew/crates/pitcrew-service/tests/supervision.rs:536` `Command::new("ps")`.
7. `apps/debrief/app/src-tauri/src/updater.rs:454` `sleep(60 s)`; `apps/debrief/app/src-tauri/src/jobs/service.rs:1127`
   `recv_timeout(500 ms).is_ok()` as the success condition.
8. `apps/debrief/app/src-tauri/src/video/tests.rs:562` (ffmpeg), `apps/debrief/crates/debrief-plugin-dotnet/tests/it/real_runtime.rs:24`
   (`dotnet --version`), `apps/pitwall/crates/pitwall-cli/tests/cli.rs:98` (own binary, no timeout).

## Decision

### 1. `paddock-testkit` grows one module per shape (the mechanism)

All pure or scratch-bound, no sleeps, each tested once in the kit on all three OSes (rule 43):

| Module | API (sketch) | Replaces |
|---|---|---|
| `paths` | `abs(&["logs", "a.log"]) -> PathBuf` absolute on the host (`/logs/a.log`, `C:\logs\a.log`), never touched; `missing(&Scratch) -> PathBuf` an absolute path that is guaranteed absent; `temp_shaped(name)` / `home_shaped(name)` paths shaped like the host OS's temp or home folder for pure classification code (`/tmp/x`, `C:\Users\tester\AppData\Local\Temp\x`) | patterns 1, the 20 local helpers, `/definitely/not/here`, `temp_exe()` |
| `text` | `assert_text_eq!(actual, expected)` comparing with `\r\n` → `\n` (the golden normaliser, moved here and shared); `read_text(path)` | pattern 2 where a test reads a checked-out file |
| `quote` | `json_str(&Path) -> String` (a JSON string literal, via `serde_json`), `toml_str(&Path) -> String` (a TOML basic string, escaped) | pattern 4 |
| `fixtures` | `files(dir, n, name)` creates `n` small files and refuses `n > MAX_FIXTURE_FILES` (2,000) with "make the limit a parameter"; `big_text(lines)` streamed into one file | pattern 3 |
| `wait` | `eventually(what, cond)` polls a condition with a backoff and fails after `HANG_GUARD` (60 s, one constant, never a product interval) naming `what`; product intervals are driven by the shared `Clock` (`FakeClock::advance`) or a subscription that shortens them, never waited out | pattern 7, the 31 local helpers |
| `tools` (feature `tools`, depends on `paddock-proc`) | `run_tool(cmd, TOOL_TIMEOUT)` on `paddock_proc::run_bounded`; `require_tool("ffmpeg") -> Option<PathBuf>` that prints why it skips, and fails instead under `PADDOCK_REQUIRE_TOOLS=1` (set in the CI jobs that install the tool) so nothing skips silently (rule 5) | pattern 8 |
| `fs` | `set_mtime(path, time)` opens a write handle (and the directory flag on Windows) on every OS | pattern 6 |

And two written rules in the kit's docs and in `docs/rules.md` (proposed rule 44, after acceptance):

- **Limits are parameters.** A production limit (match cap, scan entries, batch size, check interval) is a field or argument
  with the constant as its default. Tests prove the behaviour with a handful of items at a small value, and one cheap test
  pins that production passes the real constant (`match_cap()` in 654404cb3). A magic number a test depends on gets a name
  (`CHECK_EVERY`).
- **No wall-clock assertions in unit tests.** Speed belongs in the benchmarks (`paddock-bench`, `debrief-bench.yml`);
  tests count items or advance a `FakeClock`. C30 already finds them; it becomes an error for new findings (below).

### 2. One conformance check over every test: C38 "Portable tests"

`conformance/src/checks/C38-portable-tests.mjs`, rule 40. It reuses C30's masking (comments and literals) and test-scope
detection, and resolves `mod` declarations so a module gated by its parent (`#[cfg(target_os = "linux")] mod linux;`)
counts as gated. Findings, all in Rust test code:

| Id | Finds | Fix it names |
|---|---|---|
| C38a | a Unix absolute literal passed to `Path::new`, `PathBuf::from`, `fs::*`, `File::*`, `.join` or a parameter named `*path*`/`*dir*`; any literal under `/tmp/`, `/private/var/folders/`, `/var/folders/`, `/Users/`, `/home/` | `testkit::paths` |
| C38b | a loop whose body creates files (`fs::write`, `File::create`, `create_dir*`) with a bound ≥ 1,000 or named like a limit (`MAX_*`, `*_LIMIT`, `*_CAP`); an in-memory loop ≥ 1,000,000 | make the limit a parameter; `testkit::fixtures` |
| C38c | `format!`/`write!` with a placeholder inside JSON/TOML quotes (or `{:?}` after a `"key":`) and a path argument | `testkit::quote` or build with `serde_json`/`toml` |
| C38d | `Command::new`/`quiet_command`/`command(` in test code whose statement does not reach `run_bounded`/`run_tool` | `testkit::tools` |
| C38e | `File::open(..)` followed by `.set_modified`/`.set_times`/`.set_len`/`.set_permissions` | `testkit::fs::set_mtime` |
| C38f | Unix-only tools (`/bin/sh`, `sh`, `ps`, `kill`, `sleep`, `true`, `launchctl`, ...) or `/proc/` in an ungated test scope | gate with `#[cfg(unix)]` or the OS, or use the fake |
| C38g | test-data folders (`tests/fixtures`, `fixtures`, `golden`, `snapshots`, `samples`, `testdata`) with a file whose `git check-attr text eol` is `text=auto` and no `eol` | add `-text` or `eol=lf` (§4) |

C30 gains one finding: a positive wait (`recv_timeout(..).unwrap()/.is_ok()`, `wait_timeout(..)`) with a literal under
10 s → `testkit::wait::eventually`. A negative wait ("nothing arrives within 100 ms") stays allowed.

**Waivers.** One line: `// conformance: portable — <reason>` on the line or the line above (the same shape as C30's
`hang-guard`), for string data that never reaches the OS (a Unix path parsed as log content, a mock path). Missing reason
is a finding. Migration backlog: per-**file** entries in `paddock.waivers.toml` (generated once, dated 2026-12-31), never a
glob over an app, so a new test file is checked from day one.

**Level.** Error from the start for the shared crates, the demos and new files; the generated per-file waivers cover
today's app findings until their work package removes them. C30 becomes `--strict` in CI for files without a waiver.

### 3. Clippy for what clippy can see, and xwin before push

- `clippy::disallowed_methods` cannot read string literals or tell test code from production code (there is no
  `allow-disallowed-methods-in-tests`; [clippy lint configuration](https://doc.rust-lang.org/clippy/lint_configuration.html)),
  so the path and loop checks live in C38. The preset (`lint/clippy.toml`, propagated by C13) adds what is type-based:
  `std::fs::File::set_modified` and `std::fs::File::set_times` (reason: `paddock-files`/`testkit::fs::set_mtime`, a write
  handle on every OS); `std::process::Command::output`, `::status` and `std::process::Child::wait` (reason:
  `run_bounded`, a timeout), allowed with a reason inside `paddock-proc` and the apps' process helpers. Production
  call sites are counted and allowed with reasons in WP-6 before the entries go in.
- **`scripts/xwin-clippy.sh`** wraps the Windows lint so nobody types it differently:
  `cargo xwin clippy --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings` for each affected Cargo
  workspace (`node scripts/affected.mjs --files` on the branch's changed files → `crates/` and `apps/<app>/...`), with its
  own target folder (`$CARGO_TARGET_DIR/xwin`), `CARGO_BUILD_JOBS=2`, through the machine-wide `heavy.sh` slot. It
  catches the compile half of pattern 5 (Unix-only APIs used ungated, `cfg(windows)` code that rotted, Windows-only clippy
  findings as in 1f701932b). It runs no tests and links nothing for the target (build scripts are built for the host),
  so the `rust-lld.exe` linker setting of `.cargo/config.toml` does not apply. A missing `cargo-xwin` is a loud message
  with the install line, never a silent pass.
- It is a documented pre-push step in `docs/dev-setup.md` and in every brief, and a `pre-push` command in `lefthook.yml`
  that runs only for staged Rust changes (decision for the user below: hook or manual).
- cargo-xwin (MIT/Apache-2.0, [rust-cross/cargo-xwin](https://github.com/rust-cross/cargo-xwin)) downloads the MSVC CRT
  and Windows SDK headers and libraries with `xwin` and builds C code with `clang-cl`. Installed on the Mac (0.23.1,
  verified with `cargo xwin --version`); not yet run against these workspaces (this ADR made no builds).

### 4. `.gitattributes` policy

- Source text: `* text=auto` plus `*.rs text eol=lf` (already at the root, 3abdd303b) also in `templates/repo/.gitattributes`
  so C28 carries it into every app's file and a future split.
- Test data is byte-exact: `**/tests/fixtures/** -text`, `**/fixtures/** -text`, `**/samples/** -text`,
  `**/testdata/** -text`; expected output is LF: `**/golden/** text eol=lf`, `*.snap text eol=lf`. C38g checks it over
  every file, not a sample.
- Tests that read checked-out text still compare through `testkit::text`, so a missed attribute fails softly.

## Alternatives

- **A Windows test host or VM before every push.** Rejected by the user (cost, load, one more machine to keep healthy).
- **Run tests under Wine (`cargo xwin test`).** Runs some tests, but Wine's file system, process and handle semantics are
  not Windows' (patterns 3 and 6 would not reproduce); a green Wine run would be false confidence.
- **Only clippy.** Cannot see literals, loops or quoting (above); leaves five of eight shapes unchecked.
- **Fix instances as CI finds them.** What happened this week; 20 local helpers show it does not converge.
- **Per-app test helpers.** Rule 42: the helpers are generic and already copied 20 and 31 times.

## Consequences

- Every app takes `paddock-testkit` as a dev-dependency (none does today); the migrated tests read shorter.
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
| 1 | `paddock-testkit`: `paths`, `text` (move the golden normaliser), `quote`, `fs`, `fixtures`, `wait` (`eventually`, `HANG_GUARD`), `tools` behind a feature; ≥ 99 % lines; tests run on all three OSes in CI | `crates/paddock-testkit/` | implementer | M (1 day) |
| 2 | C38 a–g with fixtures and tests; C30 positive-wait finding; generate per-file waivers for today's app findings; CI runs C38 strict for shared crates | `conformance/src/checks/C38-portable-tests.mjs`, `conformance/test/`, `paddock.waivers.toml` | implementer (reviewer for the heuristics) | M (1 day) |
| 3 | `.gitattributes` policy in root and `templates/repo`; renormalise; C28 sync of the app files | `.gitattributes`, `templates/repo/.gitattributes`, `apps/*/.gitattributes` | implementer | S |
| 4 | `scripts/xwin-clippy.sh` (affected workspaces, own target dir, heavy slot), `docs/dev-setup.md`, optional lefthook `pre-push`; one cold run per workspace to record time and any `-sys` crate trouble | `scripts/`, `docs/dev-setup.md`, `lefthook.yml` | implementer | S–M |
| 5 | Migrate the shared crates (38 at-risk paths, 12,000-file walk, 50 M loop, 12 short positive waits) and remove their waivers | `crates/*` | implementer | S |
| 6 | Clippy preset entries (`set_modified`, `set_times`, `output`, `status`, `wait`) after counting and allowing the production sites | `lint/clippy.toml`, app `clippy.toml` | implementer | S |
| 7 | Per app, one branch each, in the release order: Debrief (largest: 28 at-risk paths, 50,001-file test, 141 `temp_dir`, 155 C30, 9 JSON/TOML), Dyno, Pitcrew, Pitwall, Spotter; delete the local helpers and the app's waivers | `apps/<app>/` | implementer | Debrief L, others S–M |
| 8 | Rule 44 into `docs/rules.md` and every app's `AGENTS.md` copy; lesson already recorded | docs | coordinator | S |

WP-1 to WP-4 can run in parallel (separate files); WP-5 and WP-7 need WP-1 and WP-2; WP-6 after WP-5.

## Risks

- **C38 noise** makes people waive by reflex. Mitigation: the waiver needs a reason; the reviewer samples waivers per WP;
  the heuristics start narrow (the "at risk" 123, not the 1,159).
- **cargo-xwin and C/C++ `-sys` crates** (DuckDB bundled, zstd, sqlite, ring/aws-lc): a crate's build script may not
  cross-compile with clang-cl, or take long. Mitigation: WP-4 measures each workspace once; a workspace that cannot be
  linted is listed with the reason and falls back to `cargo check --target x86_64-pc-windows-msvc` for the crates that
  can (the existing AGENTS.md rule).
- **Microsoft licence:** xwin downloads the MSVC CRT and Windows SDK, whose licence the user accepts on first use
  (`--accept-license` / `XWIN_ACCEPT_LICENSE`); see the decision below.
- **`require_tool` skipping** hides a missing tool locally; CI must set `PADDOCK_REQUIRE_TOOLS=1` where tools exist, or the
  tool test never runs anywhere.
- **Dev-dependency cycle:** `paddock-testkit[tools]` → `paddock-proc`, while `paddock-proc` may want the kit in its tests;
  Cargo allows dev-dependency cycles, publishing order must follow `scripts/publish.mjs`.

## Migration and rollback

Migration is WP-5 then WP-7, one small commit per pattern group, with C38 turning green for the files touched as the
proof. Rollback: C38 can be dropped from `checks/index.mjs` and the waivers deleted; the testkit modules are additive; the
`.gitattributes` change is reverted with a renormalise.

## Decisions for the user

1. Accept this ADR (status Accepted, date) and proposed rule 44 ("tests ask the test kit for anything OS-specific; limits
   are parameters; no wall-clock assertions in unit tests").
2. xwin clippy as a lefthook `pre-push` command (automatic, minutes on a cold cache) or a manual documented step.
3. Accept the Microsoft CRT/SDK licence for `xwin` on the Mac (and in a CI job, if one is wanted later).
4. C38 level: error with per-file waivers (proposed) or warn first.
