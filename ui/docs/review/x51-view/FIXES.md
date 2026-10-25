# `<pd-view>`: review 1 to fixes (review 2 is NOT self-approved)

Review 1 is `REVIEW.md` (not approved: 3 P1, 13 P2, 12 P3). The new contact sheet is `review-2/` (open the `sheet-*.png` files; see its
README). Tests: Vitest `view/src/review1.spec.ts` (and the specs it extends), Playwright Chromium `e2e/view.spec.ts` (the `P*` tests, axe in
both themes, 320 px), `paddock-view` unit tests.

| Id | Change | Test |
|---|---|---|
| P1-1 | `layout.ts` `radialPlace`: the best-connected node is the hub in the middle, the others on rings by distance (another component one ring out), each ring wide enough and far enough that circumscribed circles cannot touch, nodes ordered by their neighbours' direction; the canvas is the bounding box plus padding | `layout.spec.ts` (hub and ring, fitted canvas, no overlap over seven graphs in both layouts and the sample's), `view.spec.ts` e2e P1-1 |
| P1-2 | Status glyph in the box (the tone's own path from `pd:`), box wider by the mark; the status word in the `<title>`, the action's name and the list; the list adds captions and members | `review1.spec.ts` P1-2, e2e P1-2 |
| P1-3 | `pd-needs-ai-card` in `@decryp7/paddock-ui/states`; `pd-needs-ai` (shell/pitwall) is a container over it; the node draws `VIEW_NEEDS_AI` (the app's live component, the repair stays the app's) or the card with the plugin's reason, titled "<feature> needs local AI" | `needs-ai-card.spec.ts`, `review1.spec.ts` P1-3, the unchanged Pitwall specs |
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
