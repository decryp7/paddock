# 94. Text is cut on character boundaries through one shared helper

- Status: Accepted 2026-11-10 (user), all decisions as recommended. History: proposed 2026-11-10 at the user's request
  (ER-21 and rule 55: one panic, `a5aca536d`, and about 60 hand-written guards with no failure, among them two shared
  helpers that only their own crates' families call); revision 2 (`2e5017b46`) answered the independent review of
  revision 1 (`55286a33f`, "changes"); the review approved revision 2; revision 3 folds in its P2s (see "Review
  answers"). Nothing here is built yet. ER-23 does not apply (no drawings).
- Trimmed 2026-11-11 (user, comparable-practice review): TRIM: keep the helper (WP-1), the corpus, the `never_panics` run, migration of untrusted-input sites, the Clippy entries and the rule text; dropped WP-0's classification and the cosmetic migrations.
- Date: 2026-11-10
- Related: ER-11, ER-21 / rule 40, ER-22 / rules 42 and 55 (share first; one implementation per mechanism; 55(e): C58
  is planned, not built), ER-33 / rule 71; ADR 0019 (`paddock-limits`), ADR 0034 (copies register, C58), ADR 0049 §3
  and D7 (release freeze, `Release-part`, C81), ADR 0088 (accepted: one ratchet engine, on
  `origin/docs/adr-windows-gate-ratchets`), C13 (the family clippy preset `lint/clippy.toml`, copied into every app's
  `clippy.toml`), memory "Debrief C13 ratchet".
- Number: 0094 was free on every `origin` branch on 2026-11-10, re-checked right before each commit (highest family ADR
  file on any branch: `0093`, on `origin/docs/adr-scheduled-owners`; `0092` on `origin/docs/adr-private-files`); no ADR
  worktree under `/Volumes/d7sst7/dev-targets/wt/` holds an uncommitted `0094` or `0095`.
- Evidence: read on 2026-11-10 with `git show`, `git grep` and `git log` after `git fetch`. Paths are from
  `origin/train/2026-11-09-3`: revision 1 read `67a633ef3`; revision 2 re-counted the guards at `c151422a4` (3 commits
  later, same 41 files). Measured on the Mac (stable rustc 1.99.1): a clippy run over the shared crates (below) and a
  6-line program for the std behaviour. No user data was read. The heads, re-read right before the commit:

```evidence-heads
read: 2026-11-10
origin/train/2026-11-09-3 c151422a4
origin/main b51c72509
origin/docs/adr-windows-gate-ratchets db7782e01
origin/docs/adr-evidence-heads b32655fef
origin/docs/adr-private-files e2f7e946b
origin/docs/adr-scheduled-owners 390ec29bd
```

## Context

### The one failure

`a5aca536d` (2026-11-05): `ScreenFacts::parse` (`apps/debrief/crates/debrief-ai/src/vision/facts.rs`) did
`m.truncate(200)` on a serde error. The error quotes the bad value, so a multi-byte character across byte 200
panicked. Found by the nightly `fuzz_vision_facts` target (`.gitea/workflows/debrief-fuzz.yml:51`), which existed since
`9c8d5d041` (2026-10-21). The coordinator reports the crash kept `debrief-fuzz` red for 11 days; not re-measured here.
The fix used yet another hand-written form, `chars().take(200).collect()`.

### Hand-written guards (no failure)

Each of these works; each is the same mechanism written again, the shape rule 55 is about:

| Commit | Guard |
|---|---|
| `29e051f4d` (2026-10-22, `feat(parsers)`) | Debrief's parser verification grew its own `floor(s, i)` (`apps/debrief/crates/debrief-core/src/parser/verify/mod.rs:383`); `07f89413d` (a `test:` commit the same day) added its test, that a cut inside `€` falls back to the character's start |
| `ec8b15b37` / `f6c87b551` (2026-10-24, the same `test:` patch on two parents) | tests for `paddock-diag`'s own boundary loop (`a_cut_never_splits_a_character`, `crates/paddock-diag/tests/report.rs`) |
| `a7276b216` (2026-10-30, Pacenotes) | `pacenotes-parse` cuts qualified names at `MAX_QUALIFIED_BYTES` (1 KiB) "on a character boundary" with its own `cut` (`extract.rs`) |
| shared, used only by the plugin crates | `paddock_plugin_api::cut_line` (`crates/paddock-plugin-api/src/protocol.rs:294`, copied as `apps/debrief/crates/debrief-plugin-api/src/protocol.rs:439`), called in production at `crates/paddock-plugin-api/src/serve.rs:93`, `crates/paddock-plugin-host/src/services.rs:142`, `:145`, `:186`, `apps/debrief/crates/debrief-plugin-api/src/forward.rs:51` and `apps/debrief/crates/debrief-plugin-host/src/output.rs:140`, plus its own tests and `paddock_service::cut` (`crates/paddock-service/src/os_log.rs:40`, characters plus `…`); and Debrief's unshared `truncate_chars` (`debrief-ai/src/assistant/tools.rs:40`) |

**The copies.** `git grep -l is_char_boundary` over `*.rs`, without `tests/`, `tests.rs`, fuzz and bench files: **41
production files, 44 calls** (the same at `c151422a4`): Debrief 19 in `apps/debrief/crates` and 3 in its app, Dyno 3,
Pitcrew 3, Pitwall 2, Spotter 1, shared crates 10, and 1 in Dyno's vendored `tray-icon` patch
(`apps/dyno/vendor/tray-icon/src/platform_impl/windows/mod.rs:702`, from `apps/dyno/vendor/tray-icon-recovery-trace.patch`).
Named cut functions over a wider pattern (`fn` names containing `cut`, `clip`, `trunc`, `floor`, `shorten`, `ellips`,
outside tests, two underscores at most): 81 matches, of which 58 are candidates (`clip` 18, `cut` 15, `truncate` 11,
`floor`/`floor_*` 6, `cut_line` 2, `clip_value` 2, `shorten` 2, `truncate_chars`, `cut_to_char`, `clip_json`) and
**about 50 cut text**; the rest are other things (`paddock-dialogs` `Size::floor`, Dyno's `nt.rs` buffer `truncate`,
Pitwall's token cut, `paddock-pitwall`'s request `truncate`, a test helper). Their units differ: `cut_to_char` cuts
bytes despite its name. With the reviewer's count of call sites, about 60 to 75 sites change (WP-0 makes it exact).

**Measured (shared crates).** `cargo clippy --workspace --lib --bins --all-features` in `crates/` with a scratch
`CLIPPY_CONF_DIR`, `-A clippy::all -W clippy::string_slice -W clippy::disallowed_methods`, 1 min 56 s:

| Lint | Hits | Notes |
|---|---|---|
| `clippy::string_slice` | 104 sites in 40 files | most index with a `find` result on an ASCII delimiter or a hex string (`paddock-download/src/error.rs:178`), correct by construction; 28 in `paddock-diag/src/personal.rs` alone |
| `disallowed-methods` `str::is_char_boundary` | 12 | every hand-written cut; the path resolves (measured) |
| `alloc::string::String::truncate` | 2 | `paddock-channels/src/message.rs:66` (after a boundary loop), `paddock-health/src/notify/os/toast.rs:140` (after `trim_end`): both correct today |
| `str::split_at` | 2 | not read further |
| `alloc::string::String::drain` | 1 | not read further |

Apps were not measured (WP-0). App code has the same shape, for example `debrief-agent/src/run.rs:249` `&name[..15]`,
guarded by `is_run_name`'s ASCII check: safe, but only by reading the caller, which no lint can do.

**The std side.** `str::floor_char_boundary` and `ceil_char_boundary` are stable since 1.91.0 (`#[stable(feature =
"round_char_boundary", since = "1.91.0")]` in the toolchain's `core/src/str/mod.rs:421`; compiled and run here:
`"ab€cd".floor_char_boundary(3) == 2`, while `truncate(3)` and `&s[..3]` panic). App MSRVs are lower: Pitcrew 1.88,
Dyno, Pitwall and Spotter 1.89, Debrief 1.95 (`apps/*/Cargo.toml`), so the helper cannot call them yet.

**The fuzz side.** `paddock_testkit::corpus` (120 files use `corpus` or `never_panics`) makes bit flips, truncations, a
lone `0xc3`, `0x00` and `0xff` runs: it never builds valid multi-byte text that puts a character across a cut point, so
the shared never-panics tests could not have found the failure.

**The gates today.** C58 (the copies-register check) is planned, not built (rule 55(e); `conformance/test/cli-list.test.mjs:44`
asserts `copies` is not a command yet). During an app's release freeze, C81 (ADR 0049 §3) refuses changes under
`apps/<app>/**` without a `Release-part` trailer, and C13 fails an app whose `clippy.toml` lacks a preset path. Pacenotes
has no `clippy.toml` (`apps/*/clippy.toml` lists Debrief, Dyno, Pitcrew, Pitwall and Spotter), so the preset does not
reach it.

## Pattern

**Code cuts text at a byte count with an operation that panics inside a character** (`String::truncate`, `&s[..n]`,
`split_off`, `drain`), or guards it with a boundary loop written again in each file. Every author writes the guard
fresh, at least four forms exist (byte loop, `chars().take`, `char_indices().nth`, two shared helpers the other crates do not use), and the
tests that would find a miss feed only ASCII or invalid bytes. One miss shipped to a fuzz target and stayed red for days.

**Invariant.** Text is cut to a byte or character budget only through one shared function that cannot split a
character; the cut primitives `String::truncate`, `String::split_off` and `str::is_char_boundary` are refused by
clippy everywhere else; and the shared hostile corpus puts multi-byte characters across the cut points a parser uses.

## Decision

1. **`paddock_limits::text`** (D1): a new module in the leaf crate that already owns "bounded replies that say what
   they left out" (ADR 0019; serde and log only, a dependency of Debrief, Dyno, Pitcrew and Pitwall today; Spotter
   gains it). The crate already exports `Cut` (`bounded.rs:351`) and `Unit` (`id.rs:317`), so the new names differ:
   - `cut_bytes(s: &str, max: usize) -> &str`: the longest prefix of at most `max` bytes that ends on a character
     boundary (byte budgets: wire caps, log lines, file sizes);
   - `cut_chars(s: &str, max: usize) -> &str`: at most `max` characters (display budgets);
   - `cut_marked(s: &str, max: usize, unit: CutUnit, mark: &str) -> TextCut<'_>` with `CutUnit { Bytes, Chars }` and
     `TextCut { text: Cow<str>, was_cut: bool, dropped: usize }`, where `dropped` counts in the call's `CutUnit` (bytes
     for `Bytes`, characters for `Chars`) and excludes the mark; the mark counts inside `max`, so callers stop
     subtracting it, and a mark longer than `max` gives the bare cut without a mark.
   The boundary search is its own five-line loop until every workspace's MSRV is at least 1.91, then
   `floor_char_boundary`. A cut may still split a grapheme cluster (`e` + combining accent); that is valid UTF-8 and
   out of scope.
   `paddock_plugin_api::cut_line` becomes a call of `cut_bytes` and then goes (a public API change: CHANGELOG entry
   and a note for plugin authors, since a plugin may call it; `debrief-plugin-api`'s copy goes with it);
   `paddock_service::cut` and `truncate_chars` become calls of `cut_marked`.
2. **Clippy refuses the primitives elsewhere** (D2). `crates/clippy.toml`, every unfrozen app's own `clippy.toml`
   (C13 allows an app to add entries) and, once no app is frozen, `lint/clippy.toml` (C13 copies it into every app) gain
   `disallowed-methods` entries for `alloc::string::String::truncate`,
   `alloc::string::String::split_off` and `str::is_char_boundary` (the signature of a hand-written cut), each with the
   reason "cut text with `paddock_limits::text` (ADR 0094)". `paddock_limits::text` is the one
   `#[expect(clippy::disallowed_methods, reason = "...")]`. A site that truly needs a byte position (a parser that
   checked boundaries itself) carries its own `#[expect]` with a reason; `#[expect]` warns when unfulfilled, so a stale
   one shows itself without a list. Pacenotes gets the preset when it gets a `clippy.toml` (its own C13 adoption, not
   this ADR). The vendored `tray-icon` site stays as it is: vendored code keeps upstream's text so the patch applies,
   and clippy does not lint it as a workspace member; it leaves with the patch.
3. **`clippy::string_slice` is not adopted** (D3): 104 sites in the shared crates alone, nearly all correct by
   construction. The cost: `&s[..n]` with a numeric `n` stays unguarded in new code; §4 finds the wrong ones in parsers
   by running them, and review catches the rest. This is the one known gap.
4. **The hostile corpus carries multi-byte text** (D5). `paddock_testkit` gains `corpus_cutting(seed, cuts: &[usize])`:
   for each cut point, a 2-, 3- and 4-byte character (`é`, `€`, `😀`) placed so that it starts 1, 2 or 3 bytes before
   the cut and once exactly at it, padded with ASCII from the seed: 12 inputs per cut point. A parser's tests pass its
   registered byte limits (the ADR 0019 rows, `MAX_QUALIFIED_BYTES` and the like), so cut points beyond 1 KiB are
   reached without huge inputs. `corpus(seed)` itself adds the same characters inserted at each of the first 64
   offsets (192 inputs) and padded to 256 and 1,024 bytes (24 inputs): about 216 more per seed. A panic it finds is fixed
   through §1, each with its failing test first (ER-11). Debrief's cargo-fuzz dictionaries get the three characters.
5. **The helper's own tests**: an exhaustive test over every string on the alphabet {`a`, `é`, `€`, `😀`} up to length
   5 and every `max` in `0..=len+4` (the result is a prefix, valid UTF-8, within the budget, maximal, and `dropped` is
   right in both units), so the boundary cases are all enumerated rather than sampled; plus a proptest over arbitrary
   strings and limits, since `paddock-limits` already uses proptest (one of 11 shared crates that do).
6. **Migration and freezes** (D4, D6). One change moves every unfrozen copy (rule 55). An app under a release freeze
   (ADR 0049 D7) is not touched before its frozen tag: C81 would refuse the change and the frozen release must not
   change. For such an app:
   - its `clippy.toml` and its sites change **after** its tag, in its first post-tag change;
   - no C13 exception is needed: while any app is frozen, the three paths go into the unfrozen apps' own
     `clippy.toml` (C13 allows additions) and `crates/clippy.toml`, and `lint/clippy.toml` gets them only after the last
     frozen tag, when every app takes them. (C13's per-app exemptions live in the `[[ratchet]]` `clippy_not_yet` list in
     `docs/gates.toml`, which C100 keeps shrink-only, ADR 0060 line 226, so a new entry there would be refused);
   - its copies-register entry `text-cut` (canonical `paddock-limits`) names as `delete_by` the tag **after** the frozen
     one, never the frozen one, so the frozen release is not blocked by its own register;
   - C58 is not built, so until it is the release steward checks the `text-cut` entries by hand at each tag (the
     release checklist gets one line).

## Failing tests the work starts with

1. `paddock_testkit`: `corpus_puts_a_multibyte_character_across_every_cut_point`: a planted parser that does
   `String::from_utf8_lossy(b).into_owned().truncate(200)` must be reported by `never_panics(&corpus(seed), ..)` and by
   `corpus_cutting(seed, &[200])`. Fails today (the corpus finds no panic).
2. `paddock_limits::text`: the exhaustive and proptest tests of §5, plus `cut_marked` with a mark longer than `max`.
3. A clippy fixture (a small crate under `lint/fixtures/` or the C13 test) that calls `String::truncate` and
   `str::is_char_boundary` must fail with the preset: the positive control that the entries resolve (ER-33).
4. Each migrated site whose output changes (a mark now counted inside the budget, a byte cut that becomes a character
   cut or the reverse) gets its snapshot or assertion updated in the same commit, with the old and new text in the
   message; a site that did not cut correctly gets a failing test first.

## Parallelism and cost (rule 51)

Nothing slow: the helper is O(cut length); the corpus adds about 216 inputs per seed plus 12 per cut point to tests that
today run a few hundred; the clippy entries cost nothing measurable in an existing clippy run.

## Alternatives

- **`clippy::string_slice` with an ADR 0088 ratchet of today's sites.** Precise about where a panic can come from, but
  104 shared-crate sites (apps unmeasured, likely several hundred) to list or annotate, nearly all correct. Kept as D3 (b).
- **Make `paddock_plugin_api::cut_line` the canonical one.** It is byte-only, in a crate apps without plugins do not
  depend on, and has no character or mark form.
- **A new crate `paddock-text`.** One more crate for three functions; `paddock-limits` already owns bounded text.
- **Keep the copies, add tests.** That is today; the failure had a fuzz target and still shipped the cut.
- **`unicode-segmentation` grapheme cuts.** A new dependency for a problem nobody reported.

## Consequences

- One function to read when a cut looks wrong; four cut forms and two narrowly used shared helpers disappear; new code cannot
  call `truncate` on a `String` or hand-write a boundary loop without a reason in the source.
- `Vec::truncate` is unaffected (the entry names `String::truncate`; clippy resolves the type).
- Shared never-panics tests may find new panics the day §4 lands; each is a real bug.
- Plugin authors lose `cut_line` (CHANGELOG, plugin note).

## Rollback

Drop the clippy entries; the helper and the corpus variants are harmless to keep.

## Review answers (revision 2)

- **P1-1 incident framing.** Only `a5aca536d` was a failure; `07f89413d` and `ec8b15b37`/`f6c87b551` are `test:`
  commits for guards that worked, and `floor` came from `29e051f4d`. Status line, Context, Pattern and the README line
  now say one panic plus about 60 hand-written guards (rule 55); the Pacenotes guard `a7276b216` is cited.
- **P1-2 freeze and gating.** C58 is not built; a `delete_by` of the frozen tag would block that release; C81 and C13
  would refuse a frozen app's change. §6 now changes a frozen app only after its tag, under a stated C13 exception,
  with `delete_by` the tag after, checked by the steward by hand until C58 exists. D6 and the closing line follow.
  (Revision 3: the C13 route corrected, see below.)
- **P2s.** `TextCut`/`CutUnit` and the unit of `dropped` (§1); the existing shared helpers and the `cut_line` removal
  with CHANGELOG and plugin note (§1); the vendored site is Dyno's `tray-icon` patch, not sysinfo (§2); proptest is
  used by 11 shared crates, `paddock-limits` among them, so §5 uses it beside the exhaustive test; the corpus takes cut
  points as a parameter and adds about 216 inputs per seed (§4); WP-2's fixes are estimated apart (WP-2b) and WP-3 is
  per app, not mechanical, within ADR 0049's 48-hour cap; named functions recounted with a wider pattern (about 50);
  failing tests listed; Pacenotes' missing `clippy.toml` and the unguarded `&s[..n]` stated (§2, §3); README blank
  line fixed.

## Review answers (revision 3, after approval)

- **P2-1 the C13 route.** A comment at `C13-rust-lints.mjs:5` is not the mechanism; per-app exemptions are the
  `clippy_not_yet` ratchet in `docs/gates.toml`, which C100 keeps shrink-only (ADR 0060 line 226), so a new entry would
  be refused. §2 and §6 now add the paths to unfrozen apps' own `clippy.toml` and to `lint/clippy.toml` only after the
  last frozen tag: no exception needed.
- **P2-2 `cut_line` callers.** It is called in production in `paddock-plugin-api`, `paddock-plugin-host` and Debrief's
  two plugin crates (sites in the table); the Status line and the row say so, and WP-3's shared part names
  `paddock-plugin-host`.

## Decisions (accepted 2026-11-10 as recommended)

| # | Decision | Options | Recommended |
|---|---|---|---|
| D1 | Where the helper lives | (a) `paddock_limits::text`; (b) a new `paddock-text` crate; (c) promote `paddock_plugin_api::cut_line` | **(a)**: the leaf crate for bounded output, used by four of five apps, no new dependency |
| D2 | Which primitives clippy refuses | (a) `String::truncate`, `String::split_off`, `str::is_char_boundary`; (b) (a) plus `str::split_at` and `String::drain`; (c) none | **(a)**: the measured cut forms; `split_at` and `drain` are mostly used with `find` results (WP-0 counts them; (b) if they turn out to be cuts) |
| D3 | `clippy::string_slice` | (a) not adopted, the gap stated; (b) adopted with an ADR 0088 ratchet of today's sites | **(a)** |
| D4 | Migration | (a) every unfrozen copy in one landing line, per app parts within ADR 0049's 48-hour cap; frozen apps after their tag (§6); (b) each app at its next release phase | **(a)** (rule 55) |
| D5 | Hostile corpus | (a) `corpus` variants plus `corpus_cutting(seed, cuts)` and the fuzz dictionaries; (b) only the helper's own tests | **(a)**: the check that would have found the failure on its first run |
| D6 | Tag gating | (a) no tag is gated by this ADR: a frozen app's `text-cut` entry has `delete_by` the tag after its frozen one, checked by hand until C58 exists; (b) every app's next tag needs zero hand-written cuts | **(a)** |

## Work packages (raw agent-days, one agent)

| WP | Content | Days |
|---|---|---|
| WP-0 | Clippy measure over every app workspace and `tools/` with the D2 entries plus `split_at`/`drain` (`CARGO_BUILD_JOBS=2`, one at a time); classify the ~58 named functions and count call sites; which apps are frozen; the per-app lists for WP-3 | 0.5 |
| WP-1 | `paddock_limits::text` (`cut_bytes`, `cut_chars`, `cut_marked`, `TextCut`, `CutUnit`), exhaustive and proptest tests, doc comments, CHANGELOG | 1 |
| WP-2a | `corpus` multi-byte variants and `corpus_cutting` with the planted positive control, fuzz dictionaries | 0.75 |
| WP-2b | Run the affected `never_panics` suites with the new inputs and fix what they find, each failing first; time-boxed at 1.5 days, anything left becomes its own bug with a failing test | 1.5 (box) |
| WP-3 | Migration per app (not mechanical: about 60 to 75 sites, some change output and snapshots): shared crates (`paddock-plugin-api` and `paddock-plugin-host` with `cut_line`, `paddock-service`), Debrief's plugin crates with its `cut_line` copy, `paddock_service::cut`, CHANGELOG and plugin note 1; Debrief 1.5; Dyno, Pitcrew, Pitwall and Spotter 1; frozen apps' parts after their tags | 3.5 |
| WP-4 | Clippy entries in `lint/clippy.toml`, `crates/clippy.toml`, each unfrozen app's `clippy.toml` and `tools/*/clippy.toml`; the clippy fixture; `lint/clippy.toml` after the last frozen tag; the `text-cut` entry and the steward's checklist line if any app is frozen | 0.75 |
| WP-5 | Rule text (proposed: "text is cut only through `paddock_limits::text`"), lesson, ADR index | 0.25 |

**8.25 days** with WP-2b at its box. First cut (helper, corpus, shared crates and their lint): WP-0, WP-1, WP-2a,
the shared part of WP-3 and WP-4 for `crates/` = 3.75 days. **Gates no tag** (D6).
