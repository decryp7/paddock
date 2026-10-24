# Shell demo "pages" frame: independent visual review, round 3 (rule 22)

Reviewed 2026-10-24 from the PNGs in this folder only (no code). Specs: `docs/design-guide.md`,
`briefs/ui-review-2-patterns.md`, `briefs/design-review-drawings-overview.md`. Measurements are from the @2x images,
given in CSS px.

The folder holds 22 PNGs (11 states x 2 themes), not 24. All 22 were checked.

Overall: clean. Every REVIEW-2 finding is fixed. There is no P1 or P2. Three small P3s are left, all optional polish:
one is a demo-data count and two are spacing.

## REVIEW-2 findings

| # | Status | Evidence |
|---|---|---|
| N1 | Fixed | focus-toggle-dark and -light both show the jobs item (spinner + "1"), as do narrow-* and focus-more-*. The status bar is the same in all six 320 px shots. |
| N2 | Fixed | The chip says "stopped" and the popover title says "The search index stopped: a segment is damaged." That is one state word plus its cause. The primary button is "Reset search index", and the body gives the consequence and what the reset does. |
| N3 | Fixed | rail-open-* and narrow-open-*: the title bar is clean above the drawer, and its hairline runs unbroken across the drawer. |
| N4 | Fixed | The spinner has a faint full track under the arc and reads as a spinner, not as a "C", in both themes. |
| N5 | Fixed | narrow-*: the warning icon sits on the first line of the wrapped warning. |
| N6 | Fixed | focus-nav-* focuses "Overview". focus-nav-selected-* focuses the selected "Runners". The ring is drawn whole inside the 216 px sidebar and is not clipped. |
| N7 | Fixed | focus-more-*: the ring now fits inside the 24 px bar, with about 1 px clear of the top hairline and of the window edge. |
| N8 | Fixed (data) | build-mac-2 is "Updating" (info dot), so it agrees with the "Update runner build-mac-2" job. The meaning of the nav badge "1" depends on its tooltip, which a still image can't show. See also R3-1. |
| N9 | Fixed | The nav shows Overview and Runners, with About at the foot. The harness pages are gone. |
| N10 | Fixed | At 320 px the chip reads "Index stopped". |
| N11 | Fixed | The status bar reads "1 running: Update runner". |
| N12 | Kept on purpose | The words stay in the State column, as agreed. |
| #9 | Fixed | See N9. |
| #12 | Fixed | No shadow falls on the status bar or the title bar. |
| #17 | Fixed | The toggle, the nav item, the focused+selected nav item and "…" all show a 2 px accent ring of at least 3:1, contained, in both themes. |

## New findings

| # | Pri | Screenshot / region | Problem | Fix |
|---|---|---|---|---|
| R3-1 | P3 | wide-*, rail-*, macos-*, health-*: status bar left, plus Runners nav badge | The status bar says "3 of 4 runners online", but the table shows 2 "Online", 1 "Updating" and 1 "Offline". The 3 only works if Updating counts as online, and the reader has to guess that. The "1" nav badge is also ambiguous: it could be the offline runner or the updating one. | Make the count follow the table's words, e.g. "2 of 4 online · 1 updating", or show only the exception ("1 offline"). Make the nav badge mean the same thing as that exception and say so in its tooltip and aria-label ("1 runner offline"). |
| R3-2 | P3 | All status bars, gaps between items | The visual gaps between items are uneven. At 1440 px: chip to spinner about 18 px, end of "Update runner" to bell about 27 px. At 320 px: about 17, 29 and 24 px. It looks like the chip has no hit-area padding while the other items do. | Give every status-bar item the same horizontal padding, chip included (or the same gap with no padding), so the visual gaps are equal (e.g. 12 or 16 px). |
| R3-3 | P3 | narrow-*, focus-*: status bar left third | At 320 px the left of the bar is empty (about 50 px), while the items on the right are spaced loosely. This is minor and could be on purpose, since the summary moved into "… 1". | No change is needed if it is on purpose. Otherwise, fixing R3-2 tightens the right group and makes this balance itself. |

## Notes (not findings)
- When a nav item is both focused and selected, the ring covers the 2 px selection bar. The selection still shows
  through the tint, the accent icon and the 600 label, so this is acceptable.
- The health popover sits about 2 px above the status bar. That is tight, but it doesn't overlap anything. An
  open/pressed state on the chip while its popover is open would make the link between them clearer (optional).
- Contrast: all text still looks at least 4.5:1 in both themes. The light warning text, the striped bar, the rings and
  the badges are unchanged from round 2, where they passed.
