# 99. Tests compare paths as paths, through one testkit helper

- Status: Accepted 2026-11-13 (user, afternoon): D1 (a) the full check, D2 (b) migrate the sites now (changed by the user, 2026-11-13, "better to build complete right for foundation"; first chosen (a) baseline the 65 sites), D3 (b) ban raw
  `canonicalize` in the apps' preset as foundation work (WP-4). Built 2026-11-13 on `feat/adr-0099-path-asserts` (WP-1, WP-2, WP-3b; WP-4 is a later task); the check is `C122`. History: proposed 2026-11-13;
  drafting approved by the user on 2026-11-13 (ER-21).
  ER-23 does not apply (no drawings). Revision 2 (same day) answers the coordinator's one review round of revision 1
  (`5b23e68c9`): an instance-to-mechanism recall table, the Clippy `canonicalize` bans of train 8, the existing
  known-copies entry, the uncaught spellings named, the cargo `[ROOT]` claim verified, D1 re-weighed.
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

Revision 2 read `origin/train/2026-11-13-8` at the merge `12378b80c` (`feat/lint-no-raw-canonicalize-r4`, `44881ad9c`).

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
  `Path::canonicalize` outside it (`real_verbatim` holds the one `#[expect]`); Clippy runs with `--all-targets`, so the ban
  covers test code. On `origin/main` the ban is in the shared crates' preset only. Train 8's merge `12378b80c`
  (`feat/lint-no-raw-canonicalize-r4`) adds the same two entries to `tools/paddock-dev/clippy.toml` (lines 4-5),
  `tools/heavy/clippy.toml` and `tools/wiki/clippy.toml`; it does not touch `lint/clippy.toml`, the apps' preset.
  The apps hold 163 raw `canonicalize` calls in 109 files (`git grep`, Debrief 114, Dyno 42, Pitcrew 5, Pitwall 2,
  Spotter 0), and C13 makes every app's `clippy.toml` list every entry of `lint/clippy.toml`, so adding it there is not a
  one-line change (D3).
- `paddock_testkit` already re-exports `winpath::{real_or_given, same}` (`crates/paddock-testkit/src/lib.rs:44`); there is
  no assertion macro and no text helper.
- Git bash spellings are converted only in `paddock-dev`: `ci_isolation::home::from_git_bash` (pub, used by five modules),
  and a second private copy `native_path_of_bash` added by `c75dc801c` in `tools/paddock-dev/src/queue/tests.rs`;
  neither is in `docs/known-copies.toml`. `ci_isolation::home::mixed` is already a registered copy of the `verbatim-path`
  mechanism (`docs/known-copies.toml`, the `item:mixed` entry, delete-by `v0.4.0`, moves to `winpath::lexical_text`);
  `ci_host::fwd` (`0e996a61c`) is a test-only wrapper of `winpath::lexical_text`, not a copy.
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
  harness normalises the actual side. Its `cargo_test_support::compare` documents the patterns "`[ROOT]` is the path to
  the test directory's root" and "`[CWD]` is the working directory of the process that was run", and "Backslashes are
  converted to forward slashes to deal with Windows paths"
  (<https://doc.rust-lang.org/nightly/nightly-rustc/cargo_test_support/compare/index.html>). snapbox, which those tests
  use, has `filter::normalize_paths` ("Normalize path separators") and `Redactions` ("Replace data with placeholders")
  (<https://docs.rs/snapbox/latest/snapbox/filter/index.html>).
- **Node**: on Windows `path` accepts both separators but its methods only produce `\`; `path.normalize` folds repeated
  separators into `\`; `path.win32`/`path.posix` give one OS's rules on any OS; `basename` does not fold case
  (<https://nodejs.org/api/path.html>).

None of these lints for text comparisons; the common practice is "compare `Path`s, normalise the actual side, write
expected values with `/`". Paddock needs a little more because three of the five spellings (verbatim, Git bash, case)
survive `Path` equality, and because the CI Windows runner is the only place the class shows, after a full CI round trip.

## Decision

Items 1 and 2 are the full mechanism, D1 (a), which the user chose on 2026-11-13 (revision 2 had recommended D1 (b), the
lite helper plus the Clippy bans; see "Recall"). Item 4 below (the apps' preset) is D3 (b).

4. **The apps' preset bans raw `canonicalize`** (WP-4): the two `disallowed-methods` entries of `crates/clippy.toml` go
   into `lint/clippy.toml` and, through C13, every app's `clippy.toml`; each raw call moves to a `winpath` door
   (`real_for_tools`, `real` or `real_verbatim`), no blanket `#[expect]`.

1. **One helper, in `paddock_testkit::path`** (Rust), built on `winpath`:
   - `same_path(actual, expected) -> bool`: `winpath::same` after reading a Git bash spelling (`/c/x`) as `C:/x` on Windows;
     `assert_same_path!(actual, expected)` prints both spellings and the normalised forms on failure. Exact off Windows.
   - `text(path) -> String`: the tools' spelling (`winpath::lexical_text` for the running OS: `/`, no prefix), for building
     an expected line that a program printed; `under(base, "a/b")`: `rel` joined component by component.
   - `from_git_bash` moves into `paddock_env::winpath` (pure, `Os` parameter) as the one implementation; the `paddock-dev`
     copies (`home::from_git_bash`, `queue/tests.rs::native_path_of_bash`, `home::mixed`, `ci_host::fwd`) call it or are
     deleted. As built, `home::from_git_bash`, `queue/tests.rs::native_path_of_bash` and `ci_host::fwd` are deleted, so no
     `[[mechanism.copy]]` row is added for them (a row names code that exists); `home::mixed` stays, as it was, because
     `lexical_text` would also drop the verbatim prefix and respell device paths in the junction call and the slot tool's
     arguments, so its `verbatim-path` row stays too (no new mechanism id, rule 55).
   - `same_path(actual, expected)` reads a Git bash spelling on the EXPECTED side only: a product that prints `/c/x` is
     run 8034's bug and must fail; `same_path_from_shell` reads the actual side too, for output that really came from Git
     bash. `text` spells a drive root `C:/`.
   - Not covered by any mechanism here: spelling 3 (a Git bash path arriving as text from a shell or a fake, as in run
     8034) and spelling 5 (a list joined with `:` by hand, as in run 8050). Both stay uncaught: the helper only helps a
     test that calls it, and no syntactic rule tells a hand-written `"/nope/a:/nope/b"` or a shell's `pwd` from correct
     code. The helper's docs name `std::env::join_paths` for lists. Line endings stay with the `.gitattributes` rules and
     a per-test `core.autocrlf=false`.
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
   is defined; then rules A and B block; with D2 (b) no site is left, so there is no baseline.
3. **TypeScript: no helper now.** The class has no recorded TS instance: `git log -i --grep=windows -- '*.mjs' '*.ts'`
   since 2026-10-15 finds product fixes (`ed1b0cba5`, `affected.mjs` cargo keys) but no test that compared path text, and the
   `node --test` and conformance jobs in `ci.yml` run on the Linux image. A TS twin is added only when a TS instance
   appears (rule of three).

## Recall: what each mechanism would have caught

Each instance's original test code (the parent of its fix, `git show <fix>^:<file>`) against each mechanism:

| Run, fix | Spelling | Check rule A or B | Clippy `canonicalize` ban | Helper (only if the author used it) |
|---|---|---|---|---|
| 7943, `1bd84ff5b` | 2 | no (raw `canonicalize` then `assert_eq!` of `PathBuf`s) | yes: `crates/clippy.toml` on main today (the test predates `winpath`, `9fb713140`) | yes |
| 8027, `0e996a61c` | 1, 2 | yes: rule A (`format!("{}/..", p.display())`) | yes for the `std::fs::canonicalize` in `build_folder.rs` tests, with train 8's `tools/paddock-dev/clippy.toml` | yes |
| 8027, `0ba535754` | 1 (key text `file:/l`, no drive) | no | no | no (a lock-key string, not a path compare) |
| 8034, `c75dc801c` | 3 | no | no | yes, if called |
| 8050, `4e9ebe00f` | 5, 1 | no (`str::strip_prefix` on `to_str()`; the list was a literal) | no | partly (spelling 1 only) |

The check (rules A and B) catches 1 of 5; the Clippy bans catch 2 of 5, both already in place on `main` or train 8 at no
further cost; together 2 of 5 (the 0e996a61c instance is caught by both). Rule C (warning) would have shown 7943 only.
Three instances (`0ba535754`, 8034, 8050) are caught by nothing proposed here: they are test text, not a pattern.

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
| WP-0 | Land train 8's Clippy bans (`12378b80c`) with the train: no extra work | 0 |
| WP-1 | `winpath::from_git_bash` (moved, `Os` parameter, tests on every OS), `paddock_testkit::path` (`same_path`, `assert_same_path!`, `text`, `under`) with failing-first tests for the instances above, the `paddock-dev` copies onto it, two known-copies rows | 0.5 |
| WP-1-lite | Only `paddock_testkit::path::text` and `same_path` over the existing `winpath` (no macro), `from_git_bash` moved, the known-copies rows, one paragraph in the testing guide | 0.25 |
| WP-2 | The check (rules A-C) in the Rust host, fixtures, report-only; first green run recorded (needs RT-5's CI wiring) | 0.5 |
| WP-3a | Baseline the 65 A/B sites (shrink-only); each is fixed when its file is next touched | 0.1 |
| WP-3b | Instead of 3a: migrate all 65 sites now (Debrief and Dyno sites after their current tags) | 0.75 |
| WP-4 | Foundation work, after train 10, app by app (Pitwall 2 sites, Pitcrew 5, Dyno 42, Debrief 114; Spotter has none): the two `canonicalize` entries in the app's `clippy.toml`, every site of that app through `winpath` (no blanket `#[expect]`), the app's Windows tests run; the last app adds the entries to `lint/clippy.toml` so C13 then holds them for every app | about 1.5 (estimate) |

Chosen 2026-11-13: WP-0, WP-1, WP-2, WP-3b and WP-4, about 3.25 days (1.75 plus 1.5; the user moved D2 from (a) to (b) the same day); WP-1-lite and WP-3a are not built.

## Decisions (the user, 2026-11-13)

- **D1. Scope.** (a) Full mechanism: WP-1, WP-2, WP-3a, 1.1 days; recall 1 of 5 for the check alone, 2 of 5 with the bans.
  (b) Common practice plus the bans: WP-0 and WP-1-lite, 0.25 day; recall 2 of 5 (the same instances (a) catches, through
  Clippy). Recommended in revision 2: (b). **Chosen 2026-11-13 by the user: (a).** The check adds no instance the bans miss, and the three uncaught instances are not a
  syntactic pattern; revisit (a) only if a later instance is one rule A or B would have caught and Clippy would not.
- **D2. Existing sites** (only with D1 (a)). (a) Shrink-only baseline, fix on touch (WP-3a). (b) Migrate all sites now
  (WP-3b, +0.65 day). Recommended: (a). First chosen 2026-11-13 by the user: (a). **Changed 2026-11-13 by the user to (b)**
  ("better to build complete right for foundation"): every site is migrated onto `paddock_testkit::path` (or, where a site
  is no path comparison, waived inline with `// conformance: path-text — <why>`), so the check starts with an empty
  baseline and needs no baseline file. The count at build time was 63 rule A and B sites (the measurement on `main` counted 65; train 8
  had already fixed some, and the check's own scanner differs slightly from the throwaway regex).
- **D3. The apps' preset.** (a) Leave `lint/clippy.toml` as it is now; the apps' raw `canonicalize` calls move through the
  existing `verbatim-path` copy rows by their delete-by dates. (b) WP-4 after the current tags, about 1.5 days (over the
  1-day line; needs a yes). Recommended in revision 2: (a); none of the five instances was in an app. **Chosen 2026-11-13
  by the user: (b)**, as foundation work, app by app after train 10, each site through `winpath` (WP-4, about 1.5 days).
- Prerequisite of WP-2: `./dev check run` wired into the conformance job (RT-5 hand-off, not on main).

## Consequences

- Raw `canonicalize` fails Clippy on every OS in the shared crates, the Rust tools and (after WP-4) every app; the check
  refuses new rule A and B sites in test code once it blocks, and a path test has `same_path`, `assert_same_path!` and
  `text` to reach for. Spellings 3 and 5, and path-like
  key text such as `file:/l`, still show only on the Windows runner.
- Git bash conversion has one implementation (`winpath`), tested on every OS.
- The check has false positives in rule C by design; rules A and B are syntactic and cheap to satisfy.
