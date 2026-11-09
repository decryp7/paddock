import type { Locator, Page } from '@playwright/test';
import { expect, test } from '@decryp7/paddock-e2e';

// ER-21: the shared Material surfaces keep the family's control geometry and type under a hostile host theme.
//
// Root cause (Material 22.2.4): `mat.theme` emits density tokens only when the scale is not 0, so an app that loads no
// `paddock-ui/css/material.css` (Pitcrew: its angular.json lists none) gets Material's own defaults (56px fields, 20px type, pill
// buttons), and an app with its own theme at another density (Pitwall: -2 on :root) fights the bridge. The family geometry is one
// section of `_material.scss`, set on `body`: a custom property on a descendant beats any html/:root declaration, and the cdk
// overlays inherit it. The hostile theme below is what such an app declares on :root.

const HOSTILE = `:root:root { --mat-form-field-container-height: 56px; --mat-form-field-container-vertical-padding: 16px;
  --mat-form-field-container-text-size: 20px; --mat-select-trigger-text-size: 20px; --mat-form-field-subscript-text-size: 20px;
  --mat-button-outlined-container-height: 48px; --mat-button-outlined-container-shape: 24px; --mat-button-filled-container-height: 48px;
  --mat-button-filled-container-shape: 24px; --mat-button-text-container-height: 48px; --mat-button-text-container-shape: 24px;
  --mat-button-toggle-height: 56px; --mat-button-toggle-shape: 28px; --mat-icon-button-state-layer-size: 56px; }`;

/** A px length for the CSS value `value` (a var() resolves where it is used), measured on a probe in the page. */
async function px(page: Page, property: string, value: string): Promise<number> {
  return page.evaluate(
    ([prop, val]) => {
      const probe = document.createElement('div');
      probe.style.setProperty(prop!, val!);
      document.body.append(probe);
      const n = parseFloat(getComputedStyle(probe).getPropertyValue(prop!));
      probe.remove();
      return n;
    },
    [property, value],
  );
}

const token = (loc: Locator, name: string): Promise<string> => loc.evaluate((el, n) => getComputedStyle(el).getPropertyValue(n).trim(), name);
const fontSize = (loc: Locator): Promise<number> => loc.evaluate((el) => parseFloat(getComputedStyle(el).fontSize));

/** What the bridge's rules that already size a field would hide: drop every `.mat-mdc-form-field`/`.mat-mdc-select` rule, so only the tokens decide a field's height. */
async function dropFieldRules(page: Page): Promise<void> {
  await page.evaluate(() => {
    for (const sheet of [...document.styleSheets]) {
      try {
        for (let i = sheet.cssRules.length - 1; i >= 0; i--) {
          const rule = sheet.cssRules[i] as CSSStyleRule;
          if (/\.mat-mdc-(form-field|select|text-field)|\.mdc-text-field/.test(rule.selectorText ?? '')) sheet.deleteRule(i);
        }
      } catch {
        // a cross-origin sheet: none here
      }
    }
  });
}

test.describe('shared settings and panels keep the family density under a host theme (ER-21)', () => {
  test('Appearance: the select text keeps the family body size, not the host theme’s', async ({ app, page }) => {
    await app.settings.open({ page: 'pd.appearance' });
    await page.addStyleTag({ content: HOSTILE });
    const body = await px(page, 'font-size', 'var(--pd-text-body)');
    for (const key of ['theme', 'contrast', 'motion', 'density']) {
      expect(await fontSize(app.settings.field(key).locator('.mat-mdc-select-value')), key).toBe(body);
    }
  });

  test('Appearance with the field rules of material.css removed: only the tokens size a field, and they stay at the family height', async ({ app, page }) => {
    await app.settings.open({ page: 'pd.appearance' });
    await dropFieldRules(page);
    await page.addStyleTag({ content: HOSTILE });
    // A labelled field is the control height plus the label's room, the family token --pd-field-height (117710c8e); not the bare control height.
    const limit = await px(page, 'height', 'var(--pd-field-height)');
    for (const key of ['theme', 'density']) {
      const box = await app.settings.field(key).locator('mat-form-field').boundingBox();
      expect(box!.height, key).toBeLessThanOrEqual(limit + 2);
    }
  });

  test('Appearance: Reset section stays at the family button height and square shape', async ({ app, page }) => {
    await app.settings.open({ page: 'pd.appearance' });
    await page.addStyleTag({ content: HOSTILE });
    const reset = page.getByRole('button', { name: 'Reset section' });
    expect((await reset.boundingBox())!.height).toBeLessThanOrEqual((await px(page, 'height', 'var(--pd-button-height)')) + 1);
    expect(await reset.evaluate((el) => parseFloat(getComputedStyle(el).borderTopLeftRadius))).toBeLessThan(12);
  });

  test('Notification channels: the field text and the buttons stay at the family size', async ({ app, page }) => {
    await app.settings.open({ page: 'pd.channels' }, '?channels=empty');
    await page.addStyleTag({ content: HOSTILE });
    await page.getByTestId('channel-add-empty').click();
    expect(await fontSize(page.locator('#pd-ch-type').locator('xpath=ancestor::pd-select').locator('.mat-mdc-select-value'))).toBe(await px(page, 'font-size', 'var(--pd-text-body)'));
    expect((await page.getByTestId('channel-save').boundingBox())!.height).toBeLessThanOrEqual((await px(page, 'height', 'var(--pd-button-height)')) + 1);
  });

  test('an overlay (the select panel) inherits the family tokens, not the host theme’s', async ({ app, page }) => {
    await app.settings.open({ page: 'pd.appearance' });
    await page.addStyleTag({ content: HOSTILE });
    await app.settings.field('theme').getByRole('combobox').click();
    const panel = page.locator('.mat-mdc-select-panel');
    await expect(panel).toBeVisible();
    expect(await token(panel, '--mat-form-field-container-height')).toBe(`${await px(page, 'height', 'var(--pd-control-height)')}px`);
    expect(await token(panel, '--mat-button-text-container-height')).toBe(`${await px(page, 'height', 'var(--pd-button-height)')}px`);
  });

  test('the undo toast (a snack bar in the overlay) inherits the family tokens too', async ({ app, page }) => {
    await app.frame.open();
    await page.addStyleTag({ content: HOSTILE });
    await page.getByRole('button', { name: 'Say hello' }).click();
    const toast = page.getByTestId('undo-toast');
    await expect(toast).toBeVisible();
    expect(await token(toast, '--mat-button-text-container-height')).toBe(`${await px(page, 'height', 'var(--pd-button-height)')}px`);
    expect((await toast.getByRole('button', { name: 'Undo', exact: true }).boundingBox())!.height).toBeLessThanOrEqual((await px(page, 'height', 'var(--pd-button-height)')) + 1);
    await toast.getByRole('button', { name: 'Dismiss' }).click();
  });

  test('the notification centre’s All / Unread control stays at the family height', async ({ app, page }) => {
    await page.goto('/');
    await app.statusBar.bell.click();
    await page.addStyleTag({ content: HOSTILE });
    const group = page.getByTestId('notices-filter-bar').locator('mat-button-toggle-group');
    await expect(group).toBeVisible();
    expect((await group.boundingBox())!.height).toBeLessThanOrEqual((await px(page, 'height', 'var(--pd-control-height)')) + 4);
  });
});
