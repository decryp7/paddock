import { expect, test, type Locator, type Page } from '@playwright/test';

/**
 * Every icon-only action of a list or table row shows a tooltip with its name (design guide: every icon-only button has an
 * aria-label and a tooltip), and the tooltip never covers a neighbouring action (geometry, not luck): for each icon button,
 * hover it, wait for the tooltip, check its text is the button's aria-label and its box overlaps no other action button of the
 * same list or table (every row's, including the ones only shown on hover). A row's actions share one tooltip, placed left of the
 * whole cluster.
 */
const url = (path: string) => `/?latency=0#${path}`;
const ACTIONS = 'button.mat-mdc-icon-button:not(.chev), a.mat-mdc-icon-button';

type Box = { x: number; y: number; width: number; height: number };
const overlaps = (a: Box, b: Box) => a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;

async function checkCluster(page: Page, container: Locator, name: string): Promise<void> {
  const actions = container.locator(ACTIONS);
  await expect(actions.first(), `${name}: has actions`).toBeAttached();
  const count = await actions.count();
  expect(count, `${name}: has actions`).toBeGreaterThan(0);
  for (let i = 0; i < count; i++) {
    const button = actions.nth(i);
    await button.scrollIntoViewIfNeeded();
    const label = (await button.getAttribute('aria-label')) ?? '';
    expect(label, `${name}: action ${i} has an aria-label`).not.toBe('');
    await button.hover({ force: true });
    const tip = page.locator('.mat-mdc-tooltip');
    await expect(tip.first(), `${name}: "${label}" shows a tooltip`).toBeVisible();
    await expect(tip.first(), `${name}: the tooltip of "${label}" is its name`).toHaveText(label);
    const tipBox = (await tip.first().boundingBox())!;
    const own = (await button.boundingBox())!;
    for (let j = 0; j < count; j++) {
      if (j === i) continue;
      const other = (await actions.nth(j).boundingBox())!;
      expect(overlaps(tipBox, other), `${name}: the tooltip of action ${i} (${JSON.stringify(own)}) must not cover action ${j} (${JSON.stringify(other)}); tooltip ${JSON.stringify(tipBox)}`).toBe(false);
    }
    await page.mouse.move(0, 0);
    await expect(tip).toHaveCount(0);
  }
}

test.describe('row-action tooltips', () => {
  test('counters: search results, charted table and pinned list', async ({ page }) => {
    await page.goto(url('/counters'));
    await page.getByRole('searchbox', { name: 'Search counters' }).fill('bytes');
    const results = page.getByRole('list', { name: 'Search results' });
    await expect(results).toBeVisible();
    await checkCluster(page, results, 'counters search results');
    for (const n of [/^Chart Bytes Received \(en0\)/, /^Chart Bytes Sent \(en0\)/]) await results.getByRole('button', { name: n }).click();
    await page.mouse.move(0, 0);
    await checkCluster(page, page.getByRole('table', { name: 'Charted counters' }), 'counters charted table');
    await checkCluster(page, page.getByRole('list', { name: 'Pinned counters' }), 'counters pinned list');
  });

  test('alarms: active rows and custom rules', async ({ page }) => {
    await page.goto(url('/alarms'));
    await expect(page.getByRole('button', { name: /Acknowledge all/ })).toBeVisible();
    await checkCluster(page, page.locator('.grid').first(), 'alarms active');
    await page.getByRole('tab', { name: 'Rules' }).click();
    await checkCluster(page, page.getByRole('table', { name: 'Custom alert rules' }), 'custom rules');
  });

  test('network: connections and listeners', async ({ page }) => {
    await page.goto(url('/network'));
    const grid = page.getByRole('grid', { name: 'Connections' });
    await expect(grid.getByRole('row', { name: /Google Chrome/ }).first()).toBeVisible();
    await checkCluster(page, grid, 'network connections');
    await page.getByRole('radio', { name: 'Listening' }).click();
    await checkCluster(page, page.getByRole('grid', { name: 'Listening ports' }), 'network listeners');
  });

  test('issues, backup health and maintenance windows', async ({ page }) => {
    await page.goto(url('/issues'));
    const issues = page.getByRole('table', { name: 'Issues', exact: true });
    await issues.getByRole('row', { name: /Safari/ }).first().click();
    await checkCluster(page, issues, 'issues');
    await page.goto(url('/backup'));
    await checkCluster(page, page.getByRole('table', { name: 'Backup tools' }), 'backup health');
    await page.goto(url('/extras?tab=maintenance'));
    await checkCluster(page, page.getByRole('list', { name: 'Windows' }), 'maintenance windows');
  });
});
