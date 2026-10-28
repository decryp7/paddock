# 27. Table layers: sticky headers stay above their rows (z-index ladder, C43, headerStaysOnTop)

- Status: Accepted for the ladder, C43 and the e2e check (2026-10-28). The rebuild of `pd-table` on Angular Material's
  table (decision 5) is **proposed**: the user decided it on 2026-10-28 on the assumption that `pd-table` caused the bug,
  which the investigation below did not confirm; it waits for the user's go-ahead as its own change
- Date: 2026-10-28
- Related: rule 40 (recurring bugs mean a missing design rule), rules 39, 42, 43; ADR 0012 (app shell); plan X21 (`pd-table`)

## Context

The user reported on 2026-10-28: "the grid … the cell will cover the header when scrolling", and asked whether it is
the Material grid. No Paddock app uses Material's table; the family's grid is the shared `pd-table`, plus `pd-view-table`
and `pd-view-matrix` (plugin views) and about twenty tables and div grids that apps style themselves.

### What paints over a sticky header (verified, Chromium and WebKit)

A `position: sticky` element with `z-index: auto` is painted in the "positioned, z-index auto or 0" layer of its stacking
context, in tree order (CSS 2.1 Appendix E, step 8). Everything in a later row that is positioned or makes a stacking
context is painted after it, so it shows on top of the header once it scrolls under it:

- cells or rows with `opacity` below 1 (muted rows, hover row actions at 40 %), `transform`, `filter`, `will-change`;
- cells with `position: relative` or `absolute` (badges, bars, the sr-only text of a cell);
- a sticky column (`position: sticky; right: 0`) with the **same** `z-index` as the header: on a tie the later cell wins.

`ui/e2e/grid-header.spec.ts` proves it on a minimal grid in both engines (the check fails on a sticky header without a
z-index above a positioned cell, and passes once the header is raised); the screenshots in
`/Volumes/d7sst7/dev-targets/review/grid-header/` (`before-*`/`after-*`, light and dark, Chromium and WebKit) show a muted
row painted over the header, as Debrief's alerts table had it.

### Where the family had it (static sweep of `origin/main`, 2026-10-28; C43 lists them)

- **Header without a z-index** (any positioned or translucent cell paints over it): Debrief's alerts (muted rows at
  opacity 0.75: a sure instance), tail status, query results, SQL console, store pane, markers, keyboard settings, runs,
  dashboard tiles, the parser editor and the format wizard; Dyno's Charted counters grid (row actions at opacity 0.4);
  the shell's diagnostics tables (`@decryp7/paddock-ui/shell/diagnostics`).
- **Sticky column at the header's z-index:** Pitwall's model library (`th.act`/`td.act` at 1, the header at 1).
- **Not affected:** `pd-table` (its header sits outside the scrolling body, which clips the rows; measured with the new
  check in Chromium and WebKit, scrolled and hovered, both themes), `pd-view-table` (header at 1 above unpositioned cells; its scroller has no height limit, so the header never sticks),
  Debrief's events grid (header at 2, rows in an unpositioned layer), Dyno's processes, network, alarms and recorder,
  Pitwall's tables, Spotter's tables (headers at 1, no positioned cells).

With the apps' mock backends the defect did not show at run time in Dyno, Pitwall or the gallery (sweeps of every route
at 1280×800 and 1280×420, scrolled and hovered, Chromium and WebKit): the instances need data that makes a row muted, a
table taller than its scroller, or a scroller that is not clipped. Which grid the user saw is not known; the classes
above are fixed wherever they occur.

## Decision

1. **A z-index ladder for table layers** in `_tokens.scss`, local to the table: `--pd-z-table-header: 3` above
   `--pd-z-table-sticky-column: 2` above `--pd-z-table-row-action: 1` above cells (auto). All are below `--pd-z-sticky`
   (10). Apps without the `--pd-*` tokens use the same numbers.
2. **Every sticky header sets a z-index from the ladder and an opaque background** (the surfaces `--pd-surface-1/2` are
   opaque in both themes). `pd-table`'s grid is its own stacking context (`isolation: isolate`), so its layers never
   compete with the page's; its header moves from `--pd-z-sticky` to `--pd-z-table-header`.
3. **C43 (conformance, rule 40)** checks every instance: a rule that sticks to the top (`position: sticky` with `top` or
   `inset-block-start`) in an app's or the shared UI's styles must set a z-index, and a sticky column must sit below the
   lowest header of the same app. A sticky element that is not a table header (a side panel, a list's row layer) says
   `/* conformance: not-a-header */`. Error level; all instances are fixed or waived in the same change.
4. **`headerCovers`/`expectHeaderOnTop` in `@decryp7/paddock-e2e`** (`grid.ts`) is the run-time check: it scrolls the
   grid's body (or the page, for a header that sticks to it) in steps, hovers the row under the header and asks
   `document.elementFromPoint` across the header row, clipped to what its scrollers show; it also fails on a header with a
   translucent background and on a body that never scrolled. The gallery runs it over `pd-table`'s virtual table in both
   themes and engines; apps add it to their main grids' e2e when they move onto `@decryp7/paddock-e2e`.
5. **Proposed: `pd-table` on Angular Material's table** (the user's decisions of 2026-10-28: "rebuild pd-table on CDK's
   table", then "use Material components as much as possible since they are tested"). What the investigation found for it:
   - `mat-table` (CDK table) 22 supports virtual scrolling natively when it sits inside a `cdk-virtual-scroll-viewport`
     (`CdkTable._setupVirtualScrolling`, `@angular/cdk/fesm2022/table.mjs`). The header row then lives **inside** the
     viewport's translated content wrapper, and CDK keeps it in place by rewriting each header cell's `top` one animation
     frame after every render (`renderedContentOffset` + `auditTime(0, animationFrameScheduler)`): the fragile pattern the
     coordinator warned of; on WebKit's threaded scrolling the header can lag a frame. To keep the header outside the
     scrolling content, the table would be two `mat-table`s (a header-only one above, a body-only one in the viewport)
     under one `role="grid"`, with both tables `role="rowgroup"` and the cells' role bound to `gridcell` (CDK derives
     `cell` from a non-grid table role).
   - Conditional row definitions (`when`, needed for group rows) throw when virtual scrolling is on ("not supported when
     virtual scrolling is enabled, at the moment"), so group rows stay hand-rolled inside one row definition.
   - Material provides: `matSort`/`mat-sort-header` (sorting with `aria-sort`, the same ascending, descending, none
     cycle), `mat-menu` with `matMenuTriggerFor` and `matContextMenuTriggerFor` (row and column menus, Shift+F10),
     `matTooltip`, `mat-icon-button`. Hand-rolled because Material lacks them: group rows (above), middle truncation
     (`pd-middle-text`), container-width column dropping (`hideBelow`, `fitColumns`), uniform densities, and the roving
     one-tab-stop grid keyboard (`mat-table` has no grid keyboard model).
   - `@angular/material` becomes a peer dependency of `@decryp7/paddock-ui` (every app already has it; the `ui` workspace
     does not), themed through `--pd-*` tokens (`--mat-table-*` system variables).
   It does not fix the reported bug (decision 2 does), so it is a separate change with its own tests (100 % for the
   component), the same public API, and before/after screenshots.

## Consequences

- A new sticky header without a z-index, or a sticky column at the header's level, fails C43 in CI before it ships.
- Apps' own grids keep their own markup; moving them onto `pd-table` is not part of this change.
- `pd-table`'s header still does not stay in view when the table is taller than its `maxHeight` is set to and the
  surrounding pane scrolls (the workbench demo's Problems view sets `maxHeight` 2000): `.pd-grid` clips with
  `overflow: hidden`, so the header scrolls away with the rows. It is never covered; whether it should stick to the pane
  belongs to decision 5.
