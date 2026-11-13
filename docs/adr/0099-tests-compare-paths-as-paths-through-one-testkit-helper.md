# 99. Tests compare paths as paths, through one testkit helper

- Status: Proposed 2026-11-13. Drafting approved by the user on 2026-11-13 (ER-21); nothing here is accepted or built.
  ER-23 does not apply (no drawings).
- Date: 2026-11-13
- Related: ER-21 / rule 40, ER-22 / rules 42 and 55, ER-37 / rule 76, ER-41; ADR 0021 amendment 2026-11-11 (Windows
  verbatim paths, `paddock_env::winpath`); ADR 0022 (Rust-first tooling, the RT-5 check host); ADR 0095 (testkit helpers).
- Number: 0099. On 2026-11-13 the highest ADR on `origin/main` and on every `origin` branch was 0097
  (`git ls-tree` of `docs/adr/` over all 254 remote branches); 0098 is left to the "combined tree before approval" ADR being
  drafted at the same time on `docs/adr-combined-tree-before-approval`.
- Evidence: read on 2026-11-13 with `git show`, `git grep`, `git merge-base --is-ancestor`, `git tag --contains` and
  `./dev ci triage <run> --no-history` after `git fetch`. No user data was read. The heads, re-read right before the commit:

```evidence-heads
read: 2026-11-13
origin/main f7bbe2b86
origin/train/2026-11-12-7 f7bbe2b86
origin/train/2026-11-13-8 3a342cb42
```

## Context: the class

A test builds the expected path as text, or compares path text, in the spelling of the OS it was written on. On the
Windows runner the code under test spells the same place differently and the test fails; the product is right. The
spellings that differ there:

1. separators: `\` from `Path::join`/`display()`, `/` from a `format!("{}/x", p.display())` or from a tool (`C:\a/x`);
2. the verbatim prefix `\\?\` from `canonicalize`;
3. Git bash's `/c/Users/...` (its `pwd`, `$HOME`, `$RUNNER_TOOL_CACHE`) for `C:\Users\...`;
4. drive-letter and name case (`c:` and `C:`), which Windows ignores;
5. the list separator (`;`, not `:`) of `PATH`-like variables.

Instances, each a real Windows-only red test in the run named (checked with `./dev ci triage`); no tag contains any fix
(`git tag --contains` is empty for all of them), so nothing shipped was affected:

| Run (job) | Failing tests | Spelling | Fixed by |
|---|---|---|---|
| 7943, `crate (windows-latest)` | `paddock-diag perf::machine::tests::a_folder_that_does_not_exist_yet_resolves_to_its_nearest_existing_parent` | 2 | `1bd84ff5b` (main) |
| 8027, `paddock-dev (windows-latest)`, train 7 | 7 `ci_host::{build_folder,node_caches,stable_workspace}` tests | 1, 2 | `0e996a61c` (main) |
| 8027, same job | 2 `wait::tests` lock-key tests (`file:/l` has no drive), `ci_isolation_windows::a_slots_port_and_kept_folder_on_a_windows_host` | 1, 2 | `0ba535754` (main) |
| 8034, `paddock-dev (windows-latest)`, train 7 | `queue::tests::the_lockfile_loop_of_the_train_gates_runs_on_the_real_repository_and_visits_no_fixture` | 3 | `c75dc801c` (main) |
| 8050, `paddock-dev (windows-latest)`, train 8 | `generators::tests::the_default_build_roots_are_the_disk_settings_roots`, `it generators::a_tool_lockfile_entry_marks_the_manifest_of_every_shared_crate_the_tool_depends_on` | 5, 1 | `4e9ebe00f` (train 8, not on main) |

Earlier ones of the same shape: `2f3cedf14` (2026-11-04, wait test plock path) and `6598f4286` (2026-10-27, Dyno tests
with a bare `/x`). The same run 8027 also failed `pre_push::tests::a_pushed_commit_is_checked_out...` on `core.autocrlf=true`
(fixed in `0ba535754`): a line-ending trait of the same runner, not a path spelling; it is noted, not covered here.

## What exists (ER-22: look before adding)

- `paddock_env::winpath` (`crates/paddock-env/src/winpath.rs`, the one door to `canonicalize`): `same` (lexical: verbatim
  prefix dropped, `/` and `\` one separator, ordinal case fold, Windows only; exact elsewhere), `starts_with`,
  `lexical_text` (`C:/a/b`), `components`, `kind`. Pure, with `Os` as a parameter, so every OS tests the Windows rules.
  Clippy's `disallowed-methods` in `crates/clippy.toml` (lines 6-7) bans `std::fs::canonicalize` and
  `Path::canonicalize` outside it (`real_verbatim` holds the one `#[expect]`). That ban is in the shared crates' preset only:
  `tools/paddock-dev/clippy.toml` and `lint/clippy.toml` do not list it.
- `paddock_testkit` already re-exports `winpath::{real_or_given, same}` (`crates/paddock-testkit/src/lib.rs:44`); there is
  no assertion macro and no text helper.
- Git bash spellings are converted only in `paddock-dev`: `ci_isolation::home::from_git_bash` (pub, used by five modules),
  and a second private copy `native_path_of_bash` added by `c75dc801c` in `tools/paddock-dev/src/queue/tests.rs`.
  `ci_isolation::home::mixed` repeats `winpath::lexical_text`; `ci_host::fwd` (`0e996a61c`) wraps it for tests; none of
  these is in `docs/known-copies.toml`.
- `release_isolation::pathfind` finds programs on `PATH`; it is not a comparison helper and is not touched.
- The Rust check host `./dev check run` (`tools/paddock-dev/src/check/`, C39 and C53 so far) with shrink-only baselines
  (`check/baseline.rs`) is on `origin/main`, but no workflow runs it yet: `ci.yml` runs only `./dev check ipc-endpoints`
  (`git grep "check run" .gitea` finds nothing). Its CI wiring is the RT-5 hand-off, a prerequisite of WP-2 below.

## Common practice (ER-37)

- **Rust std.** `Path` equality goes by components: repeated and trailing separators and interior `.` are normalised, and
  on Windows `/` and `\` both separate, so comparing `Path`s (not text) already fixes spelling 1. It is case-sensitive
  and `Prefix::VerbatimDisk` is a different variant from `Prefix::Disk`, so `\\?\C:\a` and `C:\a` differ (spellings 2 and
  4 stay). Sources: <https://doc.rust-lang.org/std/path/struct.Path.html> (`components`),
  <https://doc.rust-lang.org/std/path/enum.Prefix.html>.
- **dunce** turns `\\?\` paths into the legacy form where possible and is a no-op elsewhere; it is what most Rust tools
  use instead of `canonicalize` (<https://docs.rs/dunce/latest/dunce/>). `winpath::real` is Paddock's equivalent.
- **path-slash** converts paths to and from `/`-separated "slash paths" (<https://docs.rs/path-slash/latest/path_slash/>);
  `winpath::lexical_text` is the equivalent.
- **cargo's own tests**: "When checking output, use `/` for paths even on Windows: the actual output of `\` on Windows will
  be replaced with `/`" (<https://doc.crates.io/contrib/tests/writing.html>): expected text is written once and the
  harness normalises the actual side. snapbox, which cargo's tests use, has `filter::normalize_paths` ("Normalize path
  separators") and `Redactions` ("Replace data with placeholders")
  (<https://docs.rs/snapbox/latest/snapbox/filter/index.html>). That cargo redacts the sandbox root as `[ROOT]` is from
  memory and was not verified on a page (unverified).
- **Node**: on Windows `path` accepts both separators but its methods only produce `\`; `path.normalize` folds repeated
  separators into `\`; `path.win32`/`path.posix` give one OS's rules on any OS; `basename` does not fold case
  (<https://nodejs.org/api/path.html>).

None of these lints for text comparisons; the common practice is "compare `Path`s, normalise the actual side, write
expected values with `/`". Paddock needs a little more because three of the five spellings (verbatim, Git bash, case)
survive `Path` equality, and because the CI Windows runner is the only place the class shows, after a full CI round trip.

## Decision

1. **One helper, in `paddock_testkit::path`** (Rust), built on `winpath`:
   - `same_path(actual, expected) -> bool`: `winpath::same` after reading a Git bash spelling (`/c/x`) as `C:/x` on Windows;
     `assert_same_path!(actual, expected)` prints both spellings and the normalised forms on failure. Exact off Windows.
   - `text(path) -> String`: the tools' spelling (`winpath::lexical_text` for the running OS: `/`, no prefix), for building
     an expected line that a program printed; `under(base, "a/b")`: `rel` joined component by component.
   - `from_git_bash` moves into `paddock_env::winpath` (pure, `Os` parameter) as the one implementation; the `paddock-dev`
     copies (`home::from_git_bash`, `queue/tests.rs::native_path_of_bash`, `home::mixed`, `ci_host::fwd`) call it or are
     deleted, and the mechanism gets its `docs/known-copies.toml` entry (rule 55).
   - Not covered: the list separator (spelling 5) is `std::env::join_paths` (as `4e9ebe00f` does), named in the helper's
     docs; line endings stay with the `.gitattributes` rules and a per-test `core.autocrlf=false`.
2. **A check in the Rust host** (`tools/paddock-dev/src/check/`, beside C53; next free C number when it lands), over test
   code only (files under `tests/`, `tests.rs`, `*_tests.rs` and `#[cfg(test)]` modules), comments and strings masked as
   C53 does:
   - rule A: a `format!` that joins a `.display()` argument with a literal `/` or `\` (use `join`, `under` or `text`);
   - rule B: a hand separator fold `.replace('\\', "/")` on path text (use `text` or `same_path`);
   - rule C (warning only): `assert_eq!`/`assert_ne!` with an operand that is path text (`.display().to_string()`,
     `.to_str().unwrap()`, `.to_string_lossy()`, not of a `file_name()`): a heuristic with many false positives, kept as a
     warning so reviewers see it.
   Clippy cannot express rules A-C (`disallowed-methods` bans calls everywhere, not patterns in tests), so the host is the
   right place. Per ER-41 the check is report-only until its first green run on the real tree, recorded by run id where it
   is defined; then rules A and B block with a shrink-only baseline of the sites left (D2).
3. **TypeScript: no helper now.** The class has no recorded TS instance: `git log -i --grep=windows -- '*.mjs' '*.ts'`
   since 2026-10-15 finds product fixes (`ed1b0cba5`, `affected.mjs` cargo keys) but no test that compared path text, and the
   `node --test` and conformance jobs in `ci.yml` run on the Linux image. A TS twin is added only when a TS instance
   appears (rule of three).

## Existing instances the check would flag

Measured on `origin/main` `f7bbe2b86` with a throwaway regex scan over test code (not committed; the check's first
report-only run is the real count). Rules A and B: **65 sites in 55 files** (A 23 in 17 files, B 42 in 38 files), by area
`tools/paddock-dev` 23, `apps/debrief` 22, `apps/dyno` 12, shared crates 6, other tools 2. Top files:
`tools/paddock-dev/tests/it/setup_rust.rs` 3, `tools/paddock-dev/tests/it/wrapper.rs` 3,
`apps/dyno/crates/dyno-agent/src/disk/folder.rs` 3, then 2 each in `tools/paddock-dev/tests/it/ci_isolation_windows.rs`,
`tools/paddock-dev/tests/it/inputs.rs`, `apps/debrief/crates/debrief-plugins/tests/it/formats_samples.rs` and
`apps/dyno/crates/dyno-platform/src/dev_folders.rs`. Rule C adds 78 candidates in 48 files; a hand sample of 8 found
most to be paths passed as command arguments, not comparisons, which is why C only warns. Rule B sites are mostly
already-working hand folds (the fix moves them onto the helper); rule A sites include path inputs that Windows accepts
with mixed separators, so not every flagged site is a latent failure (precision of A was not measured beyond the sample).

## Work packages and cost (agent-days)

| WP | What | Days |
|---|---|---|
| WP-1 | `winpath::from_git_bash` (moved, `Os` parameter, tests on every OS), `paddock_testkit::path` (`same_path`, `assert_same_path!`, `text`, `under`) with failing-first tests for the five runs above, the four `paddock-dev` copies onto it, the known-copies entry | 0.5 |
| WP-2 | The check (rules A-C) in the Rust host, fixtures, report-only; first green run recorded | 0.5 |
| WP-3a | Baseline the 65 A/B sites (shrink-only); each is fixed when its file is next touched | 0.1 |
| WP-3b | Instead of 3a: migrate all 65 sites now (Debrief and Dyno sites after their current tags, as in ADR 0094) | 0.75 |

Total: **1.1 days** with WP-3a, **1.75 days** with WP-3b (0.75 over the 1-day budget the user named; needs a yes).
The common-practice option (D1 (b)) costs **0.25 day**. Prerequisite for WP-2 to run in CI: `./dev check run` wired into
the conformance job (RT-5 hand-off, not on main); until then it runs only by hand (`./dev check run`).

## Decisions for the user

- **D1. Scope.** (a) Helper plus check (WP-1, WP-2, WP-3a), 1.1 days. (b) Common practice only: compare `Path`s, use the
  existing `paddock_testkit::same` and a new `text`, one paragraph in the testing guide, no check, 0.25 day; the class
  then rests on review and the Windows runner. Recommended: (a); (b) is the cheaper common-practice option (ER-37).
- **D2. Existing sites.** (a) Shrink-only baseline, fix on touch (WP-3a). (b) Migrate all 65 now (WP-3b, +0.65 day).
  Recommended: (a).
- **D3. Until RT-5 wires `./dev check run` into CI.** (a) Wait; WP-2 lands report-only and starts gating with RT-5.
  (b) Add a temporary JavaScript conformance check now (+0.25 day, deleted when the Rust host is wired). Recommended: (a).

## Consequences

- A path test written on the Mac either compares `Path`s through `same_path` or builds its expected text with `text`, so
  separators, the verbatim prefix, Git bash spellings and case no longer turn the Windows runner red.
- Git bash conversion has one implementation (`winpath`), tested on every OS.
- The check has false positives in rule C by design; rules A and B are syntactic and cheap to satisfy.
