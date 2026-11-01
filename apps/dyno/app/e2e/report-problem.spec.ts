import { expect, test } from '@playwright/test';
import { pressMod } from '@decryp7/paddock-e2e';

/** Report a problem in the browser build (mock backend, the in-memory report engine): reachable from the Help menu and the command palette. */
const url = '/?latency=0#/';

test.describe('Report a problem', () => {
  test('opens from the Help menu and lists the app log and the agent log with their sizes', async ({ page }) => {
    await page.goto(url);
    await expect(page.getByRole('heading', { level: 1, name: 'Dashboard' })).toBeVisible();
    await page.getByRole('menubar').getByRole('menuitem', { name: 'Help' }).click();
    await page.getByRole('menuitem', { name: 'Report a problem…' }).click();
    const dialog = page.getByRole('dialog', { name: 'Report a problem' });
    await expect(dialog).toBeVisible();
    // The report lists every file with its size before anything is saved: the dated log of each process, the agent's included
    // (the real engine reads both from disk, so they are there with the agent down).
    const files = dialog.getByTestId('diag-files');
    await expect(files.getByText('logs/dyno.2026-10-31.log')).toBeVisible();
    await expect(files.getByText('logs/dyno-agent.2026-10-31.log')).toBeVisible();
    await expect(dialog.getByTestId('diag-total')).toContainText(/\d+ files/);
  });

  test('is a command of the palette', async ({ page }) => {
    await page.goto(url);
    await expect(page.getByRole('heading', { level: 1, name: 'Dashboard' })).toBeVisible();
    await pressMod(page, 'Mod+Shift+P');
    await expect(page.getByRole('combobox', { name: /Type a command/ })).toBeFocused();
    // Typed, not filled: Mod+Shift+P opens the commands mode, which a fill would replace with the pages mode's empty prefix.
    await page.keyboard.type('Report a problem');
    await page.getByRole('option', { name: /Report a problem/ }).first().click();
    await expect(page.getByRole('dialog', { name: 'Report a problem' })).toBeVisible();
  });
});
