import { expect, test, type Page } from '@playwright/test';
import { pressMod } from '@decryp7/paddock-e2e';

/** The Administrator readings card of Settings against the mock backend (`?helper=…` picks the state it starts in). */
const url = (query = '') => `/?latency=0${query}#/`;

async function openSettings(page: Page, query = ''): Promise<void> {
  await page.goto(url(query));
  await expect(page.getByRole('heading', { level: 1, name: 'Dashboard' })).toBeVisible();
  await pressMod(page, 'Mod+Comma');
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await dialog.getByText('Dyno', { exact: true }).click();
}

const card = (page: Page) => page.getByRole('region', { name: 'Administrator readings' });

test.describe('administrator readings', () => {
  for (const [helper, chip, text] of [
    ['off', 'Off', 'Turn on administrator readings'],
    ['silent', 'Not running', 'Repair the helper'],
    ['foreign', 'Not trusted', 'Repair the helper'],
    ['outdated', 'Update needed', 'Update the helper'],
    ['notIncluded', 'Not available here', 'AppImage'],
  ] as const) {
    test(`${helper}: says ${chip} in words, with its one action`, async ({ page }) => {
      await openSettings(page, `&helper=${helper}`);
      await expect(card(page).locator('pd-status-pill')).toContainText(chip);
      await expect(card(page)).toContainText(text);
    });
  }

  test('turning on shows the job running while it is held at a step', async ({ page }) => {
    // The job is held (its steps are 10 minutes apart), so the running state is not a moment the assertion can miss under load.
    await openSettings(page, '&helper=off&jobsteps=6&jobstepms=600000');
    await card(page).getByRole('button', { name: 'Turn on administrator readings' }).click();
    await expect(card(page).locator('pd-status-pill')).toContainText('Turning on');
    await expect(card(page).getByRole('button', { name: 'Turn on administrator readings' })).toHaveCount(0);
  });

  test('turning on ends with the helper On; turning off returns it to Off', async ({ page }) => {
    await openSettings(page, '&helper=off');
    await card(page).getByRole('button', { name: 'Turn on administrator readings' }).click();
    await expect(card(page).locator('pd-status-pill')).toContainText('On', { timeout: 20_000 });
    await expect(card(page).getByRole('button', { name: 'Turn off administrator readings' })).toBeVisible();
    await card(page).getByRole('button', { name: 'Turn off administrator readings' }).click();
    await expect(card(page).locator('pd-status-pill')).toContainText('Off', { timeout: 20_000 });
  });

  test('a declined prompt is explained and nothing changes', async ({ page }) => {
    await openSettings(page, '&helper=off&fail=helperJob');
    await card(page).getByRole('button', { name: 'Turn on administrator readings' }).click();
    await expect(card(page).getByRole('alert')).toContainText('declined');
    await expect(card(page).locator('pd-status-pill')).toContainText('Off');
  });
});
