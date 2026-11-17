import { expect, test } from '@playwright/test';
import { expectNoScrollMovesOfItsOwn } from '../../../../ui/projects/paddock-e2e/src/index.ts';
import { openApp } from './app';
import { openAiApp, openChat } from './ai-support';
import { viewport } from './mock-url';

// Family ADR 0105: a scroller our code positions makes no scroll moves of its own (no scroll anchoring). The events grid was two rows
// off after a resize in WebKit when the browser moved scrollTop by the change of its row layer. Each check is red without `pdOwnedScroll`
// on the scroller, and its positive control shows the browser would anchor there.
test.describe('scrollers the app positions', () => {
  test('the events grid makes no scroll moves of its own', async ({ page }) => {
    await openApp(page);
    await expect(viewport(page)).toBeVisible();
    await expectNoScrollMovesOfItsOwn(viewport(page), 'the events grid');
  });

  test('the AI chat log makes no scroll moves of its own', async ({ page, browserName }) => {
    // Not measured in WebKit (ER-33): there a scroller placed anywhere inside this panel's host does not anchor even with overflow-anchor
    // forced on, while one placed in its parent does (probe of 2026-11-17, family ADR 0105), so the check cannot see the thing.
    test.skip(browserName === 'webkit', 'not measured in WebKit: no anchoring inside the chat panel host, even forced');
    await openAiApp(page);
    const chat = await openChat(page);
    await expectNoScrollMovesOfItsOwn(chat.locator('.log[role="log"]'), 'the AI chat log');
  });
});
