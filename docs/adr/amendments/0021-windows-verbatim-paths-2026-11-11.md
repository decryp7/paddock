# Amendment to ADR 0021 (2026-11-11): Windows verbatim paths are made plain in one shared place

- Status: Proposed 2026-11-11. The coordinator's brief of 2026-11-11 reports that the user approved the class and the
  shape of the fix that day ("one shared Windows path helper"); the user's words are not in the repository, and the
  decisions below (D-P1 to D-P4) are open. An amendment to ADR 0021 (accepted 2026-10-27, on `origin/main`). It stands
  next to that ADR until it is merged into the ADR's text (WP-P6). Nothing here is built yet. ER-23 does not apply (no
  drawings).
- Amends: ADR 0021 §1 (the kit's `paths` module and decision 5, "the shared crates are the only path for OS-specific
  calls"), §2 (a check beside C38), §3 (one more Clippy entry) and the pattern table (a ninth shape).
- Related: ER-21 / rule 40, ER-22 / rule 42 (share first), rule 55 and ADR 0034 (one implementation per mechanism; the
  copies register `docs/known-copies.toml`), ER-9 (Linux and Windows before a push), ADR 0087 (accepted 2026-11-10, on
  `origin/docs/adr-windows-gate-ratchets` only: Windows-only code runs on the test VM before review), ADR 0094 (the
  rollout route for a new Clippy entry while apps are frozen, §6), ADR 0096 (the register row
  `docs/adr/register/R-2026-11-11-0021-windows-verbatim-paths.toml`).
- Evidence: read on 2026-11-11 with `git show`, `git grep`, `git log`, `git tag --contains` and the Gitea API (run and job
  status, the failing job's log) after `git fetch`. Code paths are from `origin/train/2026-11-11-5` unless a line names
  another ref. No user data was read; the CI log is of a test run on the CI runner. The heads, re-read right before the
  commit:

```evidence-heads
read: 2026-11-11
origin/main ea94d9f1e
origin/train/2026-11-11-5 55c056089
origin/fix/paddock-dev-windows-tests 1cb9a4a5b
origin/docs/adr-windows-gate-ratchets db7782e01
```

## Trigger: the incidents

**Run 7904** (`ci.yml` on `train/2026-11-10-4` at `95736f30e`, conclusion failure; job 35067 "paddock-dev
(windows-latest)", 14 of 557 tests failed). Three of the failures are this class, read from the job log:

| # | Test | What the log shows | Fixed in |
|---|---|---|---|
| 1 | `ci_isolation::home::tests::path_needs_a_source_for_a_src_layout_and_gives_two_folders_for_two_roots` (rt1) | the expected text was built from `canonicalize`: `right: "\\?\C:\gitea-runner\...\ci/target-x-...-s1"` against the tool's plain output | `e160bdbb8` (`home.rs` compares through `mixed`) |
| 2 | `worktree::git::tests::a_path_that_does_not_exist_yet_resolves_through_its_existing_parent` (rt3) | `right: "\\?\C:\gitea-runner\...\a\b"`: the test compared with a verbatim canonical path | `fc2249ad6` (the local `strip_verbatim` copy replaced by `paddock_repo_graph::strip_verbatim`) |
| 3 | `hook::install::tests::*` (rt4, three tests) | `git clone: fatal: could not open '\\?\C:\gitea-runner\...'`: git was given a verbatim path | `e160bdbb8` (`paddock_repo_graph::real_path`) |

All three fixes are on `origin/fix/paddock-dev-windows-tests` and the train; `git tag --contains` lists no tag for
any of them.

**Earlier instances of the same shape** (each a fix that strips the prefix by hand; read in the code, not re-run):
Debrief's `debrief-agent/src/disk.rs:96` and `debrief-ai/src/models/disk.rs:64` carry the comment "Regression:
canonicalised (`\\?\C:\…`) paths matched no mount on Windows, so free space was never known"; `cargo-debrief-plugin`
`main.rs:73` "Cargo does not read Windows' verbatim `\\?\` paths"; `paddock-service` `privileged/install_tests.rs:1702`
"the chain check and the 'already inside the protected folder' comparison".

## Instances: every hand-written handling of the prefix

**Method.** `git grep -F '\\?'` over every tracked `*.rs` outside `vendor/`, then each hit read; plus the three
`Prefix::VerbatimDisk`/`VerbatimUNC` matches. "Non-test" means outside `tests/`, `*_tests.rs` and `#[cfg(test)]` items.
Columns: what the copy does with `\\?\C:\x` (drive), `\\?\UNC\srv\share\x` (UNC) and `\\?\Volume{..}\x` (volume).

| # | Site (non-test unless marked) | Drive | UNC | Volume | Shipped in |
|---|---|---|---|---|---|
| 1 | `paddock-repo-graph` `src/real.rs:13` `strip_verbatim` (the "shared" one) | plain | `\\srv\share\x` | **`Volume{..}\x`, a relative path** | not shipped (a tools library) |
| 2 | `paddock-files` `src/path.rs:48` `plain` | plain | `\\srv\share\x` | kept | apps through `paddock-files` |
| 3 | Debrief `app/src-tauri/src/paths.rs:367` `plain` | copy of #2 (same text) | | | Debrief |
| 4 | Debrief `app/src-tauri/src/opener.rs:55` `plain_path` | plain | `\\srv\share\x` | kept | Debrief |
| 5 | `tools/paddock-dev` `release_isolation/ledger.rs:234` `simplify_text` (drive or UNC, then #1) | plain | `\\srv\share\x` | kept | no |
| 6 | `tools/paddock-dev` `ci_isolation/home.rs:38` `mixed` | plain, `/` | **`UNC/srv/share/x`** | **`Volume{..}/x`** | no |
| 7 | `tools/paddock-dev` `tests/it/golden.rs:660` (test) | plain | `//srv/share` | **`Volume{..}/x`** | no |
| 8 | `paddock-setup` `src/samefolder.rs:30` `normalise` | plain | `\\srv\share` (case-insensitive) | **`Volume{..}`** | `dyno-v1.1.0` |
| 9 | `paddock-setup` `src/apply.rs:174` `volume_root` | `C:\` | `None` (by accident: `UNC\` is not a drive) | `None` | `dyno-v1.1.0` |
| 10 | `paddock-setup` `src/native/facts.rs:139` `strip_prefix` | plain | **`UNC\srv\share`** | **`Volume{..}`** | `dyno-v1.1.0` |
| 11 | `paddock-setup` `src/checkdir.rs:101` `components` | plain | **first component `UNC`** | **`Volume{..}`** | `dyno-v1.1.0` |
| 12 | `paddock-proc` `src/installed.rs:213` `parts`, `:231` `join_win` | plain | **`unc` component; `UNC\srv\..\image`** | **`volume{..}` component** | `dyno-v1.1.0` |
| 13 | `paddock-service` `privileged/install.rs:197` `without_verbatim_prefix` | plain | kept verbatim (documented) | **`Volume{..}\x`** | Dyno helper |
| 14 | `paddock-service` `privileged/trust.rs:531` `require_same_path` | plain | **`UNC\srv` ≠ `\\srv`: a false "redirected" refusal** | stripped on both sides | `dyno-v1.1.0` |
| 15 | Debrief `cargo-debrief-plugin/src/main.rs:75` | plain | **`UNC\srv\share` handed to cargo** | **`Volume{..}`** | Debrief SDK tool |
| 16 | Debrief `debrief-plugin-native/src/paths.rs:35` `is_within` | plain (lowercased) | **`unc\srv\...`, never within `\\srv\...`** | **stripped** | Debrief |
| 17 | Dyno `dyno-helper/src/protect.rs:379` | plain | **`UNC\srv\share\..` to `verify_chain`** | **stripped** | `dyno-v1.1.0` |
| 18 | Pitcrew `pitcrew-service/src/probe.rs:242` `normalise` | plain | **`UNC/srv/...`** | **stripped** | Pitcrew |
| 19-21 | Debrief `debrief-agent/src/disk.rs:52`, `debrief-ai/src/models/disk.rs:42`, `paddock-metrics/src/disk.rs:41` (`Prefix::Verbatim*`) | plain | `\\srv\share\x` | kept | Debrief, Dyno |
| 22 | Debrief `debrief-core/src/workspace/tail/identity.rs:145` `is_network_path` | (classifies) | network | not network | Debrief |

Not copies (they use the prefix on purpose, and stay): Dyno `dyno-platform/src/fileid.rs:61` (adds it for long paths),
Pitcrew `pitcrew-service/src/cleanup.rs:153` `os_path` (keeps it for deep trees), Pitwall
`pitwall-server/src/service/placement.rs:199` (refuses such paths).

**Counts.** 21 hand-written strippers or comparers in 20 non-test files plus one test (#7), in 12 crates. Nine give a
wrong spelling for a UNC path (6, 10, 11, 12, 14 to 18) and twelve turn a volume path into a relative one or a wrong
component (1, 6, 7, 8, 10 to 13, 15 to 18); bold marks both. None of them checks what makes stripping unsafe: a reserved
device name (`\\?\C:\x\con.txt`), a trailing dot or space, `.`/`..` components, or a path over 260 characters, all of
which name a different file (or none) once the prefix is gone. The `dunce` crate's rules for this are a reference
(`dunce` 1.0.5, `src/lib.rs` `is_safe_to_strip_unc`, read in the local cargo registry: verbatim disk paths only, every
component a valid non-reserved name, at most 260 UTF-16 units; on non-Windows hosts it never strips, and it leaves
`\\?\UNC\` alone). `dunce` 1.0.5 is already in every app's `Cargo.lock` (transitively) and its licence (CC0-1.0 OR
MIT-0 OR Apache-2.0) is on the allow-list of `crates/deny.toml:15`.

**Where verbatim paths come from.** 237 `canonicalize(` calls in 145 Rust files (67 in files under `tests/`), by area:
Debrief 113, Dyno 40, `tools/paddock-dev` 37, shared crates 39 (`paddock-proc` 16), Pitcrew 5, Pitwall 2, `tools/heavy`
1. Each returns `\\?\` on Windows; none of them says whether its result may meet git, text, a file the user reads, or a
comparison with a plain path.

## Pattern

**A path from `canonicalize` keeps Windows' verbatim prefix and later meets something that does not speak it**: git,
cargo, a text file, a log line, a string comparison with a path that came from elsewhere (a config value, `git`
output, a mount list, the layout's plain paths). Each site that noticed wrote its own stripper, and the copies disagree
on UNC and volume paths and on when stripping is safe.

**Invariant.** A path leaves `canonicalize` through one of two named doors: **plain** (for tools, text, display and
comparison with plain paths: drive and UNC paths without the prefix when that names the same file, the verbatim form
otherwise) or **verbatim** (for Win32 calls past 260 characters, compared only with other verbatim paths). No other
code removes or tests the prefix.

## Change

1. **One helper, `paddock_env::winpath` (D-P1).** A module in `paddock-env`, the crate ADR 0021 decision 5 names for
   OS-specific calls in product code; it has no Paddock dependency (`crates/paddock-env/Cargo.toml`: serde, serde_json,
   log, libc, windows-sys), so every crate that strips today, `paddock-proc` included, can depend on it without a
   cycle. Pure text functions take the platform as a parameter, so every OS tests the Windows rules (ADR 0087's rule,
   "Windows logic that can be pure takes the platform as a parameter"):
   - `plain_text(s: &str) -> Cow<str>`: `\\?\C:\x` gives `C:\x` and `\\?\UNC\srv\share\x` gives `\\srv\share\x`, only
     when every component is a valid, non-reserved Windows name, there is no `.`/`..`, and the result is at most 260
     UTF-16 units; anything else (volume GUIDs, `\\.\` devices, unsafe names, long paths) is returned unchanged;
   - `plain(p: &Path) -> Cow<Path>` (that rule on the host's paths; a no-op off Windows);
   - `real(p) -> io::Result<PathBuf>` = `canonicalize` then `plain`, and `real_or_given(p) -> PathBuf` (today's
     `paddock_repo_graph::real_path`: the given path when it cannot be resolved);
   - `real_verbatim(p) -> io::Result<PathBuf>`: `canonicalize` unchanged, the one named door to the verbatim form;
   - `for_tools(p) -> String`: `plain` with `/` separators (what `cygpath -m` prints and git, cargo and Git bash read;
     today's `home.rs` `mixed`);
   - `same(a, b, os) -> bool` and `components(p, os)`: comparison by components after `plain_text`, case-insensitive on
     Windows (today's #8, #11, #12, #14, #16, #18 each do a part of this);
   - `kind(p) -> Kind { Drive, Unc, Device, Relative, Unix }` for classifiers such as #22.
   `paddock_repo_graph::{strip_verbatim, real_path}` and `paddock_files::plain` become calls of it and then go (no shims
   before v1; memory "no backward compatibility"); `paddock_testkit` re-exports `real_or_given` and `same` for tests
   (ADR 0021 §1's `paths` module gains them).
2. **Every copy moves in one change per frozen state (rule 55).** Rows 1 to 22 call the helper, each with a failing test
   first for the UNC and volume cases it gets wrong (ER-11); an app under a release freeze changes after its tag, by
   ADR 0094 §6's route, with a `docs/known-copies.toml` entry `verbatim-path` (canonical `paddock-env`) whose
   `delete_by` is the tag after the frozen one.
3. **Clippy names the door (D-P2).** `disallowed-methods` gains `std::fs::canonicalize` and `std::path::Path::canonicalize`
   with the reason "use `paddock_env::winpath::real` (plain: tools, text, comparison with plain paths) or `real_verbatim`
   (Win32 long paths, compared only with verbatim paths); ADR 0021 amendment 2026-11-11". `winpath` holds the one
   `#[expect]`. Entries go first into `crates/clippy.toml`, `tools/*/clippy.toml` and each unfrozen app's `clippy.toml`,
   and into `lint/clippy.toml` (which C13 copies into every app) only after the last frozen tag (ADR 0094 §6; C13's
   per-app exemption list `clippy_not_yet` in `docs/gates.toml` is shrink-only under C100, so no entry goes there). Each
   of the 237 calls is migrated or carries `#[expect(clippy::disallowed_methods, reason = "verbatim: <why>")]`; an
   unfulfilled `#[expect]` warns, so a stale one shows itself.
4. **C109 "verbatim paths by hand" (number: D-P3).** A check in the RT-5 Rust check host (`tools/paddock-dev/src/check/`,
   comment-masked Rust like C53, string literals read): outside `crates/paddock-env/src/winpath*` and outside test code,
   (a) a string or byte-string literal containing the verbatim prefix (`\\?\` in any spelling: raw, escaped, `b"..."`)
   used as an argument of `strip_prefix`, `starts_with`, `trim_start_matches`, `replace`, `split_once`, `find`,
   `eq_ignore_ascii_case` or compared with `==` against a slice, and (b) a match on `Prefix::VerbatimDisk` or
   `Prefix::VerbatimUNC`. A deliberate user (the three "not copies" above) carries `// conformance: verbatim — <why>` on
   the line or the comment line above, the C54 waiver shape; a waiver that waives nothing warns. **Positive controls
   (ER-33):** fixtures that must fail (`s.strip_prefix(r"\\?\")`, `starts_with("\\\\?\\UNC\\")`,
   `b.starts_with(br"\\?\")`, a `Prefix::VerbatimUNC(..)` arm) and must pass (the literal inside a test module, inside
   a comment, in `winpath`, and with a waiver).
5. **Run-time net (no new mechanism).** Once ADR 0087 is built, its `docs/windows-paths.toml` declares the modules that
   compare paths (rows 8, 11, 12, 14, 16, 18 after migration), so a change there runs on the test VM before review; until
   then CI's Windows legs are the only run (ER-9).

## What this does not cover

- `\\.\` device paths, `\??\` NT paths and 8.3 short names beyond what `canonicalize` already resolves.
- Paths typed by the user or read from config (`paddock-setup`'s `LocalPath::parse` already refuses namespaces; that
  stays).
- Separator and case differences of paths that never went through `canonicalize` (the `same` helper serves them, but
  no check finds them).
- TypeScript: the UI never receives a verbatim path from a migrated command; no TS check.

## Release impact (ER-31)

Rows 8 to 12, 14 and 17 are in `dyno-v1.1.0` (read at the tag with `git grep`). Whether a wrong UNC spelling there ever
changes an outcome for a user (a program on a network share is refused by the trust rule before most of them run) is
**not analysed** here; WP-P0 reads each shipped row and reports any wrong decision as a finding for its app's next
tag (D-P4). Nothing here gates a tag by itself.

## Work packages (raw agent-days, one agent)

| WP | Content | Days |
|---|---|---|
| WP-P0 | Re-read rows 1 to 22 and the 237 calls on the then-current train; for each shipped row say whether a wrong spelling can change a decision; record each call's door (plain, verbatim, display) in the WP report | 0.5 |
| WP-P1 | `paddock_env::winpath` with the table tests above, `dunce`'s reserved-name and length cases, UNC and volume cases, and `same`/`components` on both platform values; 99 % lines (crates gate); CI's Windows leg runs it | 1 |
| WP-P2 | Shared crates and tools onto it, each row's UNC/volume test failing first: `paddock-repo-graph`, `paddock-files`, `paddock-proc`, `paddock-service`, `paddock-setup`, `paddock-metrics`, `tools/paddock-dev` (rows 1, 2, 5 to 14, 21); delete the old helpers | 1.5 |
| WP-P3 | Apps: Debrief (rows 3, 4, 15, 16, 19, 20, 22), Dyno (17), Pitcrew (18); frozen apps after their tag with the `verbatim-path` copies entry | 1.5 |
| WP-P4 | C109 in the RT-5 host with fixtures and positive controls; README row | 0.75 |
| WP-P5 | Clippy entries and the triage of the 237 calls (migrate or `#[expect]` with a reason), per workspace; `lint/clippy.toml` after the last frozen tag; a fixture crate that calls `std::fs::canonicalize` fails with the preset | 2 |
| WP-P6 | Lesson, ADR 0021 text merge, register row states | 0.25 |
| | **Total** | **7.5** |

Order: WP-P1 before P2 and P3; P4 any time (it reports the remaining copies, so it lands report-only until P3 is done,
then strict); P5 after P2. One shared landing for P1 and P2 (rule 43), outside a release freeze (ADR 0049).

## Failing tests the work starts with (ER-11; not written in this change)

- `paddock-env` `winpath`: `a_unc_path_is_plain_with_two_backslashes`, `a_volume_guid_path_keeps_its_prefix`,
  `a_reserved_name_keeps_the_prefix` (`\\?\C:\x\con.txt`), `a_path_over_260_units_keeps_the_prefix`.
- `paddock-repo-graph`: `strip_verbatim(r"\\?\Volume{x}\a")` is not the relative path `Volume{x}\a` (fails today).
- `tools/paddock-dev` `home.rs`: `mixed(Path::new(r"\\?\UNC\srv\share\x"))` is `//srv/share/x` (today `UNC/srv/share/x`).
- `paddock-service` `trust.rs`: `require_same_path(r"\\srv\share\a.exe", r"\\?\UNC\srv\share\a.exe", Style::Windows)` is
  `Ok` (today `Redirected`).
- `paddock-setup` `checkdir.rs`: `components(r"\\?\UNC\srv\share\a")` starts with the server, not `UNC`.

## Decisions for the user

| # | Question | Options | Recommendation |
|---|---|---|---|
| D-P1 | Where the helper lives | (a) a `winpath` module in `paddock-env` (ADR 0021 decision 5: no new crate); (b) a new zero-dependency crate `paddock-paths`; (c) depend on `dunce` | **(a)**. (c) does not convert UNC, never strips off Windows (so macOS cannot test the rule), and would still need our own `same`/`for_tools`; its safety rules are copied into (a)'s tests instead |
| D-P2 | Clippy on `canonicalize` | (a) disallow both forms everywhere, every call migrated or `#[expect]`ed with its reason (2 days for 237 calls); (b) no Clippy entry, C109 only (finds strippers, not unmarked verbatim paths) | **(a)**: the three incidents of run 7904 had no stripper at all; they used `canonicalize` and compared. Only (a) makes that choice visible |
| D-P3 | The check's number | (a) C109, the next free number on 2026-11-11 (C108 is ADR 0097's; ADR 0081's liveness check and ADR 0096's index check are still written unnumbered); (b) leave unnumbered until landing | **(a)**, confirmed by the integrator at landing |
| D-P4 | Shipped rows (8 to 12, 14, 17 in `dyno-v1.1.0`) | (a) WP-P0 decides per row; a wrong decision found is a finding for the app's next tag; (b) block Dyno's next tag on WP-P2/P3 regardless | **(a)** |

## Rollback

C109 leaves the check host's registry with one line; the Clippy entries are removed from the `clippy.toml` files; the
helper is additive, and reverting WP-P2/P3 restores the copies (rule 55 then needs a `verbatim-path` copies entry).
