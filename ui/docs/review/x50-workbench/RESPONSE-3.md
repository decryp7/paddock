# X50 workbench: response to review 3

This file lists what changed for each finding of [REVIEW-3.md](REVIEW-3.md). The contact sheet is regenerated (with
`demo-icons-compare.png`) for review 4. Review 4 covers the shots it asked for:
- 07b and 12;
- 11b, the light code block;
- 13c and 13e;
- the demo icon beside the shell demo's at 16, 32 and 1024 px.

None of it is self-approved.

## P1

**P1-1 Shift+F10 menu focus.** A failing e2e came first (`workbench-demo.spec.ts`, "review 3 P1-1"). It checks that:
- the first item is focused;
- ArrowDown moves the focus to the second item;
- Escape returns the focus to the row;
- the items and icons match the mouse-opened menu.

The cause was wider than the Explorer. Review 2's reveal animation kept every `pd-popmenu` at `visibility: hidden` for
its first frames, and a hidden element cannot take the focus. The CDK's own `focusFirstItem` therefore did nothing, in
this menu and in every keyboard-opened menu of the suite.

The fixes:
- **Reveal:** menus now reveal by opacity, which keeps the first frame invisible and the menu focusable. The first-frame
  e2e now checks `opacity: 0`.
- **Focus:** the Explorer focuses the first item with the keyboard focus style once the items are rendered.
- **Escape:** the Explorer gives the focus back to the row it was opened from.

The icon difference in 07b came from the second item being focused in one shot and not in the other: "Copy path" has
its icon in both paths, and "Open" has none in both. Shot 07b is re-shot with the focus moved to the second item.

## P2

- **P2-1** A new token `--pd-surface-code` (dark `#22262c`, light `#eceff3`) is used for code blocks and inline code. A
  token test checks four modes (dark and light, standard and high contrast): the surface differs from the pane, text on
  it is ≥ 7:1, and muted text is ≥ 4.5:1. See shot 11b.
- **P2-2** Native checks and radios take `accent-color: var(--pd-accent-control)` from one rule in the base stylesheet,
  covered by a test. The language page's own rule is removed.
- **P2-3** A tool stopped by the user is drawn as a neutral card with the stop glyph and a muted "Stopped" (13c). Danger
  stays for real failures.
- **P2-4** Under rule 40, there is now one map in `@decryp7/paddock-ui/icons`: `PD_STATUS_GLYPHS` (error is circle-×,
  warning is triangle, info is circle-i, success is a check), with `statusGlyph()` and `<pd-status-glyph status>`.
  - A lint in `status.spec.ts` scans every template of the library, the demos and the gallery, and refuses a status glyph
    written by name.
  - It found instances in form, settings, schedule and pause errors, the chart, table and view errors, packs, the
    recovery notice, the failed turn, tool cards, the demo's Problems and the gallery. All are converted. Errors now draw the circle-×, and "This cannot be
    undone." and the recovery notice keep the triangle as warnings.
- **P2-5** Deleting the open conversation switches at once to the next one, or to a new one. Undo reopens it and makes it
  active again (store and component tests; 13e).
- **P2-6** The demo icon has its own silhouette: an activity-bar strip of three dots, a tall pane, and a split pane with
  two tabs, with no window frame or sidebar pills.
  - **Tile:** neutral near-black (`#050505`, a `#1C1C1C`→`#000000` ramp). A grid search with `cvd.mjs` found it to be the
    only family-safe neutral.
  - **Ink:** white, at 20.4:1 on the base.
  - **Distance:** min ΔE (normal / protan / deutan / tritan) is 75.5 / 75.6 / 75.4 / 75.4 against the shell demo and
    19.3 / 19.7 / 19.2 / 19.2 against Paddock. The family minimums are unchanged at 17.7 / 11.6 / 13.6 / 10.7.
  - **Ramp:** strictly neutral, with no tinted 8-bit bands.
  - **Records:** icons/README records the numbers. See `demo-icons-compare.png`.

## P3

- **P3-1** Shot 12 clicks the Details summary, then gives the focus back to Decline by keyboard: the tool, the
  arguments and the focused default are all visible.
- **P3-2** An overflowed tab strip scrolls only to tab boundaries, so no sliver of a tab shows at its start. The active
  tab is always fully shown, the tabs out of view are the `⌄ n` count, and the strip has no scrollbar (unit tests).
- **P3-3** A bottom slide-out stays over its own column. It measures the docked groups (`bottomFlyoutInsets`, a pure
  function with tests) and keeps off a full-height side column such as the Assistant's. An e2e checks that it stays
  clear of the composer. The pin and close already float over the pane's corner, with no header row, so no "1 running"
  line is added.
- **P3-4** The heading shows the conversation's title as soon as the question is sent.
- **P3-5** One chip text, "Current file: src/main.rs", is used in the composer and the bubble.
- **P3-6** A running tool card shows the 14 px status-bar spinner (static under reduced motion) and keeps "Working…".
- **P3-7** The severity column is 16 px, so the glyph sits 12 px from the problem text (e2e at 320 px).
- **P3-8** A notice can carry a short form (`ShellNotice.short`), shown as the one-line toast at 480 px and below
  ("Layout fitted", "Pop-out failed"). The full sentence stays in Notifications.
- **P3-9** The mock answers what happened: "I did not erase anything." after a declined erase, the locked index after
  a failed tool, and a cut-off sentence for a failed turn.
- **P3-10** The palette footer shows the mode in use in text colour and semibold.
- **P3-11** Conversation list rows have the 12 px pane inset on both sides.
- **P3-12** The activity item's focus ring is drawn above its badge, so the ring stays continuous.
