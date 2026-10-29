# Open UX item: markers take most of the annotation bands' clickable area (2026-10-29)

Found by the shared hit-test (`expectNothingCovers`, ADR 0027 run-time check) and measured by its reviewer with the real
pointer (no forced `pointer-events`) on the demo workspace: the event markers (`button.marker`, z-index 1) take **71 to 84 %**
of the visible annotation bands' (`button.ann`, z-index 0) clickable area, so clicking most of a band selects a marker, not
the band's evidence.

This is the documented design, not a regression: `annotation-track.ts` says "event markers sit on top of them (so a band may be
partly covered), and the timeline legend's Evidence menu lists every annotation in view, which is the keyboard path and the
equivalent control for WCAG 2.5.8 (target size)". The hit test therefore allows the pair `['button.marker', 'button.ann']`
(`apps/debrief/app/e2e/hit-test.e2e.ts`).

What it costs a pointer user: a band under a busy lane is hard to hit, and the band's tooltip rarely shows. Options to weigh,
none decided:

- give the bands their own strip (a row above or below the markers) so neither covers the other;
- let a click on a marker also offer the annotations under it (a small chooser), keeping the markers on top;
- shrink the marker hit area to the glyph (14 px) where a band lies under it, keeping 24 px elsewhere (WCAG 2.5.8 allows
  the spacing exception only when targets do not overlap).

Removing the allow entry from the hit test is the check that the item is closed.
