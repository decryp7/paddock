import { test } from '@playwright/test';
import { expectNoScrollMovesOfItsOwn } from '../../../../ui/projects/paddock-e2e/src/index.ts';
import { openApp } from './app';

// Family ADR 0105: a scroller our code keeps at its newest text makes no scroll moves of its own (no scroll anchoring); each check is red
// without `pdOwnedScroll` on it, and its positive control shows the browser would anchor there. The Compare conversations (`.msgs`) are not
// measured: in the mock's page they grow with their content instead of scrolling (a 3518 px tall pane in a 900 px window, 2026-11-17).
test.describe('scrollers the app positions', () => {
  test('the Chat log makes no scroll moves of its own', async ({ page }) => {
    await openApp(page, '/playground');
    await expectNoScrollMovesOfItsOwn(page.locator('pw-playground-chat').getByTestId('assistant-log'), 'the Chat log');
  });
});
