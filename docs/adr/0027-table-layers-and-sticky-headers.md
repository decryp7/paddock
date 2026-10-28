# 27. Table layers: sticky headers stay above their rows (z-index ladder, C43, headerStaysOnTop)

- Status: Accepted (2026-10-28). Decisions 1-4 shipped with the fix (`fix/grid-header-on-top`). Decision 5, `pd-table` on
  Angular Material's table, was accepted by the user the same day after reading the findings below ("full mat-table
  rebuild", knowing that Material's sticky header sits inside the virtual-scroll content) and built as its own change
  (`feat/pd-table-mat-table`)
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

1. **A z-index ladder for table layers** in `_tokens.scss`, local to the table: `--pd-z-table-corner: 4` (the header cell
   of a sticky column, which sticks both ways) above `--pd-z-table-header: 3` (header and footer rows) above
   `--pd-z-table-sticky-column: 2` above `--pd-z-table-row-action: 1` above cells (auto). All are below `--pd-z-sticky`
   (10). Apps without the `--pd-*` tokens (Debrief, Dyno, Pitcrew, Pitwall) use the same numbers.
2. **Every sticky header sets a z-index from the ladder and an opaque background** (the surfaces `--pd-surface-1/2` are
   opaque in both themes). `pd-table`'s grid is its own stacking context (`isolation: isolate`), so its layers never
   compete with the page's; its header moves from `--pd-z-sticky` to `--pd-z-table-header`.
3. **C43 (conformance, rule 40)** checks every instance in an app's or the shared UI's styles: a rule that sticks to the
   top or the bottom (`top`, `bottom`, `inset-block-*`, or an `inset`/`inset-block` shorthand with a side that is not
   `auto`) is a header or footer row and needs a z-index of at least 3 (not `auto`, not 0, not a property the check cannot
   read); a rule that sticks both ways is a corner and needs at least 4; a sticky column must sit below the lowest header of
   the same app (or shared entry point). A sticky element that is not a table row (a side panel, a list's row layer) says
   `/* conformance: not-a-header */`. Error level; all instances are fixed or waived in the same change (the review of
   2026-10-28 raised the nine headers at 1 and the four sticky action bars to the header level).
4. **`headerCovers`/`expectHeaderOnTop` in `@decryp7/paddock-e2e`** (`grid.ts`) is the run-time check: it scrolls the
   grid's body (or the page, for a header that sticks to it) in steps and then jumps more than two viewports, and asks
   `document.elementFromPoint` across the header row, clipped to what its scrollers show, twice at each position: in the
   first frame after the scroll, and once a drawn row reaches the header's bottom edge with the row under it hovered; it also fails on a header with a
   translucent background and on a body that never scrolled. The gallery runs it over `pd-table`'s virtual table in both
   themes and engines; apps add it to their main grids' e2e when they move onto `@decryp7/paddock-e2e`.
5. **`pd-table` is built on Angular Material's table** (the user's decisions of 2026-10-28: "rebuild pd-table on CDK's
   table", "use Material components as much as possible since they are tested", then the full `mat-table` rebuild with
   Material's own sticky header). The public API (inputs, outputs, `PdCell`, `PdTableEmpty`, `moveTo`) is unchanged.
   - **Material's parts:** `mat-table` with a sticky header row (`matHeaderRowDef … sticky: true`); `matSort` and
     `mat-sort-header` (sorting, `aria-sort`, the ascending, descending, none cycle, the sort described as "Sort by …");
     `mat-menu` for the column menu (`menuitemcheckbox` items) and the row menu, opened by the "..." button
     (`matMenuTriggerFor`), a right-click and Shift+F10 (`matContextMenuTriggerFor`); `matTooltip` for a cell's full text
     and the icon buttons' names; `matIconButton` for the row actions. The menus keep the family's popup-menu classes
     (`pd-popmenu`, `pd-popmenu__item`); Material's component tokens are set to `--pd-*` tokens on the table's host, and
     the menus and tooltips use the Material bridge (`css/material.css`, now in the gallery and the demos as in the apps).
   - **Sticky header under virtual scroll (Material's documented way):** the table sits inside the CDK
     `cdk-virtual-scroll-viewport`; CDK 22's table attaches to the viewport (`CdkTable._setupVirtualScrolling`) and keeps
     its sticky header row at the top of the viewport while the rows' wrapper is translated, by setting the header's `top`
     from the viewport's rendered offset. The header is an opaque surface, the grid is its own stacking context, and CDK
     puts the header at z-index 100 above the rows. `expectHeaderOnTop` checks it on the gallery's 5,000-row grouped table,
     scrolled in steps and hovered, in both themes, in Chromium and WebKit.
   - **Always in the viewport:** every table is drawn in the viewport, so there is one template and one header. Above 100
     lines (`virtual="auto"`) the viewport draws the rows in view plus 8; below that, or with `virtual="false"`, its buffer
     covers every line and all rows are drawn. The viewport is as tall as the header plus the rows, up to `maxHeight` for
     the rows.
   - **Built on top, because Material has no equivalent:** group rows (Material's conditional row definitions, `when`,
     throw under virtual scrolling, so a group heading is drawn by the one row definition: one cell across the row, or the
     group's own cells under the columns); text shortened in the middle (`pd-middle-text`); columns dropped by the table's
     own width (`hideBelow`, `fitColumns`, a CSS grid over shared tracks instead of Material's flex cells); the family's
     uniform row heights (24, 32 or 36 px and a 28 px header, also the virtual scroll's item size); the roving one-tab-stop
     grid keyboard (Up, Down, Page Up, Page Down, Home, End, Enter; `mat-table` has no keyboard model), which now scrolls
     a row into view below the header only when it is out of view; the loading placeholders, empty and error states.
   - `@angular/material` is an optional peer dependency of `@decryp7/paddock-ui` (needed by `@decryp7/paddock-ui/table`;
     every app already has it) and a dev dependency of the `ui` workspace.

## Consequences

- A new sticky header without a z-index, or a sticky column at the header's level, fails C43 in CI before it ships.
- Apps' own grids keep their own markup; moving them onto `pd-table` is not part of this change.
- `pd-table`'s header stays in view while its own body scrolls. When an app sets a `maxHeight` taller than its pane and
  lets the pane scroll (the workbench demo's Problems view sets 2000), the header scrolls away with the rows, never
  covered; such an app should give the table a `maxHeight` that fits.
- Material's sort arrow and menu items replace the family's own chevron glyphs and CDK menus in `pd-table`; a sortable
  header's accessible name is its label (the button inside the column header), with "Sort by …" as its description.
