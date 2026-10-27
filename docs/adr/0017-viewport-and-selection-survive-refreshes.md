# 17. Viewport and selection survive data refreshes

- Status: Proposed (2026-10-27; the user reviews it before anything is built)
- Date: 2026-10-27
- Related: rule 33 (`@for` with `track`), rule 39 (shared parts live in `paddock-ui`), rule 40 (recurring bugs mean a
  missing design rule), rule 43 (shared behaviour is tested once); ADR 0009 (dialogs as windows), ADR 0013 (workbench);
  Debrief ADR 0050 (dialogs are windows), Debrief ADR 0061 (controls stay put while a view loads)

## Context

Reported against Debrief 1.1.1: "After Explain event the events grid scrolls to the top." The cause is not in the AI
code. Paths below are under `apps/debrief/app/src/app/`.

1. "Explain event" (`ai/ai-launcher.ts:59-81`) starts a job; "Explanation ready · View" opens the `ai-narrative`
   dialog (`dialog-windows/dialog-registry.ts:316-327`), which is an OS window on desktop
   (`dialog-windows/tauri-dialog-backend.ts:54`, `windowed = true`). Without a model it opens the AI set-up wizard
   window instead; a citation click closes the narrative window first (`ai/dialogs/narrative-dialog.ts:45-48`).
2. When **any** dialog window closes, `ui/dialog.service.ts:114` calls `refreshAfterWindow()` (lines 158-164), which
   calls `WorkspaceStore.refreshSummary()` (added with Debrief ADR 0050, commit `bb194f16a`, 2026-10-23).
3. `refreshSummary` (`core/workspace/workspace-data.store.ts:191-195`) bumps `appendVersion`, so `dataVersion` changes
   and a new `summary` object is stored (which also re-derives `effectiveQuery` through `effectiveGroupBy`,
   `core/workspace/query.store.ts:34-45`).
4. The grid's effect (`grid/events-grid.ts:437-450`) treats every data change as "start from the top" unless it came
   from live tail (`workspace/live-tail.ts:77-79`, `dataVersion === liveVersion`); `reset()` (lines 500-507) clears
   the selection, calls `setTop(0)` and empties the page cache (`grid/paged-rows.store.ts:123-138`). A pending
   `jumpTo` to the cited event is then dropped by its generation check (line 561).

So the grid jumps after closing Settings, About, Regex, Propose or any other dialog window too, and after the other
non-live `refreshSummary` callers (`video/video.store.ts:136`, `watched/watched-folders.store.ts:61`,
`skew/clock-skew.store.ts:64`) and pop-out host changes (`popout/shared-state.ts:77-82`). The browser build uses
`InPageDialogBackend.windowed = false` (`dialog-windows/in-page-dialog-backend.ts:18`), so no unit or e2e test ever ran
the path.

**The pattern:** position and selection are tied to row positions, and each feature decides on its own when to keep
them. The events grid tracks rows by position (`@for (v of rowVms(); track v.index)`, `events-grid.ts:170`) and keeps
its multi-selection as index ranges (`grid/row-selection.ts`), so any refresh that is not a proven append must throw
both away; live tail got a special case (`shift`, lines 523-544), nothing else did. Hotspots has the same shape
(`profiling/hotspots/hotspots-view.ts:148-158` sets `scrollTop = 0` on every query recompute, selection is a row index
in `hotspots.store.ts:33`). Earlier fixes of the same kind: `c608527ff`, `c6798ab46` (chat, Jobs, Notifications and
Runs lists scrolled into empty space), `61a1d2399` (SQL results scroll), `50200fb55`, `9dd8bbaf2` (assistant
stick-to-bottom), `09d18cd63` (grid selection reset moved to a `linkedSignal`).

The right model already exists once: `PdTable` (`ui/projects/paddock-ui/table/src/table.ts`) tracks by `rowKey` (line
523), selects by `selectedKey` (line 419) and virtualizes with CDK above `PD_TABLE_VIRTUAL_FROM` (lines 168-176).

### Instances (survey 2026-10-27)

**30 data lists** in the family (`paddock-ui` 6, Debrief 15, other apps 9; 11 virtualized). Debrief's analysis views
(patterns, outliers, anomalies, traces), search palette, plugins table and runs are keyed already and counted with the
keyed group; its six result tables are counted as one group:

| Where | Lists | Virtualized | Tracks by position or `$index` |
|---|---|---|---|
| `paddock-ui` | `PdTable`; view `table.ts`, `tree.ts`, `matrix.ts`; workbench explorer; assistant turns | 2 (`PdTable`, CDK) | view `table.ts:66,68`; assistant `assistant.ts:76` |
| Debrief | events grid; `workspace/virtual-list.ts` (facets, Columns); Home findings; Hotspots; call tree; heap dominators; heap types; Problems; Notes; Jobs; Alerts; Notifications; sidebar files; Exceptions; analysis views; query results / SQL console / playbook results / dashboard tile / report block / chat (counted as one family of result tables) | 4 (grid and virtual-list custom; Home CDK; Hotspots custom) | grid `:170`, virtual-list `:25`, Hotspots `:72`, and the six result tables (`query-results-view.ts:40`, `sql-console.ts:117`, `playbook-run.ts:92`, `dashboard-tile.ts:66`, `report-block.ts:77`, `chat-panel.ts:82`) |
| Dyno | network, processes, cleanup (CDK); alarms history; alarm detail | 3 | `alarms-history.ts:23`, `alarm-detail.ts:55` |
| Pitwall | requests (CDK); benchmarks | 1 | `benchmarks-page.ts:94` |
| Pitcrew | logs (CDK); config panel | 1 | `config-panel.html:257` |
| Spotter | extraction panel | 0 | `extraction-panel.ts:84` |

Across all templates (specs, `node_modules` and `dist` excluded) **204 of 1084 `@for` blocks track by index** (ui 34,
Debrief 115, Dyno 29, Pitcrew 14, Pitwall 3, Spotter 9), plus 12 that track a positional `.index` field. Most are
static lists where it is harmless; the data lists above are where it is not. `*ngFor`: none. `cdkVirtualFor`: 7, all
with a key `trackBy`.

## Decision

1. **Rows have identity.** Every data list declares a `rowKey(row) => string` that comes from the engine (event id,
   finding id, file id, request id), never from the position. Rendering tracks by that key (`track rowKey(row)` /
   `trackBy`). A list whose rows have no id gets one from the engine (AGENTS.md: the engine is the source of truth),
   not a content hash in the UI.
2. **Selection is kept by key.** Single selection is a key; multi-selection is a set of keys plus an anchor key and a
   focus key (ranges are computed for display, not stored). After a refresh, keys that are gone are dropped; the rest
   stay selected wherever they now are.
3. **The viewport has an anchor row.** The list records the key of the first fully visible row and its pixel offset
   (`ViewportAnchor { key, offsetPx }`) whenever the user scrolls. After a data refresh the list restores the anchor:
   the row is looked up (for paged lists through the existing anchor query, Debrief's `queryPage({anchor})` used by
   `jumpTo`), the list scrolls so it sits at the same offset, and only if the row is gone does it fall back to the
   nearest neighbour by sort key, then to the top.
4. **What resets, explicitly.** A list starts from the top only on a *navigation*: the user changed the query, filter,
   sort or source set, or invoked a jump. A *refresh* (same query, new data version: a dialog closed, a summary
   reloaded, an append, a pop-out sync) keeps anchor and selection. The decision is a pure function
   `classifyChange(prevQueryKey, nextQueryKey, reason) -> 'navigate' | 'refresh' | 'append'` compared by value, not by
   "did this come from live tail". Live tail's "follow the end" is an `append` with `stickToEnd` set, the same
   mechanism, not a special case.
5. **One shared mechanism** (rule 39) in `@decryp7/paddock-ui`: `rowKey`, `KeySelection`, `ViewportAnchor`,
   `classifyChange` and an `anchoredViewport()` helper used by `PdTable`, the view `table`/`tree`, the workbench
   explorer and by custom virtual scrollers such as Debrief's events grid (which keeps its own scroll compression,
   `grid/scroll-map.ts`, and its `PagedRowsStore`, but delegates identity, selection and anchoring). Debrief's
   `workspace/virtual-list.ts` is replaced by `PdTable` or the shared helper.
6. **Fix the trigger too.** Closing a dialog window does not refresh the workspace summary blindly: a dialog whose
   result changed engine state says so in its declared contract (ADR 0009 `result`), and only then is the summary
   reloaded. With 3 and 4 the refresh would be harmless anyway; this removes needless queries.
7. **Scrollbar markers (opt-in, same component).** A list can pass `markers: Signal<ScrollMarker[]>` where
   `ScrollMarker = { position: number /* 0..1 of the full result */, kind: 'error' | 'warning' | 'match' | 'selection'
   | 'bookmark' | string, key?: string }`. The shared component draws them as ticks in a thin gutter beside the
   scrollbar (an overview ruler, like an editor's), using theme tokens and a palette validated for colour-vision
   deficiency; clicking a tick jumps to its row (a navigation). For a paged result the positions come from the engine
   (a bucketed "where are the errors, matches and marks" query with a fixed bucket count, bounded by ADR 0018), never
   from the loaded pages alone. It composes with the grid's scroll compression because positions are fractions of
   the result, not pixels. Debrief's events grid is the first user (levels, search matches, markers and notes);
   Pitcrew's and Pitwall's logs/requests lists can adopt it.

## The check

1. **One shared list contract test** (`paddock-ui` testkit, run by every app over every data list it registers, rule
   43): given a list and a fixture of 10 000 rows, (a) scroll to row 5 000 and select rows 5 010-5 012; (b) trigger a
   refresh with the same rows plus 100 inserted above and 100 below; (c) assert the anchor row is at the same pixel
   offset and the same three keys are selected; (d) remove the anchor row and assert the neighbour is shown; (e) change
   the query and assert the list is at the top with no selection. A Playwright variant runs it on the events grid and
   on one list per app in Chromium (WebKit in the full suite).
2. **The Debrief regression first** (rule 5): a failing test that opens a dialog with a windowed backend fake, closes
   it, and asserts the grid's anchor and selection are unchanged; then the same for Explain event's narrative window
   and a citation click (the grid ends on the cited row).
3. **Lint `paddock/template-no-index-track`** in `@decryp7/paddock-eslint` (`eslint/src/rules/template-rules.cjs`):
   `track $index`, `track i` where `i` is `$index`, and `track x.index` are errors in a `@for` whose collection is not a
   literal array or a `computed` marked `// static-list: <reason>`. It starts as a warning, lists the 204 + 12 sites,
   and becomes an error once the data lists above are fixed and the static ones are marked. Rule 33 gains "track by a
   stable key".
4. **Registration check:** every data list is registered with the list contract (a `PD_DATA_LIST` multi-provider or a
   conformance entry); a conformance check fails when a component uses `cdkVirtualFor`, `PdTable` or the anchored
   helper without being registered, so a new list cannot skip the contract test.

## Consequences

- The class "the list jumped" or "the selection vanished" becomes a failing contract test for every registered list,
  and new lists get the behaviour by using the shared component.
- Index-based multi-selection (`grid/row-selection.ts`) and the live-only `shift` go away; huge results keep working
  because selection holds keys and the anchor query finds rows by id.
- The events grid gets the overview-ruler markers as a separate, opt-in step once identity and anchoring are in.
- About 30 lists change; most are keyed already and only need registration and the contract test.

## Migration plan

1. Debrief regression tests (dialog close, Explain event, citation) failing; then `classifyChange` and the anchor in
   the events grid, then Hotspots.
2. Extract `KeySelection`, `ViewportAnchor`, `classifyChange` and the contract test into `paddock-ui`; `PdTable` adopts
   them; Debrief's `virtual-list.ts` moves to `PdTable`.
3. Lint as a warning; fix the data lists in the table above; mark static lists; lint becomes an error.
4. Scrollbar markers on the events grid (engine bucket query + shared gutter); then offered to Pitcrew and Pitwall.
5. Narrow `refreshAfterWindow` to dialogs whose declared result changes engine state.

## Alternatives considered

- **Fix the trigger only** (stop refreshing after a dialog closes). The grid would still jump after video, watched
  folders, clock skew and pop-out refreshes, and the next refresh source would bring the bug back.
- **Save `scrollTop` and restore it.** Pixel positions are wrong once rows are inserted above or the scroll map
  compresses differently; an anchor by key is what stays meaningful.
- **Per-list special cases** (more `isLiveRefresh`-style flags). That is how the grid got here.
