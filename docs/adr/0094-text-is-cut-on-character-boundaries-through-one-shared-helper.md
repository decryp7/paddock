# 94. Text is cut on character boundaries through one shared helper

- Status: Proposed 2026-11-10. Drafted at the user's request of 2026-11-10 (ER-21: one class of bug, seen in three crates
  and copied by hand in about 40 more). Nothing here is built yet. ER-23 does not apply (no drawings).
- Date: 2026-11-10
- Related: ER-21 / rule 40, ER-22 / rules 42 and 55 (share first; one implementation per mechanism), ER-11, ER-33 /
  rule 71; ADR 0019 (`paddock-limits`: limits that explain themselves), ADR 0034 (copies register, C58), ADR 0088
  (accepted: one ratchet engine, on `origin/docs/adr-windows-gate-ratchets`), C13 (the family clippy preset
  `lint/clippy.toml`, copied into every app's `clippy.toml`), memory "Debrief C13 ratchet" (all Debrief crates pass C13
  by `debrief-v1.2.0`).
- Number: 0094 was free on every `origin` branch on 2026-11-10, re-checked right before the commit (highest family ADR
  file on any branch: `0093`, on `origin/docs/adr-scheduled-owners`; `0092` on `origin/docs/adr-private-files`); no ADR
  worktree under `/Volumes/d7sst7/dev-targets/wt/` holds an uncommitted `0094` or `0095`.
- Evidence: read on 2026-11-10 with `git show`, `git grep` and `git log` after `git fetch`; paths without a branch are
  from this ADR's base `origin/train/2026-11-09-3`. Measured on the Mac (stable rustc 1.99.1): a clippy run over the
  shared crates (below) and a 6-line program for the std behaviour. No user data was read. The heads, re-read right
  before the commit:

```evidence-heads
read: 2026-11-10
origin/train/2026-11-09-3 67a633ef3
origin/main b51c72509
origin/docs/adr-windows-gate-ratchets db7782e01
origin/docs/adr-evidence-heads f2bdba1df
origin/docs/adr-private-files 3ba5a4a16
origin/docs/adr-scheduled-owners 34e7eeb00
```

## Context: the incidents

| # | Commit | What happened |
|---|---|---|
| 1 | `a5aca536d` (2026-11-05) | `ScreenFacts::parse` (`apps/debrief/crates/debrief-ai/src/vision/facts.rs`) did `m.truncate(200)` on a serde error. The error quotes the bad value, so a multi-byte character at byte 200 panicked. Found by the nightly `fuzz_vision_facts` target (`.gitea/workflows/debrief-fuzz.yml:51`), which existed since `9c8d5d041` (2026-10-21). The coordinator reports the crash kept `debrief-fuzz` red for 11 days; not re-measured here. The fix used a third hand-written form, `chars().take(200).collect()` |
| 2 | `07f89413d` (2026-10-22) | The core parser grew its own `floor(s, i)` (`apps/debrief/crates/debrief-core/src/parser/verify/mod.rs:383`, then `imt-core`) and a test that a cut inside `€` falls back to its start |
| 3 | `ec8b15b37` / `f6c87b551` (2026-10-24, the same patch on two parents `6e4139ed4` and `6bb2533e9`) | `paddock-diag` cuts files and descriptions with its own boundary loop; the test `a_cut_never_splits_a_character` (`crates/paddock-diag/tests/report.rs`) |

**The copies.** `git grep -l is_char_boundary` over `*.rs`, without `tests/`, `tests.rs`, fuzz and bench files: **41
production files, 44 calls**: Debrief 19 in `apps/debrief/crates` and 3 in its app, Dyno 3 (+1 in the vendored sysinfo
copy, which goes with the sysinfo 0.40 move), Pitcrew 3, Pitwall 2, Spotter 1, shared crates 10 (`paddock-assistant`,
`-channels`, `-diag` ×2, `-download`, `-log`, `-mcp`, `-plugin-api`, `-testplan`). Named private cut functions
(`fn cut|clip|truncate|floor`): 43 matches, of which about 38 cut text (the others are `paddock-dialogs` `Size::floor`,
Dyno's `nt.rs` buffer `truncate`, a test helper in `paddock-limits`, Pitwall's token cut and `paddock-pitwall`'s request
`truncate`; WP-0 classifies them). Exactly one helper is named for the job and it is not shared:
`truncate_chars` at `apps/debrief/crates/debrief-ai/src/assistant/tools.rs:40`.

**Measured (shared crates).** `cargo clippy --workspace --lib --bins --all-features` in `crates/` with a scratch
`CLIPPY_CONF_DIR`, `-A clippy::all -W clippy::string_slice -W clippy::disallowed_methods`, 1 min 56 s:

| Lint | Hits | Notes |
|---|---|---|
| `clippy::string_slice` | 104 sites in 40 files | most index with a `find` result on an ASCII delimiter or a hex string (`paddock-download/src/error.rs:178`), correct by construction; 28 in `paddock-diag/src/personal.rs` alone |
| `disallowed-methods` `str::is_char_boundary` | 12 | every hand-written cut; the path resolves (measured) |
| `alloc::string::String::truncate` | 2 | `paddock-channels/src/message.rs:66` (after a boundary loop), `paddock-health/src/notify/os/toast.rs:140` (after `trim_end`): both correct today |
| `str::split_at` | 2 | not read further |
| `alloc::string::String::drain` | 1 | not read further |

Apps were not measured (WP-0). A quick grep of app code shows the same shape, for example
`debrief-agent/src/run.rs:249` `&name[..15]`, guarded by `is_run_name`'s ASCII check: safe, but only by a reading of the
caller, which is what the lint cannot do.

**The std side.** `str::floor_char_boundary` and `ceil_char_boundary` are stable since 1.91.0 (`#[stable(feature =
"round_char_boundary", since = "1.91.0")]` in the toolchain's `core/src/str/mod.rs:421`; compiled and run here:
`"ab€cd".floor_char_boundary(3) == 2`, while `truncate(3)` and `&s[..3]` panic). App MSRVs are lower: Pitcrew 1.88,
Dyno, Pitwall and Spotter 1.89, Debrief 1.95 (`apps/*/Cargo.toml`), so the helper cannot call them yet.

**The fuzz side.** `paddock_testkit::corpus` (120 files use `corpus` or `never_panics`) makes bit flips, truncations, a
lone `0xc3`, `0x00` and `0xff` runs: it never builds valid multi-byte text that puts a character across a cut point, so
the shared never-panics tests could not have found incident 1.

## Pattern

**Code cuts text at a byte count with an operation that panics inside a character** (`String::truncate`, `&s[..n]`,
`split_off`, `drain`), or guards it with a boundary loop written again in each file. Every author writes the guard
fresh, three forms exist (byte loop, `chars().take`, `char_indices().nth`), and the tests that would find a miss feed
only ASCII or invalid bytes.

**Invariant.** Text is cut to a byte or character budget only through one shared function that cannot split a
character; the byte-index cut primitives are refused by clippy everywhere else; and the shared hostile corpus puts
multi-byte characters across every cut point.

## Decision

1. **`paddock_limits::text`** (D1): a new module in the leaf crate that already owns "bounded replies that say what
   they left out" (ADR 0019; serde and log only, already a dependency of Debrief, Dyno, Pitcrew and Pitwall; Spotter
   gains it). Pure functions, no new dependency:
   - `cut_bytes(s: &str, max: usize) -> &str`: the longest prefix of at most `max` bytes that ends on a character
     boundary (for byte budgets: wire caps, log lines, file sizes);
   - `cut_chars(s: &str, max: usize) -> &str`: at most `max` characters (for display budgets);
   - `cut_marked(s: &str, max: usize, unit: Unit, mark: &str) -> Cow<'_, str>` and its `Cut { text, was_cut, dropped }`
     form: the prefix plus a mark (`…`, `\n[cut]`), the mark counted inside the budget, so callers stop subtracting it.
   The boundary search is its own five-line loop until every workspace's MSRV is at least 1.91, then
   `floor_char_boundary`. A cut may still split a grapheme cluster (`e` + combining accent); that is valid UTF-8 and
   out of scope (no `unicode-segmentation` dependency for it).
2. **Clippy refuses the primitives elsewhere** (D2). `lint/clippy.toml` (C13 copies it into every app) and
   `crates/clippy.toml` gain `disallowed-methods` entries for `alloc::string::String::truncate`,
   `alloc::string::String::split_off` and `str::is_char_boundary` (the signature of a hand-written cut), each with the
   reason "cut text with `paddock_limits::text` (ADR 0094)". `paddock_limits::text` is the one
   `#[expect(clippy::disallowed_methods, reason = "...")]`. A site that truly needs a byte position (a parser that
   checked boundaries itself) carries its own `#[expect]` with a reason; `#[expect]` warns when unfulfilled, so a stale
   one shows itself without a list. `clippy::string_slice` is **not** adopted (D3): 104 sites in the shared crates
   alone, nearly all correct by construction; the corpus (§4) finds the wrong ones at run time instead.
3. **All copies move in one change** (D4, rule 55): the 41 files, the ~38 named cut functions and `truncate_chars`
   call the helper; the three incident tests stay as regression tests of their call sites. An app under a release
   freeze (ADR 0049 D7) at landing time keeps its sites with an `#[expect]` naming this ADR and a copies-register entry
   `text-cut` (canonical `paddock-limits`, `delete_by` its next tag), so C58 holds that tag until they are gone; no
   ADR 0088 list is needed because the `#[expect]` lines are self-checking.
4. **The hostile corpus carries multi-byte text** (D5). `paddock_testkit::corpus(seed)` adds, for a 2-, 3- and 4-byte
   character (`é`, `€`, `😀`), the seed with the character inserted at every offset of its first 64 bytes and the seed
   padded with that character to 256 and 1024 bytes plus 0 to 3 ASCII bytes (the shape of incident 1). Bounded: about
   400 more inputs per seed. Every existing `never_panics` test gets them for free; a panic it finds is fixed through
   §1, each with its failing test first (ER-11). Debrief's cargo-fuzz dictionaries get the same three characters.
5. **The helper's own test is exhaustive, not sampled**: for every string over the alphabet {`a`, `é`, `€`, `😀`} up to
   length 5 and every `max` in `0..=len+4`, the result is a prefix, valid UTF-8, within the budget and maximal; the
   mark variants also for a mark longer than `max`. No proptest dependency in `crates/` (only Debrief has one).

## Parallelism and cost (rule 51)

Nothing slow: the helper is O(cut length); the corpus adds about 400 inputs per seed to tests that today run a few
hundred; the clippy entries cost nothing measurable in an existing clippy run.

## Alternatives

- **`clippy::string_slice` with an ADR 0088 ratchet of today's sites.** Precise about where a panic can come from, but
  104 shared-crate sites (apps unmeasured, likely several hundred) to list or annotate, nearly all correct; the review
  cost buys little over §4, which finds the wrong ones by running them. Kept as D3 (b).
- **A new crate `paddock-text`.** One more crate, publish setup and CODEOWNERS line for three functions;
  `paddock-limits` already owns bounded text. Revisit if a text crate is needed for other reasons (rule of three).
- **Keep the copies, add tests.** That is today; incident 1 had a fuzz target and still shipped the cut.
- **`unicode-segmentation` grapheme cuts.** Correct for display but a new dependency for a problem nobody reported.

## Consequences

- One function to read when a cut looks wrong; the three cut forms disappear; new code cannot call `truncate` on a
  `String` without a reason in the source.
- `Vec::truncate` is unaffected (the entry names `String::truncate`; clippy resolves the type).
- Shared never-panics tests may find new panics the day §4 lands; each is a real bug.

## Rollback

Drop the clippy entries; the helper and the corpus variants are harmless to keep.

## Decisions for the user

| # | Decision | Options | Recommended |
|---|---|---|---|
| D1 | Where the helper lives | (a) `paddock_limits::text`; (b) a new `paddock-text` crate; (c) `paddock-testkit` plus copies | **(a)**: the leaf crate for bounded output, already used by four of five apps, no new dependency |
| D2 | Which primitives clippy refuses | (a) `String::truncate`, `String::split_off`, `str::is_char_boundary`; (b) (a) plus `str::split_at` and `String::drain`; (c) none | **(a)**: the measured cut forms; `split_at` and `drain` are mostly used with `find` results (WP-0 counts them; (b) if they turn out to be cuts) |
| D3 | `clippy::string_slice` | (a) not adopted; (b) adopted with an ADR 0088 ratchet of today's sites | **(a)**: 104 shared sites, nearly all correct; §4 finds the wrong ones |
| D4 | Migration | (a) every copy in one change, freeze-held apps by `#[expect]` plus a `text-cut` copies entry to their next tag; (b) per app by its next tag | **(a)** (rule 55; mechanical, 2.5 days) |
| D5 | Hostile corpus | (a) multi-byte variants in `paddock_testkit::corpus` and the fuzz dictionaries; (b) only the helper's own test | **(a)**: it is the check that would have found incident 1 on the first run |
| D6 | Tag gating | (a) none beyond C58 for an app that kept sites under D4 (a); (b) every app's next tag needs zero hand-written cuts | **(a)** |

## Work packages (raw agent-days, one agent)

| WP | Content | Days |
|---|---|---|
| WP-0 | Clippy measure over every app workspace and `tools/` with the D2 entries plus `split_at`/`drain` (CARGO_BUILD_JOBS=2, one at a time); classify the 43 named cut functions; the list for WP-3 | 0.5 |
| WP-1 | `paddock_limits::text` (`cut_bytes`, `cut_chars`, `cut_marked`, `Cut`), the exhaustive test, doc comments, CHANGELOG | 1 |
| WP-2 | `corpus` multi-byte variants with a positive control (a planted `truncate` in a test parser fails), fuzz dictionaries; run the affected `never_panics` suites and fix what they find, each failing first | 1 |
| WP-3 | Migration (implementer): shared crates 0.5, Debrief 1, Dyno, Pitcrew, Pitwall and Spotter 0.75, `truncate_chars` and the incident sites included | 2.25 |
| WP-4 | Clippy entries in `lint/clippy.toml`, `crates/clippy.toml`, every app's `clippy.toml` and `tools/*/clippy.toml`; C13 green; `text-cut` copies entry only if D4 (a) leaves sites | 0.5 |
| WP-5 | Rule text (proposed: "text is cut only through `paddock_limits::text`"), lesson, ADR index | 0.25 |

**5.5 days.** First cut (helper, corpus, shared crates and the lint): WP-0, WP-1, WP-2, the shared part of WP-3, WP-4
for `crates/` = 3.25 days. Gates no tag unless an app keeps sites under D4 (a), then C58 gates that app's next tag.
