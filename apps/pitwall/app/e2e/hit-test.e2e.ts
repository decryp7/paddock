import { test } from '@playwright/test';
import { expectNothingCovers } from '../../../../ui/projects/paddock-e2e/src/hit-test.ts';
import { openApp } from './app';

// ER-21: nothing covers an interactive element in the model library (the shared check of @decryp7/paddock-e2e, ADR 0027).

test('the model library: nothing covers an interactive element', async ({ page }) => {
  await openApp(page, '/models');
  await expectNothingCovers(page.locator('body'));
});
