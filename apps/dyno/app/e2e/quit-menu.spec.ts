import { expect, test } from '@decryp7/paddock-e2e';

/**
 * What Quit says and does in the tray popup's menu (the mock stands for the engine): Quit Dyno while nothing keeps the agent,
 * Quit Dyno (keeps measuring) while a login service does, and the second entry that stops the agent too. Waits are on conditions.
 */
const POPUP = (query: string) => `/?latency=0${query}#/tray`;

async function openMenu(page: import('@playwright/test').Page, query: string) {
  await page.setViewportSize({ width: 360, height: 640 });
  await page.goto(POPUP(query));
  await expect(page.getByRole('button', { name: /Open Dyno/ })).toBeVisible();
  await page.getByRole('button', { name: 'More actions' }).click();
}

test.describe('Quit in the tray popup', () => {
  test('says Quit Dyno and offers Quit and stop measuring when no service keeps the agent', async ({ page }) => {
    await openMenu(page, '');
    await expect(page.getByRole('menuitem', { name: 'Quit Dyno', exact: true })).toBeVisible();
    await expect(page.getByRole('menuitem', { name: 'Quit and stop measuring' })).toBeVisible();
  });

  test('says Quit Dyno (keeps measuring) while a login service keeps the agent', async ({ page }) => {
    await openMenu(page, '&keeps=1');
    await expect(page.getByRole('menuitem', { name: 'Quit Dyno (keeps measuring)' })).toBeVisible();
    await expect(page.getByRole('menuitem', { name: 'Quit and stop measuring' })).toBeVisible();
  });
});
