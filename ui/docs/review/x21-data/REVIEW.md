# Independent design review: shared data UI (table, chart, forms), plan X21

Reviewer: independent agent (rule 22), 2026-10-25. I did not build these parts. This review is read-only; I changed no code.

Inputs: `contact-sheet.png` and the 14 per-part PNGs in this folder (2x device pixels, light and dark), code in
`projects/paddock-ui/{table,chart,forms}` and `projects/gallery/src/app/data-sections.ts` (branch `shell/x21-ui-data`,
`be2a5bc`). References: `paddock/docs/design-guide.md` (§3, §6, §7, §8), the UI review 2 patterns, and `../REVIEW-2.md`.
All measurements come from the PNG pixels: 1 CSS px = 2 device px. Contrast uses the WCAG formula on sampled values.
I simulated CVD with Machado 2009 matrices at severity 1.0 (protan, deutan, tritan) on `light-charts.png` and
`dark-charts.png`, and I computed CIELAB ΔE76 on the token hex values.

**Counts: 3 P1, 9 P2, 9 P3.**

Verdict: this is a solid base. Numbers are right-aligned in tabular figures, and the header units ("Size (KiB)") follow
the numbers. Hover actions and the "…" button follow the pattern. Destructive items sort last in danger text. Gaps
stay gaps. The legend shows only with 2 or more series. Light and dark mostly match. Three things block approval: the
table breaks at narrow widths, keyboard focus is invisible inside menus, and two adjacent donut slices fail the
guide's own CVD threshold. The P2s are mostly polish that keeps the work from reaching the "professional and
beautiful" bar: density, sort affordance, menu alignment, chart titles and units, and the stacked-bar corners.

---

## P1

### P1-1: At 320 px the table hides its primary column (`*-table-320.png`)
In the pixels, every name is truncated to its first letter ("S…", "J…", "T…"). The Name header reads "N…". The Size
column (96 px fixed) and an empty 72 px action column take most of the width, and the filter placeholder is clipped to
"Filte". This breaks rule 18 and the guide's "first column strong". A narrow table that shows only sizes cannot be used.
The cause is in `table.ts`: `gridTemplate` always appends `72px` for actions, and the fixed column widths do not
shrink, so `minmax(0, 2fr)` for Name gets what is left over, which here is about 20 CSS px.
*Fix:*
- Give the primary (`required`) column a floor, e.g. `minmax(8rem, 2fr)`, and let the other columns drop out
  (`hideBelow`) before the primary column goes under its floor. Add a model test: at any width ≥ 320, the primary
  column gets at least its floor.
- Size the action column from what the row can show: 28 px when only "…" is visible (no hover, narrow container, or
  no inline actions), and 0 when the table has no actions at all (the virtual demo reserves 72 px for nothing).
- Toolbar below about 400 px container width: the Columns button becomes icon-only (aria-label stays), the row count
  wraps under the filter or hides, and the filter keeps at least 8rem.
- Add a Playwright assertion that the Name cell's text is not truncated to one character at 320 px.

### P1-2: Keyboard focus is invisible in the row and column menus
`.pd-menu-item:focus-visible { background: var(--pd-hover); outline: none; }`. `--pd-hover` is 5% ink on the menu's
surface-2. Light gives #F0F2F5 → about #E5E7EA, which is **1.1:1**. Dark is about the same. The design guide (§6) and
SC 2.4.11 require a 3:1 focus indicator. `REVIEW-2.md` already fixed the same defect for the listbox. A keyboard user
who opens the "…" menu with Shift+F10 cannot see which item Enter will run, and one of the items is "Remove…".
*Fix:* use the family's focus indicator on menu items. Either the 2 px `--pd-focus-ring` inset ring (offset −2 px,
radius 6), or the accent tint (`--pd-accent-tint`) plus the ring. Keep the 5% fill for pointer hover only. Do this
in the shared menu style so the table, the Columns menu and every other CDK menu get it. Add a contact-sheet frame
of a menu opened from the keyboard with the second item active.

### P1-3: Two adjacent donut slices fail the guide's CVD threshold (`*-charts.png`, simulated)
The guide (§6) requires adjacent pairs to have ΔE ≥ 20 under protan and deutan. A ring closes on itself, so slot 4
("Other", reddish purple) sits next to slot 1 ("Logs", sky blue). Measured:

| Pair (adjacent in the ring) | Light protan | Light deutan | Dark protan | Dark deutan |
|---|---|---|---|---|
| 1 sky blue ↔ 4 reddish purple | **17.9** | 37.9 | **14.4** | 27.5 |

In the protan simulation, Logs and Other are both mid blue, and only the 2 px separator tells them apart. Tritan is
also weak for 1 ↔ 3 (light 14.1, dark 15.2): those slices are not adjacent, but the legend chip must still be matched to
its slice.
*Fix (any one):*
- Label the slices directly (name + %, outside the ring or in a list beside it) so colour is not the only key. This
  is the guide's "direct-label rather than legend".
- For a donut, choose slot order so the closing pair passes, e.g. 1, 2, 3, 5, then validate the wrap-around pair in
  `options.spec.ts`. Add a palette test: every pair that can be adjacent in a ring of n ≤ 5 slices has ΔE ≥ 20 under
  protan and deutan in both schemes.

---

## P2

### P2-1: Table density does not follow the family scale
The guide (§3, §8) says Standard rows are 32 px (Comfortable 36, Compact 24) and the header is 28 px sticky.
`PdTable` offers 32/40/48, defaults to **40**, and draws the header at the row height (40 px in `*-table-rest.png`).
Rows at 40 px with 13 px text look airy next to every other family table, and 40 and 48 are not densities the guide
names. *Fix:* default `rowHeight` to the density token (`--pd-row-height`, 32 Standard), allow 24/32/36 only, and fix
the header at 28 px.

### P2-2: Sort affordance is invisible until a column is sorted
In `*-table-rest.png` no header shows that it can be sorted. The only cue is the pointer cursor, and the guide (§8)
says "Sort affordance visible". In end-aligned columns the chevron is drawn after the label (`Size (KiB) ^` in
`*-table-hover.png`), which pushes the label off the number edge. *Fix:* on header hover and focus, show a muted
two-way chevron (`--pd-text-subtle`). In end-aligned columns put the chevron before the label, so the label's right
edge stays on the digits.

### P2-3: Row menu: labels misaligned, wrong surface
- "Enable" has no icon, so its label starts about 22 CSS px left of "Reveal in folder", "Copy path" and "Remove…".
  *Fix:* when any item in a menu has an icon, reserve the 14 px icon slot plus gap on every item.
- The menu is drawn on surface-2 (sampled #F0F2F5 light, #1B1E23 dark). The guide (§8) says menus use surface-3
  (#FFFFFF + shadow-2 light, #22262C dark). In dark, #1B1E23 on the #15171B rows barely separates from the table,
  and only the border holds the edge. *Fix:* `background: var(--pd-surface-3)`.

### P2-4: Middle truncation of paths has a ragged join
In `*-table-rest.png` the gap between the head's "…" and the tail changes from row to row: "Deb… plugin-0.bin" has
about a 5 px space, "Debr…plugin-7.bin" and "Deb…plugin-11.bin" have none, and "De…  plugin-10.bin" has about 7 px.
Down the column this reads as sloppy. *Fix (in `PdPathText`):* lay the head out as `flex: 0 1 auto` and the tail as
`flex: none` with no gap, and keep a separator in the tail text (start it at the last `/`, so it reads "…/plugin-0.bin").
The join is then the same in every row.

### P2-5: Stacked bars round every segment
The Mon, Wed, Thu, Fri and Sun bars show the grey "Succeeded" segment with a rounded top tucked under the red
"Failed" segment, which leaves a notch at the join. The guide (§7) says 4 px top radius on the **bar**. *Fix:* in
`chartOption`, for `stackedBar` set `borderRadius: 0` on every segment, and `[4,4,0,0]` only on the topmost non-null
segment of each category (per data item `itemStyle`). Add a spec test.

### P2-6: The unit label collides with the legend and with the card edge
In the stacked-bar chart, "jobs" sits directly under the legend, about 4 CSS px away, and reads as a third legend
row. In the line and area charts, "req" and "ms" touch the card's top border (about 3 CSS px). *Fix:* when there is a
legend, raise `grid.top` to at least legend height + unit line + 8 px. Better still, move the unit into the title
(P2-7) or the header and drop the floating y-axis name, as the guide's title rule ("metric + unit + window") implies.

### P2-7: Charts have no visible title
Each chart has an accessible `label` ("Requests per hour"), but sighted users see four untitled boxes: nothing says
the line is requests, the area is p95 latency, or the donut is disk use. The guide (§7) says titles state the metric
+ unit + window. *Fix:* `pd-chart` draws `label` as a visible 13/600 title (opt-out `showTitle=false` for cards that
already have a header), and the unit can go with it ("Requests per hour, req").

### P2-8: The donut does not earn its place
No values, no total, and the legend is detached at the top-left, while the ring sits left of centre with a large
empty area to its right in a wide card. The guide (§7) says donuts are only for a bounded single value, and to prefer
a bar. *Fix:* either replace the generic `donut` kind with a "parts" kind (one 100% horizontal bar with direct labels
and values), or make the donut put the total (`88 GB`) in the centre and list slices with value and % beside the ring
(this also fixes P1-3).

### P2-9: The contact sheet does not show the states it claims, so they cannot be approved
- `*-table-hover.png` shows the sorted state, not a hovered row (no row is highlighted). The only hover evidence is
  the hovered row in `*-table-menu.png`.
- Missing states: focused row, focused sortable header, descending sort, selected row (guide: accent 12% tint + 2 px
  left bar), collapsed group, empty table ("No rows" + next step), the table and the chart while loading and after
  an error (guide §7: ghosted axes, skeleton, Retry), a form field with focus, a disabled field, dark at 320 px for
  the chart.
*Fix:* add these frames, then ask for a re-review of those states.

---

## P3

1. **Columns button icon.** A gear means settings. Use a columns glyph (or `pd:view-columns`) for "Columns".
2. **No separator before the destructive item.** Add a hairline divider (4 px margin) above "Remove…" in the row
   menu when non-destructive items come before it.
3. **First column not strong.** The guide (§8) says the first column is strong. Name is drawn at 400. Use 500, or 600
   at 13 px.
4. **Two default blues.** The single-series line uses `accent` (#2F6FE0 family), while the single-series area uses
   palette slot 1 (#0C6DBE light, #60AEEB dark). In the light sheet these are two different blues side by side.
   Default a single-series chart to `accent` (the guide's "single highlighted series").
5. **Gap ends have no marker.** The 12:00 and 16:00 ends of the gap are plain line ends. A lone value between two
   nulls would draw nothing. Show the symbol for points with no neighbour (`showSymbol` per data item), as §7
   allows for single points. Add a spec test with `[null, 5, null]`.
6. **Palette drift.** Light slot 6 is #494FDB in `paddock.css` but #4C5BD4 in the design guide. Dark slot 1 ↔ 6
   (#60AEEB / #8FA3FF) is ΔE **9.0** under deutan and 10.8 under protan. They are not adjacent in order, but a
   6-series line chart would confuse them. Bring the guide and the CSS back in line and re-run the validator over
   all 8 slots.
7. **"…" at rest.** At 40% it samples #B8BCC1 on white (**1.9:1**) and #4D5157 on #15171B (**2.2:1**). This follows
   the guide, but it is faint on a laptop panel in daylight. Consider 55%, which still reads as secondary, or put
   the 3:1 non-text contrast question to the guide owner.
8. **Error below the hint.** The order is control → hint ("Shown in the list") → error. The error is the
   actionable line, but it sits furthest from the field, and a 1 px danger border is the only mark on the control.
   Put the error directly under the control and hide the hint while the error shows (it is still referenced by
   `aria-describedby` through the error text if needed). The alert glyph keeps its 14 px box (the drawn triangle
   measures about 12 × 10.5 CSS px, fine, and no longer the 12 px blob from REVIEW-2).
9. **Field width.** Port (5 digits) spans the full 393 px field. Let `pd-field` take a `size="short"` (about 8rem)
   for numbers, ports and codes, so the field's width suggests what goes in it.

---

## What is good (keep)

- Numbers: Size is right-aligned with `tabular-nums`, the header is end-aligned and carries the unit, and the group
  count and row count are tabular.
- Row actions: up to two inline icon buttons on hover or focus, "…" at 40% at rest and 100% on hover or when open, and
  the same list in the right-click / Shift+F10 menu. Destructive items are sorted last (`menuActions`), shown in danger
  text with a trash icon and an ellipsis ("Remove…"), and kept out of the inline buttons.
- Row focus: a 2 px `--pd-focus-ring` inset ring, and the global `:focus-visible` covers headers and the toolbar.
- Group rows: a surface-2 band, a chevron, a medium label and a muted count. The band reads clearly in light and dark.
- Charts: colours come from tokens and the palette, nulls are never connected, bars and areas start at zero, the
  legend shows only with 2 or more series, the tooltip text is escaped, and there is a text twin for screen readers.
  The stacked "Failed" on neutral passes under every CVD type (ΔE ≥ 28.8).
- Forms: the error appears after blur or on submit, with the alert icon and danger text, and is linked by
  `aria-describedby`. The required asterisk and the hint both work in both themes.
- Light/dark parity: same layout and the same states in both themes, with the dark danger lifted to #FF6B6B for
  contrast.
