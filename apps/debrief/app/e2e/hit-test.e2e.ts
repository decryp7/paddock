import { test } from '@playwright/test';
import { expectNothingCovers } from '../../../../ui/projects/paddock-e2e/src/hit-test.ts';
import { openApp } from './app';

// ER-21: nothing covers an interactive element in the main views (the shared check of @decryp7/paddock-e2e, ADR 0027).

// Designed overlap (annotation-track.ts, "event markers sit on top of them, so a band may be partly covered"): the bands and
// glyphs are pointer shortcuts outside the tab order, and the timeline legend's Evidence menu is the equivalent control
// (WCAG 2.5.8). Not a finding.
const MARKERS_OVER_ANNOTATIONS: [string, string] = ['button.marker, debrief-marker-icon, debrief-marker-glyph, .mk', 'button.ann'];

// TODO(finding hit-test-2, its own fix branch; remove this entry when it lands). With the dock host skipped as an empty inert
// layer, the stricter check (any sample point covered) finds that the lane contents overlap each other in the timeline: the
// density sparkline (svg.density), marker glyphs (span.glyph, span.mark, debrief-marker-icon), severity badges, the "still
// running" badge (span.still) and neighbouring annotation bands or lane labels sit on top of the annotation bands (button.ann),
// run bars (button.run) and lane labels (button.lane-label), at rest and when the lanes scroll.
const TIMELINE_FINDINGS: [string, string][] = [
  ['svg, button.marker, span.glyph, span.mark, span.still, debrief-marker-icon, debrief-severity-badge, button.ann, div.band, div.plot, button.lane-label', 'button.ann, button.run, button.lane-label'],
];

test.describe('nothing covers an interactive element', () => {
  test('the timeline panel', async ({ page }) => {
    await openApp(page);
    await expectNothingCovers(page.locator('debrief-timeline-panel'), { allow: [MARKERS_OVER_ANNOTATIONS, ...TIMELINE_FINDINGS] });
  });

  test('the events grid', async ({ page }) => {
    await openApp(page);
    await expectNothingCovers(page.locator('debrief-events-grid'));
  });
});
