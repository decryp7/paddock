# X50 workbench: response to review 4

This file lists what changed for each finding of [REVIEW-4.md](REVIEW-4.md). The re-shoots are 12 and 13e, and
`demo-icons-compare.png` is re-made. None of it is self-approved.

## P2

**P2-1 Seams in the 1024 px tile.**
- **Cause:** not the sheen or the inner edge (the seams stayed with both removed). The macOS tile's drop-shadow filter
  blended in linearRGB, the SVG default, which quantises a dark ramp into steps of 6 to 13 levels.
- **Fix:** the filter now uses `color-interpolation-filters="sRGB"`. Columns x=230 and x=880 have no steps left in the
  tile body.
- **New check:** `icons/tools/scan.mjs`, run by `tools/build.mjs` for every icon (macOS and Windows masters).
  - It renders the bare tile, without glyph or livery, at 1024 px.
  - It walks every column of the opaque body.
  - It fails the build on a step above 3 levels in any channel.
  - It has three tests (`icons` `npm test`).
- **Family icons:** with the fix, all ten icons scan clean. Without it, every macOS master except Sighter's and the shell
  demo's fails. The source fix is global, but only the workbench demo is rebuilt. Rebuilding the family's approved
  masters changes them by up to about 20 levels in the tile and shadow, so that is left as a decision for the
  coordinator, with an independent review (rule 22).

**P2-2 A reopened conversation looked different from the live one.**
- **Test first:** a store test runs a tool with a citation, a declined write, a failed turn and a stopped tool. It then
  switches away and back, and expects the same turn views. It failed first; commit ddd974b holds the failing test.
- **Engine (paddock-assistant):** now stores the call as the user read it, write and destructive, and `outcome`
  (declined or stopped). It also stores a failed turn's error with its needs-AI state, and the budget message. A turn
  that ended without text keeps an empty closing message.
  - The format version is unchanged; the new fields are optional, so old files load (Rust test).
  - Sizes are bounded: a large result keeps only up to 50 citations, and notes are capped at 2000 characters.
- **Live events:** `ToolFinished` carries the same optional `outcome`, so the live view and the reopened one read one
  field.
- **Mock backend:** the memory backend stores turns the way the engine does.
- **Restore:** `turnsOf` rebuilds tool cards, citations, the error with Retry, and the end reason.
- See shot 13e.

**P2-3 The second status-glyph map in pd-view.**
- The pd-view owner derived both maps from `PD_STATUS_GLYPHS`.
- The status-glyph lint now also covers TypeScript: an object entry that maps a status-like key (error, bad, warn,
  warning and similar) to a glyph name fails. A map typed with `PdStatus` is allowed, because it names a status that the
  shared map draws.
- The lint is blocking.

## P3

- **P3-1** While the user is at the bottom, the log keeps the latest item in view when it gets shorter, for example
  when an approval opens its Details below it (a ResizeObserver; a test came first). Shot 12 shows the waiting `erase`
  card above the approval.
- **P3-2** The 16 px glyph sits on whole pixels: a 1 px tab row, 1 px gaps and 1 px dots. The 32 px glyph is snapped
  the same way.
- **P3-3** The filled blocks stay, as the review allows for a demo outside the family sheet.

## Also

`workbench-demo.spec.ts` "the Jobs view…" failed 3 in 8 runs on integrate. The test clicked the activity bar while the
slide-out was open, and a click outside hides it, as it should. The test now opens the Assistant column first, then
shows Jobs. It passed 12 of 12 runs.
