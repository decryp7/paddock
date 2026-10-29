import AxeBuilder from '@axe-core/playwright';
import { clickShape, expect, hoverShape, test, type Page } from '@decryp7/paddock-e2e';
import { mkdirSync } from 'node:fs';

// `pd-sunburst` in the gallery (docs/plans/pd-sunburst.md): drill by click and keyboard, the centre going up, the states, axe in both
// themes. `SUNBURST_SHOTS=<dir>` also writes light and dark screenshots of the section for the independent review (rule 22).

const THEMES = ['dark', 'light'] as const;

/** Resolves once the sunburst section has kept the same top for five frames (the charts above it have finished drawing). */
async function layoutStable(page: Page): Promise<void> {
  await page.waitForFunction(
    () =>
      new Promise<boolean>((resolve) => {
        const el = document.querySelector('section[data-testid="sunburst"]');
        let last = Number.NaN;
        let same = 0;
        const tick = () => {
          const top = el ? el.getBoundingClientRect().top : Number.NaN;
          same = top === last ? same + 1 : 0;
          last = top;
          if (same >= 5) resolve(true);
          else requestAnimationFrame(tick);
        };
        tick();
      }),
  );
}

async function open(page: Page, theme: string): Promise<void> {
  await page.setViewportSize({ width: 1280, height: 1000 });
  await page.goto(`/?theme=${theme}&accent=debrief`);
  // The charts above the section draw late and move it: wait for them, so a point measured on a segment is still there when the mouse goes.
  await page.locator('section[data-testid="chart"] canvas').first().waitFor();
  await page.locator('section[data-testid="sunburst"] [role=tree]').first().waitFor();
  await layoutStable(page);
}

const box = (page: Page) => page.locator('[data-testid="sunburst-box"]');
// Real pointer events go to a point inside the segment: the centre of an annular segment's box is usually in the hole of the ring.
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
  await hoverShape(page, item(page, 'users'));
  await expect(centreName(page)).toHaveText('Users');
  await clickShape(page, item(page, 'users'));
  // The pointer is now over a child of Users, which the centre would show: move it away to read the centre itself.
  await page.mouse.move(0, 0);
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

test('sunburst: at a narrow width the centre text stays inside the circle and labels stay at reading size', async ({ page }) => {
  await open(page, 'dark');
  const narrow = page.locator('[data-testid="sunburst-narrow-box"] pd-sunburst');
  const width = (await narrow.boundingBox())?.width ?? 0;
  expect(width).toBeLessThan(320);
  const overruns = await narrow.locator('.pd-centre').evaluate((el) => Array.from(el.children).filter((c) => c.scrollWidth > c.clientWidth + 1).map((c) => `${c.className}: ${c.scrollWidth} > ${c.clientWidth}`));
  expect(overruns).toEqual([]);
  const px = await narrow.locator('text.pd-label').evaluateAll((els) => els.map((e) => (e.getBoundingClientRect().height)));
  expect(px.every((h) => h >= 8)).toBe(true);
  const design = await page.locator('[data-testid="sunburst-box"] pd-sunburst').boundingBox();
  expect(design?.width ?? 0).toBeGreaterThan(500);
});

const shots = process.env['SUNBURST_SHOTS'];
if (shots) {
  for (const theme of THEMES) {
    test(`sunburst screenshots (${theme})`, async ({ page }) => {
      mkdirSync(shots, { recursive: true });
      await open(page, theme);
      await page.setViewportSize({ width: 1280, height: 1500 });
      await layoutStable(page);
      await page.locator('section[data-testid="sunburst"]').screenshot({ path: `${shots}/pd-sunburst-${theme}.png`, animations: 'disabled' });
      await clickShape(page, item(page, 'users'));
      await hoverShape(page, item(page, 'alex'));
      await expect(item(page, 'alex')).toHaveClass(/pd-arc--active/);
      await page.locator('section[data-testid="sunburst"]').screenshot({ path: `${shots}/pd-sunburst-${theme}-drilled-hover.png`, animations: 'disabled' });
      await page.locator('[data-testid="sunburst-narrow-box"]').screenshot({ path: `${shots}/pd-sunburst-${theme}-narrow.png`, animations: 'disabled' });
    });
  }
}
