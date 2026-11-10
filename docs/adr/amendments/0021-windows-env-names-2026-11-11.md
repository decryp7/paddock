# Amendment to ADR 0021 (2026-11-11): environment variables are read through one case-aware map

- Status: Accepted 2026-11-11 (user), all decisions as recommended (D-E1, D-E2). The user accepted it on 2026-11-11 in chat, as relayed by the coordinator on 2026-11-11 (the user's words are not in
  the repository; ADR 0097's signed approvals are not built). History: Proposed
  2026-11-11; revision 2 (`f3d313123`) answered the independent review of revision 1 (`dbd7028a1`: see "Review answers")
  and was approved, with its wording P2s (the standard library's source cited, `apply_to` clearing the environment)
  folded in here. An amendment to ADR 0021 (accepted 2026-10-27, on `origin/main`); merged into its text by WP-E4.
  Nothing here is built yet. ER-23 does not apply.
- Amends: ADR 0021 decision 5 (the shared crates are the only path for OS-specific calls: `paddock-env` gains the
  environment map), §3 (two Clippy entries) and the pattern table (a tenth shape).
- Related: ER-21 / rule 40, ER-22 / rule 42, rule 55 / ADR 0034, ADR 0087 (accepted, on
  `origin/docs/adr-windows-gate-ratchets` only), ADR 0094 §6 (Clippy rollout while apps are frozen), ADR 0096 (register
  row `docs/adr/register/R-2026-11-11-0021-windows-env-names.toml`).
- Overlap checked: `paddock-env`'s `Vars`/`ProcessVars`/`MapVars` (`dirs.rs:34-70`) is the existing port, extended, not
  duplicated; `tools/paddock-dev`'s `release_isolation::pathfind` is the copy that goes. No ADR or amendment on
  `origin/main`, the train or the pinned ADR branches defines an environment map (`git grep -i "env::vars\|EnvMap"
  docs/adr`). ADR 0087 is cited only as the later run-time check.
- Evidence: read on 2026-11-11 with `git show`, `git grep` and `git log` after `git fetch`, and the Rust standard library
  documentation (below). Paths are from `origin/train/2026-11-11-5` unless named. No user data was read. Heads, re-read
  right before the commit:

```evidence-heads
read: 2026-11-11
origin/main ea94d9f1e
origin/train/2026-11-11-5 55c056089
origin/fix/paddock-dev-windows-tests 1cb9a4a5b
origin/docs/adr-windows-gate-ratchets db7782e01
```

## Trigger: the instance

`tools/paddock-dev`'s release-isolation check (ADR 0085) read the job's environment into `Env(BTreeMap<String,
String>)` with `std::env::vars()` and looked variables up exactly (`verify.rs` at `9a0187724^`: `Env::from_process` is
`Env(std::env::vars().collect())`, `Env::get` is `self.0.get(key)`). The test fixture filled the same map with
`std::env::var(key)` for `"PATH"`, `"SYSTEMROOT"`, ... and stored the value under the upper-case name
(`fixture.rs` at `9a0187724^`). On Windows the process variable is spelled `Path`, so the fixture held `PATH` and passed,
while every real job's map held only `Path` and `env.get("PATH")` was `None`: the npm and node probes would have
reported "check broken" on every Windows release job. The commit's own comment: "on Windows the path variable is
`Path`, and a probe that reads `PATH` exactly would pass here and fail on every real job" (`9a0187724`,
`release_isolation/fixture.rs`).

The fixes: `9a0187724` (lookups ignore case on Windows, the fixture keeps the process's spelling) and `1cb9a4a5b`
(`pathfind::normalise`: one key per variable on Windows, so `get`, `set` and a child's environment agree). Both are on
`origin/fix/paddock-dev-windows-tests` and the train, in no tag. **No CI run failed on it** (the commits name no failing
run; the defect is shown by reading the two sides at `9a0187724^`): run 7904's two release-isolation failures (`a_missing_kind_or_no_crates_is_check_broken_never_a_pass`,
`every_planted_leak_fails_and_names_its_probe`) are the neighbouring npm-probe defect fixed in `c5dbc875a`, a different
cause.

**Platform facts.** The standard library: "environment variable names are case-insensitive (but case-preserving) on
Windows and case-sensitive on all other platforms" (`std::process::Command::env` and `::envs`,
<https://doc.rust-lang.org/std/process/struct.Command.html>, read 2026-11-11). So `Command::env("PATH", ..)` replaces
`Path` in a child, while a `HashMap`/`BTreeMap` built from `std::env::vars()` keeps two distinct keys. The
standard library's source agrees for children: `EnvKey`'s `Ord` compares names with `CompareStringOrdinal(.., TRUE)`
(`library/std/src/sys/process/windows.rs:74-82`), and `CommandEnv::set` inserts into a `BTreeMap<EnvKey, ..>`
(`library/std/src/sys/process/env.rs:50-53`), whose `insert` keeps the existing key and replaces the value; read in the
local toolchain's sources, rustc 1.99.1 (`58cf8f9c3`), 2026-11-11. That `std::env::var("PATH")` finds `Path` on
Windows is the behaviour the fixture relied on and the commit describes; WP-E0 records it on the test VM.

## Instances: every map of the process environment

`git grep` for `env::vars()` and `env::vars_os()` over tracked `*.rs`, outside `vendor/`: **13 call sites**, grouped in
the 10 rows below (row 4 holds four), each read:

| # | Site | What it builds | Case rule today |
|---|---|---|---|
| 1 | `tools/paddock-dev` `release_isolation/verify.rs:19` | `Env` via `pathfind::normalise` | fixed (`1cb9a4a5b`) |
| 2 | `tools/paddock-dev` `release_isolation/fixture.rs:344` (test fixture) | names filtered with `eq_ignore_ascii_case` | fixed (`9a0187724`) |
| 3 | `tools/paddock-dev` `ci_isolation/mod.rs:242` | `HashMap<String, String>` read with exact `ctx.get(..)` | **at risk**: reads only names the workflows set in upper case (`RUNNER_TEMP`, `PADDOCK_*`, `HOME`); a Windows system name read later would miss |
| 4 | `tools/paddock-dev` `hook/install.rs:149`, `input_hash.rs:417`, `tests/it/golden.rs:383`, `tests/it/wrapper.rs:97` | name lists and filters | not read in detail (WP-E2) |
| 5 | `crates/paddock-build/src/lib.rs:79` `Env::process` | `BTreeMap<String, String>`, exact `get` | at risk (reads Cargo's upper-case build variables today) |
| 6 | `crates/paddock-setup/src/native/apply.rs:397` | the installer's environment block, `TEMP`/`TMP` matched upper-cased | correct (folds case by hand) |
| 7 | `crates/paddock-proc/src/testchild.rs:126` | a test child's environment | not read in detail (WP-E2) |
| 8 | Debrief `debrief-collect/src/collector.rs:370` into `env.rs:19` `snapshot` | allow-list filter with `eq_ignore_ascii_case` | correct (folds case by hand) |
| 9 | Debrief `debrief-ipc/src/sandbox.rs:413` | the names only | not read in detail (WP-E2) |
| 10 | Pitcrew `pitcrew-service/src/disclaim.rs:90` | `BTreeMap<OsString, OsString>` for `posix_spawn` | Unix only (`std::os::unix`), not affected |

Hand-made maps that stand in for the environment: `paddock-env`'s `MapVars` (`dirs.rs:51`, a `BTreeMap` with an exact
`get`, the test double of `ProcessVars`, whose `get` is `std::env::var_os` and so ignores case on Windows): a test that
writes `MapVars::new().with("Path", ..)` and code that asks for `PATH` disagree with the real process the same way.
Pitcrew `pitcrew-core/src/runner_env.rs:150` inserts `"PATH"` into a map the runner's `[runners.env]` overrides extend; a
user override spelled `Path` becomes a second key, which `Command::envs` then folds (later wins; `BTreeMap` order puts
`PATH` before `Path`, so the override wins by accident).

**Copies of the rule:** three hand-written case folds (rows 2, 6, 8) and one shared-looking one inside a tool
(`release_isolation/pathfind.rs` `lookup`, `normalise`).

## Pattern

**A process environment copied into an ordinary string map loses Windows' case-insensitive names**, while the code that
fills tests and the OS itself (`std::env::var`, `Command::env`) keep them; so tests pass and real jobs miss `Path`,
`SystemRoot`, `ComSpec`, `windir` and the like, or children get two spellings.

**Invariant.** The family reads the process environment as a map only through one type whose lookups follow the
platform's rule, and builds test environments with the same type.

## Change

1. **`paddock_env::EnvMap` (D-E1).** In `paddock-env` (it already owns the `Vars` port, `ProcessVars` and `MapVars`):
   - `EnvMap::from_process()` (the one `std::env::vars_os` call, with the one `#[expect]`), `EnvMap::of(pairs, os)`;
   - **the name rule.** On Windows two names are the same variable when they are equal under **Windows ordinal
     ignore-case** (`CompareStringOrdinal(.., bIgnoreCase = TRUE)`, the OS uppercase table;
     <https://learn.microsoft.com/en-us/windows/win32/api/stringapiset/nf-stringapiset-comparestringordinal>, read
     2026-11-11), not ASCII-only folding (revision 1 said ASCII, as `pathfind::normalise` does today). The pure
     function uses the same per-character simple uppercase as `paddock_env::winpath::same` (one shared fold, tested
     once); that it equals the OS table outside ASCII is **unverified** and WP-E0 compares the two on the Windows CI
     leg. The standard library's own Windows `Command` environment uses the same comparison (`EnvKey`'s `Ord`,
     `sys/process/windows.rs:74-82`, rustc 1.99.1). Off Windows names are exact;
   - **construction from the process, in a fixed order.** `from_process` collects `std::env::vars_os()` into a
     `BTreeMap` (byte order of the names, as `pathfind::normalise` iterates today) and then folds: for each group of
     names that are the same variable, the **first in byte order** keeps its spelling (`PATH` sorts before `Path`), and
     the value is the one the process reports for that name (`var_os`, the OS's own lookup), so a duplicate in the
     process block cannot decide the value by iteration order;
   - **`of` and `set` with a name that is already there** (in any spelling): the **later value wins and the existing
     spelling is kept**, the case-preserving rule `Command::env` documents ("case-insensitive (but case-preserving) on
     Windows"); so `of([("PATH","a"),("Path","b")], Windows)` holds one entry `PATH=b`. On Unix they are two entries;
   - `get`, `set`, `remove`, `names`, `iter`, and `apply_to(&mut Command)` (`env_clear` then `envs`), so a child gets
     exactly one spelling. Because `apply_to` clears the inherited environment first, the family stays self-consistent
     even when the inherited block itself holds two spellings of one name: the child gets the map's one entry, never the
     OS's leftovers;
   - it implements `Vars`, and `MapVars` becomes `EnvMap` (or a thin constructor of it), so `AppDirs` tests and product
     code share the rule. The platform is a parameter (`Os`), so macOS and Linux test the Windows rule.
2. **Every copy moves (rule 55).** `release_isolation`'s `Env` and `pathfind::{lookup, normalise}`, `ci_isolation`'s
   `HashMap`, `paddock-build`'s `Env`, rows 4, 7 and 9 after reading, and the two hand folds (rows 6 and 8) where they
   read a map; Pitcrew's `runner_environment` builds an `EnvMap` so an override replaces `PATH` whatever its spelling.
3. **Clippy (the case lint).** `disallowed-methods` gains `std::env::vars` and `std::env::vars_os`, reason "read the
   environment as a map through `paddock_env::EnvMap` (Windows names ignore case); ADR 0021 amendment 2026-11-11". Rollout
   by ADR 0094 §6 (`crates/clippy.toml`, `tools/*/clippy.toml`, unfrozen apps, `lint/clippy.toml` after the last frozen
   tag). A site that only lists names, or is Unix-only (row 10), carries `#[expect(clippy::disallowed_methods, reason =
   "...")]`. **Positive control (ER-33):** a fixture crate that calls `std::env::vars()` fails with the preset.
4. **The contract test, once (ER-22).** `EnvMap`'s tests: `of([("Path", "a")], Windows).get("PATH") == Some("a")`; on
   Unix the same is `None`; `of([("PATH","a"),("Path","b")], Windows)` has one entry; `apply_to` gives a child one
   variable (asserted on the test VM through ADR 0087 once built; on CI's Windows leg until then).

## What this does not cover

- Maps built by hand that never came from the process and are never handed to one (a config file's tables): Clippy
  cannot see them. A text scan for literal Windows names used as map keys (`.get("PATH")` on an `env`-named receiver)
  was measured on the train: 9 hits, all on `release_isolation`'s already fixed `Env`; it would add noise without a
  current instance, so it is not proposed (D-E2).
- Values: `PATH` separators and `PATHEXT` handling stay in their users (`pathfind::find_program`).

## Release impact (ER-31)

None of rows 1 to 5 ships in an app (`tools/` and `paddock-build`, a build-script helper). Row 6 is in `dyno-v1.1.0`
and correct. Nothing here gates a tag.

## Work packages (raw agent-days, one agent)

| WP | Content | Days |
|---|---|---|
| WP-E0 | On the CI Windows leg or the test VM, once: what `std::env::vars()` spells (`Path`), that `std::env::var("PATH")` finds it, that `Command::env("PATH", x)` replaces `Path` in the child, and the fold compared with `CompareStringOrdinal` over a word list; recorded in the WP report | 0.25 to 0.5 |
| WP-E1 | `paddock_env::EnvMap` with `Vars`, `MapVars` folded into it, the construction order, `of`/`set` duplicates, `apply_to`, the contract tests on both platform values; 99 % lines | 1 to 1.25 |
| WP-E2 | Migrate the 13 sites and Pitcrew's `runner_environment`, each at-risk row with a failing test first (a fixture spelled `Path`, a lookup of `PATH`); delete `pathfind::{lookup, normalise}`; rows 4, 7 and 9 are read for the first time here and may need more | 0.75 to 1.25 |
| WP-E3 | Clippy entries and the fixture crate, by ADR 0094 §6's route | 0.25 |
| WP-E4 | Lesson, ADR 0021 text merge, register row states | 0.25 |
| | **Total** | **2.5 to 3.5** |

**CI runs (rule 50).** WP-E1 and the shared-crate part of WP-E2 are one landing (`crates/`: every workspace that uses
`paddock-env` runs, one branch run); the `tools/paddock-dev` and Pitcrew parts ride in it or take one run each; WP-E3's
Clippy entries ride with them. About **1 to 3 branch runs**, plus a fix round if red and the train's landing run.

## Failing tests the work starts with (ER-11; not written in this change)

- `tools/paddock-dev` `ci_isolation`: a context built from an environment spelled `Runner_Temp` on Windows finds
  `RUNNER_TEMP` (today `None`).
- `paddock-build`: `Env::of([("Out_Dir", "x")])` with the Windows rule finds `OUT_DIR`.
- `paddock-env`: `MapVars::new().with("Path", "x")` resolved for `Os::Windows` answers `get("PATH")`.
- Pitcrew `runner_env`: `runner_environment(Some("/a"), {"Path": "/b"})` holds one path variable, `/b`.

## Decisions (accepted 2026-11-11 as recommended)

| # | Question | Options | Recommended and chosen |
|---|---|---|---|
| D-E1 | Where the map lives | (a) `paddock_env::EnvMap`, beside `Vars`; (b) keep `pathfind` in `tools/paddock-dev` and copy the rule where needed | **(a)**: product (`paddock-build`, Pitcrew) and tools both need it (ER-22) |
| D-E2 | The lint | (a) Clippy on `std::env::vars` and `vars_os` plus the shared type; (b) also a text scan for literal Windows variable names as map keys | **(a)**: (b) has no current instance outside the fixed site and its heuristic cannot tell an environment map from an event's `Path` field (Debrief has several) |

## Review answers (revision 1, `dbd7028a1`)

| Finding | Answer in revision 2 |
|---|---|
| P2 `EnvMap` underspecified | construction order (byte order, first spelling kept, the process's value), `of`/`set` duplicates (later value wins, spelling kept), Windows ordinal ignore-case with its unverified edge |
| P2 register summary "ten maps" | 13 call sites in 10 rows, here and in the register row |
| P2 estimate ranges and CI runs | ranges (2.5 to 3.5); 1 to 3 branch runs |
| P2 pins and overlap | `origin/docs/adr-windows-gate-ratchets` pinned; "Overlap checked" line |

## Rollback

Remove the two Clippy entries; `EnvMap` is additive, and reverting WP-E2 restores the copies.
