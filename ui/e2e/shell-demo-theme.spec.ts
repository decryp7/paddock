import { axeViolations, expect, test, type Page } from '@decryp7/paddock-e2e';

// Theme switching (plan S15): system / dark / light, high contrast and reduced motion, the same in every window of the
// app (here: tabs and a dialog window of one origin, through the BroadcastChannel backend), kept across a reload. That
// only the appearance store writes the look (rule 40) is checked in shell/testing's reference-sources spec.

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
    localStorage.removeItem('sd.settings');
  });
});

test('system follows the OS; dark and light are fixed; the choice is kept across a reload', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'light' });
  await open(page);
  await expect(html(page)).not.toHaveAttribute('data-theme');
  await expect(html(page)).toHaveAttribute('data-accent', 'graphite');
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

test('a theme change in one window is shown in the others at once, a dialog window included', async ({ app, context, page }) => {
  await open(page);
  const other = await context.newPage();
  await open(other);
  const dialog = await context.newPage();
  await dialog.goto(app.dialogs.url('sd.about', {}, { title: 'About' }));
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
  // No colour transitions: axe measures the colours of the scheme just chosen, not a frame in between.
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await open(page);
  await expect.poll(() => token(page, '--pd-hairline')).not.toBe(await token(page, '--pd-border-control'));
  await choose(page, 'High contrast');
  await expect(html(page)).toHaveAttribute('data-contrast', 'more');
  await expect.poll(() => token(page, '--pd-hairline')).toBe(await token(page, '--pd-border-control'));
  await expect.poll(() => token(page, '--pd-shadow-1')).toBe('none');
  for (const [theme, scheme] of [
    ['Theme: dark', 'dark'],
    ['Theme: light', 'light'],
  ] as const) {
    await choose(page, theme);
    await expect(html(page)).toHaveAttribute('data-theme', scheme);
    // Two frames: the new scheme's styles are applied everywhere before axe reads colours.
    await page.evaluate(() => new Promise<void>((r) => requestAnimationFrame(() => requestAnimationFrame(() => r()))));
    expect(await axeViolations(page), theme).toEqual([]);
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
  const transition = await page.locator('pd-jobs-status .pd-pill').evaluate((el) => getComputedStyle(el).transitionDuration);
  expect(transition.split(',').every((d) => Number.parseFloat(d) <= 0.0001)).toBe(true);
  await choose(page, 'Reduce motion');
  await expect(html(page)).toHaveAttribute('data-motion', 'full');

  await page.emulateMedia({ reducedMotion: 'reduce' });
  await expect.poll(() => token(page, '--pd-duration-large')).not.toBe('100ms');
  await page.evaluate(() => (localStorage.removeItem('paddock.appearance'), localStorage.removeItem('sd.settings')));
  await page.reload();
  await expect(html(page)).not.toHaveAttribute('data-motion');
  await expect.poll(() => token(page, '--pd-duration-large')).toBe('100ms');
});

test('the macOS-less menu bar names the app once: in the title bar, its menu entries under File', async ({ page }) => {
  await open(page);
  await expect(page.getByRole('menubar').getByRole('menuitem')).toHaveText(['File', 'View', 'Help']);
  await expect(page.getByText('Paddock Shell Demo', { exact: true })).toHaveCount(1);
  await page.getByRole('menubar').getByRole('menuitem', { name: 'File', exact: true }).click();
  await expect(page.getByRole('menu', { name: 'File' }).getByRole('menuitem', { name: 'Settings…' })).toBeVisible();
  await page.keyboard.press('Escape');
});

test('a View-menu change and a Settings change in another window never revert each other', async ({ context, page }) => {
  await open(page);
  const other = await context.newPage();
  await open(other);
  // Window A: View > Appearance > High contrast. Window B: Settings > Appearance > Theme: Light.
  await choose(page, 'High contrast');
  await other.getByRole('button', { name: 'Settings…', exact: true }).click();
  const win = other.getByRole('dialog', { name: 'Settings' });
  await win.locator('pd-setting-field[data-key="theme"]').getByRole('combobox').click();
  await other.getByRole('option', { name: 'Light', exact: true }).click();
  for (const p of [page, other]) {
    await expect(html(p)).toHaveAttribute('data-theme', 'light');
    await expect(html(p)).toHaveAttribute('data-contrast', 'more');
  }
  // The Settings window shows the menu's change too, and A's next change keeps B's theme.
  await expect(win.locator('pd-setting-field[data-key="contrast"]').getByRole('combobox')).toHaveText('High contrast');
  await choose(page, 'Reduce motion');
  for (const p of [page, other]) {
    await expect(html(p)).toHaveAttribute('data-motion', 'reduce');
    await expect(html(p)).toHaveAttribute('data-theme', 'light');
    await expect(html(p)).toHaveAttribute('data-contrast', 'more');
  }
  await other.close();
});
