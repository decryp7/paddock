# Independent design re-review: pd: icons and controls (round 2)

Reviewer: independent agent (rule 22), 2026-10-24. I did not write the first review. This is a read-only review; I
changed no code.

Inputs: `contact-sheet.png` and `parts/*.png` (2x device pixels, both themes, Debrief and Pitwall accents, the graphite
Settings window). Fixes reviewed: branch `shell/ui-review-fixes` (`4f242b0`, `8dd0b2a`, `7bf1bef`, `edb7521`). References:
`docs/design-guide.md` on this branch (§4, §6) and UI review 2 patterns. All measurements come from the PNG pixels.
1 CSS px = 2 device px. Contrast uses the WCAG formula on the sampled hex values.

Counts: of the 8 earlier P1s, **7 fixed, 1 partly**. Of the 12 earlier P2s, **9 fixed, 2 partly, 1 not fixed**. Of the
11 earlier P3s, **5 fixed, 1 partly, 5 not fixed**.
New: **0 P1, 5 P2, 6 P3**.

Verdict: the P1 rendering and contrast defects are fixed, and I confirmed each one in the pixels. The set now has one
real line weight, and the focus, selection and switch states meet 3:1 everywhere. What remains is polish: a popup that
does not match its trigger's width, inputs with no hover state, an icon that turns into a blob at 12 px, and one
alignment that the fix did not reach.

---

## Earlier findings: verdicts

### P1

| # | Finding | Verdict | Evidence (pixels) |
|---|---|---|---|
| P1-1 | Dots draw as hollow rings | **Fixed** | `alert` and `info` dots, the four `keyboard` keys and the three `more` dots are solid ink (#14171C or #ECEEF1 at the centre) at 16 and 24 px in both themes. They are now filled `<circle>`s (`PD_ICON_DOTS`). |
| P1-2 | "!" merges into the base of `alert` | **Partly** | At 16 px, a column through the mark gives: stem 7 dev px, gap 2, dot 2, gap 2, base 3. The dot is separate from the base, but only by 1 CSS px. At **12 px** (the states row, and the size the error helpers use) the "!" fills the triangle and the glyph reads as a solid blob. The same happens to `info` at 12 px. See new N-P2-3. |
| P1-3 | Stroke differs between `pd-glyph` and the mat-icon literal | **Fixed** | `minus`, measured at 16 and 24 px for both the `pd-glyph` row and the literal row, in light and dark: always 3.00 dev px = **1.5 CSS px**. `vector-effect: non-scaling-stroke` is on both paths. |
| P1-4 | Themed select taller than inputs | **Fixed** | Input, native select, `pd-select` (rest, focus and disabled) and the "Same height" input are all 56 dev px = **28 px** (Standard), on the page and on the card. |
| P1-5 | Focus indicator below 3:1 | **Fixed** | The focus indicator is a solid 2 px ring with a 2 px offset. Debrief light #1D5FD1: **5.3:1** on the page, 5.2:1 on the card, 5.8:1 on white. Debrief dark #5B9BFF: **6.9 / 6.0:1**. Pitwall dark #A58BFA: **7.0 / 6.1:1**. Graphite Settings: dark **7.0:1**, light **6.5:1**. Design guide §6 is corrected on this branch. |
| P1-6 | Selected segment invisible or inverted in dark | **Fixed** | The selected segment has an accent tint, a 1 px accent ring (#1D5FD1 light, #5B9BFF dark; ≥5:1 against the track) and a 600-weight label. The tint alone is only 1.23 to 1.25:1, but the ring carries the selection, and it looks the same in light and dark. |
| P1-7 | Path text cuts the file name and extension | **Fixed** | One ellipsis per path; `.log` is always visible: `/Users/lin/Library/App… /builder.service.log`, `C:\ProgramD… 2026-10-24.log`. The tail no longer shrinks. The way long names are cut is a new P2 (N-P2-4). |
| P1-8 | Pitwall dark toggle and checkbox fill 2.95:1 | **Fixed** | `--pd-accent-control` is #A58BFA in dark: **6.1:1** on the card and 7.0:1 on the page, with a dark handle (6.9:1 on the track). Debrief light #2F6FE0 is 4.2 to 4.3:1. The off track draws its outline in the control border colour (#6B7380 / #838B98), and the handle is muted at 6.3 to 7.3:1 against the track. |

### P2

| # | Finding | Verdict | Note |
|---|---|---|---|
| 1 | `settings` blob at 16 px | **Fixed** | Six teeth and a hub of r=3. It reads cleanly at 16 px and fills the same optical box as `clock`. |
| 2 | Native and themed select carets differ | **Fixed** | Both draw the same chevron. |
| 3 | Unchecked checkbox is the browser default | **Fixed** | Themed: control border #6B7380 / #838B98 on surface-1, at 3:1 or better. "Show advanced" in Settings matches. |
| 4 | Lone checkbox sits on the label line | **Not fixed** | "Start with the system" is still at label height, 42 CSS px above the control line. The new rule `.pd-field > .pd-check { align-self: end }` never applies: in the gallery, `.pd-check` sits directly in the row, not inside a `.pd-field`. See N-P2-5. |
| 5 | Invalid input has no icon | **Fixed** | The alert glyph and the danger text are present, and so is `aria-describedby`. The glyph is drawn at 12 px, though; see N-P2-3. |
| 6 | Settings nav bar looks like a "(" | **Fixed** | The bar is straight, 2 px wide and inset 3 px top and bottom, with a tint behind the item (1.28:1 dark, 1.24:1 light; the bar is 7.0 / 6.5:1). |
| 7 | No visible keyboard-active option | **Fixed** | `cdk-option-active` gets the hover fill. The sheet shows "Comfortable" active while "Compact" is selected (600 weight + check), so the two states can be told apart. |
| 8 | Global `:focus-visible` rewrites the radius | **Fixed** | The line is removed. Focus rings follow each control's radius (buttons, inputs, checkbox). |
| 9 | Path text not in the mono face | **Fixed** | Mono 12 px by default; `plain` opts out. |
| 10 | Tail starts mid-word | **Partly** | The tail now starts at a word (`2026-10-24.log`), but the start of the name (`Builder.Service.`) is now dropped, while the less useful `C:\ProgramD` is kept. Commit `7bf1bef` says "the file name … never cut", which is not what the pixels show. See N-P2-4. |
| 11 | Contact sheet missing states; crop clipped | **Partly** | The states strip, the mat-icon row, switches, checkboxes and the full open select are all added. But the open-select part is a 152 × 174 CSS crop that cuts through the neighbouring input, the checkbox and the "K" key, and the sheet scales it up about 6x, so it looks blurred and out of scale next to the other cells. See N-P3-5. |
| 12 | `kbd` keys touch | **Fixed** | There is a 2 px gap, and the keys sit on the baseline. |

### P3

| # | Verdict | Note |
|---|---|---|
| 1 `stop` too small | **Fixed** | 11.5-unit square. |
| 2 `globe` dense at 16 | **Not fixed** | Same path. Still busy at 16 px and illegible at 12. |
| 3 `keyboard` keys sub-pixel | **Partly** | The keys are solid now. At 16 px they are 1.2 px dots, under the 1.5 px stroke (see N-P3-1). |
| 4 Safe margin doc | **Fixed** | The docblock states 2.5 px for the wide glyphs. |
| 5 Empty-state folder metaphor | **Not fixed** | It is still a folder for "No runners yet". This is acceptable in the shared demo; each app must use its own domain glyph. |
| 6 Popup wider than the trigger | **Not fixed, and worse in Settings** | In the controls shot the popup is still 5 CSS px wider, on the right only. In Settings it is now **68 CSS px narrower** than the trigger: N-P2-1. |
| 7 `sm` button top-aligned in its row | **Not fixed** | "Compact" spans dev rows 100 to 155, while the 32 px buttons span 100 to 163: top-aligned, not centred. |
| 8 Settings select widths differ | **Fixed** | Theme, Motion and Density are all 240 CSS px. |
| 9 Chip remove glyph too heavy | **Fixed** | The glyph is now muted (#4F5764), the same colour as the label. |
| 10 "Ctrl" shown on macOS | **Not fixed** | The demo still shows "Ctrl K". |
| 11 Gap after the head ellipsis | **Not fixed** | In the mono face it is wider than before: "App…␣/builder". The ellipsis takes a full cell with its dots on the left. |

---

## New findings

### P2

**N-P2-1 Settings: the open select list is 68 px narrower than its trigger.**
The Theme trigger spans dev x 480 to 959 (240 CSS px). The list spans 480 to 823 (172 CSS px), so it ends in the
middle of the field and the lower half of the helper text shows beside it. In the controls shot the list is 5 px
*wider* than its trigger. The list's width therefore follows its content, not the trigger. It looks unfinished in the
window people open most.
*Fix:* in `pd-select`, set the overlay width to the trigger's width (`[cdkConnectedOverlayWidth]` = the trigger's
`getBoundingClientRect().width`, or `minWidth` = that width and `maxWidth` = the larger of that width and 20rem). Add a
browser test: open the list and assert `|panel.left − trigger.left| ≤ 1` and `panel.width ≥ trigger.width − 1`, in the
gallery and in the Settings window.

**N-P2-2 Inputs (and the native select) have no hover state.**
The "hover" input is pixel-identical to "rest" (border #6B7380, fill #FFFFFF in light; #838B98 and #15171B in dark).
The sheet labels a hover column that shows nothing. Design-guide checklist item 18 says hover is defined for every
control, and the themed select trigger does change on hover (to surface-2). So the two selects behave differently.
*Fix:* `.pd-input:hover:not(:disabled, :focus-visible), .pd-select:hover:not(:disabled)` → `border-color:
var(--pd-text-muted)`. That colour is what Material already uses for the unselected hover of checkboxes, so the
controls agree. Use the same rule on the `pd-select` trigger instead of (or as well as) the surface-2 fill.

**N-P2-3 At 12 px, `alert` and `info` turn into blobs, and that is the size the error helper uses.**
A 1.5 px non-scaling stroke at 12 px is 3 grid units: the "!" and the triangle merge, and the "i" touches its ring
(states row, both themes). Both the gallery Port error and `pd-setting-field` draw `alert` at `[size]="12"`. The
design guide says inline icons beside 12 px text are **14 px**.
*Fix:* draw error and status glyphs beside 12 px text at 14 px (gallery and `setting-field.ts`). Add a lint or test
that rejects `pd-glyph name="alert|info"` below 14. If a 12 px status glyph is ever needed, give `alert` and `info` a
small-size variant: a shorter stem, a dot of r ≥ 1.25 on the grid, and a 1.25 px stroke.

**N-P2-4 Path text: a long file name loses its distinctive start, while the drive or directory survives.**
`C:\ProgramData\Builder\Logs\Builder.Service.2026-10-24.log` in a box about 27 characters wide renders as
`C:\ProgramD… 2026-10-24.log`. "Builder.Service" is the part a person looks for, and it is gone. The box could have
shown `C:\…\Builder.Service.2026-10-24.log` or `C:\…Service.2026-10-24.log`.
*Fix:* split into three parts: the directory head (shrinks first, down to the root such as `C:\` or `/Users/x`), the
name head (shrinks second) and the name tail (the extension plus the last word, never shrinks). With CSS, use flex
shrink weights such as 1000 for the directory and 1 for the name head. Add a regression test with this path in a
240 px box that asserts the visible text contains `Service.2026-10-24.log`. Correct the commit and docblock claim
that the name is never cut.

**N-P2-5 The lone-checkbox alignment fix does not reach the case it was for.**
`.pd-field > .pd-check { align-self: end; … }` matches only a checkbox inside a `.pd-field`. The gallery, and any
form row that sets a checkbox next to fields, puts `.pd-check` directly in the row, which is what the earlier finding
described. Inside a `.pd-field` grid, the rule only moves the checkbox within that field's own row.
*Fix:* one shared pattern, e.g. `.pd-field.pd-field--check` with an empty, `aria-hidden` label line (same height as
`.pd-field > label`) followed by the check, set to `min-height: var(--pd-control-height); align-items: center`. Use
it in the gallery and document it. Add a layout test that the checkbox's centre y equals the centre y of the input
next to it, ±1 px.

### P3

**N-P3-1 Dots scale with the icon size, but strokes do not.** The strokes are fixed at 1.5 px on screen. The dots
are grid circles, so the `alert` dot is 1.2 px at 16 px and 1.8 px at 24 px, and the `more` dots are 2 px at 16 px and
3 px at 24 px. At 16 px the "!" dot is lighter than its stem, and at 24 px `more` looks heavier than its neighbours.
*Fix:* size the dots in screen px, like the strokes: r = max(grid r × size/24, 0.75 × stroke). `pd-glyph` knows the
size; for the mat-icon literal, register 16 and 24 variants or draw the dot as a non-scaling round cap (`M12 15.75h0.01`
with `vector-effect`), which has the stroke's width at every size.

**N-P3-2 Pitwall dark has two violets in one row.** Primary buttons use the fill #7546E0 with white text, while
switches, checkboxes and radios use #A58BFA with dark ink. Side by side, two accent violets compete, which goes against
"at most one saturated element". *Fix:* either accept this and document it (§6 already allows it), or in dark use the
graphite pattern for the primary too (`fill-dark` = the accent with dark ink), so that a Pitwall screen has one violet.
The same applies to every app accent in dark.

**N-P3-3 Light secondary hover goes to white.** Rest is surface-2 #F0F2F5 and hover is #FFFFFF (1.12:1). On a white
surface-1 card, the hovered button's fill disappears into the card. *Fix:* hover = `--pd-hover`, the same overlay
ghost buttons use, so it darkens on every surface.

**N-P3-4 The "card" in the states sheet is surface-2, which is 1.03:1 against the light page.** There is no border, so
in light the "page vs card" comparison shows nothing. Real cards are surface-1 (white) with a hairline (§9). *Fix:*
render the second strip on a surface-1 card with a hairline (and keep the surface-2 strip if insets matter), so that
white-on-white inputs and the secondary hover are actually checked.

**N-P3-5 The open-select part is a tiny crop scaled up about 6x in the contact sheet.** It is blurry, it cuts the
neighbours mid-element (an input edge, the checkbox, "K"), and it is out of scale with every other cell. *Fix:* capture
the open select inside the full controls card at the sheet's scale (raise the crop height to fit the list), or give
the cell a fixed 1:1 scale.

**N-P3-6 The gallery's bottom control row has four baselines.** The chips, the segmented control, the tabs and the
"Press Ctrl K" text all sit at different heights (text baselines at dev y ≈ 205, 211, 221 and 205). *Fix:* `align-items:
center` on `.gal-row`, or put the tabs in their own row; they belong to a pane header, not a control row.

Also seen, not worth a finding: `setting-field.ts` gives `.pd-input:focus-visible` a 1 px offset and the raw accent
colour, instead of the shared `--pd-focus-ring` with a 2 px offset. It still passes 3:1, but it is a second
definition of the same ring. Remove it and use the base `:focus-visible` rule.

## Checked and fine
- Line weight: 1.5 CSS px at 16 and 24 px, the same through `pd-glyph` and `mat-icon`, in light and dark.
- Control heights: equal (28 px Standard) across inputs and both selects, in every state.
- Focus: one solid ring with a 2 px offset on buttons, inputs, selects and the checkbox, in all three accents, at 5.2 to 7.0:1.
- Switch, checkbox and radio: the on state is 4.2 to 7.0:1 on the page and on the card; the off state uses the control
  border; disabled is clearly lighter.
- Tabs, `kbd`, chips, the error helper, the native select chevron and the Settings select widths all match.
- Settings window (graphite): a straight nav bar with tint, the open trigger keeps its ring, the list shows the active
  and selected options separately, and the themed checkbox is used.
