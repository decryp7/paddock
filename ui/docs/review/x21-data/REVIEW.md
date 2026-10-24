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

---

# Review 2 (re-review after fixes)

Reviewer: a second independent agent (rule 22), 2026-10-25. I did not build or review these parts before. Read-only: no
code changed. Inputs: `contact-sheet.png` and the 34 per-part PNGs (2x device pixels), commit `bd43623`, code read only to
explain what the pixels show. Measurements are from the PNG pixels (1 CSS px = 2 device px); contrast is WCAG; CVD is
Machado 2009 at severity 1.0 on colours **sampled from the rendered donut**, ΔE76 in CIELAB.

**Counts: 2 P1, 5 P2, 9 P3 (new).** All three earlier P1s are fixed. Two new P1s block approval: the middle ellipsis now
cuts paths that have room to spare (a regression from the P2-4 fix), and the donut breaks at 320 px.

## Verdicts on Review 1

| # | Verdict | Evidence in the pixels |
|---|---|---|
| P1-1 320 px | **Fixed** | `*-table-320`: Name is the only data column and every name shows in full ("Trace viewer", "Net capture"). "…" sits in a ~24 CSS px track at the edge (dots at 213–219 of 240 CSS px). Toolbar: icon-only Columns, the count hidden, the filter at 8rem. No horizontal overflow. |
| P1-2 menu focus | **Fixed** | `*-menu-keyboard`: a 2 CSS px ring on the active item. Ring vs item fill: 5.25:1 light (#1D5FD1 on #F3F3F4) and 4.72:1 dark (#5B9BFF on #2D3137). Ring vs menu: 5.8:1 and 5.5:1. |
| P1-3 donut CVD | **Fixed** | Ring order is Logs (1), Models (2), Cache (6), Other (5). The sampled slices match the tokens exactly. The worst ring-adjacent pair is 68.2 (dark tritan, 2↔6); the wrap-around pair 5↔1 is ≥ 85 under every type. |
| P2-1 density | **Fixed** | Rows 32 CSS px (64 device between hairlines), header 28 px (54 + 2 hairlines), both themes. |
| P2-2 sort affordance | **Partly fixed** | End-aligned columns now put the chevron before the label ("˄ Size (KiB)"). The hover/focus hint is a single ˅, which is the same glyph as a descending sort: see N4. |
| P2-3 menu | **Fixed** | Surface-3 (#FFFFFF light, #22262C dark), a 14 px icon slot on every item ("Enable"/"Disable" line up), and a hairline above "Remove…". |
| P2-4 path join | **Join fixed, new defect** | The join is now one string ("…/plugin-1.bin") and looks the same in every row. But the head length is wrong: see N1 (P1). |
| P2-5 stacked corners | **Fixed** | Only the top segment has its corners rounded (Mon, Wed, Thu, Fri, Sun). Tue and Sat are rounded on their only segment. No notch. |
| P2-6 unit collision | **Fixed** | The unit moved into the title. The legend sits under the title, with clear space above the plot. |
| P2-7 titles | **Fixed** | Every chart has a visible 13/600 title. See N9 for the wording. |
| P2-8 donut | **Partly fixed** | Every slice is listed with its value and share. The centre total is set up in the code but is not drawn (N3), and the layout breaks at 320 px (N2). |
| P2-9 states | **Mostly fixed** | New frames: focused row, focused header, descending sort, selected row (accent tint #DFE8F9 / #1F293B with a 2 px bar), collapsed group, empty table with a next step, table loading and error, chart loading and error, field focus, disabled field, charts at 320 px. The hover frame now highlights a row (#F3F3F4, 1.11:1, fine for hover). The charts-320 frame is broken, though: see N7. |
| P3-1 Columns glyph | Fixed | |
| P3-2 separator | Fixed | |
| P3-3 first column | Fixed | Name is drawn at medium weight. |
| P3-4 two blues | Fixed | The single-series line and area both use the accent. |
| P3-5 lone value | Not visible | The demo has no lone value, so the frames cannot show it. The spec covers it. |
| P3-6 palette drift | Guide synced (#494FDB) | Dark 1↔6 is still open. Recommendation below. |
| P3-7 "…" at rest | Open | Measured 1.93:1 light, 2.22:1 dark. Recommendation below. |
| P3-8 error order | Fixed | The error now sits directly under the control, and the hint hides while it shows. |
| P3-9 short field | Fixed, new defect | Port is now short. Its error wraps: see N6. |

## New P1

### N1: The middle ellipsis cuts paths that have room, and never grows back (`*-table-rest`, `*-table-virtual`, all table frames)
The source paths are all about the same length (`/Users/alex/Library/Application Support/Debrief/plugins/<kind>/plugin-N.bin`),
yet in one column of fixed width the shown heads range from nothing ("…/plugin-4.bin", "…/plugin-9.bin") to 37
characters ("/Users/alex/Library/Application Support…/plugin-1.bin"). In `*-table-virtual`,
`/data/records/0002/entry.bin` (28 characters) shows as "/data/recor…/entry.bin" in a Location column with about 400
CSS px of empty space. A user sees "…/plugin-4.bin" and has lost the information the column exists to show. This is
worse than the ragged join it replaced.
Cause (code read to explain the pixels): `pd-middle-text` is `display: block` inside `.pd-cell { display: flex }`
with no `flex` value, so it is a flex item sized to its own content. Its `ResizeObserver` measures the width of the text
it is already showing, and `middleTruncate` fits to that width. Once a row is cut during an early, narrower layout (or
while hover actions take space), the box shrinks to the cut text and stays there. The width only ratchets down.
*Fix:* in the cell, give the host `flex: 1 1 0; min-width: 0` (or `width: 100%`) so it measures the space it was given,
not its own text. Tests: (a) widening the container shows the full value again; (b) values of equal length in one column
get heads within ±1 character; (c) a value that fits is never cut (the virtual demo).

### N2: The donut breaks at 320 px (`*-charts-320.png`)
The legend (`right: 4`) is drawn on top of the ring (centre at 28%). "Logs", "Models", "Cache" and "Other" are printed over
the slices, and their first letters are hidden behind them. The ring's left edge runs past the card border. This fails
rule 18 and is unreadable.
*Fix:* lay the donut out by the chart's own width. Below about 400 px, put the ring on top (centre `['50%', '38%']`) and
the list under it (`legend.bottom`, `orient: 'vertical'`), or switch to the parts bar. Above that width, place the ring
centre from the measured legend width instead of a fixed percentage. Add a Playwright check at 320 px that the legend's
box and the ring's box do not overlap and that both lie inside the card.

## New P2

### N3: The donut's centre total is not drawn
`options.ts` builds a `graphic` text with the total ("88 GB"), but the ring's centre has 0 pixels that differ from the
background, in both themes. The likely causes are that the graphic is dropped when the option is applied, or that it is
placed at `left: '24%'` while the ring centre is at 28%. Even when it is drawn, it would sit off-centre. *Fix:* draw the
total where the ring centre actually is (one source for both positions), and add a spec that the option carries the
total and a screenshot assertion that the centre is not empty.

### N4: The sort hint looks like a descending sort (`*-table-focus-header`, `*-table-selected`)
An unsorted "Kind" with focus shows a single ˅ in #5F6775 next to a #4F5764 label. Those colours are nearly the same,
and the glyph is the one the sorted "Size (KiB)" uses for descending. A user cannot tell "can sort" from "sorted
descending". *Fix:* use a two-way (up and down) glyph in `--pd-text-subtle` for the hint. Keep the single chevron in
`--pd-text` for the active sort only.

### N5: The table's loading, empty and error states give wrong counts, and the columns jump
- While loading, the toolbar says "0 rows". That is false: the count is not known yet. Hide the count, or say "Loading…".
  In the empty state, "0 rows" above "No plugins installed" is redundant: hide the count when the table is empty.
- With no rows, the action track disappears, so the columns move. In the same 504 CSS px frame, Kind starts at 190 CSS px
  when loaded and at 286 when loading, empty or in error, and Size moves from 393 to 489. The table jumps sideways when
  data arrives. *Fix:* size the action track from the declared actions, not from the rows present.

### N6: The short field's error wraps (`*-forms.png`)
`pd-field--short` caps the whole field at 8rem, so "Enter 65535 or less." breaks over two lines, and the alert glyph
centres between them. *Fix:* apply the short width to the control only, and let the message use the full column. Align
the glyph with the first line of the message (`align-items: start` plus a small top offset).

### N7: The charts-320 frame cannot be approved
The frame starts with about 250 CSS px of blank space, the first chart is cut off (its title and y-axis top are missing),
the loading card is cut at the bottom, and the error chart is not in the frame. The caption says "donut with its
labels", while the donut is broken (N2). *Fix:* screenshot the chart section's element (not a scrolled clip) at 320 px,
include every chart and state, and re-shoot after N2.

## New P3

1. **Chart error layout.** In the chart, the message wraps to two lines while Retry is parked at the far right, about 50
   CSS px away. The table's error (glyph, one line, Retry beside it, centred) is better. Use the same pattern in both.
2. **Title wording.** "Jobs by outcome per day, jobs" and "Requests per hour, req" stutter. Use the table's
   convention, the unit in parentheses: "Jobs per day by outcome", "Requests per hour", "Latency p95 (ms)", "Disk use by
   kind (GB)". Add the window when there is one ("last 7 days").
3. **Percentages sum to 101%** (48 + 31 + 16 + 6). Use largest-remainder rounding so the shares add up to 100.
4. **No thousands separators.** "5000 rows" and the group counts "1250" should be locale-formatted ("5,000", "1,250").
5. **Legend inset.** The stacked-bar legend starts about 7 CSS px left of the title and the y-axis labels. Align it to
   the title.
6. **Skeleton alignment.** In the end-aligned Size column, the loading bars are start-aligned. Align them to the end, as
   the numbers will be.
7. **Focused header ring hugs the label.** The ring touches the "K" of "Kind". Add 2–4 px of inline padding, or an
   outline offset, on `.pd-sort`.
8. **Disabled field reads as a placeholder.** "Debrief" in Workspace has the same border as an enabled field, and its
   grey text looks like placeholder text. Use the disabled treatment (surface-2 fill, hairline border) so it reads as
   read-only at a glance.
9. **Gallery frame padding (staging only).** In the table frames, the filter box and the "0 rows"/"5 rows" count touch
   the demo frame's border (about 1.5 CSS px). Pad the gallery frame so the sheet shows the component as an app will.

## Questions for the design-guide owner: recommendations

### Dark palette slot 1 ↔ 6 (deutan ΔE 9.0, protan 10.8, tritan 16.5)
Slots 1 and 6 are not the only weak pair. Under protan or deutan, the dark palette also has 4↔8 at 9.3, 2↔7 at 13.8,
1↔4 at 14.4, 2↔5 and 3↔8 at 16.5, and 5↔7 at 19.8. No 8-colour categorical palette passes all 28 pairs. Still,
1↔6 is the worst pair among the first six slots, and both are blues, so they are the most likely to appear together.
**Recommendation:**
1. Darken and push dark slot 6 toward violet: **#7A70FF** (L\* 55, 4.8:1 on #15171B). Its minimum ΔE against every other
   dark slot is 34 under protan/deutan (25 tritan, 55 normal), up from 9.0, and it stays a "periwinkle" that sits
   beside the light value #494FDB.
2. Write the scope of the rule into the guide (§6): ΔE ≥ 20 is required for order-adjacent and ring-adjacent pairs.
   Any chart with 5 or more coloured series must also be keyed without colour (direct labels, or hover isolation with
   the name in the tooltip), and series beyond 6 fold into "Other" (neutral).
3. Make the palette test enforce both: all adjacent pairs ≥ 20, and slots 1–6 pairwise ≥ 20 under protan and deutan.

### "…" row button at 40% at rest (1.93:1 light, 2.22:1 dark)
It is the only visible sign that a row has actions, for anyone who does not hover first (touch, keyboard users scanning,
and new users). SC 1.4.11 asks 3:1 of the visual information needed to identify a control. The right-click /
Shift+F10 path does not make the visible button exempt. **Recommendation:** drop the opacity approach and give the
resting button its own colour token at ≥ 3:1, e.g. `--pd-row-action-rest` = `--pd-text-subtle` mixed at **65%**
(measured: 3.19:1 light, 3.80:1 dark). This still reads clearly as secondary next to 7.5:1 body text. Hover, focus and
open stay at 100%. Update the guide's "40% at rest" line, and add a token contrast test.

## Verdict
**Not yet approved.** The three original blockers are fixed, and density, menus, stacked bars, titles and state
coverage are now at the family standard. Fix N1 and N2 (each is small and well located), then N3–N7, and re-shoot the
sheet. N1 affects every table in the family that shows paths, so it should be fixed first.

---

# Review 3 (re-review after the review-2 fixes)

Reviewer: a third independent agent (rule 22), 2026-10-25. I did not build these parts or do reviews 1–2. Read-only: no
code changed. Inputs: `contact-sheet.png` and the 34 per-part PNGs at commit `0fe2d36` (merged as `487b228`), the
updated `docs/design-guide.md` (§4 palette scope, §7 tables), and `css/tokens.css` for the token values. Measurements are
from the PNG pixels (1 CSS px = 2 device px). Contrast is WCAG 2.x; CVD is Machado 2009 at severity 1.0, ΔE76 in CIELAB,
on the token hex values (the rendered donut slices match the tokens).

**Counts: 0 P1, 1 P2, 9 P3 (new).** Both review-2 P1s and all five review-2 P2s are fixed in the pixels. One new P2: the
resting "…" drops below 3:1 on a selected row, which is the one rule the new token exists to keep.

## Verdicts on Review 2

| # | Verdict | Evidence in the pixels |
|---|---|---|
| N1 middle ellipsis (P1) | **Fixed** | `*-table-rest`, `-hover`, `-menu`, `-focus-row`, `-selected`: every Location head is "/Users/alex/Library/Application Support/" (40 chars, rows 0–9) or "…Support" (39 chars, rows 10–13, whose tail is one char longer). Spread 1 char, within ±2. The head no longer collapses: no "…/plugin-4.bin" anywhere. The cut is stable when hover/focus actions appear (Journal and Archive rows keep the same head as their neighbours). `*-table-virtual` / `-collapsed`: "/data/records/24/entry.bin" is shown in full; nothing with room is cut. |
| N2 donut at 320 px (P1) | **Fixed** | `*-charts-320`: card spans x 32–510 device px; ring 158–385 (inside, 63 px clear on the left). Ring y 1612–1839; legend starts at 1890, below the ring, no overlap. Same geometry in both themes. The wide frame also stacks (the card is ~330 CSS px, under the 400 px break). |
| N3 centre total (P2) | **Fixed** | "88 GB" is drawn. Its bounding-box centre is (270.5, 1724.5) device px against a ring centre of (271.5, 1725.5): within 1 device px. See R3-P3-1 for the wrap. |
| N4 sort hint (P2) | **Fixed** | `*-table-focus-header`: an unsorted, focused "Kind" shows a two-way ⌃⌄ glyph in `--pd-text-subtle` (#5F6775, 5.1:1 on the header). The active sort is a single chevron in `--pd-text` (#14171C, 16:1; ⌃ ascending in `-hover`/`-menu`, ⌄ descending in `-selected`). The two can no longer be confused. The focus ring now has inline padding around the label (review-2 P3-7 fixed). |
| N5 states (P2) | **Fixed** | No count while loading, empty or in error. Column starts are identical across rest/loading/empty/error at the same width: Kind 507, Version 719, Size end 1086, Location 1110 (display px). Skeleton bars in Size are end-aligned (review-2 P3-6 fixed). See R3-P3-4 for the filter box. |
| N6 short-field error (P2) | **Fixed** | `*-forms`: Port's control is ~8rem, "Enter 65535 or less." is one line, the glyph sits on the first line. |
| N7 charts-320 frame (P2) | **Fixed** | The frame is an element shot: section heading, all four charts, loading and error, nothing clipped. |
| P3-1 chart error layout | Partly | Now centred, glyph + one-line message, but Retry floats ~75 CSS px below the message (R3-P3-3). |
| P3-2 title wording | Fixed (nit) | Unit in parentheses everywhere. "Requests per hour (req)" still repeats itself; "Requests per hour" is enough. No window ("last 24 h") in any title. |
| P3-3 percentages | Fixed | 48 + 31 + 16 + 5 = 100. |
| P3-4 separators | Fixed | "5,000 rows", "1,250". |
| P3-5 legend inset | Fixed | The stacked legend's swatch starts 4.5 CSS px right of the title, 1 px from the y-axis labels' left edge (was 7 px left). |
| P3-6 skeleton | Fixed | |
| P3-7 header ring | Fixed | |
| P3-8 disabled field | Fixed | Surface-2 fill and hairline border in light. In dark the fill is close to the card (see R3-P3-8). |
| P3-9 gallery padding | Fixed | Filter box and count sit inside the frame with clear padding. |
| Dark 1↔6 / slot 6 #7A70FF | **Fixed** | 4.78:1 on #15171B. Slot 6's minimum ΔE against every other dark slot: protan 36.6, deutan 34.3, tritan 25.6 (all vs slot 1). Light slot 6 #494FDB: protan 25.1, deutan 23.5. |
| Adjacent pairs ≥ 20, p/d/t | **Fixed** | Order-adjacent minimum, dark: protan 47.0 (3↔4), deutan 25.7 (3↔4), tritan 28.1 (7↔8). Light: protan 36.7 (2↔3), deutan 29.8 (3↔4), tritan 23.5 (7↔8). Donut ring (1→2→6→5→1), every pair ≥ 70 in both themes under every type. |
| "…" at rest | **Fixed** | Sampled darkest dot pixel on a plain row: #878D98 on #FFFFFF = **3.34:1** light, #6E747F on #15171B = **3.82:1** dark. Matches `--pd-text-subtle` at 75% over surface-1. |

## The two deviations from review 2

**"…" at 75% instead of 65%: accept.** The implementer is right and review 2 was wrong: 65% of #5F6775 over white is
#979CA5, **2.76:1**, which fails the 3:1 the recommendation was meant to reach. 75% gives 3.34:1 light and 3.82:1 dark,
and in the pixels the dots still read clearly as secondary next to the 7.5:1 cell text and the 100% hovered button. The
guide line (§7) states the value and the reason. The only gap is that the mix is computed against surface-1, so it does
not hold on tinted rows: see R3-P2-1.

**ΔE ≥ 20 for adjacent pairs + slot 6 only (first five ≥ 14 pairwise): accept, with one guide gap.** Review 2 itself
observed that no 8-colour palette passes all 28 pairs, and the numbers bear the implementer out: first-five pairwise
minimum is protan 14.4 (1↔4), deutan 16.5 (2↔5), tritan 15.2 (1↔3) in dark, and 17.9 / 15.2 / 14.1 in light. Raising
these to 20 would cost lightness/hue separation elsewhere. The scope paragraph is clear and is enforced by
`tokens.spec.ts` and `ring.spec.ts`. The remaining gap is a chart with exactly **four** series: §8/rule 10 asks for direct
labels only up to 3 series and §4 asks for a non-colour key only from 5, so a 4-line chart may rely on slots 1 and 4 at
ΔE 14.4 under protan, which is weak for 1.5 px lines. See R3-P3-6.

## New P2

### R3-P2-1: The resting "…" falls below 3:1 on a selected row (`light-table-selected`)
`--pd-row-action-rest` is `color-mix(--pd-text-subtle 75%, --pd-surface-1)`, a fixed colour. On the selected row's accent
tint (#DFE8F9) it measures **2.71:1** in light (dark: 3.11:1 on #1F293B, just passing). The selected row is exactly where a
user goes next to act, and the guide line written in this round promises "3:1 or better". *Fix:* show the "…" at 100%
`--pd-text-subtle` (or the hover treatment) on selected rows, as on hovered and focused rows, or mix against the row's
own background. Extend the token contrast test to the selected tint and the group band (surface-2) in both themes.

## New P3

1. **Centre total wraps to two lines.** "88" and "GB" stack inside a ring with room for one line (both widths, both
   themes). It reads as two values. Put it on one line, or make the number the hero and the unit smaller beneath it,
   by intent rather than by wrapping (a fixed `width`/`overflow: 'none'` on the graphic text, or a rich-text style).
2. **Chart loading looks like an empty chart.** `*-charts` and `*-charts-320`: the loading card is four faint hairlines
   and nothing else; at a glance it is a chart with no data. The table's loading state (skeleton bars) is unmistakable.
   Use a skeleton plot (a few muted bars or a muted line shape) or a small "Loading…" label; the empty state must look
   different from loading.
3. **Chart error: Retry is detached.** The message is centred at about a third of the plot, and Retry sits ~75 CSS px
   below it, near the bottom. Group them (Retry 8–12 px under the message, the pair centred vertically), or use the
   table's one-line pattern when the width allows.
4. **The filter box disappears while loading, empty and in error.** The toolbar changes shape when data arrives (the
   filter appears and pushes nothing, but the bar jumps from "Columns only" to "Filter … Columns"). Keep the filter visible
   and disabled while loading; hiding it when the table is empty is fine.
5. **Path truncation keeps the head and drops the part that differs.** Every Location row shows the same
   "/Users/alex/Library/Application Support/" and the file name; the `<kind>` folder, which tells rows apart, is the part
   cut. The middle ellipsis is working as specified (rule 18), so this is a refinement: cut at path-segment boundaries
   and prefer to keep trailing segments (`…/Debrief/plugins/viewer/plugin-1.bin`) when the shared head is long. Worth a
   design-guide note so every app's path column behaves the same.
6. **Guide gap at 4 series.** Rule 10 ("direct labels for <= 3 series") and §4 ("5 or more keyed without colour") leave
   4-series charts on colour alone. Lower the §4 threshold to **4**, which covers dark 1↔4 (protan 14.4). One-line
   guide change plus the matching chart-wrapper default.
7. **Light slot 1↔6 under tritan is 11.7.** Both blues; not adjacent and within the documented scope (slot 6's rule is
   protan/deutan only), and tritanopia is rare, so no change is required. Record the number in the guide's table so the
   next palette pass sees it.
8. **Dark disabled field is faint.** In `dark-forms` the Workspace fill is barely different from the card; the hairline
   carries it. Acceptable; a touch more lift on the dark surface-2 for inputs would match the light version's clarity.
9. **Sheet hygiene.** (a) The "Grouped rows in a 5,000-row virtual viewport" frame shows the Columns button with a filled
   background while every other frame shows it ghost: either a stray hover/focus in the capture or a style difference;
   make it consistent. (b) The caption "Focused row (2 px ring) and a hovered header showing the sort chevron" does not
   match the frame (no hovered header is visible). (c) Demo data: "Record 0001" lives at `/data/records/0/` and is 0 KiB;
   align numbering so the demo does not look like an off-by-one. (d) Design guide §4 line "adjacent pairs delta-E >= 20
   under deuteranopia/protanopia" now sits directly above the fuller scope paragraph that adds tritanopia; merge them.

## What is good (keep)

- Paths: the fix is the right one (the cell measures the room it is given), stable through hover, focus and selection.
- Sorting: the hint/active distinction (two-way subtle vs one-way strong) is now textbook, in both alignments.
- Donut: one layout source for ring, total and legend; it centres to the pixel at both widths, and shares sum to 100.
- States: loading, empty (with a next step), and error keep the columns still and drop the count; table error is the
  model for the family.
- Palette: the scope paragraph turns an impossible rule into an enforced one, and the ring order keeps every
  slice boundary above ΔE 70 under every CVD type.
- Forms: short control, full-width message, glyph on the first line, error before hint, disabled reads as read-only.

## Verdict
**Approved**, on the condition that R3-P2-1 is fixed with its contrast test (a measured number in the test is enough; it
needs no new sheet review). The P3s can follow in normal work; R3-P3-2 (chart loading vs empty) and R3-P3-6 (4-series
guide gap) are the ones I would take before the chart wrapper is adopted by a second app.

## Review 3 verification

Same reviewer, 2026-10-25, against `1f613fb` (merged as `a58e5d0`) and the regenerated sheet. Read-only; pixels measured as
above.

| Item | Verdict | Evidence |
|---|---|---|
| R3-P2-1 "…" on the selected tint | **Fixed** | `*-table-selected`: the selected row's dots are #5F6775 on #DFE8F9 = **4.63:1** light, #8B93A0 on #1F293B = **4.71:1** dark. Plain rows are unchanged at 3.34 / 3.82. `tokens.spec.ts` now checks subtle on the 14% accent tint (the same mix as `--pd-accent-tint`) and on surface-2. |
| Rest mix on surface-2 (2.98:1 light, 3.56:1 dark) | **Accept the argument** | The rest colour never meets surface-2 at rest: `.pd-grid` paints rows on surface-1 whatever panel the table sits in; group bands (the only surface-2 rows) carry no row button (0 differing pixels at the band's right edge in `*-table-virtual`); hover and focus switch the button to `--pd-text`. The spec records 2.98 as information only. If a later variant (zebra rows, a surface-2 table) puts resting rows on surface-2, it must use the selected-row rule. |
| P3-1 centre total | Fixed | "88 GB" on one line at 16 px, centred, both widths. |
| P3-2 chart loading | Fixed | Seven muted ghost bars on a baseline plus "Loading chart…"; it no longer reads as an empty chart. Reduced motion stops the pulse. |
| P3-3 chart error | Fixed | Glyph, message and Retry are one centred group, Retry 8 px under the message. |
| P3-4 filter while loading | Fixed in code, not visible | The gallery's loading/empty/error tables are `[filterable]="false"`, so the frames were never showing the component hide its filter (my finding came from the demo). The component now keeps the filter, disabled, while loading or empty (`table.spec.ts`). No frame shows it; fine. |
| P3-5 path cut | Fixed | Every Location row is "…/Debrief/plugins/<kind>/plugin-N.bin": the part that tells rows apart is kept, cut at segment boundaries, one join in every row, stable on hover/focus/selection. Full values still show where they fit (`*-table-virtual`). Optional later: keep the root segment too ("/Users/…/Debrief/…") when the column has room, which would also use the ~25% of the column now left empty. |
| P3-6 4-series key | Fixed | Guide §4 and rule 10 say "4 or more"; `pd-chart` sets `emphasis.focus: 'series'` from 4 series. |
| P3-7 light 1↔6 tritan | Fixed | Recorded in the guide as a known weak pair. |
| P3-8 dark disabled field | Fixed | The Workspace fill is now visibly lifted from the card in `dark-forms`. |
| P3-9 sheet hygiene | Fixed | Columns button is ghost in every frame (the pointer is parked); the focus-row caption matches; records now map 0001 → `/data/records/1/`; the guide's palette lines say which one supersedes. Titles carry the window ("last 7 days", "last 24 h"). |

No new findings: no regressions in the table, chart, or form frames in either scheme.

**Final verdict: Approved.** All P1s and P2s from reviews 1–3 are fixed in the pixels, and the two deviations from review 2
are sound and documented in the design guide.
