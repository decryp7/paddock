import { mkdirSync } from 'node:fs';
import { join } from 'node:path';
import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';

// Theme switching (plan S15): system / dark / light, high contrast and reduced motion, the same in every window of the
// app (here: tabs and a dialog window of one origin, through the BroadcastChannel backend), kept across a reload.

const html = (page: Page) => page.locator('html');
const token = (page: Page, name: string) => page.evaluate((n) => getComputedStyle(document.documentElement).getPropertyValue(n).trim(), name);

async function open(page: Page, path = '/'): Promise<void> {
  await page.goto(path);
  await expect(page.getByTestId('app-context')).toContainText('loaded');
}

async function choose(page: Page, item: string, role: 'menuitemcheckbox' = 'menuitemcheckbox'): Promise<void> {
  await page.getByRole('menubar').getByRole('menuitem', { name: 'View', exact: true }).click();
  await page.getByRole('menu', { name: 'View' }).getByRole('menuitem', { name: 'Appearance' }).click();
  await page.getByRole('menu', { name: 'Appearance' }).getByRole(role, { name: item }).click();
  await expect(page.getByRole('menu')).toHaveCount(0);
}

test.beforeEach(async ({ context }) => {
  await context.addInitScript(() => {
    if (sessionStorage.getItem('paddock.e2e.seeded')) return;
    sessionStorage.setItem('paddock.e2e.seeded', '1');
    localStorage.removeItem('paddock.appearance');
  });
});

test('system follows the OS; dark and light are fixed; the choice is kept across a reload', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'light' });
  await open(page);
  await expect(html(page)).not.toHaveAttribute('data-theme');
  await expect(html(page)).toHaveAttribute('data-accent', 'pitwall');
  await expect.poll(() => token(page, '--pd-bg')).toBe('#f4f5f7');
  await choose(page, 'Theme: dark');
  await expect(html(page)).toHaveAttribute('data-theme', 'dark');
  await expect.poll(() => token(page, '--pd-bg')).toBe('#0e0f12');
  await page.reload();
  await expect(html(page)).toHaveAttribute('data-theme', 'dark');
  await choose(page, 'Theme: system');
  await expect(html(page)).not.toHaveAttribute('data-theme');
  await page.emulateMedia({ colorScheme: 'dark' });
  await expect.poll(() => token(page, '--pd-bg')).toBe('#0e0f12');
});

test('a theme change in one window is shown in the others at once, a dialog window included', async ({ context, page }) => {
  await open(page);
  const other = await context.newPage();
  await open(other);
  const dialog = await context.newPage();
  await dialog.goto(`/#/dialog?id=sd.about&host=main&label=d-main-1&title=About&dialogInput=${encodeURIComponent('{}')}`);
  await expect(dialog.locator('pd-dialog-shell')).toHaveAttribute('data-kind', 'viewer');
  await choose(page, 'Theme: light');
  await expect(html(other)).toHaveAttribute('data-theme', 'light');
  await expect(html(dialog)).toHaveAttribute('data-theme', 'light');
  await choose(other, 'High contrast');
  await expect(html(page)).toHaveAttribute('data-contrast', 'more');
  await expect(html(dialog)).toHaveAttribute('data-contrast', 'more');
  await other.close();
  await dialog.close();
});

test('high contrast: hairlines become control borders, popovers lose their shadow; axe finds nothing in either scheme', async ({ page }) => {
  await open(page);
  await expect.poll(() => token(page, '--pd-hairline')).not.toBe(await token(page, '--pd-border-control'));
  await choose(page, 'High contrast');
  await expect(html(page)).toHaveAttribute('data-contrast', 'more');
  await expect.poll(() => token(page, '--pd-hairline')).toBe(await token(page, '--pd-border-control'));
  await expect.poll(() => token(page, '--pd-shadow-1')).toBe('none');
  for (const theme of ['Theme: dark', 'Theme: light']) {
    await choose(page, theme);
    const results = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag22aa']).analyze();
    expect(results.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`), theme).toEqual([]);
  }
  // Off again: the menu toggle sets "standard", which also overrides the OS preference.
  await choose(page, 'High contrast');
  await expect(html(page)).toHaveAttribute('data-contrast', 'off');
});

test('a theme pack recolours the tokens it names, is removed when turned off, and never applies in high contrast', async ({ page }) => {
  await open(page);
  await choose(page, 'Theme: light');
  await choose(page, 'Warm paper');
  await expect(html(page)).toHaveAttribute('data-pack', 'sd.theme.warm-paper');
  await expect.poll(() => token(page, '--pd-bg')).toBe('#f6f3ee');
  await choose(page, 'Theme: dark');
  await expect.poll(() => token(page, '--pd-bg')).toBe('#12100e');
  await choose(page, 'High contrast');
  await expect(html(page)).not.toHaveAttribute('data-pack');
  await expect.poll(() => token(page, '--pd-bg')).toBe('#0e0f12');
  await choose(page, 'High contrast');
  await expect(html(page)).toHaveAttribute('data-pack', 'sd.theme.warm-paper');
  await choose(page, 'Warm paper');
  await expect(html(page)).not.toHaveAttribute('data-pack');
  await expect.poll(() => token(page, '--pd-bg')).toBe('#0e0f12');
});

test('the OS asking for more contrast applies high contrast unless the app turned it off', async ({ page }) => {
  await page.emulateMedia({ contrast: 'more' });
  await open(page);
  await expect.poll(() => token(page, '--pd-hairline')).toBe(await token(page, '--pd-border-control'));
  await choose(page, 'High contrast');
  await expect(html(page)).toHaveAttribute('data-contrast', 'off');
  await expect.poll(() => token(page, '--pd-hairline')).not.toBe(await token(page, '--pd-border-control'));
});

test('reduced motion: the menu toggle and the OS setting shorten every duration to 100 ms and stop translation', async ({ page }) => {
  await open(page);
  await expect.poll(() => token(page, '--pd-motion-scale')).toBe('1');
  await choose(page, 'Reduce motion');
  await expect(html(page)).toHaveAttribute('data-motion', 'reduce');
  await expect.poll(() => token(page, '--pd-duration-large')).toBe('100ms');
  await expect.poll(() => token(page, '--pd-motion-scale')).toBe('0');
  const transition = await page.locator('pd-jobs-status .pill').evaluate((el) => getComputedStyle(el).transitionDuration);
  expect(transition.split(',').every((d) => Number.parseFloat(d) <= 0.0001)).toBe(true);
  await choose(page, 'Reduce motion');
  await expect(html(page)).toHaveAttribute('data-motion', 'full');

  await page.emulateMedia({ reducedMotion: 'reduce' });
  await expect.poll(() => token(page, '--pd-duration-large')).not.toBe('100ms');
  await page.evaluate(() => localStorage.removeItem('paddock.appearance'));
  await page.reload();
  await expect(html(page)).not.toHaveAttribute('data-motion');
  await expect.poll(() => token(page, '--pd-duration-large')).toBe('100ms');
});

test.describe('screenshots (independent review)', () => {
  test.skip(!process.env['SCREENSHOTS'], 'set SCREENSHOTS=1 to rewrite docs/screenshots');
  const root = join(__dirname, '..', 'docs', 'screenshots', 'shell-demo');

  for (const [name, theme, contrast] of [
    ['dark', 'dark', 'standard'],
    ['light', 'light', 'standard'],
    ['high-contrast-dark', 'dark', 'more'],
    ['high-contrast-light', 'light', 'more'],
  ] as const) {
    test(`shell demo ${name}`, async ({ page }) => {
      await page.addInitScript(
        ([t, c]) => localStorage.setItem('paddock.appearance', JSON.stringify({ version: 1, theme: t, contrast: c, motion: 'reduce', density: 'standard', pack: null })),
        [theme, contrast],
      );
      await open(page);
      await page.locator('button.cmd[data-command="sd.job.start"]').click();
      await page.locator('button.cmd[data-command="sd.engine.break"]').click();
      await page.getByTestId('service-health-chip').click();
      mkdirSync(root, { recursive: true });
      await page.screenshot({ path: join(root, `${name}.png`), fullPage: false });
    });
  }

  test('shell demo pseudo-locale (dark, the View menu open)', async ({ page }) => {
    await page.addInitScript(() => {
      localStorage.setItem('paddock.appearance', JSON.stringify({ version: 1, theme: 'dark', contrast: 'standard', motion: 'reduce', density: 'standard', pack: null }));
      localStorage.setItem('paddock.language', JSON.stringify({ version: 1, language: 'pseudo' }));
    });
    await open(page);
    await page.locator('button.cmd[data-command="sd.job.start"]').click();
    await page.getByRole('menubar').getByRole('menuitem').nth(1).click();
    await page.getByRole('menu').first().locator('[aria-haspopup]').first().click();
    mkdirSync(root, { recursive: true });
    await page.screenshot({ path: join(root, 'pseudo-locale-dark.png'), fullPage: false });
  });
});
