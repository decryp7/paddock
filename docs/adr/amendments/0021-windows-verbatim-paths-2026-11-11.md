# Amendment to ADR 0021 (2026-11-11): Windows verbatim paths are made plain in one shared place

- Status: Proposed 2026-11-11, revision 2. The coordinator's brief of 2026-11-11 reports that the user approved the
  class and the shape of the fix that day ("one shared Windows path helper"); the user's words are not in the
  repository, and the decisions below (D-P1 to D-P4) are open. Revision 1 (`dbd7028a1`) was reviewed independently
  ("changes": one P1, P2s); this revision answers it (see "Review answers"). An amendment to ADR 0021 (accepted
  2026-10-27, on `origin/main`). It stands next to that ADR until it is merged into the ADR's text (WP-P6). Nothing here
  is built yet. ER-23 does not apply (no drawings).
- Amends: ADR 0021 §1 (the kit's `paths` module and decision 5, "the shared crates are the only path for OS-specific
  calls"), §2 (a check beside C38), §3 (one more Clippy entry) and the pattern table (a ninth shape).
- Related: ER-21 / rule 40, ER-22 / rule 42 (share first), ER-27 (errors give the cause and the next step), rule 55 and
  ADR 0034 (one implementation per mechanism; the copies register `docs/known-copies.toml`), ER-9 (Linux and Windows
  before a push), ADR 0087 (accepted 2026-11-10, on `origin/docs/adr-windows-gate-ratchets` only: Windows-only code runs
  on the test VM before review), ADR 0094 (the rollout route for a new Clippy entry while apps are frozen, §6), ADR 0096
  (the register row `docs/adr/register/R-2026-11-11-0021-windows-verbatim-paths.toml`).
- Overlap checked: ADR 0087's planned `docs/windows-paths.toml` (§3.4) declares modules that compare paths so the
  detector routes them to the VM; it holds no helper and no check of prefix handling, so this amendment adds the
  mechanism and 0087 stays the run-time net (item 6). ADR 0094 is reused for the Clippy rollout, not changed.
  `docs/known-copies.toml` on the train has no path entry. No other accepted ADR or amendment on `origin/main`, the
  train or the pinned ADR branches names `canonicalize`, `strip_verbatim` or a verbatim-path check (`git grep` over
  `docs/adr`).
- Evidence: read on 2026-11-11 with `git show`, `git grep`, `git log`, `git tag --contains` and the Gitea API (run and job
  status, the failing job's log) after `git fetch`; Microsoft's path-length page (below). Code paths are from
  `origin/train/2026-11-11-5` unless a line names another ref. No user data was read; the CI log is of a test run on the
  CI runner. The heads, re-read right before the commit:

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
| 2 | `worktree::git::tests::a_path_that_does_not_exist_yet_resolves_through_its_existing_parent` (rt3) | `right: "\\?\C:\gitea-runner\...\a\b"`: the test compared with a verbatim canonical path | `fc2249ad6`: the local `strip_verbatim` (at `fc2249ad6^`, `worktree/git.rs:196`: kept `\\?\UNC\` verbatim and turned a volume path into a relative one) was replaced by `paddock_repo_graph::strip_verbatim`, which fixes UNC but has the same volume defect (row 1) |
| 3 | `hook::install::tests::*` (rt4, three tests) | `git clone: fatal: could not open '\\?\C:\gitea-runner\...'`: git was given a verbatim path | `e160bdbb8` (`paddock_repo_graph::real_path`) |

All three fixes are on `origin/fix/paddock-dev-windows-tests` and the train; `git tag --contains` lists no tag for
any of them.

**Earlier instances of the same shape** (each a fix that strips the prefix by hand; read in the code, not re-run):
Debrief's `debrief-agent/src/disk.rs:96` and `debrief-ai/src/models/disk.rs:64` carry the comment "Regression:
canonicalised (`\\?\C:\…`) paths matched no mount on Windows, so free space was never known"; `cargo-debrief-plugin`
`main.rs:73` "Cargo does not read Windows' verbatim `\\?\` paths"; `paddock-service` `privileged/install_tests.rs:1702`
"the chain check and the 'already inside the protected folder' comparison".

## Instances: every hand-written handling of the prefix

**Method.** `git grep -F '\\?'` over every tracked `*.rs` outside `vendor/`, then each hit read; plus every match on
`Prefix::Verbatim*` (5 files: `debrief-agent/src/disk.rs`, `debrief-ai/src/models/disk.rs`,
`debrief-core/src/workspace/tail/identity.rs`, `paddock-download/src/error.rs`, `paddock-metrics/src/disk.rs`).
"Non-test" means outside `tests/`, `*_tests.rs` and `#[cfg(test)]` items. Columns: what the copy does with `\\?\C:\x`
(drive), `\\?\UNC\srv\share\x` (UNC) and `\\?\Volume{..}\x` (volume). **Bold** marks a wrong result.

| # | Site (non-test unless marked) | Use of the result | Drive | UNC | Volume |
|---|---|---|---|---|---|
| 1 | `paddock-repo-graph` `src/real.rs:13` `strip_verbatim` (the "shared" one) | path | plain | `\\srv\share\x` | **`Volume{..}\x`, a relative path** |
| 2 | `paddock-files` `src/path.rs:48` `plain` | path, message | plain | `\\srv\share\x` | kept |
| 3 | Debrief `app/src-tauri/src/paths.rs:367` `plain` | path, message | copy of #2 (same text) | | |
| 4 | Debrief `app/src-tauri/src/opener.rs:55` `plain_path` | path for Explorer/editors | plain | `\\srv\share\x` | kept |
| 5 | `tools/paddock-dev` `release_isolation/ledger.rs:234` `simplify_text` (drive or UNC, then #1) | path | plain | `\\srv\share\x` | kept |
| 6 | `tools/paddock-dev` `ci_isolation/home.rs:38` `mixed` | path for git and Git bash | plain, `/` | **`UNC/srv/share/x`** | **`Volume{..}/x`** |
| 7 | `tools/paddock-dev` `tests/it/golden.rs:660` (test) | path in git's `alternates` | plain | `//srv/share` | **`Volume{..}/x`** |
| 8 | `paddock-setup` `src/samefolder.rs:30` `normalise` | comparison | plain | `\\srv\share` (case-insensitive) | stripped (both sides) |
| 9 | `paddock-setup` `src/apply.rs:174` `volume_root` | drive root | `C:\` | `None`, documented (`apply.rs:173`: "`None` for anything else (a share, `\\?\UNC\`, a pipe)") | `None` |
| 10 | `paddock-setup` `src/native/facts.rs:139` `strip_prefix` | path text | plain | **`UNC\srv\share`** | **`Volume{..}`** |
| 11 | `paddock-setup` `src/checkdir.rs:101` `components` | comparison | plain | **first component `UNC`** | stripped |
| 12 | `paddock-proc` `src/installed.rs:213` `parts` (comparison), `:231` `join_win` (path for Restart Manager) | both | plain | **`unc` component; `UNC\srv\..\image`** | stripped |
| 13 | `paddock-service` `privileged/install.rs:197` `without_verbatim_prefix` | path for the trust rule and the layout | plain | kept verbatim (documented) | **`Volume{..}\x`** |
| 14 | `paddock-service` `privileged/trust.rs:531` `require_same_path` | comparison | plain | **`UNC\srv` ≠ `\\srv`: a false "redirected" refusal** | stripped (both sides) |
| 15 | Debrief `cargo-debrief-plugin/src/main.rs:75` | path for cargo | plain | **`UNC\srv\share` handed to cargo** | **`Volume{..}`** |
| 16 | Debrief `debrief-plugin-native/src/paths.rs:35` `is_within` | comparison | plain (lowercased) | **`unc\srv\...`, never within `\\srv\...`** | stripped |
| 17 | Dyno `dyno-helper/src/protect.rs:379` | path walked by `verify_chain` | plain | **`UNC\srv\share\..`** | **`Volume{..}\..`** |
| 18 | Pitcrew `pitcrew-service/src/probe.rs:242` `normalise` | comparison | plain | **`UNC/srv/...`** | stripped |
| 19-21 | Debrief `debrief-agent/src/disk.rs:52`, `debrief-ai/src/models/disk.rs:42`, `paddock-metrics/src/disk.rs:41` (`Prefix::Verbatim*`) | mount match | plain | `\\srv\share\x` | kept |
| 22 | Debrief `debrief-core/src/workspace/tail/identity.rs:145` `is_network_path` | classification | local | network | local |
| 23 | `paddock-download` `src/error.rs:225` `drive_of` (`Prefix::Disk \| Prefix::VerbatimDisk`) | classification | `C:` | `None` | `None` |

Not copies (they use the prefix on purpose, and stay): Dyno `dyno-platform/src/fileid.rs:61` (adds it for long paths),
Pitcrew `pitcrew-service/src/cleanup.rs:153` `os_path` (keeps it for deep trees), Pitwall
`pitwall-server/src/service/placement.rs:199` (refuses such paths).

**Counts.** 23 hand-written strippers, comparers or classifiers: 22 in non-test code (22 files) plus one test (#7), in 16
crates.
- **Wrong for UNC paths:** 9 (rows 6, 10, 11, 12, 14 to 18).
- **Volume paths, output used as a path:** 7 hand on a relative or wrong path (rows 1, 6, 7, 10, 13, 15, 17). Revision
  1 counted row 17 with the comparers; its result is the path `verify_chain` walks, so it is a path user.
- **Volume paths, comparers:** rows 8, 11, 12, 14, 16 and 18 strip the prefix before comparing. That is harmless when
  both sides went through the same strip, and wrong when one side is a plain path from elsewhere. Not bold: their
  outcome depends on the caller.

None of the copies checks what makes stripping unsafe: a reserved device name (`\\?\C:\x\con.txt`), a trailing dot or
space, `.`/`..` components, or a path over the length limit. All of these name a different file (or none) once the
prefix is gone.

**The length limit.** Microsoft: `MAX_PATH` is 260 "characters" including the terminating null ("D:\some 256-character
path string<NUL>"), so a plain path has at most **259** usable UTF-16 units. "When using an API to create a directory,
... the directory name cannot exceed MAX_PATH minus 12", that is 248, so that an 8.3 file name can be appended
(<https://learn.microsoft.com/en-us/windows/win32/fileio/maximum-file-path-limitation>, read 2026-11-11). Whether the
248 counts the null is not stated; the helper uses 247 for a directory it is told will be created.

**`dunce`, the reference.** `dunce` 1.0.5 (`src/lib.rs` `is_safe_to_strip_unc`, read in the local cargo registry)
strips verbatim disk paths only, when every component is a valid non-reserved name, and refuses only when the length is
**over 260**: it accepts 260, one more than the 259 usable, an off-by-one. On non-Windows hosts it never strips, and it
leaves `\\?\UNC\` alone. `dunce` 1.0.5 is already in every app's `Cargo.lock` (transitively), and its licence (CC0-1.0
OR MIT-0 OR Apache-2.0) is on the allow-list of `crates/deny.toml:15`.

**Where verbatim paths come from.** 237 `canonicalize(` calls in 145 Rust files (67 in files under `tests/`), by area:
Debrief 113, Dyno 40, `tools/paddock-dev` 37, shared crates 39 (`paddock-proc` 16), Pitcrew 5, Pitwall 2, `tools/heavy`
1. Each returns `\\?\` on Windows; none says whether its result may meet git, text, a file the user reads, or a
comparison with a plain path.

## Pattern

**A path from `canonicalize` keeps Windows' verbatim prefix and later meets something that does not speak it**: git,
cargo, a text file, a log line, a string comparison with a path that came from elsewhere (a config value, `git`
output, a mount list, the layout's plain paths). Each site that noticed wrote its own stripper, and the copies disagree
on UNC and volume paths and on when stripping is safe.

**Invariant.** A path leaves `canonicalize` through one of three named doors:
- **for tools**: a plain spelling, or a refusal with a reason; never a verbatim path handed to a program that cannot
  read it;
- **plain where possible**: for display, and for Rust's own file APIs, which accept either form;
- **verbatim**: for Win32 calls past the limit, compared only with other verbatim paths.

Comparison is its own operation (`same`). No other code removes or tests the prefix.

## Change

1. **One helper, `paddock_env::winpath` (D-P1).** A module in `paddock-env`, the crate that ADR 0021 decision 5 names
   for OS-specific calls in product code. It has no Paddock dependency (`crates/paddock-env/Cargo.toml`: serde,
   serde_json, log, libc, windows-sys), so every crate that strips today, `paddock-proc` included, can depend on it
   without a cycle. The text functions are pure and take the platform as a parameter, so every OS tests the Windows
   rules (ADR 0087's rule, "Windows logic that can be pure takes the platform as a parameter").
   - **The safety rule** (`safe_to_strip`): a verbatim drive path (`\\?\C:\...`) or UNC path (`\\?\UNC\srv\share\...`)
     may lose its prefix only when all of these hold:
     - every component is a valid Windows name that is not reserved (`CON`, `PRN`, `AUX`, `NUL`, `COM1`-`COM9`,
       `LPT1`-`LPT9`, with or without an extension, and with trailing dots or spaces; `dunce`'s cases are copied into
       the tests);
     - no component is `.` or `..`;
     - the result is at most 259 UTF-16 units, or 247 for `Purpose::CreateDir`.
   - `plain(p, os) -> Cow<Path>`: the prefix removed when `safe_to_strip`, else **unchanged**. This is for display and
     for Rust's file APIs only, which open either form. Its doc comment says it may return a verbatim path.
   - **`for_tools(p, os) -> Result<String, NotPlain>`** (review P1-1): the path for another program (git, cargo, npm,
     Git bash, Explorer). It is the plain spelling with `/` separators (`cygpath -m`'s form; today's `home.rs` `mixed`),
     or an error that refuses anything that cannot be made plain: a volume GUID path, a `\\.\` device path, an unsafe
     name, or a path over the limit. `NotPlain { path, reason, next_step }` displays as one ER-27 line, for example:
     - "cannot hand `\\?\C:\work\con.txt` to git: `con` is a reserved Windows name; rename the folder or file";
     - "cannot hand `\\?\Volume{1b2c…}\x` to git: the folder is on a volume without a drive letter; mount it on a drive
       letter or a folder path";
     - "cannot hand `\\?\C:\…` (312 characters) to git: the path is longer than Windows' 259; move the work folder to a
       shorter path (or enable long paths: git `core.longpaths`)".

     Callers return the error to their user; none falls back to the verbatim text. Incident 3 (git given a verbatim
     path) can then only end in this error, never in git's "could not open".
   - `real(p) -> io::Result<PathBuf>` = `canonicalize` then `plain`; `real_for_tools(p) -> Result<String, RealError>`
     = `canonicalize` then `for_tools`; `real_or_given(p) -> PathBuf` (today's `paddock_repo_graph::real_path`: the given
     path when it cannot be resolved).
   - `real_verbatim(p) -> io::Result<PathBuf>`: `canonicalize` unchanged, the one named door to the verbatim form.
   - **`same(a, b, os) -> bool` and `components(p, os)`** (review P2-3): comparison never opens anything, so it strips
     the prefix **lexically** from drive and UNC paths whatever `safe_to_strip` says: `\\?\C:\x\con` equals
     `C:\x\con`, and `\\?\UNC\srv\share\x` equals `\\srv\share\x`. Volume GUID and device paths compare only with the
     same form. On Windows, `/` and `\` are one separator, a trailing separator is ignored, and names compare by
     **Windows ordinal ignore-case**, the rule `CompareStringOrdinal(.., bIgnoreCase = TRUE)` applies "using the
     operating system uppercase table" (<https://learn.microsoft.com/en-us/windows/win32/api/stringapiset/nf-stringapiset-comparestringordinal>,
     read 2026-11-11), never a linguistic or locale-dependent comparison. The pure function maps each `char` through
     its single-character simple uppercase (`char::to_uppercase` when it yields one character, else the character
     itself). That matches a 1:1 table for the BMP letters paths use. Whether it equals the OS table in every case
     (for example `ß`, `ſ`, the Turkish dotted and dotless `i`) is **unverified**, and WP-P1 compares it with
     `CompareStringOrdinal` over a word list on the Windows CI leg. Off Windows the comparison is exact. Rows 8, 11, 12,
     14, 16 and 18 each do a part of this today.
   - **`kind(p, os) -> Kind`** (review P2-5), for classifiers such as #22 and #23:

     | Spelling | `Kind` |
     |---|---|
     | `C:\x`, `\\?\C:\x` | `Drive { letter, verbatim }` |
     | `\\srv\share\x`, `\\?\UNC\srv\share\x` | `Unc { verbatim }` |
     | `\\?\Volume{..}\x` | `Volume` |
     | `\\.\x`, `\\?\GLOBALROOT\..`, other `\\?\` forms | `Device` |
     | anything else | `Relative` |
     | any path on macOS or Linux | `Unix` |

   `paddock_repo_graph::{strip_verbatim, real_path}` and `paddock_files::plain` become calls of `winpath` and then go
   (no shims before v1; memory "no backward compatibility"). `paddock_testkit` re-exports `real_or_given` and `same`
   for tests (ADR 0021 §1's `paths` module gains them).
2. **Every copy moves in one change per frozen state (rule 55).** Rows 1 to 23 call the helper, each with a failing test
   first for the UNC and volume cases it gets wrong (ER-11). An app under a release freeze changes after its tag, by ADR
   0094 §6's route, with a `docs/known-copies.toml` entry `verbatim-path` (canonical `paddock-env`) whose `delete_by` is
   the tag after the frozen one.
3. **Clippy names the door (D-P2).** `disallowed-methods` gains `std::fs::canonicalize` and
   `std::path::Path::canonicalize`, with the reason "use `paddock_env::winpath::real_for_tools` (programs), `real`
   (display, Rust file APIs) or `real_verbatim` (Win32 long paths, compared only with verbatim paths); ADR 0021 amendment
   2026-11-11". `winpath` holds the one `#[expect]`.
   - Rollout by ADR 0094 §6: first `crates/clippy.toml`, `tools/*/clippy.toml` and each unfrozen app's `clippy.toml`;
     `lint/clippy.toml` (which C13 copies into every app) only after the last frozen tag. C13's per-app exemption list
     `clippy_not_yet` in `docs/gates.toml` is shrink-only under C100, so no entry goes there.
   - Each of the 237 calls is migrated or carries `#[expect(clippy::disallowed_methods, reason = "verbatim: <why>")]`;
     an unfulfilled `#[expect]` warns, so a stale one shows itself.
4. **C109 "verbatim paths by hand" (number: D-P3).** A check in the RT-5 Rust check host (`tools/paddock-dev/src/check/`,
   comment-masked Rust like C53, with string literals read). It runs outside `crates/paddock-env/src/winpath*` and
   outside test code (`tests/`, `#[cfg(test)]` items and the files they reach, as C38 scopes them). It reports:
   - (a) **any** non-comment string, raw-string or byte-string literal that contains the verbatim prefix in any spelling
     (`\\?\` raw, `\\\\?\\` escaped, `br"\\?\"`), whatever it is passed to. A `const` holding the prefix (row 8's
     `const UNC: &str = r"\\?\UNC\";`) is reported at the `const`, so the call through it (`p.get(..UNC.len())`) needs no
     data-flow analysis;
   - (b) a pattern or path naming `Prefix::Verbatim`, `Prefix::VerbatimDisk` or `Prefix::VerbatimUNC`.

   A deliberate user (the three "not copies" above, Pitwall's refusal message at `placement.rs:83`) carries
   `// conformance: verbatim — <why>` on the line, or on the comment line above. This is the C54 waiver shape, and a
   waiver that waives nothing warns.

   **Positive controls (ER-33).** Each fixture below must give the stated result:

   | Fixture | Must |
   |---|---|
   | `s.strip_prefix(r"\\?\")` | fail |
   | `starts_with("\\\\?\\UNC\\")` | fail |
   | `b.starts_with(br"\\?\")` | fail |
   | a `const P: &str = r"\\?\";` used only through `P.len()` | fail at the `const` |
   | `const UNC` used through `p.get(..UNC.len())` (row 8's shape) | fail |
   | a `Prefix::VerbatimUNC(..)` arm | fail |
   | the literal inside a test module | pass |
   | the literal inside a comment | pass |
   | the literal in `winpath` | pass |
   | the literal on a line with a waiver | pass |
5. **The error is seen in tests.** `for_tools`'s refusals have tests in `winpath`; each migrated program caller
   (rows 5, 6, 15, the git callers in `tools/paddock-dev`) has one test that the error reaches its user's output with
   the path and the next step (ER-27).
6. **Run-time net (no new mechanism).** Once ADR 0087 is built, its `docs/windows-paths.toml` declares the modules that
   compare paths (rows 8, 11, 12, 14, 16 and 18 after migration), so a change there runs on the test VM before review.
   Until then CI's Windows legs are the only run (ER-9).

## What this does not cover

- **Mapped and `subst` drives.** `canonicalize` resolves a mapped network drive to its share (`\\?\UNC\srv\share\..`),
  and a `subst` drive to its target folder; that is the common report, **not verified here**. `real` then gives
  `\\srv\share\x`, not `Z:\x`, and `same` cannot equate `Z:\x` with `\\srv\share\x` lexically. A caller that compares a
  path the user typed with a canonical one canonicalises both sides. WP-P1 records what `canonicalize` returns for a
  `subst` drive on the CI Windows leg; mapped network drives are not testable there and stay unverified.
- `\??\` NT paths and 8.3 short names beyond what `canonicalize` already resolves.
- Paths typed by the user or read from config (`paddock-setup`'s `LocalPath::parse` already refuses namespaces; that
  stays).
- Separator and case differences of paths that never went through `canonicalize` (the `same` helper serves them, but
  no check finds them).
- TypeScript: the UI never receives a verbatim path from a migrated command; no TS check.

## Release impact (ER-31)

Read at `dyno-v1.1.0` with `git grep`, and Dyno's `Cargo.lock` at the tag. Compiled into Dyno 1.1.0, through
`paddock-setup`, `paddock-proc`, `paddock-service`, `paddock-files` and `paddock-download`, all in that lockfile:
- rows 8 to 12 and 14;
- row 2 (`paddock-files` `plain`);
- row 13 (`paddock-service` `without_verbatim_prefix`, which the Dyno helper builds);
- row 17 (`dyno-helper`);
- row 23 (`paddock-download`).

**Reachability is not checked.** Whether a wrong UNC or volume spelling there ever changes an outcome for a user is not
analysed here; for example, a program on a network share may be refused by the trust rule before most of these rows
run. Row 21 (`paddock-metrics`) is in the tag's tree but in no app's `Cargo.lock`, there or on the train: not shipped.
WP-P0 reads each shipped row and reports any wrong decision as a finding for its app's next tag (D-P4). Nothing here
gates a tag by itself.

## Work packages (raw agent-days, one agent)

| WP | Content | Days |
|---|---|---|
| WP-P0 | Re-read rows 1 to 23 and the 237 calls on the then-current train; for each shipped row, say whether a wrong spelling can change a decision; record each call's door (tools, plain, verbatim, compare) in the WP report | 0.5 to 0.75 |
| WP-P1 | `paddock_env::winpath` with `safe_to_strip`, `plain`, `for_tools` and `NotPlain`, `real*`, `same` with the ordinal fold, `components`, `kind`; table tests on both platform values (`dunce`'s reserved-name cases, 259/260/247/248 boundaries, UNC, volume, device, `same` across the prefix); the fold compared with `CompareStringOrdinal` and a `subst` probe on the CI Windows leg; 99 % lines (crates gate) | 1 to 1.5 |
| WP-P2 | Shared crates and tools onto it, each row's UNC/volume test failing first: `paddock-repo-graph`, `paddock-files`, `paddock-proc`, `paddock-service`, `paddock-setup`, `paddock-metrics`, `paddock-download`, `tools/paddock-dev` (rows 1, 2, 5 to 14, 21, 23); delete the old helpers | 1.5 to 2 |
| WP-P3 | Apps: Debrief (rows 3, 4, 15, 16, 19, 20, 22), Dyno (17), Pitcrew (18); frozen apps after their tag with the `verbatim-path` copies entry | 1.5 to 2 |
| WP-P4 | C109 in the RT-5 host with the fixtures of item 4; README row; report-only until WP-P3 has landed, then strict | 0.75 to 1 |
| WP-P5 | Clippy entries and the triage of the 237 calls (migrate or `#[expect]` with a reason) across about nine Cargo workspaces (`crates/`, `tools/paddock-dev`, `tools/heavy`, and the Debrief, Dyno, Pitcrew and Pitwall workspaces, Debrief's app and SDK tool among them); `lint/clippy.toml` after the last frozen tag; a fixture crate that calls `std::fs::canonicalize` fails with the preset | 2 to 3.5 |
| WP-P6 | Lesson, ADR 0021 text merge, register row states | 0.25 |
| | **Total** | **7.5 to 11** |

**Order.** WP-P1 comes before P2 and P3. P4 can come any time: it lands report-only, and goes strict after P3. P5 comes
after P2. P1 and P2 are one shared landing (rule 43), outside a release freeze (ADR 0049).

**CI runs (rule 50).** One branch run is the minimum per landing unit:
- P1 and P2 together: one run, which reaches every workspace that depends on the changed crates;
- P3: one run per app branch, three in all;
- P4: one run (tools and plan only);
- P5: one run per workspace batch, about three when batched with P2 and P3.

That is **6 to 8 branch runs**, plus a fix round for any that turn red (ER-10), and the train's full landing run on
every OS (ADR 0080). WP-P0 and WP-P6 are docs-only (no CI, rule 50 (d)).

## Failing tests the work starts with (ER-11; not written in this change)

- `paddock-env` `winpath`:
  - `a_unc_path_is_plain_with_two_backslashes`;
  - `a_volume_guid_path_keeps_its_prefix_in_plain`;
  - `a_reserved_name_keeps_the_prefix_in_plain`;
  - `a_path_of_260_units_keeps_the_prefix_and_259_loses_it` (the `dunce` off-by-one);
  - `same_ignores_the_prefix_even_for_a_reserved_name`.
- `paddock-env` `winpath`, `for_tools` (review P1-1):
  - `for_tools_refuses_a_volume_guid_path` (`\\?\Volume{1b2c…}\x`: `Err`, the message names the path, "no drive
    letter" and the next step);
  - `for_tools_refuses_a_reserved_name` (`\\?\C:\work\con.txt`: `Err` naming `con`);
  - `for_tools_refuses_a_path_over_259_units` (a 260-unit drive path: `Err` naming the length and the next step;
    259 units: `Ok`).
- `paddock-repo-graph`: `strip_verbatim(r"\\?\Volume{x}\a")` is not the relative path `Volume{x}\a` (fails today).
- `tools/paddock-dev` `home.rs`: `mixed(Path::new(r"\\?\UNC\srv\share\x"))` is `//srv/share/x` (today
  `UNC/srv/share/x`); for `\\?\Volume{x}\a` it is an error, not `Volume{x}/a`.
- `paddock-service` `trust.rs`: `require_same_path(r"\\srv\share\a.exe", r"\\?\UNC\srv\share\a.exe", Style::Windows)`
  is `Ok` (today `Redirected`).
- `paddock-setup` `checkdir.rs`: `components(r"\\?\UNC\srv\share\a")` starts with the server, not `UNC`.

## Decisions for the user

| # | Question | Options | Recommendation |
|---|---|---|---|
| D-P1 | Where the helper lives | (a) a `winpath` module in `paddock-env` (ADR 0021 decision 5: no new crate); (b) a new zero-dependency crate `paddock-paths`; (c) depend on `dunce` | **(a)**. (c) does not convert UNC, never strips off Windows (so macOS cannot test the rule), accepts 260 units, and would still need our own `same`/`for_tools`. Its reserved-name rules are copied into (a)'s tests instead |
| D-P2 | Clippy on `canonicalize` | (a) disallow both forms everywhere; every call migrated or `#[expect]`ed with its reason (2 to 3.5 days for 237 calls); (b) no Clippy entry, C109 only (finds strippers, not unmarked verbatim paths) | **(a)**: the three incidents of run 7904 had no stripper at all; they used `canonicalize` and compared. Only (a) makes that choice visible |
| D-P3 | The check's number | (a) C109, the next free number on 2026-11-11 (C108 is ADR 0097's; ADR 0081's liveness check and ADR 0096's index check are still written unnumbered); (b) leave unnumbered until landing | **(a)**, confirmed by the integrator at landing |
| D-P4 | Shipped rows (2, 8 to 14, 17, 23 compiled into `dyno-v1.1.0`) | (a) WP-P0 decides per row; a wrong decision found is a finding for the app's next tag; (b) block Dyno's next tag on WP-P2/P3 regardless | **(a)** |

## Review answers (revision 1, `dbd7028a1`)

| Finding | Answer in revision 2 |
|---|---|
| P1-1 `for_tools` handed git the verbatim form | `for_tools` returns `Result<String, NotPlain>` with an ER-27 message (path, reason, next step); `plain` is for display and Rust file APIs only; three failing tests (volume GUID, reserved name, over 259) |
| P2 volume count and bold | split into "output used as a path" (rows 1, 6, 7, 10, 13, 15, 17; row 17 moved there because `verify_chain` walks the result) and comparers; bold only on wrong results |
| P2 release impact | rows 2, 13, 23 added; row 21 is in no app's lockfile; "reachability not checked" stated |
| P2 `same` | lexical strip for drive and UNC whatever the safety rule; Windows ordinal ignore-case, with its approximation and a VM comparison |
| P2 limit | 259 usable, 248 for directory creation (247 used), `dunce`'s off-by-one |
| P2 `kind`, mapped and `subst` drives | `kind` table; mapped and `subst` drives in "does not cover", with a `subst` probe in WP-P1 |
| P2 C109 scope | any non-comment literal holding the prefix; const-indirection fixtures |
| P2 row 9, incident 2, `paddock-download`, `Prefix` files | row 9 documented; incident 2's local copy described; row 23 added; five `Prefix::Verbatim*` files listed |
| P2 estimate ranges and CI runs | ranges per WP (total 7.5 to 11); 6 to 8 branch runs |
| P2 overlap | "Overlap checked" line |

## Rollback

C109 leaves the check host's registry with one line; the Clippy entries are removed from the `clippy.toml` files; the
helper is additive, and reverting WP-P2/P3 restores the copies (rule 55 then needs a `verbatim-path` copies entry).
