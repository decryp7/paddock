import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@decryp7/paddock-e2e';
import { mkdirSync } from 'node:fs';

// `pd-sunburst` in the gallery (docs/plans/pd-sunburst.md): drill by click and keyboard, the centre going up, the states, axe in both
// themes. `SUNBURST_SHOTS=<dir>` also writes light and dark screenshots of the section for the independent review (rule 22).

const THEMES = ['dark', 'light'] as const;

async function open(page: Page, theme: string): Promise<void> {
  await page.setViewportSize({ width: 1280, height: 1000 });
  await page.goto(`/?theme=${theme}&accent=debrief`);
  await page.locator('section[data-testid="sunburst"] [role=tree]').first().waitFor();
}

const box = (page: Page) => page.locator('[data-testid="sunburst-box"]');
// A segment's box centre is often in the hole of the ring, so pointer events are sent to the segment itself.
const item = (page: Page, id: string) => box(page).locator(`path[role=treeitem][data-id="${id}"]`);
const centreName = (page: Page) => box(page).locator('[data-testid="sunburst-centre-name"]');

for (const theme of THEMES) {
  test(`sunburst: axe finds no violations (${theme})`, async ({ page }) => {
    await open(page, theme);
    const results = await new AxeBuilder({ page }).include('section[data-testid="sunburst"]').withTags(['wcag2a', 'wcag2aa', 'wcag22aa']).analyze();
    expect(results.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`)).toEqual([]);
  });
}

test('sunburst: click drills in, the centre goes up, hover shows the segment', async ({ page }) => {
  await open(page, 'dark');
  await expect(centreName(page)).toHaveText('Macintosh HD');
  await item(page, 'users').dispatchEvent('mouseenter');
  await expect(centreName(page)).toHaveText('Users');
  await item(page, 'users').dispatchEvent('click');
  await expect(page.locator('[data-testid="sunburst-centre"]')).toHaveText('Centre: users');
  await expect(centreName(page)).toHaveText('Users');
  await expect(box(page).getByRole('button', { name: 'Up to Macintosh HD' })).toBeVisible();
  await box(page).getByRole('button', { name: 'Up to Macintosh HD' }).click();
  await expect(page.locator('[data-testid="sunburst-centre"]')).toHaveText('Centre: root');
});

test('sunburst: the keyboard moves among segments and Enter drills', async ({ page }) => {
  await open(page, 'light');
  const at = (id: string) => item(page, id);
  await at('users').focus();
  await page.keyboard.press('ArrowDown');
  await expect(at('apps')).toBeFocused();
  await page.keyboard.press('ArrowRight');
  await expect(at('xcode')).toBeFocused();
  await page.keyboard.press('ArrowLeft');
  await expect(at('apps')).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(page.locator('[data-testid="sunburst-centre"]')).toHaveText('Centre: apps');
  await expect(box(page).locator('[data-testid=sunburst-live]')).toHaveText('Applications, 19.0 GB');
});

test('sunburst: hidden space is hatched, and loading, empty and error are designed', async ({ page }) => {
  await open(page, 'dark');
  await expect(box(page).locator('path[data-id="hidden"]')).toHaveAttribute('aria-label', /Hidden space, 8\.0 GB/);
  await expect(page.locator('[data-testid="sunburst-loading-box"] [role=status]')).toBeVisible();
  await expect(page.locator('[data-testid="sunburst-empty-box"]')).toContainText('Nothing scanned yet');
  await expect(page.locator('[data-testid="sunburst-error-box"]')).toContainText('The scan stopped');
});

const shots = process.env['SUNBURST_SHOTS'];
if (shots) {
  for (const theme of THEMES) {
    test(`sunburst screenshots (${theme})`, async ({ page }) => {
      mkdirSync(shots, { recursive: true });
      await open(page, theme);
      await page.waitForTimeout(400);
      await page.locator('section[data-testid="sunburst"]').screenshot({ path: `${shots}/pd-sunburst-${theme}.png`, animations: 'disabled' });
      await item(page, 'users').dispatchEvent('click');
      await item(page, 'alex').dispatchEvent('mouseenter');
      await page.waitForTimeout(400);
      await page.locator('section[data-testid="sunburst"]').screenshot({ path: `${shots}/pd-sunburst-${theme}-drilled-hover.png`, animations: 'disabled' });
    });
  }
}
