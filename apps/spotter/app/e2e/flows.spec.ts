import { expect, test } from '@playwright/test';

test.describe('Spotter against the mock backend', () => {
  test('the first load shows content, not a blank window', async ({ page }) => {
    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'Dashboard', level: 1 })).toBeVisible();
    await expect(page.getByRole('heading', { name: "What's new" })).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Today' })).toBeVisible();
    await expect(page.locator('sp-whats-new li', { hasText: 'Lumen CLI' }).first()).toBeVisible();
    // A failing watch has a status word; a healthy one only a dot.
    await expect(page.getByText('Unreachable').first()).toBeVisible();
  });

  test('adds a watch through the wizard with a live preview', async ({ page }) => {
    await page.goto('/#/watches/new');
    await page.getByRole('button', { name: /Node\.js/ }).click();
    await expect(page.getByRole('heading', { name: 'Test this source' })).toBeVisible();
    await expect(page.getByRole('table', { name: 'Releases found' })).toBeVisible();
    await page.getByRole('button', { name: 'Next' }).click();
    await page.getByRole('button', { name: 'Next' }).click();
    await page.getByRole('button', { name: 'Next' }).click();
    await page.getByLabel('Name').fill('Node runtime');
    await page.getByRole('button', { name: 'Add watch' }).click();
    await expect(page.getByRole('heading', { name: 'Node runtime', level: 1 })).toBeVisible();
    await page.getByRole('link', { name: 'All watches' }).click();
    await expect(page.getByRole('link', { name: 'Node runtime' })).toBeVisible();
  });

  test('a wizard that is not ready says what is missing', async ({ page }) => {
    await page.goto('/#/watches/new');
    await page.getByRole('button', { name: 'Source kinds', exact: true }).click();
    await page.getByRole('button', { name: /GitHub/ }).click();
    await expect(page.getByText('Still needed: Repository')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Next' })).toBeDisabled();
  });

  test('pauses, resumes and checks a watch', async ({ page }) => {
    await page.goto('/#/watches');
    const row = page.locator('tbody tr', { hasText: 'Lumen CLI' });
    await row.hover();
    await row.getByRole('button', { name: 'More actions for Lumen CLI' }).click();
    await page.getByRole('menuitem', { name: 'Pause' }).click();
    await expect(row.getByText('Paused').first()).toBeVisible();
    await row.getByRole('button', { name: 'More actions for Lumen CLI' }).click();
    await page.getByRole('menuitem', { name: 'Resume' }).click();
    await expect(row.getByText('Paused')).toHaveCount(0);
    await row.hover();
    await row.getByRole('button', { name: 'Check Lumen CLI now' }).click();
    await expect(page.getByText('Check finished')).toBeVisible();
  });

  test('acknowledges a release', async ({ page }) => {
    await page.goto('/#/watches');
    const dots = page.locator('tbody .unseen-dot:not(.off)');
    await expect(dots.first()).toBeVisible();
    const before = await dots.count();
    expect(before).toBeGreaterThan(0);
    const row = page.locator('tbody tr').filter({ has: page.locator('.unseen-dot:not(.off)') }).first();
    await row.hover();
    await row.getByRole('button', { name: /^More actions for/ }).click();
    await page.getByRole('menuitem', { name: 'Mark as seen' }).click();
    await expect(dots).toHaveCount(before - 1);
  });

  test('a settings change applies at once and is still there after navigating', async ({ page }) => {
    await page.goto('/#/settings');
    await page.getByRole('radio', { name: 'Dark' }).click();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
    await page.getByLabel('Show notifications').uncheck();
    await page.getByRole('link', { name: 'Watches' }).click();
    await page.getByRole('link', { name: 'Settings' }).click();
    await expect(page.getByRole('radio', { name: 'Dark' })).toBeChecked();
    await expect(page.getByLabel('Show notifications')).not.toBeChecked();
  });

  test('the tray popup lists unseen releases', async ({ page }) => {
    await page.goto('/#/tray');
    await expect(page.getByText(/\d+ new/)).toBeVisible();
    await expect(page.getByRole('button', { name: 'Open Spotter' })).toBeVisible();
    await expect(page.getByRole('button', { name: /Mark .* as seen/ }).first()).toBeVisible();
  });

  test('a release opens a side panel that Esc closes', async ({ page }) => {
    await page.goto('/#/watches/w-2');
    const row = page.locator('tbody tr').nth(1);
    await row.click();
    const panel = page.getByRole('complementary', { name: 'Release details' });
    await expect(panel).toBeVisible();
    await expect(panel.getByText('SHA-256')).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(panel).toBeHidden();
  });

  test('a watch with products lists them in a table', async ({ page }) => {
    await page.goto('/?mock=products#/watches/w-9');
    await expect(page.getByRole('heading', { name: 'Products' })).toBeVisible();
    await expect(page.getByRole('cell', { name: /Acme Installer/ }).first()).toBeVisible();
  });

  test('the watches table does not wrap or clip at 1100 px', async ({ page }) => {
    await page.setViewportSize({ width: 1100, height: 800 });
    await page.goto('/#/watches');
    const cells = page.locator('tbody tr:first-child td.nowrap');
    for (const h of await cells.evaluateAll((els) => els.map((e) => e.getBoundingClientRect().height))) expect(h).toBeLessThanOrEqual(60);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  });

  test('a failing backend shows an inline error with Retry', async ({ page }) => {
    await page.goto('/?mock=broken');
    await expect(page.getByRole('button', { name: 'Retry' }).first()).toBeVisible();
    await expect(page.getByText('Spotter cannot reach its engine')).toBeVisible();
  });

  test('an empty install invites you to add the first watch', async ({ page }) => {
    await page.goto('/?mock=empty');
    await expect(page.getByText('Nothing to watch yet')).toBeVisible();
    await expect(page.getByRole('link', { name: 'Add your first watch' })).toBeVisible();
  });
});

test.describe('Check schedule', () => {
  test('switches between every, at times and cron with a live preview', async ({ page }) => {
    await page.goto('/#/watches/new');
    await page.getByRole('button', { name: /Node\.js/ }).click();
    await expect(page.getByRole('table', { name: 'Releases found' })).toBeVisible();
    await page.getByRole('button', { name: 'Next' }).click();
    await page.getByRole('button', { name: 'Next' }).click();
    await expect(page.getByRole('heading', { name: 'Schedule' })).toBeVisible();

    const modes = page.getByRole('group', { name: 'How to schedule checks' });
    await expect(modes.getByRole('button', { name: 'Every…' })).toHaveAttribute('aria-pressed', 'true');
    await page.getByRole('button', { name: '12 h' }).click();
    await expect(page.getByText('Next check').first()).toBeVisible();

    await modes.getByRole('button', { name: 'At times' }).click();
    await page.getByRole('button', { name: 'Friday' }).click();
    await page.getByRole('button', { name: 'Add a time' }).click();
    await expect(page.getByRole('list', { name: 'Upcoming checks' }).getByRole('listitem')).toHaveCount(5);
    await page.getByLabel('Time zone').fill('Nowhere/Land');
    await page.getByLabel('Time zone').blur();

    await modes.getByRole('button', { name: 'Cron' }).click();
    const expr = page.getByLabel('Cron expression');
    await expr.fill('* *');
    await expect(page.getByRole('alert').filter({ hasText: 'five fields' })).toBeVisible();
    await expr.fill('0 9 * * 1-5');
    await expect(page.getByRole('list', { name: 'Upcoming checks' }).getByRole('listitem')).toHaveCount(5);
    await expect(page.getByRole('alert')).toHaveCount(0);

    await page.getByRole('button', { name: 'Next' }).click();
    await expect(page.getByText('Cron 0 9 * * 1-5')).toBeVisible();
  });
});
