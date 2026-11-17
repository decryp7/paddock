import { expect, test } from './test';
import { expectNoScrollMovesOfItsOwn } from '../../../../ui/projects/paddock-e2e/src/index.ts';
import { openApp } from './app';
import { viewport } from './mock-url';

// Family ADR 0105: a scroller whose position our code computes makes no scroll moves of its own (no scroll anchoring). The events grid was
// two rows off after a resize in WebKit when the browser moved scrollTop by the change of its row layer. The check is red without
// `pdOwnedScroll` on the scroller, and its positive control shows the browser would anchor there. The AI chat log keeps anchoring on
// purpose (it keeps the reader's place), so it has no row here.
test.describe('scrollers the app positions', () => {
  test('the events grid makes no scroll moves of its own', async ({ page }) => {
    await openApp(page);
    await expect(viewport(page)).toBeVisible();
    await expectNoScrollMovesOfItsOwn(viewport(page), 'the events grid');
  });
});
