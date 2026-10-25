# X50 workbench: independent design review 4 (rule 22, limited)

- Reviewer: the review-3 reviewer. I did not build X50. Date: 2026-10-25.
  Branch: `origin/shell/integrate` @ f2de310.
- Scope, as asked:
  - the re-shoots 07b, 11b, 12, 13c and 13e;
  - `demo-icons-compare.png` and the icon masters in `icons/workbench-demo/`;
  - a spot check that failures now draw the circle-× through the shared status map.
  - Inputs were REVIEW-3.md and RESPONSE-3.md. The other shots were not reviewed again.
- Method: crops at full resolution, colours sampled from the PNGs, a scan of each pixel column of the 1024 px icon
  master, and a search of the library sources for glyph maps.
- **Verdict: not approved yet. There is no P1.** Three P2s remain, and each is narrow:
  1. two hard seams in the new icon's dark tile;
  2. a reopened conversation that loses its tool cards, citations and error state;
  3. a second status-glyph map in `@decryp7/paddock-ui/view` that the new lint does not catch.

  Fix those three, and approval needs only the 13e re-shot, a seam-free 1024 master and the grep below. Everything
  else asked for in review 3 is fixed and looks right.

Counts (new findings in this review): **P1: 0, P2: 3, P3: 3.**

## 1. Re-shoots, verified

| Shot | Review 3 finding | Verdict | Evidence |
|---|---|---|---|
| 07b | P1-1 keyboard menu | **Fixed** | Both themes, after Shift+F10 and ArrowDown: "Copy path" has the 2 px accent ring and the hover tint, and the README row is back to the 1 px target outline, so focus has left the tree. "Copy path" has its icon in both themes, and "Open" has none, as in 07. The e2e checks the first item focused, ArrowDown, Escape back to the row, and items equal to the mouse menu. The root cause (`visibility: hidden` during the reveal) was suite-wide, and it is fixed at the source. |
| 11b | P2-1 light code surface | **Fixed** | Light: the block and the inline code are on #eceff3 against the white pane, and they read as code. Dark: unchanged and good. A token test covers four modes. |
| 12 | P2-C, P3-1 | **Fixed** | Details is open with the 14 px chevron rotated, "Tool: erase" and "Arguments: No arguments". Decline is focused with the 2 px accent ring (olive in dark, blue in light). Allow erase is danger text on a neutral button, and "This cannot be undone." keeps the warning triangle, which is correct for a warning. |
| 13c | P2-3, P2-4 | **Fixed** | The stopped tool is a neutral card with the stop glyph and a muted "Stopped". The failed tool has a danger border and the circle-×. The mock's answer now reports the locked index (P3-9). |
| 13e | P2-5 | **Fixed, with a new P2-2** | After deleting the open "Request handler", the view switches to "Explain the request pipeline" at once. The list strip shows Deleted "Request handler" with Undo focused (2 px ring). The checkbox is in the accent colour (P2-2 of review 3, also seen in 13). |
| 13 (spot check) | P2-4 | **Fixed** | The failed turn shows the circle-×, the partial sentence ("…starting with the router and"), the cause, Retry and Show details. |
| demo-icons-compare | P2-6 | **Fixed, with a new P2-1** | See §2. |

## 2. The demo icon

- **Silhouette: fixed.** An activity strip of three dots, a tall pane, and a split pane with two tabs. Nothing in it is
  shared with the shell demo's window, title bar and pills, so the two icons are told apart at once at 1024, 32 and 16 px.
- **Tile: fixed.** Near-black #050505 with white ink (20.4:1). RESPONSE-3's cvd numbers (75.5 against the shell
  demo, 19.3 against the Paddock mark, the family minimums unchanged) are plausible. Near-black is the neutral that
  my review-3 run also pointed to.
- **Banding: not clean (P2-1).** Two flat horizontal steps cross the whole tile. They are clearly visible in the
  comparison sheet as lighter bands across the upper third.
- **Small frames:** the 32 px frame is crisp. The 16 px frame keeps the bar and the dots, but its top-right
  (tabs over pane) blurs into grey (P3-2).

## 3. New findings

### P2

**P2-1. The 1024 px tile has two hard seams.** The red channel down each column of
`workbench-demo-macos-1024.png`, at x = 230 and at x = 880:

| y (px) | Change | Notes |
|---|---|---|
| 150 to 300 | 0x3c → 0x36 | smooth |
| 307 | 0x39 → 0x31 | a step of 8 at x = 230; 0x1e → 0x16 at x = 880 |
| 307 to 514 | 0x31 → 0x21 | smooth |
| 524 | 0x21 → 0x15 | a step of 12 |

- **Why they show:** on near-black, a step of 8 to 12 levels is a visible edge (Mach band). The same scan of
  `pacenotes-macos-1024.png` has no such steps, only the diagonal livery edges.
- **Likely cause:** the top-left sheen or the inner-edge light ends at a fixed y instead of fading out. A
  `#1C1C1C → #000000` ramp has only 28 levels in all, so any layer with a hard edge shows.
- **Fix:**
  - Let those layers fade to zero alpha.
  - Or dither the dark ramp, so the darkest tile in the family stays free of steps.
  - Add a check to `tools/build.mjs`: no step above 3 levels along a vertical scan of the tile body, away from the
    livery.

**P2-2. A reopened conversation loses its tool cards, citations and error state.** `*-13e`: the conversation
"Explain the request pipeline", reopened after the delete, shows only the bubbles and plain text.
- **What is lost:**
  - the `find` tool card and the "2 things found" note;
  - the `item 7` citation, now plain text instead of a link;
  - the declined `erase` card;
  - the whole `/fail` turn. Its partial text, the circle-× error, Retry and Show details are all gone, so the
    conversation ends with an unanswered "/fail" bubble.
- **Why it matters:** the same transcript looks different, and less true, depending on whether it is live or reopened.
  A failed answer that comes back as no answer misleads the user.
- **Fix:** persist and restore each turn's tool calls (with status and reason), the citations and the error state.
  Start with a failing store test: run a turn with a tool, a citation and a failure, switch away and back, and the
  rendered turn equals the live one. Then re-shoot 13e.

**P2-3. A second status-glyph map survives in `@decryp7/paddock-ui/view` (the review-3 P2-4 class).**
`view/src/model.ts`:
- `TONE_GLYPH` maps `bad` to `'close'`, a plain ×, not the circle-×.
- `ICON_GLYPH` maps `error` to `'alert'`, the warning triangle.

`scene.ts` and `tone.ts` render through these maps, so an error in a view still draws the triangle. A "bad" tone draws
a bare × that looks like a close button. `status-lint.ts` scans only literal names in templates, so maps in
TypeScript and bound `[name]` values escape it.
- **Fix:** derive both maps from `PD_STATUS_GLYPHS` (`bad`/`error` → `statusGlyph('error')`, `warn` →
  `statusGlyph('warning')`).
- **Lint:** extend it to fail on any object literal that maps a status-like key (`error`, `bad`, `warn`, `warning`)
  to a glyph name.
- **Also check:** `needs-ai.ts` uses `'alert'` for incompatible or unavailable. That reads as a warning, which is
  acceptable, but route it through the map too.

### P3

- **P3-1.** In shot 12 the pending `erase` card, which used to say "Waiting for your decision", is no longer visible
  above the approval. The log ends at a hairline under the `/write` bubble. If the log does not stay at the bottom while
  an approval is open, the user loses the card that says what is waiting. Check that the log follows to the latest item
  when the approval appears.
- **P3-2.** At 16 px the icon's tabs and pane (top right) are mid-grey anti-aliasing. Snap them to the 16 px grid:
  one white tab row of 1 px and a 1 px gap, or drop the tabs at 16 px as the family drops detail.
- **P3-3.** The new glyph is drawn as filled rounded blocks. The family glyphs, and the shell demo's, use 60 to 68
  unit strokes with round joins (icons/README, "Grid and shape language"). For a demo that stays out of the family
  sheet, this is acceptable. If it is meant to sit beside the family, outline the panes with the family stroke, and
  keep the dots filled.

## 4. What approval needs

- **P2-1:** a master without the two seams, with the scan check added to the build.
- **P2-2:** a failing store test first, then restore, then a re-shot 13e that shows the tool cards, the citation and
  the failed turn after the switch.
- **P2-3:** both `view` maps derived from `PD_STATUS_GLYPHS`, and a lint that catches maps in TypeScript.

These can be checked from one shot, one pixel scan and one search. No further full review is needed.
