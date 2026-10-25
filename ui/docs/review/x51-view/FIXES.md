# `<pd-view>`: review 1 and review 2 to fixes (review 3 is NOT self-approved)

Review 1 is `REVIEW.md` (not approved: 3 P1, 13 P2, 12 P3). The new contact sheet is `review-2/` (open the `sheet-*.png` files; see its
README). Tests: Vitest `view/src/review1.spec.ts` (and the specs it extends), Playwright Chromium `e2e/view.spec.ts` (the `P*` tests, axe in
both themes, 320 px), `paddock-view` unit tests.

| Id | Change | Test |
|---|---|---|
| P1-1 | `layout.ts` `radialPlace`: the best-connected node is the hub in the middle, the others on rings by distance (another component one ring out), each ring wide enough and far enough that circumscribed circles cannot touch, nodes ordered by their neighbours' direction; the canvas is the bounding box plus padding | `layout.spec.ts` (hub and ring, fitted canvas, no overlap over seven graphs in both layouts and the sample's), `view.spec.ts` e2e P1-1 |
| P1-2 | Status glyph in the box (the tone's own path from `pd:`), box wider by the mark; the status word in the `<title>`, the action's name and the list; the list adds captions and members | `review1.spec.ts` P1-2, e2e P1-2 |
| P1-3 | `pd-needs-ai-card` in `@decryp7/paddock-ui/states`; `pd-needs-ai` (shell/pitwall) is a container over it; the node draws `VIEW_NEEDS_AI` (the app's live component, the repair stays the app's) or the card with the plugin's reason, titled "<feature> needs local AI", whose only action is the app's `VIEW_OPEN_AI_SETTINGS` button (no button and a plain line when the host gives none); the plugin's `setup` ActionRef is never sent | `needs-ai-card.spec.ts`, `review1.spec.ts` P1-3, the unchanged Pitwall specs |
| P2-1 | Badge and link nodes `flex: 0 1 auto`, progress `0 1 14rem`, blocks `1 1 14rem`; row gap 8 px; badge `vertical-align: top` | e2e P2-1 |
| P2-2 | `overflow-wrap: break-word` everywhere (was `anywhere`); status cells, badges and button labels nowrap; the form's chosen text `flex: 1 1 8rem`; review grid items `min-width: 0`; long buttons wrap inside themselves | e2e P2-2, P2-12 |
| P2-3 | heading = Subtitle, subheading = Body strong | e2e P2-2/3 |
| P2-4 | Stale: warning colours, "Reload"; failed: danger icon, "Retry", optional `plugin` input names the plugin, optional `logsLink` + `(openLogs)` | `review1.spec.ts` P2-4, e2e P2-4 |
| P2-5 | Skeleton at the geometry of a view; busy: a thin bar and "Working…" (`role=status`), reduced motion: static bar | `review1.spec.ts` P2-5, e2e P2-5 |
| P2-6 | `primaryNode`: one filled button per view; the review filter is the family `pd-segmented` | `review1.spec.ts` P2-6, e2e P2-6 |
| P2-7 | `pd-view-writes` used by buttons, links, the reason buttons and the gallery's confirmation | `review1.spec.ts` P2-7 |
| P2-8 | Tree row text colour stays; only `pd-view-tone` is coloured; the word is the row's tooltip | `review1.spec.ts`, e2e P2-8 |
| P2-9 | Rust `Column.align` (`Align`), `Column.unit`, `ChartSpec.unit`; TS regenerated, TS mirror validates them; header "Time (ms)", cells end-aligned; charts and the scatter axis use the unit | `paddock-view` tests (round trip, limits), `limits.spec.ts`, `chart.spec.ts`, `review1.spec.ts`, e2e P2-9 |
| P2-10 | `ViewHost.pickPath/pickSecret` resolve `{ value, name }`; the form shows the name through `pd-path-text` | `view.spec.ts`, `review1.spec.ts`, e2e P2-10 |
| P2-11 | Class diagram: stereotype «group», rule, members from the left | `review1.spec.ts`, e2e P2-11 |
| P2-12 | Review list: `grid-template-columns: minmax(0, 1fr)`, `min-width: 0`, wrapping buttons | e2e P2-12 (every element inside the review, 320 px, both themes) |
| P2-13 | `review-2/`: sheets by section, focus, hover, busy, loading shots; tall subjects in 1,800 px slices (the blank stale shot was a clip below the gallery's inner scroll container; the window is now tall enough and the container is scrolled) | the contact-sheet spec |
| P3-1 | Inline-code look for the `code` style | contact sheet |
| P3-2 | Empty icon 32 px | `review1.spec.ts` |
| P3-3 | Progress track `--pd-surface-3` with an inset border | contact sheet |
| P3-4 | Scatter series coloured from the tokens (one series: the accent); chart, table, matrix and scene frames use the card radius | `chart.spec.ts` |
| P3-5 | Back edge is a curve like the others; labels sit beside the line (`anchor`) | `layout.spec.ts` |
| P3-6 | "Show as a list" is a footer inside the frame with the family chevron | `review1.spec.ts` |
| P3-7 | Matrix frame fits its content; cell actions in text colour on tinted cells, accent only on neutral ones | contact sheet |
| P3-8 | The unknown kind is a muted suffix and the tooltip | `view.spec.ts`, `review1.spec.ts` |
| P3-9 | The gallery's confirmation: Cancel left of the action, the same "Changes data" badge (real hosts use an OS window, rule 17) | contact sheet |
| P3-10 | `PdViewScroll`: a scroll region is a tab stop only while it overflows | `review1.spec.ts`, e2e P3-10 |
| P3-11 | The name field's placeholder is "e.g. v1.5" | contact sheet |
| P3-12 | Signal Forms exception written down (code comment, `docs/plans/shared-next.md` X51); path, secret and toggle fields are marked touched | `review1.spec.ts` |

Not done: a spike of Signal Forms for dynamic fields (scheduled before Pacenotes, see the plan).

## Review 2 (0 P1, 6 P2, 9 P3) to fixes

The new contact sheet is `review-3/` (every section captured in one run from one commit, named in its README). Tests: Vitest
`view/src/layout.spec.ts`, `route.spec.ts`, `review1.spec.ts`, `chart.spec.ts`; Playwright Chromium `e2e/view.spec.ts` (the `R2` tests).

| Id | Change | Test |
|---|---|---|
| P2-1 | Edges are routed around boxes: `route.ts` (`pathCrosses`, `roundedPath`, `flattenPath`). A back, sideways or long forward edge runs in a lane right of every box and through the free space between rows (exit and entry by the right side, or by the bottom and top, whichever is clear); a radial edge is straight, then bows to each side, then goes round the outside of all boxes; the canvas grows to hold what bows out | `layout.spec.ts` "an edge never crosses a box" over six graphs in both layouts and the sample's graphs and class diagram, `route.spec.ts`, e2e R2 P2-1 (the real SVG paths) |
| P2-2 | Matrix: row headings `min-width: min(8rem, 40vw)`, column headings `6rem` | e2e R2 P2-2 (320 px) |
| P2-3 | One rule for every region that scrolls or clips: the ring is drawn inside (`outline-offset: -2px`): sort headers, "Show as a list", the three scroll regions (`SCROLL_CSS`), the review filter | e2e R2 P2-3 focuses every focusable element of the view and checks its ring box against every clipping ancestor |
| P2-4 | `VIEW_HOVER` (9% of the text colour) and `VIEW_HOVER_FILL` for table rows, tree rows and drawing boxes (the family `--pd-hover` is 5%); the contact sheet puts the pointer on the target after scrolling and blurs the focus first | e2e R2 P2-4 (the fill changes on hover, no focus) |
| P2-5 | `review-3/`: every section recaptured in one run, the commit in its README; the invalid and loading shots are the view only | the contact-sheet spec |
| P2-6 | A dev-mode check in `PdViewNode`: a `needsAi` node drawn with neither `VIEW_NEEDS_AI` nor `VIEW_OPEN_AI_SETTINGS` reports `NEEDS_AI_WITHOUT_REPAIR` to the `ErrorHandler`; the no-opener hint is "Set up local AI in this app's settings." Hosts that show `<pd-view>`: the gallery (provides the opener; the shell demo and the workbench demo do not show views) | `review1.spec.ts` P1-3 (reported once, not when either is provided), e2e "needs local AI" |
| P3-1 | Class members: 8 units above the first and under the last | `layout.spec.ts`, contact sheet |
| P3-2 | The scatter's ticks are plain numbers; the unit is in the title once | `chart.spec.ts`, e2e R2 P3 |
| P3-3 | The secret's name sits 4 units from its lock (measured; the 20 px of the old shot was not reproducible in the layout) | e2e R2 P3 |
| P3-4 | 4 units between a table cell's status mark and its word | e2e R2 P3 |
| P3-5 | The review filter scrolls on one row instead of wrapping | e2e R2 P3 |
| P3-6 | Under 360 px of view width the "Changes data" badge is the pencil and "Edits" (the full words stay for assistive technology) | e2e R2 P3 |
| P3-7 | A drawing wider than the pane opens at its middle | e2e R2 P3 |
| P3-8 | A region that scrolls fades the side that continues (`data-more`, `PdViewScroll`) | e2e R2 P3 |
| P3-9 | Tree hover is a pointer-only shot | the contact-sheet spec |

## Review 3 (0 P1, 1 P2, 3 P3) to fixes

The new contact sheet is `review-4/` (one run from one commit, named in its README): NOT self-approved. Tests: e2e `view.spec.ts` ("R2 P2-3 and R3 P2-1"), Vitest `layout.spec.ts`.

| Id | Change | Test |
|---|---|---|
| P2-1 | The sort button, the "Show as a list" summary and the tree row have 4 units of inline padding, so the inset ring is clear of the text; the sort button has the matching negative margin, so header text stays aligned with the cells | e2e: the ring check also asserts that an inset ring band does not overlap the text box of its element (a Range over the element's contents); it failed on the old CSS for the four sort headers, the list summary and the tree row |
| P3-1 | The drawings caption says "back edge in an orthogonal lane outside the boxes" | contact-sheet spec |
| P3-2 | Sample node `g3`: a second radial graph with two spoke-to-spoke edges (they bow); the canvas grows to hold them | `layout.spec.ts` (bows, crossings over `g3`), e2e radial checks |
| P3-3 | The contact sheet moves the pointer off the view before every shot that is not about hover | the contact-sheet spec |
| P3-4 | Still open, as planned: the Signal Forms spike for dynamic fields before Pacenotes | none |

Found while fixing, not in the review: `SCROLL_CSS` makes every scroll region `position: relative`, because the visually hidden text of a table or matrix (`position: absolute`) escaped the region and widened the whole page by 3 px at 320 px (the e2e "no horizontal scroll at 320 px" test was red on the integration branch); that test and P2-12 now treat the review filter as a region that scrolls by design. Known, for the next review: in the new radial sample the label "tokens" touches the right edge of the hub box (`graph-radial-bows`).

Rule 40 (from the workbench review 4): `model.ts` no longer has a status to glyph map. `TONE_STATUS` names the status of a tone and `toneGlyph` / `ICON_GLYPH` take the glyph from `statusGlyph` (`@decryp7/paddock-ui/icons`), so a `bad` tone and the `error` icon draw the circle-x and `warn` / `warning` the triangle. Test: `model.spec.ts` "has no status glyph map of its own", and the icons lint (`status.spec.ts`) over every template. `TONE_GLYPH` is gone from the public API (`TONE_STATUS`, `toneGlyph` replace it). Recaptured in `review-4/`, both themes.
