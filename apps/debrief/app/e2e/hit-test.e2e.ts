import { expect, test } from '@playwright/test';
import { expectNothingCovers } from '../../../../ui/projects/paddock-e2e/src/hit-test.ts';
import { openApp } from './app';

// ER-21: nothing covers an interactive element in the main views (the shared check of @decryp7/paddock-e2e, ADR 0027).

// Designed overlaps, one exact pair each.
const DESIGNED: [string, string][] = [
  // annotation-track.ts: "event markers sit on top of them (so a band may be partly covered)"; the bands and glyphs are pointer
  // shortcuts outside the tab order, and the timeline legend's Evidence menu is the equivalent control (WCAG 2.5.8).
  ['button.marker', 'button.ann'],
  // timeline-lanes.ts `.density`: the lane's volume fill is drawn over the bands and is inert on purpose (`pointer-events: none`).
  ['svg.density', 'button.ann'],
  // run-track.ts `.still`: the "still running" label at the open end of a run is text only (`pointer-events: none`, aria-hidden).
  ['span.still', 'button.run'],
  // The same rationale for the rest of the lane's own overlays over an annotation button (the Evidence menu lists every one):
  // the ticks bar, another annotation band and the severity badge or marker icon of a neighbouring annotation.
  ['svg.ticks', 'button.ann'],
  ['button.ann', 'button.ann'],
  ['debrief-severity-badge', 'button.ann'],
  ['debrief-marker-icon', 'button.ann'],
];

test.describe('nothing covers an interactive element', () => {
  test('the timeline panel', async ({ page }) => {
    await openApp(page);
    await expectNothingCovers(page.locator('debrief-timeline-panel'), { allow: DESIGNED });
  });

  test('the events grid', async ({ page }) => {
    await openApp(page);
    await expectNothingCovers(page.locator('debrief-events-grid'));
  });

  // hit-test-2 (fixed): a marker at a lane's first moment overhung the label column by 7 px (`.glyph { margin-left: -7px }`) and its
  // button took the clicks on the label's right edge.
  test('the right edge of a lane label belongs to the label, not to a marker', async ({ page }) => {
    await openApp(page);
    const wrong = await page.evaluate(() =>
      [...document.querySelectorAll('debrief-timeline-panel button.lane-label')].flatMap((label) => {
        const r = label.getBoundingClientRect();
        const own = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
        if (r.width === 0 || !own || !label.contains(own)) return []; // scrolled out of the panel
        const hit = document.elementFromPoint(r.right - 2, r.top + r.height / 2);
        return hit && label.contains(hit) ? [] : [`${label.getAttribute('aria-label')?.slice(0, 30)} -> ${hit?.className}`];
      }),
    );
    expect(wrong).toEqual([]);
  });
});
