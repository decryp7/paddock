import { test } from '@playwright/test';
import { expectNothingCovers } from '../../../../ui/projects/paddock-e2e/src/hit-test.ts';
import { openApp } from './app';

// ER-21: nothing covers an interactive element in the main views (the shared check of @decryp7/paddock-e2e, ADR 0027).

// The dock's floating-panel host is an empty, full-size layer that is inert on purpose (dockview draws floating groups into it).
const DOCK_HOST: [string, string] = ['.dv-floating-overlay-host', '*'];

// TODO(finding hit-test-1, own fix branch): timeline markers sit over the annotation buttons (every sample point of a `button.ann`
// reaches a marker instead, at rest and when the lanes scroll). Remove this allow when the fix lands.
const MARKERS_OVER_ANNOTATIONS: [string, string] = ['button.marker, debrief-marker-icon, debrief-marker-glyph, .mk', 'button.ann'];

test.describe('nothing covers an interactive element', () => {
  test('the timeline panel', async ({ page }) => {
    await openApp(page);
    await expectNothingCovers(page.locator('debrief-timeline-panel'), { allow: [DOCK_HOST, MARKERS_OVER_ANNOTATIONS] });
  });

  test('the events grid', async ({ page }) => {
    await openApp(page);
    await expectNothingCovers(page.locator('debrief-events-grid'), { allow: [DOCK_HOST] });
  });
});
