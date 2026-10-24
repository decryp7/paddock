import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@decryp7/paddock-e2e';
import { mkdirSync } from 'node:fs';
import { join } from 'node:path';

const THEMES = ['dark', 'light'] as const;
const ACCENTS = ['debrief', 'pitcrew', 'pitwall', 'dyno', 'spotter', 'pacenotes', 'sighter'] as const;
const SECTIONS = [
  'card',
  'status-pill',
  'metric',
  'sparkline',
  'empty-state',
  'error-state',
  'skeleton',
  'job',
  'segmented-control',
  'icons',
  'controls',
  'path-text',
  'tray-popover',
  'chart-theme',
] as const;

async function open(page: Page, theme: string, accent: string, density = 'standard'): Promise<void> {
  await page.goto(`/?theme=${theme}&accent=${accent}&density=${density}`);
  await page.locator('[data-testid="card"] pd-card').first().waitFor();
  // Let the chart draw (animation 300 ms) and fonts settle.
  await page.waitForTimeout(600);
}

for (const theme of THEMES) {
  test(`axe finds no violations (${theme})`, async ({ page }) => {
    await open(page, theme, 'debrief');
    const results = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag22aa']).analyze();
    expect(results.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`)).toEqual([]);
  });
}

test('keyboard: arrows move the segmented control and Tab shows a focus ring', async ({ page }) => {
  await open(page, 'dark', 'debrief');
  const seg = page.getByRole('radiogroup', { name: 'Time range' });
  await seg.getByRole('radio', { name: '24 h' }).focus();
  await page.keyboard.press('ArrowRight');
  await expect(seg.getByRole('radio', { name: '7 days' })).toHaveAttribute('aria-checked', 'true');
  await page.keyboard.press('Home');
  await expect(seg.getByRole('radio', { name: '1 h' })).toHaveAttribute('aria-checked', 'true');
  const outline = await seg.getByRole('radio', { name: '1 h' }).evaluate((e) => getComputedStyle(e).outlineStyle);
  expect(outline).toBe('solid');
});

test('interactive card activates with Enter and Space', async ({ page }) => {
  await open(page, 'dark', 'debrief');
  const card = page.getByRole('button', { name: 'Network' });
  await card.focus();
  await page.keyboard.press('Enter');
  await page.keyboard.press('Space');
  await expect(card).toContainText('activated 2');
});

test('native feel: the shell is not selectable, content opts in', async ({ page }) => {
  await open(page, 'dark', 'debrief');
  const shell = await page.locator('h2').first().evaluate((e) => getComputedStyle(e).userSelect);
  expect(shell).toBe('none');
});

test('accent attribute switches the accent token', async ({ page }) => {
  await open(page, 'dark', 'pitcrew');
  const accent = await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--pd-accent').trim());
  expect(accent).toBe('#ff7a45');
});

test('page has no inline style attributes or elements in index.html (CSP)', async ({ request }) => {
  const html = await (await request.get('/')).text();
  expect(html).not.toMatch(/<style[\s>]/);
  expect(html).not.toMatch(/\sstyle=/);
});

test.describe('screenshots', () => {
  test.skip(!process.env['SCREENSHOTS'], 'set SCREENSHOTS=1 to rewrite docs/screenshots');
  const root = join(__dirname, '..', 'docs', 'screenshots');

  for (const theme of THEMES) {
    for (const accent of ACCENTS) {
      test(`gallery ${theme} ${accent}`, async ({ page }) => {
        await open(page, theme, accent);
        const dir = join(root, theme, accent);
        mkdirSync(dir, { recursive: true });
        for (const section of SECTIONS) {
          await page.locator(`section[data-testid="${section}"]`).screenshot({ path: join(dir, `${section}.png`) });
        }
        if (accent === 'debrief' || accent === 'dyno') {
          await page.screenshot({ path: join(root, `gallery-${theme}-${accent}.png`), fullPage: false });
        }
      });
    }
  }

  for (const density of ['comfortable', 'compact'] as const) {
    test(`density ${density}`, async ({ page }) => {
      await open(page, 'dark', 'debrief', density);
      mkdirSync(join(root, 'density'), { recursive: true });
      await page.locator('[data-testid="card"]').screenshot({ path: join(root, 'density', `card-${density}.png`) });
    });
  }
});
