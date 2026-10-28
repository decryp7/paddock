import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@decryp7/paddock-e2e';
import { mkdirSync } from 'node:fs';
import { join } from 'node:path';

const THEMES = ['dark', 'light'] as const;
/** Set MATERIAL_SHOTS to a folder to also write the review screenshots of the page there. */
const SHOTS = process.env['MATERIAL_SHOTS'];

async function open(page: Page, theme: string, accent = 'debrief'): Promise<void> {
  await page.setViewportSize({ width: 1280, height: 4200 });
  await page.goto(`/?theme=${theme}&accent=${accent}&density=standard`);
  await page.locator('[data-testid="material"] .mat-mdc-unelevated-button').first().waitFor();
}

const section = (page: Page) => page.getByTestId('material');
const bg = (page: Page, selector: string) => page.locator(selector).first().evaluate((e) => getComputedStyle(e).backgroundColor);
const token = (page: Page, name: string) =>
  page.evaluate((n) => {
    const probe = document.createElement('div');
    probe.style.setProperty('background-color', `var(${n})`);
    document.body.append(probe);
    const colour = getComputedStyle(probe).backgroundColor;
    probe.remove();
    return colour;
  }, name);

for (const theme of THEMES) {
  test(`Material controls: axe finds no violations in the section (${theme})`, async ({ page }) => {
    await open(page, theme);
    const results = await new AxeBuilder({ page }).include('[data-testid="material"]').withTags(['wcag2a', 'wcag2aa', 'wcag22aa']).analyze();
    expect(results.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`)).toEqual([]);
  });

  test(`Material controls: buttons and fields follow the tokens (${theme})`, async ({ page }) => {
    await open(page, theme);
    // filled = primary: the accent fill; outlined = secondary: surface-2; danger: the danger fill.
    expect(await bg(page, '[data-testid="material"] .mat-mdc-unelevated-button:not(.pd-button-danger)')).toBe(await token(page, '--pd-accent-fill'));
    expect(await bg(page, '[data-testid="material"] .mat-mdc-outlined-button:not(.pd-button-danger):not([disabled])')).toBe(await token(page, '--pd-surface-2'));
    expect(await bg(page, '[data-testid="material"] .mat-mdc-unelevated-button.pd-button-danger')).toBe(await token(page, '--pd-danger-fill'));
    // The button is the family's button height (--pd-button-height), not Material's 40 px.
    const height = await section(page).locator('.mat-mdc-unelevated-button').first().evaluate((e) => e.getBoundingClientRect().height);
    const want = await page.evaluate(() => parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--pd-button-height')));
    expect(height).toBe(want);
    // Fields are outlined with the label always floated, by providePaddockMaterial alone.
    const field = section(page).locator('mat-form-field').first();
    await expect(field).toHaveClass(/mat-form-field-appearance-outline/);
    await expect(field).toHaveClass(/mat-mdc-form-field-label-always-float/);
  });

  test(`Material controls: a real select, menu and tooltip open themed (${theme})`, async ({ page }) => {
    await open(page, theme);
    await section(page).getByRole('combobox', { name: 'Level' }).click();
    const panel = page.locator('.mat-mdc-select-panel').last();
    await expect(panel).toBeVisible();
    expect(await panel.evaluate((e) => getComputedStyle(e).backgroundColor)).toBe(await token(page, '--pd-surface-3'));
    await page.keyboard.press('Escape');
    await page.getByTestId('material-menu').click();
    const menu = page.locator('.mat-mdc-menu-panel').last();
    await expect(menu).toBeVisible();
    expect(await menu.evaluate((e) => getComputedStyle(e).backgroundColor)).toBe(await token(page, '--pd-surface-3'));
    await page.keyboard.press('Escape');
    await page.getByTestId('material-tooltip').hover();
    await expect(page.locator('.mdc-tooltip__surface').last()).toBeVisible();
  });

  test(`Material controls: screenshot (${theme})`, async ({ page }) => {
    test.skip(!SHOTS, 'set MATERIAL_SHOTS to a folder to write the review screenshots');
    mkdirSync(SHOTS as string, { recursive: true });
    await open(page, theme);
    await page.waitForTimeout(400);
    await section(page).screenshot({ path: join(SHOTS as string, `material-controls-${theme}.png`) });
    // The live overlays, one at a time, in an ordinary window.
    await page.setViewportSize({ width: 1280, height: 900 });
    const shot = async (name: string) => page.screenshot({ path: join(SHOTS as string, `material-${name}-${theme}.png`) });
    await section(page).getByRole('combobox', { name: 'Level' }).scrollIntoViewIfNeeded();
    await section(page).getByRole('combobox', { name: 'Level' }).click();
    await shot('select-open');
    await page.keyboard.press('Escape');
    await page.getByTestId('material-menu').click();
    await shot('menu-open');
    await page.keyboard.press('Escape');
    await page.getByTestId('material-tooltip').hover();
    await page.locator('.mdc-tooltip__surface').last().waitFor();
    await shot('tooltip');
    await page.getByTestId('material-snack').click();
    await page.locator('.mat-mdc-snack-bar-container').waitFor();
    await page.waitForTimeout(500);
    await shot('snack-bar');
  });
}
