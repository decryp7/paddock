# 65. Every bounded reply reports what it left out

- Status: Accepted 2026-11-06 (user), revision 5: D1 to D7 as recommended, full scope with D3's paged Show more. Revision 5 fixes the last diff review of `e447e4753`; revision 4 folded in the third review of `4e1fc4afe`, revision 3 the second of `fd622064a`, revision 2 the first of `1eaa14850`
- Date: 2026-11-05, revised 2026-11-06
- Related: family ADRs [0018](../../../../docs/adr/0018-bounded-loading-everywhere.md) (bounded loading, `Truncated`, `boundedBy`),
  [0019](../../../../docs/adr/0019-limits-explain-themselves.md) (limits explain themselves: `LimitId`, the registry, `limit_case!`, the completeness check), 0014 (monorepo, shared code); Debrief ADR 0016 (command table, generated
  types), 0042 (store port table), 0063 (binary wire, on `docs/debrief-binary-wire`, in review), 0067 (folder import dialog, Accepted 2026-11-06, the first new user of `Bounded`); plan packages 1.4, 2.3, 2.5, 3.6, 4.2, 4.5 (`docs/plans/debrief-1-2.md`); Paddock rules 1, 5, 21, 39, 40, 42, 43, 49, 51, 55; ER-21, ER-27
- Evidence marks: **[S]** read in source at `origin/debrief/1.2-int` 852b35ac0, paths under `apps/debrief/`; a short path such as `api.rs` or `skew/store.rs` means under `crates/debrief-core/src/`, and any other crate is named; **[C]** counted with grep or a heuristic scan at the same commit, so a
  rough number; **[J]** judgement. Sizes in days are judgement. Nothing in this ADR quotes a benchmark.

## Answers

| Question | Answer |
|---|---|
| Is this a new contract? | **No.** Family ADR 0018 already says "every answer says whether it was cut" and ADR 0019 says "no silent truncation". Both were accepted on 2026-10-27. Neither is built: no `Truncated` type, no `paddock-limits`, no registry and no `boundedBy` check exist in any crate or check, on `origin/main` or `origin/debrief/1.2-int` [C]. Plan package 1.4 builds 0019's half and is not started (release-scope plan, section 1.4). This ADR is how the cut half gets built on top of 1.4, and the deviations from 0018 and 0019 are listed in section 2. |
| What is the mechanism? | **`Bounded<T>`**: a value plus a list of typed `Cut`s, each saying which list was cut, from where, how many were shown, a total that is exact, a lower bound or unknown, how the shown part was chosen, the limit's `LimitId`, and what the person can do next. One generated TypeScript type, one UI note, one check. |
| Where does it live? | **In `paddock-limits`, beside 0019's `LimitExceeded`** (option D, recommended): one crate for "a limit was hit", whether the work stopped or the reply was cut, with one `LimitId`, one registry type and one `limit_case!`. |
| What about `Coverage`? | **It becomes a `Cut`.** `Analyzed<T>` is the first use of the mechanism and does half the job. Its `limits: Vec<String>` is why it is not enough: English text, no numbers, nothing checks it. |
| Is the `boundedBy` lint built? | **No.** It exists only as words in ADR 0018 and plan package 2.5. Its rule alone would miss most instances here (see "Why not just the lint"). |
| Old builds and old files? | **No migration code** (user, 2026-11-06). The one stored change (instance 12) bumps the store's `FORMAT_VERSION` by one, on its own: every schema change takes the next number, so if ADR 0066's change lands first this one takes the number after it; the existing check refuses a newer store in an older build ("saved by a newer Debrief … update Debrief") and an older store in a newer build ("saved by an older preview of Debrief … open its log files again"), `crates/debrief-store-duck/src/lib.rs:913-923` [S]. The store worker's `WIRE_VERSION` is internal to one build. |
| Does it fit ADR 0063? | **Yes, in either order.** Cuts travel in the MessagePack head, rows in a table. The checker must accept `#[wire_command]` as well as `#[tauri::command]`. |

## Context

The same bug keeps coming back in different places: a limit that does not report itself. Fourteen instances are listed under "Migration". What they have in common:

- **The label and the data disagree.** "Selected: 340 events" sends 20 (`app/src/app/ai/chat/chat-model.ts:211`, `:222`) [S].
- **A total exists and nobody shows it.** `HeapDiffPage.total_rows`, `HeapGrowthPage.total_rows` (`crates/debrief-profile/src/api.rs:992`, `:1034`), and `total` of dump threads and modules are in the reply. The views use them for empty states at most, and have no pager [S].
- **No total exists.** `Butterfly` truncates both lists to the request's limit (`crates/debrief-profile/src/ops/butterfly.rs:89-90`) and has no field to say so. `umdhTree` returns a plain array [S].
- **The reply claims to be complete.** `requests` runs through `analyse`, whose `Coverage` compares events analysed with events matched. `rank_requests` then cuts the list at 500 (`analysis/trace.rs:263`), so the person reads "everything was analysed", which is true, above a list that is not everything. `trace_summaries` does the same at 200 (`analysis/moment.rs:296`) [S].
- **The reply carries a flag for a different limit.** `StoreFields.truncated` is about `MAX_FIELD_ROWS` = 2,000 (`ports/inspect.rs:21`), while keys beyond 512 per segment were already dropped when the segment was written (`crates/debrief-store-duck/src/stats.rs:178`). `Sampled.truncated` is about groups (`survey/sample.rs:73`), not about the ids cut at `:115` [S].
- **Two cuts stack up.** `selectedRecords()` caps at 200 *and* skips rows of pages not in memory (`app/src/app/grid/events-grid.ts:52`, `:643`). `suggest_over` stops at `MAX_SCAN` and also stops learning ids at `MAX_IDS` (`skew/store.rs:30`, `:44`) [S].

The shape of the fixes so far explains why it recurs: about 25 reply structs have grown their own `truncated: bool`, `capped: bool`, `dropped: usize`, `truncated_lanes` or `sampled` field (for example `config/dto.rs:198`, `ports/console.rs:99`, `spans/pairing.rs:95`, `api.rs:647`, `app/src-tauri/src/storage/model.rs:107`) [C]. Each UI then decides for itself whether to say anything, and in what words. There are also two coverage types: `ports::Coverage` and `api::AnalysisCoverage` (`api.rs:312`) [S].

The size of the surface, at this commit [C]:

| What | Count | How |
|---|---|---|
| Commands parsed from `#[tauri::command]` | 389 | scan of `app/src-tauri/src`; the table in `lib.rs` lists 351 by a simpler pattern |
| Replies holding a collection, directly or through named structs | 194 | field walk by type name; heuristic, it follows no generics or re-exports |
| ... of those with no field that looks like a marker or a total | 111 | so the reply check would start with up to about this many entries |
| Limit-shaped constants in `debrief-core`, `-store-duck`, `-ai`, `-profile`, `-dump`, `app/src-tauri` | 557 | `MAX`, `LIMIT`, `CAP`, `TOP`, `PAGE` in the name; many validate input and never cut a reply |
| `.truncate(` outside tests in those crates | 110 | includes text and buffers |
| `limit: <number or constant>` in TypeScript, outside specs and mocks | 39 | |
| `.slice(0, <number or constant>)` in TypeScript, outside specs and mocks | 93 | includes display trims |

**The pattern (rule 40):** a limit is enforced where the work happens and the fact that it was hit does not travel. Fixing instances one at a time has not stopped new ones.

## Decision

### 1. The type

In `paddock-limits` (section 6, option D), beside 0019's `LimitExceeded`, using the same `LimitId`:

```rust
/// A value and what was left out of it. Fields are private: a `Cut` is made only by the constructors, and
/// deserializing goes through `TryFrom<CutWire>`, which runs the same checks.
pub struct Bounded<T> { result: T, cuts: Vec<Cut> }

#[serde(try_from = "CutWire", into = "CutWire")]
#[ts(export)]
pub struct Cut {
    limit: LimitId,          // paddock-limits' key: "traces.list", "analysis.events" (plan 1.4)
    of: FieldPath,           // which list in `result` was cut: "traces", "callers", "groups[3].ids", a path inside `result`, never starting with `result`; "" for the value itself
    shown: u64,              // items in the reply, or items looked at
    total: Total,
    selection: Selection,    // which ones were kept, and from where
    next: Next,              // what the person can do about it; derived from the registry row, section 3
}

pub enum Total {
    Exact { #[ts(type = "number")] n: u64 },
    AtLeast { #[ts(type = "number")] n: u64 },
    /// Something may have been left out, and nobody knows how much. Only `Cut::maybe` makes it.
    Unknown,
}
pub enum Selection {
    /// Items `from .. from + shown` in the source's own order (a page, or the first ones from 0).
    First { from: u64 },
    /// The last `shown`, such as the newest lines of a tail.
    Last,
    /// Ranked by `key`; ranks `from + 1 ..= from + shown`.
    TopBy { key: SortKey, from: u64 },
    Sample,
}
pub struct SortKey { field: FieldPath, order: Order }
pub enum Order { Desc, Asc }
/// A dotted path of field names in the reply's camelCase (`durationMs`, `callees.selfMs`), where a part may carry one
/// index into a list (`groups[3].ids`): parsed once, at most 64 bytes, `[a-z][a-zA-Z0-9]*` or that with `[<u32>]` per
/// part. Never shown to a person: the words come from the message (section 4).
pub struct FieldPath(Box<str>);
pub enum Next { Page, ShowMore, RaiseSetting, Narrow, None }
```

Serde is camelCase with `tag = "kind"`, like `OffsetSpec`. Counts are `u64` in Rust and `number` in TypeScript (`#[ts(type = "number")]`; ts-rs would otherwise write `bigint`); no count comes near 2^53. In TypeScript:

```ts
interface Bounded<T> { result: T; cuts: Cut[] }
type Total = { kind: 'exact'; n: number } | { kind: 'atLeast'; n: number } | { kind: 'unknown' };
```

Rules the constructors enforce, so the illegal states cannot be built:

- A cut is made only if something was left out: `shown < n` for both `Exact` and `AtLeast`. `shown == total` yields no `Cut`, and an empty `cuts` means complete. `Total::Unknown` means "something may have been left out" and comes only from `Cut::maybe(row, of, shown, selection)`, for a list whose producer knows it may have dropped items but not how many (instance 12).
- `from + shown <= n` for `Exact`, and `from + shown < n` for `AtLeast` with `First { from }`. `AtLeast { n }` is rendered as "more than {n − 1}": `first` answers `AtLeast { n: limit + 1 }`, which reads "more than 5,000,000" for a limit of 5,000,000. `of` must be a path the bounded-check (section 5, part 2) finds as a collection field of the reply type, or empty when the value itself is the list.
- **Rows are typed by what they may report.** The registry generates one handle per row and declared selection, `CutRow<First>`, `CutRow<TopBy>`, `CutRow<Last>`, `CutRow<Sample>`, `CutRow<Maybe>`, and only for rows of `kind = cut` or `both`. `Bounded::first` takes only a `&CutRow<First>`, `ranked` only a `&CutRow<TopBy>`, and so on, so a call that passes a stop row or a row that does not declare that selection does not compile; `syn` never has to tell which row a call passes.
- `Bounded::first(iter, row, of)` takes `limit + 1` items and answers `AtLeast { n: limit + 1 }` when the extra one exists. **It never counts further just to print a number** (rule 51). `Bounded::page(items, offset, total, row, of)` is a page with `First { from: offset }` and `Next::Page`. `Bounded::ranked(vec, row, of, key)` has seen them all and answers `Exact` with `TopBy { from: 0 }`. `Bounded::counted(items, total, row, of)` is for a total the store already knows. `Bounded::last(..)`, `Bounded::sampled(..)` likewise. `Bounded::complete(value)` has no cut. Several cuts on one value: `.and(cut)`, where the cut comes from the public `Cut::counted(row, of, shown, total, selection) -> Option<Cut>` (or `Cut::first`, `Cut::page`, `Cut::maybe`), which answers `None` when nothing was left out.
- **Many lists, few cuts.** One cut per list, and a list inside a list is named by index (`groups[3].ids`). The index is the position in the list as sent in `result`, counting from that list's start in this reply (a page's first row is `[0]`), so a view must match cuts to rows before it re-sorts them. At most **32 cuts** on one value, the aggregate included. When there would be more than 32, the constructors keep the first 31 by the position of their list in `result` (outer list first, then index), not by string order, and add one aggregate cut last, on its own registry row `bounded.cuts`: `of: ""`, `shown: 31`, `Total::Exact` of the cuts there were, `Selection::First { from: 0 }`, `Next::None`, and its own message ("31 lists are marked as cut; 12 more were cut too"), which B2 writes. `.and(cut)` on a value that already has an aggregate re-aggregates, and the aggregate stays last. `Bounded` itself deserializes through `TryFrom<BoundedWire>`, which refuses more than 32 cuts, an aggregate not last, or two cuts with the same `of` and `limit`.
- **Limits set by the request still produce a cut.** `heapDiff` with `limit: 500` and `total_rows: 1,204` answers a `Cut` with `First { from: offset }` and `Next::Page`; a page is a cut with a known total and a way to the rest. "The caller asked for 500" is not a reason to stay silent, as the person never typed the 500 (instances 6 to 8). The row is `kind = cut`, `value = by-request` in the registry.
- Fixed limits come from the registry (section 3), not from a loose number at the call site: the constructors take a registry row, not a number.

**Each constructor has a test** that asserts every field it sets: `limit`, `of`, `from`, `shown`, `total`, `selection`, `next`. A proptest over any lengths checks that no constructor yields a cut with nothing left out, that `TryFrom<CutWire>` refuses every wire value a constructor could not have made, and that round trips are identical.

**What each guard covers.** `TryFrom<CutWire>` and `TryFrom<BoundedWire>` check what a value can show on its own: the arithmetic (`shown < n`, `from + shown` against `n`, an `Unknown` with no `n`), the `FieldPath` syntax and the 32-cut bound. They cannot know the registry or the reply. The typed rows make the row's `kind` and declared `Selection`s a compile-time matter, and the constructors derive `next` from the row (above). The bounded-check (section 5, part 2) checks, at build time, that `of` names a collection field of the reply type. That `shown` equals the length of the list it names, for `Cut::counted` where `shown` is an argument, is checked only by each row's `limit_case!` (part 4) at the edge, not for every value. In TypeScript, `boundedSlice` makes a cut in the UI itself and is outside the constructors-only guarantee: it is one function with its own spec, and ESLint (part 5) keeps every other `.slice(0, n)` out.

**One `Bounded`, not one per list.** A reply with two lists (`Butterfly`, `traces`) has one `Bounded<Reply>` with two cuts, told apart by `of`. So the same type covers *a list was cut* and *the analysis saw only part of its input*: both are "shown M of N somethings", and the words come from the limit. `Coverage { matched, analysed }` becomes a cut with limit `analysis.events`, `of: ""`, `shown: analysed`, `total: Exact(matched)`, `selection: First { from: 0 }`, which matches what the engine does: "above 2 million matching events it analyses the oldest 2 million" (CHANGELOG line of `adaa192c0` on `fix/debrief-ai-coverage-facet-search`) [S].

**Wrapped, not flattened.** A bounded reply is `Bounded<T>` on the wire, `{ result, cuts }`, for every command, including those whose `T` is a page struct. Flattening `cuts` into each `T` would need a field in every reply type and lose the one generated shape the UI checks against. So each migrated command's UI reads `.result`; that is the cost B6 carries (below), and `Analyzed` already has this shape.

**Paging is a cut too, with `next: Page`.** A page has a known total and a way to reach the rest, so the note reads "500 of 1,204" with a "Show more" button, and the views listed below stop hiding it. `queryPage` (the events grid) answers its cut like any page, so its reply is wrapped too and its callers read `.result` (B6); the grid shows the cut through its scrollbar instead of the note (section 5, part 5).

### 2. What changes from ADR 0018 and ADR 0019

| ADR | Says | Here | Why |
|---|---|---|---|
| 0018 | `truncated: Option<Truncated { shown, total_or_lower_bound, limit }>` | `cuts: Vec<Cut>`, `Total` as an enum, `of` and `from` | One number cannot say whether it is exact, and "at least 5,000,001" must read differently from "5,000,001". A reply can be cut in more than one way (three examples above), so each cut must say which list it is about and from where. |
| 0018 | Lint: a command returning a collection with no `Limit` or `Page` argument | A check on replies, on limit-shaped constants (0019's scan, extended) and on TypeScript (section 5) | See "Why not just the lint". |
| 0018 | Types in `paddock-view` or a new `paddock-query` | `paddock-limits` (option D) | `paddock-view` pulls in `paddock-ipc` and postcard (`crates/paddock-view/Cargo.toml`) [S], which `debrief-core` does not depend on; and a cut is a limit, which 0019 already gives a crate. |
| 0019, decision 6 | "A cut result carries `Truncated` (ADR 0018); a dropped item increments a counter that the UI shows" | A cut result carries a `Cut`; a dropped item is a `Cut` too ("12 annotations not shown: the limit is 10,000" is `shown 10,000`, `Exact 10,012`, `First`) | One shape for both, so one note and one check. |
| 0019, decision 7 | Registry row: `LimitId`, unit, default, hard min/max, Setting path or `hard`, template, remedy | The same row gains `kind: Stop \| Cut \| Both` and, for `Cut`, the `Selection`s it may report and `value = by-request` as a third choice beside a Setting and `hard` | The completeness check must know which rows can produce a `Cut`, and a by-request limit has neither a Setting nor a constant. |
| 0019, decision 3 | "The message is generated from the type": one template per limit | One ICU message per `LimitId` × `Selection` in the UI's i18n; `Cut::describe()` in Rust stays English, for logs and the AI | A cut's sentence depends on how the shown part was chosen ("the 200 longest of", "the first 512 of"), and a template per limit cannot say both. |
| 0019, check 1 | `limit_case!` asserts `LimitExceeded` with id, value, Setting path and text | `limit_case!` also has a cut form: data at the limit gives no cut; one over gives a `Cut` with the exact `limit`, `of`, `shown`, `total`, `selection` | The same harness, one line per limit, as 0019 intends. |
| 0019, check 2 | Constants used "in a comparison that returns an error, truncates or kills" and not in the registry warn | The same scan (`conformance/src/limits.mjs`, plan 1.4) also counts `.take(`, `.truncate(`, `.slice(0,` and a SQL `LIMIT` built from a constant as cutting | Most silent cuts here are `.truncate` and `.take`. |

**In the same change as acceptance** (package B0), so no plan or family ADR keeps building the old shape:

B0 edits `docs/plans/debrief-1-2.md` on `debrief/1.2-int`, where the plan is kept (it reaches `main` with that branch), and the two family ADRs on the same branch. Line numbers are on `debrief/1.2-int`:

- Section 1's row for ADR 0018 (`:58`: `Truncated` in `paddock-view` and the command-layer `boundedBy` lint): `Truncated` becomes `paddock_limits::Cut`, the lint becomes this ADR's check.
- Package 0.5 (`:102`): no longer decides where `Truncated` lives.
- Decision row D5 (`:412`; `:379` on `main`): answered by this ADR, `paddock-limits`.
- Plan package 2.5 (`:131`): `Truncated` in `paddock-view` and the `boundedBy` conformance check are struck; it points here for `Cut` and the check, and keeps what "What remains of plan package 2.5" lists.
- Plan package 4.2 (`:168`): "`Limit`/`Page` and `Truncated`" becomes "`Limit`/`Page`, replies in `Bounded` (ADR 0065)", and its per-method test becomes part 4 of the check.
- Family ADR 0018: one line, "`Truncated` is `paddock_limits::Cut` (Debrief ADR 0065); the `boundedBy` check is Debrief's `xtask bounded-check` until a second app adopts it, then `./dev`".
- Family ADR 0019: one line, "a cut result carries a `Cut` in `paddock-limits` (Debrief ADR 0065); the registry rows gain `kind` and their `Selection`s".

These change family ADRs, so they are the user's decision (D5). 0018's typed arguments (`Limit`, `Page`, `Buckets`) stay as they are, as do its Settings and virtualization decisions. This ADR adds no Settings.

### 3. The registry

B3 needs Debrief to build against the in-tree shared crates, which is plan package 0.4 ("Debrief consumes in-tree shared code": path dependencies to `crates/`), as well as 1.4.


The registry **type**, `LimitId` and `limit_case!` come from plan package 1.4 (`crates/paddock-limits/`, `crates/paddock-testkit/src/limits.rs`). Debrief's **rows** live in `crates/debrief-core/src/limits.rs`, the file plan package 3.6 creates for ADR 0019's rows 1-50: one registry per app, as 0019 decision 7 says. This ADR adds the rows that can cut a reply (`kind = cut` or `both`) with their message keys. If B3 runs before 3.6, it creates the file with only the cut rows and 3.6 adds the stop rows; either order, one file.

**`next` is derived from the row,** not chosen at the call site: `Page` for `Bounded::page`; `RaiseSetting` when the row names a Setting; otherwise the row's `remedy` (0019's field): `ShowMore`, `Narrow` or `None`. **`RaiseSetting` cannot occur before the Settings exist:** every cut row starts as `hard` or `by-request`, and a row gets a Setting path only in the change that adds the field (plan 3.6, and 4.2's Limits group in Settings ▸ Performance). The registry test resolves every Setting path against the settings schema, so a row cannot point at a field that does not exist. So B5 does not wait for 4.2: the note offers "Raise it…" for a limit on the day its Setting lands.

A generated `core/generated/limits.ts` gives the UI `LimitId` as a union and the values as constants, so `slice(0, 20)` becomes `boundedSlice(ids, LIMITS.aiContextEvents)`. The number lives in one place. Today it lives in two: `slice(0, 20)` in the UI and `MAX_SELECTED = 20` in the engine, which caps again and tells the model (`crates/debrief-ai/src/chat/context.rs:9`, `:78`) while the person's label still says N [S].

### 4. One note in the UI

`<pd-limit-note [cuts]>` replaces `debrief-coverage-note`. **Words:** one ICU message per `LimitId` × `Selection`, with `{shown}`, `{total}` and `{from}` as plural-aware numbers. The generator writes the list of required keys from the registry (each cut row and the `Selection`s it declares), so a new row or a new selection fails the i18n check until it has its words in every catalogue. `Cut::describe()` writes the English sentence in Rust for the AI and for logs (no user content, ER-27); the UI never shows it.

| Total | Example |
|---|---|
| `Exact` with `TopBy` | Showing the 200 longest of 1,340 traces. |
| `Exact` with `First { from: 500 }` | Showing 501 to 1,000 of 1,204 rows. |
| `AtLeast` with `First` | Looked at the first 5,000,000 of more than 5,000,000 events. |
| `Unknown` with `First` (instance 12) | Some fields may not be listed: segments with more than 512 field names kept only the first 512. |
| `Exact` with `Sample` | Sampled 10,000 of 14,255 sources. |
| Two cuts | One line each, placed by `of` next to the list it is about. |

`next` gives the action: **Show more** (`Page`, `ShowMore`), **Raise it…** through ADR 0019's `LimitNotice` (plan 2.3) when the row has a Setting, "Narrow the query" as words only. Status is words, never colour alone. The note keeps a fixed slot so it does not move controls when data arrives (ADR 0061).

`AsyncResource`'s `coverage` computed (`app/src/app/analysis/async-resource.ts:57`) becomes `cuts`, so views already on it change one binding. `isPartial` and `fullCoverage` go away. `Coverage.limits` strings go away.

**AI tools.** All four `.map(|a| a.result)` in `crates/debrief-ai/src/assistant/tools.rs` (`:598`, `:613`, `:621`, `:636`) drop coverage at 852b35ac0 [S]. `fix/debrief-ai-coverage-facet-search` (`adaa192c0`, pushed, not merged at this commit) fixes that with a shape of its own [S]:

- `coverage_json` puts `"coverage": {"matched", "analysed", "partial": !complete(), "limits"}` into the output of `patterns`, `exception_groups`, `overview` and `moment`;
- `with_coverage` appends "(from A of M matching events)" to the tool's one-line summary;
- `ANALYST_SYSTEM` (`assistant/narrative.rs`) tells the model to state the numbers "when a tool result has a coverage object with partial true";
- tests `analysis_tools_report_a_partial_coverage` and `the_analyst_is_told_to_state_a_partial_coverage` (`debrief-ai/tests/it/tools.rs`), driven by a `coverage: Option<Coverage>` in the core's test delegate.

That branch lands first. **B4 replaces exactly that shape:** one helper `cuts_json(&[Cut])` writes `"cuts": [{limit, of, shown, total, selection, text: describe()}]` into **every** tool reply, not only the four; the summary suffix comes from `describe()`; the prompt's sentence says "when a tool result has `cuts`"; and the two tests assert `cuts` instead of `coverage`. Nothing else of that branch changes.

### 5. The check

A new limit cannot stay silent because five things must each be passed. Each is a failing build or test, not a review item:

1. **Type.** `Cut` and `Bounded` cannot be built by hand or deserialized past their checks (`TryFrom`). Most constructors count `shown` themselves (`first`, `page`, `ranked`); `Cut::counted` and `Bounded::counted` take it as an argument, so for those the numbers are checked by the row's `limit_case!` (part 4), not by the type.
2. **Replies** (`cargo xtask bounded-check`, with `syn`, in Debrief's CI). For every row of `command_table!` and `store_port_table!`, it walks the reply type through Debrief's struct definitions, through `Option`, `Vec`, maps, tuples, `Analyzed<T>` and other generics and through `pub use` re-exports. If the type holds a collection without `Bounded` around it, the row must be listed in `apps/debrief/bounded-exempt.toml` with a kind (`fixed-set`: bounded by the domain, such as levels; `per-source`: one item per source, bounded by the source limit) and a reason. There is no `by-request` exemption: such a reply returns its cut (section 1). It checks that each `of` a constructor names is a collection field of the reply. It accepts `#[tauri::command]` and ADR 0063's `#[wire_command]`. Starts with up to about 111 entries [C], each with a reason or a migration.
3. **Limits.** Plan 1.4's completeness check (`conformance/src/limits.mjs`, ADR 0019 check 2), extended as section 2 says: a limit-shaped constant used in a comparison, `.truncate`, `.take` or a SQL `LIMIT` in a crate that reaches a reply must be in the registry, or in its waiver list as input validation or a buffer. It also flags `.truncate(x)` or `.take(x)` of a non-literal `x` (such as `req.limit`) in a function that returns `Bounded::complete`: the butterfly bug (`crates/debrief-profile/src/ops/butterfly.rs:89-90`) in its new form. One scan for stops and cuts, not a second one in the `xtask` (rule 55: one implementation per mechanism). The waiver list is seeded and can only shrink by hand; a new unlisted one fails.
4. **Each limit, at its edge** (`limit_case!`, ADR 0019 check 1, from plan 1.4). For every cut row, a test with data at the limit (no cut) and one over it (a cut with the exact numbers). The registry and the test list must match both ways, which 1.4's harness already records. This catches off-by-one and "the flag is about another limit". ADR 0018's check 1, a large-fixture test per port method, is kept here: each of 4.2's port methods gets one through 2.5's large-fixture generator, asserting the reply is bounded and carries its cut.
5. **UI.** The generator also writes `BoundedCommand`, the union of commands whose reply holds a `Bounded`. A hand-written `BOUNDED_SURFACES: Record<BoundedCommand, Surface>` names the component and spec that shows each, `scrollbar` for the events grid, or `engine-only` with a reason, so a new command fails type-check until it has an answer. Each surface's spec gets a cut from the mock and asserts the note's text (no fixed waits, rule 49). ESLint flags `.slice(0, <n>)` and `limit: <literal>` outside `boundedSlice` and `LIMITS` unless marked `// bounded: display-only — <why>`.

Parts 2 and 3 warn for the first package that adds them and become errors when the Debrief list is reasoned, as ADR 0019 phases its own check. **What still gets through:** a hand-written loop with a literal (`if n > 1000 { break }`) in a reply path; a cut made by hand-written iteration rather than `.truncate` or `.take` before `Bounded::complete`; a `.truncate` or `.take` inside a helper function whose caller returns `Bounded::complete`, as part 3 looks only at the function that returns; a wrong `shown` passed to `Cut::counted` on a path its `limit_case!` does not reach; and a UI that gets a cut and renders the note where nobody reads it. Reviewers keep these five.

**The red tests come first** (rule 5): AI label with 340 ids; `traces` with 201; `suggest_over` over a lowered scan cap; 513 field keys in one segment; `heapDiff` with `limit` 500 over 1,204 rows. Reviewers run the mutation check on each (rule 5).

**Why not just the lint.** ADR 0018's rule asks whether the *arguments* have a limit. Of the fourteen instances, it would flag only those whose argument list has none. `heapDiff` has `limit`, so it passes, and the bug is still there. So does `butterfly`. A hard-coded cap inside a function returning `Analyzed<Vec<_>>` looks fine to it, and `slice(0, 20)` in TypeScript is out of its sight. So the check looks at what is left out, not at how the question was asked [J].

### 6. Where the types live (ADR 0014, rules 39, 42, 55)

Rule 42 says that what is generic or needed elsewhere is built in Paddock, and rule 39 that a store feature or component used by two apps lives in `paddock-ui`. The types are generic. Shared crates already have the same habit (`paddock-storage` `capped`, `paddock-proc`, `paddock-watch`) [C], ADR 0018 section 9 already assigns them to shared code, and ADR 0019 already plans a shared crate for limits. Dyno's disk map is the likely second user [J].

| Option | Types | UI note | Cost | Verdict |
|---|---|---|---|---|
| **D. In `paddock-limits`** (plan 1.4), beside `LimitExceeded`, with the same `LimitId`, registry type and `limit_case!`; `ui/.../limit-note` beside 2.3's `LimitNotice` | shared | shared | About 1 day on top of 1.4; the crate stays a leaf (serde, ts-rs behind a feature), which 1.4 needs anyway because `paddock-ipc` and the others depend on it | **Recommended.** One crate, one key type, one registry, one harness and one completeness check for every limit, stopped or cut |
| A. A new `crates/paddock-bounded` | shared | shared | The new-crate checklist and 99 % coverage, and a second key type or a dependency on `paddock-limits` for it | No: a second home for what 0019 already homes |
| B. In `debrief-core`, moved later | Debrief | Debrief | Once 1.4 exists, a Debrief registry type beside `paddock-limits`' is a second copy of one mechanism: a `docs/known-copies.toml` entry with its canonical, the app that deletes the copy and the tag it is gone by (rule 55), then a move across about 25 structs | Fallback only if 1.4 slips past B4 |
| C. In `paddock-view` | shared | shared | `debrief-core` gains `paddock-ipc` and postcard | No |

**Nothing shared lands before the Dyno tag** (rule 1). The release-scope plan lists "`paddock-limits` core" under "now, on a branch; lands after T", and 2.5 under "wait for T" (`docs/plans/debrief-1-2-release-scope.md:221-222`, on `docs/debrief-1-2-release-scope`), with T about 2026-11-07. So B1 is written on 1.4's branch and lands with or after it; B3 and later wait for 1.4 on `main` or on `debrief/1.2-int`, provided that branch reaches `main` only afterwards. Each app's registry rows and messages are its own. `xtask bounded-check` starts in Debrief and moves to `./dev` when a second app adopts it (rule of three).

**ADR 0067 needs two matching edits** if this is accepted (0067 is not edited here). First, its line 194, the `folder.scan_entries` bullet, should read: a row (`kind = cut`, Setting) in Debrief's limit registry `debrief_core::limits` on plan 1.4's `paddock-limits` registry type; the listing is `paddock_limits::Bounded::first(.., row, of: "entries")`; above the limit the `Cut` says `of`, `shown`, `Total::AtLeast` and `First { from: 0 }`, and `next` comes from the row (`RaiseSetting` once the Setting's field exists, so the note offers "Raise it…" beside the dialog's own "open a subfolder", and `Narrow` until then); the words are the ICU message for `folder.scan_entries` × `First`. Second, `folder_scan_rows` (0067 r5 line 265, and its fit section, line 521) uses `Bounded::page(rows, offset, count, row, of: "rows")`, which gives `First { from: offset }` and `Next::Page`, not `Bounded::counted`; the row is a `by-request` cut row, and its `BOUNDED_SURFACES` entry is `scrollbar`, as 0067 already says. The reference to `paddock-bounded` in its line 521 becomes `paddock-limits`.

### 7. Fit with ADR 0063

Nothing here depends on 0063 landing, and neither blocks the other [J]:

- **Today's wires.** The store worker carries port DTOs as JSON text (`crates/debrief-store-client/src/protocol.rs:1-2`) [S], so `tag = "kind"` and `try_from` are fine; postcard's limits on internal tags do not apply to these. `WIRE_VERSION` (`crates/debrief-store-client/src/port_table.rs:24`, 3 today) bumps once, as the DTOs change shape. Worker and app ship in one build, so no older peer exists.
- **After 0063.** `Bounded<Table<T>>`: `cuts` in the MessagePack head, rows in a table. Internally tagged enums hold under its `with_struct_map()` (0063 section 3 and its count of 46 `tag =` uses). No value comes near 2^53. `shown` is the count of rows, so the UI reads `cut.shown`, never `RowTable.length`, for the words.
- **Order.** If this lands first, 0063's W-packages see `Analyzed` already gone. If 0063 lands first, `RowTable` is the `result`. The touch point is the list of about ten `queryPage` callers: `queryPage`'s reply is wrapped, so they read `.result` (B6), and its `cuts` travel in the head.

## Migration

### The named instances

| # | Instance | Today | Reply knows the total? | Change | Total |
|---|---|---|---|---|---|
| 1 | AI selection, `ai/chat/chat-model.ts:211`, `:222` | Label says N, sends 20 | UI has it | `boundedSlice`, label "Selected: 340 events; the first 20 go to the assistant" | Exact |
| 2 | Grid actions, `grid/events-grid.ts:52`, `:643` | Cap 200, unloaded rows skipped | UI has it | Two cuts, shown in the menu's confirmation | Exact |
| 3 | Clock skew, `skew/store.rs:30`, `:44` | Stops at 5M, stops adding ids at 500k | No, but `summary` has event counts | `Bounded<SuggestResult>`, two cuts; the clock dialog shows them | Exact for events, `AtLeast` for ids |
| 4 | Dump threads and modules, `dumps/dump-view.ts:17` | 500 of a possible 4,096 | Yes, unused | Note with Show more | Exact |
| 5 | UMDH growth and tree, `dumps/umdh-view.ts:11`, `:13` | 200; 100 children | Growth yes; tree no | Tree becomes `Bounded` | Exact, `AtLeast` |
| 6 | Heap diff, `profiling/heap/heap-diff.ts:100` | Literal 500 | Yes, unused | `Bounded::page`, `LIMITS` | Exact |
| 7 | Heap growth, `heap-growth.ts:83` | Literal 500 | Yes, unused | Same | Exact |
| 8 | Heap instances, `heap-instances.ts:107` | Literal 200; count shown, no "of" | Yes | Same | Exact |
| 9 | Butterfly, `profiling/butterfly/butterfly-view.ts:103` | 50 callers and 50 callees | No | Two cuts from `ops/butterfly.rs:89-90`, `of: "callers"` and `"callees"` | Exact, as it ranks |
| 10 | Traces, `analysis/moment.rs:296` | Top 200, reply says complete | No | `trace_summaries` returns a cut, both engines | Exact |
| 11 | Requests, `analysis/trace.rs:263` | Top 500, reply says complete | No | Same | Exact |
| 12 | Fields panel, `crates/debrief-store-duck/src/stats.rs:178` | Keys beyond 512 per segment dropped at write | No, and lost by then | New `segments.fields_dropped` count, with its own `FORMAT_VERSION` bump (no migration: section "Answers"); `StoreFields` gets both cuts | `Unknown` (`Cut::maybe`) when any segment dropped, since another segment may still list the key |
| 13 | AI tools, `crates/debrief-ai/src/assistant/tools.rs:598`, `:613`, `:621`, `:636` | Drop coverage | Dropped | Fix branch first, then `cuts_json` in every tool (section 4) | As the source |
| 14 | Survey, `survey/sample.rs:115` | First 10,000 source ids of a group | The group knows | Second cut beside `truncated`; `First { from: 0 }`. Could spread over the group instead: not measured. Largest group in the sample bundle is 1,211 files, so this is not hit there | Exact |

### Fields that move to the type (about 25) [C]

`known_issues.rs:103`; `api.rs:647`; `spans/pairing.rs:95`, `spans/dto.rs:57`, `spans/infer.rs:101`; `config/snapshot.rs:52`, `config/diff.rs:101`, `config/dto.rs:198`, `:236`; `redact/view.rs:69`; `survey/dto.rs:29`, `:51`; `ports/console.rs:99`, `:190`, `:219`; `ports/inspect.rs:220`; `catalogue/mod.rs:206`; `crates/debrief-ai/src/assistant/proposals.rs:129`, `:149`, `:168`; `app/src-tauri/src/agent_commands.rs:42`, `storage/model.rs:107`, `:120`, `stores.rs:55`, `walk.rs:58`, `browser/mod.rs:53`. Other `dropped` counters count something else (events a pipeline removed, `api.rs:28`, `:173`, `:439`; a full event queue, `events.rs:72`) and stay; the check's list decides each.

### The rest

Whatever parts 2 and 3 of the check list. Each is migrated or gets a reason. Plan package 4.2's eight unbounded port methods gain `Bounded` here too, so 4.2 reduces to adding `Limit` and `Page` arguments and its Limits group in Settings.

### What remains of plan package 2.5

2.5 (`Limit`, `Page`, `Buckets`, `Truncated`, `bounded_query`; the large-fixture generator; the `boundedBy` conformance check; two ESLint rules; the inventory script) keeps `Limit`, `Page`, `Buckets` and `bounded_query`, the large-fixture generator in `paddock-testkit`, `paddock/template-no-index-track` and `paddock/collections-virtualized`, and `scripts/collections-inventory.mjs`. It loses `Truncated` (now `Cut`, in `paddock-limits`) and the `boundedBy` check (now parts 2 and 3). Its test "the conformance check lists Debrief's 10 unbounded methods" becomes part 2's seeded list.

## Packages

Rough days; one agent; red tests first in each.

| # | Package | Owns | Days | Waits for |
|---|---|---|---|---|
| B0 | At acceptance, on `debrief/1.2-int`: the plan's section 1 row for ADR 0018 (`:58`), package 0.5 (`:102`), package 2.5 (`:131`), package 4.2 (`:168`) and decision row D5 (`:412`); one line each in family ADRs 0018 and 0019 (D5); ADR 0067's lines 194, 265 and 521 (D6) | `docs/plans/debrief-1-2.md`, `docs/adr/0018-*`, `docs/adr/0019-*` | 0.5 | acceptance and D5 |
| B1 | `Bounded`, `Cut`, `Total`, `Selection`, `SortKey`, `FieldPath` with indices, `Next` in `paddock-limits`; constructors and `Cut::*` with a test each; the 32-cut aggregate and its `bounded.cuts` row; the row checks in each constructor; `TryFrom<CutWire>` and `TryFrom<BoundedWire>`; ts-rs with `number` counts; proptests; `limit_case!`'s cut form | `crates/paddock-limits/src/bounded.rs`, `crates/paddock-testkit/src/limits.rs` (cut form) | 2 | 1.4's crate (same branch, or after it); lands after the Dyno tag |
| B2 | `limit-note`, `formatCut`, the ICU messages and the required-keys check, gallery entry; Raise it through `LimitNotice` | `ui/projects/paddock-ui/limit-note` | 1.5 | B1; 2.3 for Raise it (until then the note shows the other actions) |
| B3 | Debrief's cut rows and their typed handles, `limits.ts`, `boundedSlice`, one `limit_case!` per cut row, the row-to-`next` rule, Setting paths resolved against the schema; ADR 0066's four `time.*` rows and the `Bounded` wrap of its `source_time` reply | `crates/debrief-core/src/limits.rs` (with 3.6), `core/generated`, `zone/report.rs` and `api.rs` for the wrap | 1.5 | B1; 1.4 and 0.4 on `main` or `debrief/1.2-int` |
| B4 | `Analyzed` and both coverage types onto `Bounded`, both engines, mocks, `AsyncResource`; the five producers of `Coverage.limits` strings become cuts (`analysis/outliers.rs:291`, `:358`; `crates/debrief-store-duck/src/analysis.rs:410`, `:777`, `:932`); `cuts_json` in every AI tool, replacing `coverage_json`, `with_coverage`, the prompt sentence and the two tests of the fix branch | `ports/dto.rs`, `store-duck/analysis.rs`, `analysis/*`, `home/*`, `crates/debrief-ai/src/assistant` | 3 | B2, B3, the AI fix branch |
| B5 | Engine instances 3, 5, 9, 10, 11, 12, 14, the `FORMAT_VERSION` bump for 12 | `skew`, `analysis`, `profile/ops`, `store-duck/stats.rs`, `store-duck/schema.rs`, `survey` | 2 | B4 (not 4.2: `RaiseSetting` follows the rows, section 3). Shares `skew/store.rs` with ADR 0066's Z5; whichever is second rebases; Z5's agent owns `skew/` and reviews this change |
| B6 | UI instances 1, 2, 4 to 9, and `BOUNDED_SURFACES`; every view of a migrated command reads `.result` (about 20 services and their specs and mocks [J]), the ten `queryPage` callers, and paged Show more on the dump, UMDH and heap views (D3) | `ai/chat`, `grid`, `dumps`, `profiling`, `core/mock` | 4 | B4 |
| B7 | `xtask bounded-check` (its seeded exempt list includes ADR 0066's interim `source_time` row until B3 wraps it): a `syn` walker over two macro tables and both command attributes, through generics and re-exports, with the `of` check; the ESLint rule; seeded exempt list; CI line (warnings) | `xtask`, `apps/debrief/bounded-exempt.toml`, `eslint/`, integrating agent for `.gitea` | 5 | B3 |
| B7b | 1.4's `conformance/src/limits.mjs` extended to the cut sites of section 2, its waiver list seeded for Debrief | `conformance/src/limits.mjs` | 1 | 1.4, B3 |
| B8 | The fields list above, the check's lists reasoned, warnings become errors | across Debrief | 3 | B5, B6, B7, B7b |

**About 23.5 days raw.** It takes over part of 2.5 (`Truncated` and the `boundedBy` check, about 1.5 of its 2.5 days) and the marker half of 4.2 (about 1 of its 3), so the net addition is about 21 days [J]. Against revision 4's 22: D3's paged Show more in B6 (1) and ADR 0066's `source_time` wrap and rows in B3 (0.5). B7 was 2.5 days in revision 1, which priced a scan by type name; walking generics and re-exports through two macro tables and two command attributes is the larger part of the work [J]. The "per-method test" of 4.2 is part 4 of the check. If the scope has to shrink, B0 to B6 fix every named instance in about 14.5 days, without the check; then the next limit can again be silent, which is what ER-21 asks to prevent, so that is not the recommendation.

## Parallelism and cost

No long operation is added. **Cheaper is the design:** `first` reads one extra item, and no reply counts further for the sake of the words. `Total::Exact` is used only where the total is known or the items were all seen anyway. An exact count that would cost a scan is not computed: that is what `AtLeast` is for. Instance 3 is the one place a new count is read, from the summary the function already fetches [S]. Instance 12 adds one integer per segment at write time. The note is a small component; budgets go through rule 35 as usual.

## Logs (ER-27)

`Cut::describe()` gives one line. Each command with a non-empty `cuts` logs limit id, `of`, shown, total kind and number at debug through the existing activity entry (`activity::logged`), so a bundle shows which limits a session hit. No user content: only ids, field paths of the reply type and counts.

## Consequences

- A person can always tell a list is partial, by how much, from where and in what order, and what to do about it.
- About 25 ad-hoc fields and two coverage types go away. A `WIRE_VERSION` bump, one `FORMAT_VERSION` bump of its own and a change to the mocks, once. A store from before the bump is refused with "open its log files again"; no migration.
- `paddock-limits` holds both halves of "a limit was hit", so 0019's registry, harness and scan are built once.
- `Analyzed.result` keeps its name, so views change little; `coverage` readers change to `cuts`.
- A reply that is whole costs an empty array.
- The first list of exemptions is long and needs real reasons; that is reviewing work, about half of B8 [J].

## Alternatives considered

- **Keep `Coverage` and add fields.** It has no notion of a list being cut, and `limits` is English text. Extending it is this ADR without the check.
- **`total: Option<u64>` plus `exact: bool`.** Can disagree (`None` with `exact`), which the principles forbid.
- **A wrapper type only on lists, `BoundedVec<T>`.** Neat for lists, and it does not cover "looked at part of the input", which is half of the instances.
- **`from` as a field of `Cut` instead of inside `Selection`.** It means nothing for `Sample` and `Last`, so it would be an `Option` that can disagree with the selection.
- **Exempting by-request limits.** The person did not type the request's limit; the view did. Five instances are of this kind.
- **Removing the limits.** ADR 0018 explains why not.

## Decisions (accepted by the user on 2026-11-06, as recommended)

- **D1. Home of the types.** D: in `paddock-limits` with plan 1.4, after the Dyno tag (recommended: one crate, key, registry and harness for every limit). B: inside Debrief with a registered copy (rule 55), moved later, only if 1.4 slips.
- **D2. Scope for 1.2.** All packages, about 23.5 days raw or 21 net of 2.5 and 4.2 (recommended, since the check is the point). Or B0 to B6 only, about 14.5 days.
- **D3. Show more on views that have none.** Dumps, UMDH and heap lists could get paged Show more (in B6, about 1 more day for four views [J], counted in the totals, recommended: the dump, UMDH-growth and heap requests already take `offset`, `crates/debrief-profile/src/api.rs:951` onwards [S]), or only the note, with the rest reachable by filter.
- **D4. Instance 14.** Keep the cut with a note (recommended, since 10,000 files in one log group has not been seen) or spread sampling across the group.
- **D5. The family change.** At acceptance, family ADR 0018's `Truncated` becomes `paddock_limits::Cut` and its family-wide `boundedBy` check becomes Debrief's `xtask bounded-check` until a second app adopts it (then `./dev`); ADR 0019 gains the cut rows. Recommended: yes, with plan 2.5 and 4.2 amended in the same change (B0). The alternative is to keep 0018's words and build both, which is the copy rule 55 forbids.
- **D6. The matching edits to ADR 0067 (Accepted).** Amend 0067's lines 194, 265 and 521 as section 6 says, in the change that accepts this ADR (recommended), or leave 0067 as it is and let its F packages read this ADR.
- **D7. Where B7's ESLint rule lives.** In Debrief's ESLint config first (recommended: one app uses it; rule of three), or in the shared `eslint/` preset now, which binds every app's UI.
