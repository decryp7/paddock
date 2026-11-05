# 65. Every bounded reply reports what it left out

- Status: Proposed
- Date: 2026-11-05
- Related: family ADRs [0018](../../../../docs/adr/0018-bounded-loading-everywhere.md) (bounded loading, `Truncated`, `boundedBy`),
  [0019](../../../../docs/adr/0019-limits-explain-themselves.md) (limits explain themselves, the limit registry), 0014 (monorepo, shared code); Debrief ADR 0016 (command table, generated
  types), 0042 (store port table), 0063 (binary wire, on `docs/debrief-binary-wire`, in review); plan packages 2.3, 2.5, 4.2, 4.5; Paddock rules 1, 5, 40, 42, 43, 49, 51, 55; ER-21, ER-27
- Evidence marks: **[S]** read in source at `origin/debrief/1.2-int` 852b35ac0, paths under `apps/debrief/`, and a short path such as `api.rs` or `skew/store.rs` means under `crates/debrief-core/src/` unless a crate is named; **[C]** counted with grep or a heuristic scan at the same commit, so a
  rough number; **[J]** judgement. Sizes in days are judgement. Nothing in this ADR quotes a benchmark.

## Answers

| Question | Answer |
|---|---|
| Is this a new contract? | **No.** Family ADR 0018 already says "every answer says whether it was cut" and ADR 0019 says "no silent truncation". Both were accepted on 2026-10-27. Neither is built: no `Truncated` type, no registry and no `boundedBy` lint exist in any crate or check [C]. This ADR is how Debrief makes them true, and it corrects three details of 0018 (below). |
| What is the mechanism? | **`Bounded<T>`**: a value plus a list of typed `Cut`s, each with what was shown, a total that is exact, a lower bound or unknown, how the shown part was chosen, the limit's id, and what the person can do next. One generated TypeScript type, one UI note, one check. |
| What about `Coverage`? | **It becomes a `Cut`.** `Analyzed<T>` is the first use of the mechanism and does half the job. Its `limits: Vec<String>` is why it is not enough: English text, no numbers, nothing checks it. |
| Is the `boundedBy` lint built? | **No.** It exists only as words in ADR 0018 and plan package 2.5; the memory audit said the same. Its rule alone would miss most instances here (see "Why not just the lint"). |
| Debrief-only or shared? | **Shared, with the types in a new crate that no other app links yet; first use in Debrief.** Land on `main` only after the Dyno tag. The user decides (D1). |
| Does it fit ADR 0063? | **Yes, in either order.** Cuts travel in the MessagePack head, rows in a table. The checker must accept `#[wire_command]` as well as `#[tauri::command]`. |

## Context

The same bug keeps coming back in different places: a limit that does not report itself. Fourteen instances are listed under "Migration". What they have in common:

- **The label and the data disagree.** "Selected: 340 events" sends 20 (`app/src/app/ai/chat/chat-model.ts:211`, `:222`) [S].
- **A total exists and nobody shows it.** `HeapDiffPage.total_rows`, `HeapGrowthPage.total_rows` (`crates/debrief-profile/src/api.rs:992`, `:1034`), and `total` of dump threads and modules are in the reply. The views use them for empty states at most, and have no pager [S].
- **No total exists.** `Butterfly` truncates both lists to the request's limit (`crates/debrief-profile/src/ops/butterfly.rs:89-90`) and has no field to say so. `umdhTree` returns a plain array [S].
- **The reply claims to be complete.** `requests` runs through `analyse`, whose `Coverage` compares events analysed with events matched. `rank_requests` then cuts the list at 500 (`crates/debrief-core/src/analysis/trace.rs:263`), so the person reads "everything was analysed", which is true, above a list that is not everything. `trace_summaries` does the same at 200 (`analysis/moment.rs:296`) [S].
- **The reply carries a flag for a different limit.** `StoreFields.truncated` is about `MAX_FIELD_ROWS` = 2,000 (`crates/debrief-core/src/ports/inspect.rs:21`), while keys beyond 512 per segment were already dropped when the segment was written (`crates/debrief-store-duck/src/stats.rs:178`). `Sampled.truncated` is about groups (`survey/sample.rs:73`), not about the ids cut at `:115` [S].
- **Two cuts stack up.** `selectedRecords()` caps at 200 *and* skips rows of pages not in memory (`app/src/app/grid/events-grid.ts:643`). `suggest_over` stops at `MAX_SCAN` and also stops learning ids at `MAX_IDS` (`crates/debrief-core/src/skew/store.rs:30`, `:44`) [S].

The shape of the fixes so far explains why it recurs: about 25 reply structs have grown their own `truncated: bool`, `capped: bool`, `dropped: usize`, `truncated_lanes` or `sampled` field (for example `config/dto.rs:198`, `ports/console.rs:99`, `spans/pairing.rs:95`, `api.rs:647`, `app/src-tauri/src/storage/model.rs:107`) [C]. Each UI then decides for itself whether to say anything, and in what words. There are also two coverage types: `ports::Coverage` and `api::AnalysisCoverage` (`crates/debrief-core/src/api.rs:312`) [S].

The size of the surface, at this commit [C]:

| What | Count | How |
|---|---|---|
| Commands parsed from `#[tauri::command]` | 389 | scan of `app/src-tauri/src`; the table in `lib.rs` lists 351 by a simpler pattern |
| Replies holding a collection, directly or through named structs | 194 | field walk by type name; heuristic, it follows no generics or re-exports |
| ... of those with no field that looks like a marker or a total | 111 | so `boundedBy` would start with up to about this many entries |
| Limit-shaped constants in `debrief-core`, `-store-duck`, `-ai`, `-profile`, `-dump`, `app/src-tauri` | 557 | `MAX`, `LIMIT`, `CAP`, `TOP`, `PAGE` in the name; many validate input and never cut a reply |
| `.truncate(` outside tests in those crates | 110 | includes text and buffers |
| `limit: <number or constant>` in TypeScript, outside specs and mocks | 39 | |
| `.slice(0, <number or constant>)` in TypeScript, outside specs and mocks | 93 | includes display trims |

**The pattern (rule 40):** a limit is enforced where the work happens and the fact that it was hit does not travel. Fixing instances one at a time has not stopped new ones.

## Decision

### 1. The type

```rust
/// A value and what was left out of it. Fields are private: a Cut can only be made through the constructors.
pub struct Bounded<T> { result: T, cuts: Vec<Cut> }

pub struct Cut {
    limit: LimitId,          // stable key from the registry: "traces.list", "analysis.events"
    shown: u64,              // items in the reply, or items looked at
    total: Total,
    selection: Selection,    // which ones were kept
    next: Next,              // what the person can do about it
}

pub enum Total { Exact { n: u64 }, AtLeast { n: u64 }, Unknown }
pub enum Selection { First, Last, TopBy { key: SortKey }, Sample }
pub enum Next { Page, ShowMore, RaiseSetting, Narrow, None }
```

Serde is camelCase with `tag = "kind"`, like `OffsetSpec`. In TypeScript:

```ts
interface Bounded<T> { result: T; cuts: Cut[] }
type Total = { kind: 'exact'; n: number } | { kind: 'atLeast'; n: number } | { kind: 'unknown' };
```

Rules the constructors enforce, so the illegal states cannot be built:

- `shown <= n` for `Exact`; `shown < n` for `AtLeast`; a cut is only made if something was left out, so `shown == total` yields no `Cut` and an empty `cuts` means complete.
- `Bounded::first(iter, limit)` takes `limit + 1` items and answers `AtLeast { n: limit + 1 }` when the extra one exists. **It never counts further just to print a number** (rule 51). `Bounded::ranked(vec, limit, key)` has seen them all and answers `Exact`. `Bounded::counted(items, total, limit)` is for a total the store already knows. `Bounded::complete(value)` has no cut.
- Fixed limits come from the registry (section 3), not from a loose number at the call site.

**One `Bounded`, not one per list.** A reply with two lists (`Butterfly`, `traces`) has one `Bounded<Reply>` with two cuts. So the same type covers *a list was cut* and *the analysis saw only part of its input*: both are "shown M of N somethings", and the noun comes from the limit. `Coverage { matched, analysed }` becomes a cut with limit `analysis.events`, `shown: analysed`, `total: Exact(matched)`, `selection: First`.

**Paging is a cut too, with `next: Page`.** A page has a known total and a way to reach the rest, so the note reads "500 of 1,204" with a "Show more" button, and the views listed below stop hiding it. The events grid is exempt because its scrollbar already is that display.

### 2. What changes from ADR 0018

| ADR 0018 | Here | Why |
|---|---|---|
| `truncated: Option<Truncated { shown, total_or_lower_bound, limit }>` | `cuts: Vec<Cut>`, `Total` as an enum | One number cannot say whether it is exact, and "at least 5,000,001" must read differently from "5,000,001". A reply can be cut in more than one way (three examples above). |
| Lint: a command returning a collection with no `Limit` or `Page` argument | Lint on replies, on limit-shaped constants, and on TypeScript (section 5) | See "Why not just the lint". |
| Types in `paddock-view` or a new `paddock-query` | A new small `paddock-bounded` (D1) | `paddock-view` pulls in `paddock-ipc` and postcard (`crates/paddock-view/Cargo.toml`) [S], which `debrief-core` does not depend on. |

If this ADR is accepted, ADR 0018 gets a one-line amendment pointing here. Its typed arguments (`Limit`, `Page`, `Buckets`) stay as they are, as do its Settings and virtualization decisions. This ADR adds no Settings.

### 3. The registry

`debrief_core::limits` declares every limit that can cut a reply: one row with id, noun for the message, value, selection, and either a Setting path or `hard`. This is the small first part of ADR 0019's registry, with the same keys; 0019's `LimitExceeded`, for work that was *stopped* rather than cut, can use it later without changing it.

A generated `core/generated/limits.ts` gives the UI `LimitId` as a union and the values as constants, so `slice(0, 20)` becomes `boundedSlice(ids, LIMITS.aiContextEvents)`. The number lives in one place. Today it lives in two: `slice(0, 20)` in the UI and `MAX_SELECTED = 20` in the engine, which caps again and tells the model (`crates/debrief-ai/src/chat/context.rs:9`, `:78`) while the person's label still says N [S].

### 4. One note in the UI

`<pd-limit-note [cuts]>` replaces `debrief-coverage-note`. Messages are generated from the type, in the UI's own i18n, from an exhaustive `Record<LimitId, noun>`, so a new limit does not compile without its words:

| Total | Example |
|---|---|
| `Exact` with `TopBy` | Showing the 200 longest of 1,340 traces. |
| `AtLeast` with `First` | Looked at the first 5,000,000 of more than 5,000,000 events. |
| `Unknown` with `First` | Showing the first 512 fields; there may be more. |
| `Exact` with `Sample` | Sampled 10,000 of 14,255 sources. |
| Two cuts | One line each. |

`next` gives the action: **Show more** (`Page`, `ShowMore`), **Raise it…** through ADR 0019's `LimitNotice` when the limit has a Setting, "Narrow the query" as words only. Status is words, never colour alone. The note keeps a fixed slot so it does not move controls when data arrives (ADR 0061).

`AsyncResource`'s `coverage` computed (`app/src/app/analysis/async-resource.ts:57`) becomes `cuts`, so views already on it change one binding. `isPartial` and `fullCoverage` go away. `Coverage.limits` strings go away; for the AI, and for logs, `Cut::describe()` writes the English sentence in Rust (no user content, ER-27).

**AI tools.** All four `.map(|a| a.result)` in `crates/debrief-ai/src/assistant/tools.rs` (`:598`, `:613`, `:621`, `:636`) drop coverage today [S]. `fix/debrief-ai-coverage-facet-search` is fixing that; its worktree holds uncommitted changes at this writing, so its final shape was not read. That branch lands first. This ADR then replaces whatever it adds with one helper that puts `cuts` in every tool reply, so the model is told every time.

### 5. The check

A new limit cannot stay silent because five things must each be passed. Each is a failing build or test, not a review item:

1. **Type.** `Cut` cannot be built by hand, and a reply cannot claim a cut it did not make: `shown` comes from the constructor.
2. **Replies** (`cargo xtask bounded-check`, with `syn`, in Debrief's CI). For every row of `command_table!` and `store_port_table!`, it walks the reply type through Debrief's struct definitions. If the type holds a collection without `Bounded` around it, the row must be listed in `apps/debrief/bounded-exempt.toml` with a kind (`fixed-set`, `per-source`, `by-request`) and a reason. It accepts `#[tauri::command]` and ADR 0063's `#[wire_command]`. Starts with up to about 111 entries [C], each with a reason or a migration.
3. **Limits** (same tool, ADR 0019 check 2). A limit-shaped constant used in a comparison, `.truncate`, `.take` or a SQL `LIMIT` in a crate that reaches a reply must be in the registry, or in a waiver list as input validation or a buffer. The waiver list is seeded and can only shrink by hand; a new unlisted one fails.
4. **Each limit, at its edge** (`limit_case!`, ADR 0019 check 1). For every registry row, a test with data at the limit (no cut) and one over it (a cut with the exact numbers). The registry and the test list must match both ways. This catches off-by-one and "the flag is about another limit".
5. **UI.** The generator also writes `BoundedCommand`, the union of commands whose reply holds a `Bounded`. A hand-written `BOUNDED_SURFACES: Record<BoundedCommand, Surface>` names the component and spec that shows each, or `engine-only` with a reason, so a new command fails type-check until it has an answer. Each surface's spec gets a cut from the mock and asserts the note's text (no fixed waits, rule 49). ESLint flags `.slice(0, <n>)` and `limit: <literal>` outside `boundedSlice` and `LIMITS` unless marked `// bounded: display-only — <why>`.

Parts 2 and 3 warn for the first package that adds them and become errors when the Debrief list is reasoned, as ADR 0019 phases its own check. **What still gets through:** a hand-written loop with a literal (`if n > 1000 { break }`) in a reply path, and a UI that gets a cut and renders the note where nobody reads it. Reviewers keep those two.

**The red tests come first** (rule 5): AI label with 340 ids; `traces` with 201; `suggest_over` over a lowered scan cap; 513 field keys in one segment. Reviewers run the mutation check on each (rule 5).

**Why not just the lint.** ADR 0018's rule asks whether the *arguments* have a limit. Of the fourteen instances, it would flag only those whose argument list has none. `heapDiff` has `limit`, so it passes, and the bug is still there. So does `butterfly`. A hard-coded cap inside a function returning `Analyzed<Vec<_>>` looks fine to it, and `slice(0, 20)` in TypeScript is out of its sight. So the check looks at what is left out, not at how the question was asked [J].

### 6. Debrief-only or shared (ADR 0014, rule 42)

Rule 42 says that what is generic or needed elsewhere is built in Paddock, and rule 39 that a second copy is not allowed. The types are generic. Shared crates already have the same habit (`paddock-storage` `capped`, `paddock-proc`, `paddock-watch`) [C], and ADR 0018 section 9 already assigns them to shared code. Dyno's disk map is the likely second user [J].

| Option | Types | UI note | Cost | Verdict |
|---|---|---|---|---|
| **A. New `crates/paddock-bounded`** (serde only, ts-rs behind a feature), `ui/.../limit-note` | shared | shared | The new-crate checklist, 99 % coverage; the crate has no users but Debrief | **Recommended.** Rule 42, and no copy to delete later |
| B. In `debrief-core`, moved later | Debrief | Debrief | A `known-copies.toml` entry with a delete-by tag (rule 55) and a later move across about 25 structs | Fallback if the user wants no shared code in this line |
| C. In `paddock-view` | shared | shared | `debrief-core` gains `paddock-ipc` and postcard | No |

**Nothing shared lands before the Dyno tag** (rule 1). The release-scope plan already says "wait for T" for package 2.5 and what is behind it (`docs/plans/debrief-1-2-release-scope.md:222`, on `docs/debrief-1-2-release-scope`), with T about 2026-11-07. So B1 and B2 start after the tag, or on `debrief/1.2-int` provided that branch reaches `main` only afterwards. A new crate no other app links does not change Dyno's dependency set either way [J]. `LimitId` is a key type in the shared crate; each app's registry and message nouns are its own. `xtask bounded-check` starts in Debrief and moves to `./dev` when a second app adopts it (rule of three).

### 7. Fit with ADR 0063

Nothing here depends on 0063 landing, and neither blocks the other [J]:

- **Today's wires.** The store worker carries port DTOs as JSON text (`crates/debrief-store-client/src/protocol.rs:1-2`) [S], so `tag = "kind"` is fine; postcard's limits on internal tags do not apply to these. `WIRE_VERSION` bumps once (`port_table.rs:24`), as the DTOs change shape.
- **After 0063.** `Bounded<Table<T>>`: `cuts` in the MessagePack head, rows in a table. Internally tagged enums hold under its `with_struct_map()` (0063 section 3 and its count of 46 `tag =` uses). No value comes near 2^53. `shown` is the count of rows, so the UI reads `cut.shown`, never `RowTable.length`, for the words.
- **Order.** If this lands first, 0063's W-packages see `Analyzed` already gone. If 0063 lands first, `RowTable` is the `result`. The only touch point is the list of about ten `queryPage` callers, which this ADR does not change: the grid is exempt.

## Migration

### The named instances

| # | Instance | Today | Reply knows the total? | Change | Total |
|---|---|---|---|---|---|
| 1 | AI selection, `ai/chat/chat-model.ts:211`, `:222` | Label says N, sends 20 | UI has it | `boundedSlice`, label "Selected: 340 events; the first 20 go to the assistant" | Exact |
| 2 | Grid actions, `grid/events-grid.ts:52`, `:643` | Cap 200, unloaded rows skipped | UI has it | Two cuts, shown in the menu's confirmation | Exact |
| 3 | Clock skew, `skew/store.rs:30`, `:44` | Stops at 5M, stops adding ids at 500k | No, but `summary` has event counts | `Bounded<SuggestResult>`, two cuts; the clock dialog shows them | Exact for events, `AtLeast` for ids |
| 4 | Dump threads and modules, `dumps/dump-view.ts:17` | 500 of a possible 4,096 | Yes, unused | Note with Show more | Exact |
| 5 | UMDH growth and tree, `dumps/umdh-view.ts:11`, `:13` | 200; 100 children | Growth yes; tree no | Tree becomes `Bounded` | Exact, `AtLeast` |
| 6 | Heap diff, `profiling/heap/heap-diff.ts:100` | Literal 500 | Yes, unused | Note, `LIMITS` | Exact |
| 7 | Heap growth, `heap-growth.ts:83` | Literal 500 | Yes, unused | Same | Exact |
| 8 | Heap instances, `heap-instances.ts:107` | Literal 200; count shown, no "of" | Yes | Same | Exact |
| 9 | Butterfly, `profiling/butterfly/butterfly-view.ts:103` | 50 callers and 50 callees | No | Two cuts from `ops/butterfly.rs:89-90` | Exact, as it ranks |
| 10 | Traces, `analysis/moment.rs:296` | Top 200, reply says complete | No | `trace_summaries` returns a cut, both engines | Exact |
| 11 | Requests, `analysis/trace.rs:263` | Top 500, reply says complete | No | Same | Exact |
| 12 | Fields panel, `debrief-store-duck/src/stats.rs:178` | Keys beyond 512 per segment dropped at write | No, and lost by then | New `segments.fields_dropped` count; `StoreFields` gets both cuts | `AtLeast` when any segment dropped |
| 13 | AI tools, `debrief-ai/src/assistant/tools.rs:598`, `:613`, `:621`, `:636` | Drop coverage | Dropped | Fix branch first, then one helper | As the source |
| 14 | Survey, `survey/sample.rs:115` | First 10,000 source ids of a group | The group knows | Second cut beside `truncated`; `Selection::First`. Could spread over the group instead: not measured. Largest group in the sample bundle is 1,211 files, so this is not hit there | Exact |

### Fields that move to the type (about 25) [C]

`known_issues.rs:103`; `api.rs:647`; `spans/pairing.rs:95`, `spans/dto.rs:57`, `spans/infer.rs:101`; `config/snapshot.rs:52`, `config/diff.rs:101`, `config/dto.rs:198`, `:236`; `redact/view.rs:69`; `survey/dto.rs:29`, `:51`; `ports/console.rs:99`, `:190`, `:219`; `ports/inspect.rs:220`; `catalogue/mod.rs:206`; `debrief-ai/assistant/proposals.rs:129`, `:149`, `:168`; `app/src-tauri/src/agent_commands.rs:42`, `storage/model.rs:107`, `:120`, `stores.rs:55`, `walk.rs:58`, `browser/mod.rs:53`. Other `dropped` counters count something else (events a pipeline removed, `api.rs:28`, `:173`, `:439`; a full event queue, `events.rs:72`) and stay; the check's list decides each.

### The rest

Whatever parts 2 and 3 of the check list. Each is migrated or gets a reason. Plan package 4.2's eight unbounded port methods gain `Bounded` here too, so 4.2 reduces to adding `Limit` and `Page` arguments.

## Packages

Rough days; one agent; red tests first in each.

| # | Package | Owns | Days | Waits for |
|---|---|---|---|---|
| B1 | `paddock-bounded`: types, constructors, ts-rs, proptests | `crates/paddock-bounded` | 1 | Dyno tag on `main` |
| B2 | `limit-note`, `formatCut`, i18n, gallery entry | `ui/projects/paddock-ui/limit-note` | 1 | B1 |
| B3 | Debrief registry, `limits.ts`, `boundedSlice`, `limit_case!` | `debrief-core/src/limits`, `core/generated` | 1 | B1 |
| B4 | `Analyzed` and both coverage types onto `Bounded`, both engines, mocks, `AsyncResource`, AI helper | `ports/dto.rs`, `store-duck/analysis.rs`, `analysis/*`, `home/*`, `debrief-ai/assistant` | 2.5 | B2, B3, the AI fix branch |
| B5 | Engine instances 3, 5, 9, 10, 11, 12, 14 | `skew`, `analysis`, `profile/ops`, `store-duck/stats.rs`, `survey` | 2 | B4 |
| B6 | UI instances 1, 2, 4 to 9, and `BOUNDED_SURFACES` | `ai/chat`, `grid`, `dumps`, `profiling` | 1.5 | B4 |
| B7 | `xtask bounded-check`, ESLint rule, seeded lists, CI line (warnings) | `xtask`, `apps/debrief/bounded-exempt.toml`, integrating agent for `.gitea` | 2.5 | B3 |
| B8 | The fields list above, the check's list reasoned, warnings become errors | across Debrief | 3 | B5, B6, B7 |

**About 14.5 days.** This takes over the type half of 2.5 and the marker half of 4.2 (2.5 and 3 days in the plan), so the net addition is about 11 days [J]. The "per-method test" of 4.2 is part 4 of the check. If the scope has to shrink, B1 to B6 fix every named instance in 9 days, without the check; then the next limit can again be silent, which is what ER-21 asks to prevent, so that is not the recommendation.

## Parallelism and cost

No long operation is added. **Cheaper is the design:** `first` reads one extra item, and no reply counts further for the sake of the words. `Total::Exact` is used only where the total is known or the items were all seen anyway. An exact count that would cost a scan is not computed: that is what `AtLeast` is for. Instance 3 is the one place a new count is read, from the summary the function already fetches [S]. Instance 12 adds one integer per segment at write time. The note is a small component; budgets go through rule 35 as usual.

## Logs (ER-27)

`Cut::describe()` gives one line. Each command with a non-empty `cuts` logs limit id, shown, total kind and number at debug through the existing activity entry (`activity::logged`), so a bundle shows which limits a session hit. No user content: only ids and counts.

## Consequences

- A person can always tell a list is partial, by how much and in what order, and what to do about it.
- About 25 ad-hoc fields and two coverage types go away. A `WIRE_VERSION` bump and a change to the mocks, once.
- `Analyzed.result` keeps its name, so views change little; `coverage` readers change to `cuts`.
- A reply that is whole costs an empty array.
- The first list of exemptions is long and needs real reasons; that is reviewing work, about half of B8 [J].

## Alternatives considered

- **Keep `Coverage` and add fields.** It has no notion of a list being cut, and `limits` is English text. Extending it is this ADR without the check.
- **`total: Option<u64>` plus `exact: bool`.** Can disagree (`None` with `exact`), which the principles forbid.
- **A wrapper type only on lists, `BoundedVec<T>`.** Neat for lists, and it does not cover "looked at part of the input", which is half of the instances.
- **Removing the limits.** ADR 0018 explains why not.

## Decisions for the user

- **D1. Home of the types.** A: new shared `paddock-bounded`, used first by Debrief, after the Dyno tag (recommended). B: inside Debrief with a registered copy, moved later.
- **D2. Scope for 1.2.** All packages, about 14.5 days raw or 11 net of 2.5 and 4.2 (recommended, since the check is the point). Or B1 to B6 only, about 9 days.
- **D3. Show more on views that have none.** Dumps, UMDH and heap lists could get paged Show more (in B6, recommended: the dump, UMDH-growth and heap requests already take `offset`, `crates/debrief-profile/src/api.rs:951` onwards [S]), or only the note, with the rest reachable by filter.
- **D4. Instance 14.** Keep the cut with a note (recommended, since 10,000 files in one log group has not been seen) or spread sampling across the group.
