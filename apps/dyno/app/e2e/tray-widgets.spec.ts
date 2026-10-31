import { axeInThemes, expect, pressMod, test } from '@decryp7/paddock-e2e';
import type { BrowserContext, Locator, Page } from '@playwright/test';
import { mkdirSync } from 'node:fs';
import { join } from 'node:path';

/**
 * The customizable widgets against the mock backend (family ADR 0035, WP-C2): choose, reorder by drag and by Alt+arrow keys, remove
 * and Undo, reset, the popup following Settings and keeping the order after a reload, every state through the mock's switches,
 * axe in both themes and a focus order equal to the widget order. The mock keeps layouts in the context's localStorage, so the
 * popup page and the Settings page see each other's edits like two windows of one app. Waits are on conditions, never sleeps.
 */
const POPUP = (query = '') => `/?latency=0${query}#/tray`;
const SHOTS = process.env['WIDGET_SHOTS'];

/** Opens the Settings window on Dyno's page and scrolls to the popup chooser. */
async function openChooser(page: Page): Promise<Locator> {
  await page.goto('/?latency=0#/');
  await expect(page.getByRole('heading', { level: 1, name: 'Dashboard' })).toBeVisible();
  await pressMod(page, 'Mod+Comma');
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await dialog.getByText('Dyno', { exact: true }).click();
  const chooser = dialog.locator('dy-popup-widgets');
  await chooser.scrollIntoViewIfNeeded();
  await expect(chooser.getByRole('list', { name: /Shown/ }).or(chooser.locator('ul.pd-wc-list').first())).toBeVisible();
  return chooser;
}

const shown = (chooser: Locator) => chooser.locator('ul.pd-wc-list').first().locator('li.pd-wc-item .pd-wc-txt b');
async function popup(context: BrowserContext, query = ''): Promise<Page> {
  const p = await context.newPage();
  await p.setViewportSize({ width: 360, height: 640 });
  await p.goto(POPUP(query));
  await expect(p.getByRole('button', { name: /Open Dyno/ })).toBeVisible();
  return p;
}

test.describe('popup widgets', () => {
  test('shows the default layout: alarms, disk space, CPU and memory, in that order', async ({ page }) => {
    await page.setViewportSize({ width: 360, height: 640 });
    await page.goto(POPUP());
    const frames = page.locator('pd-widget-frame');
    await expect(frames).toHaveCount(3);
    await expect(frames.nth(0)).toContainText('Active alarms');
    await expect(frames.nth(1)).toContainText('Disk space');
    await expect(frames.nth(2)).toContainText('CPU and memory');
  });

  test('choose: adding a widget in Settings shows it in the popup', async ({ page, context }) => {
    const chooser = await openChooser(page);
    await chooser.getByRole('button', { name: 'Add Network' }).click();
    await expect(shown(chooser)).toHaveText(['Active alarms', 'Disk space', 'CPU and memory', 'Network']);
    const p = await popup(context);
    await expect(p.locator('pd-widget-frame')).toHaveCount(4);
    await expect(p.locator('pd-widget-frame').nth(3)).toContainText('Network');
  });

  test('keyboard reorder: Alt+ArrowUp on a handle moves the row and keeps focus on it, and the popup follows', async ({ page, context }) => {
    const chooser = await openChooser(page);
    const handle = chooser.getByRole('button', { name: /^Reorder CPU and memory/ });
    await handle.focus();
    await page.keyboard.press('Alt+ArrowUp');
    await expect(shown(chooser)).toHaveText(['Active alarms', 'CPU and memory', 'Disk space']);
    await expect(chooser.getByRole('button', { name: /^Reorder CPU and memory, position 2 of 3/ })).toBeFocused();
    await page.keyboard.press('Alt+Home');
    await expect(shown(chooser)).toHaveText(['CPU and memory', 'Active alarms', 'Disk space']);
    const p = await popup(context);
    await expect(p.locator('pd-widget-frame').first()).toContainText('CPU and memory');
  });

  test('drag: dragging a handle below the last row reorders', async ({ page }) => {
    const chooser = await openChooser(page);
    await chooser.getByRole('button', { name: /^Reorder CPU and memory/ }).scrollIntoViewIfNeeded();
    const from = await chooser.getByRole('button', { name: /^Reorder Active alarms/ }).boundingBox();
    const to = await chooser.getByRole('button', { name: /^Reorder CPU and memory/ }).boundingBox();
    expect(from && to).toBeTruthy();
    await page.mouse.move(from!.x + 10, from!.y + 10);
    await page.mouse.down();
    await page.mouse.move(from!.x + 10, from!.y + 40, { steps: 5 });
    await page.mouse.move(to!.x + 10, to!.y + 30, { steps: 10 });
    await page.mouse.up();
    await expect(shown(chooser)).toHaveText(['Disk space', 'CPU and memory', 'Active alarms']);
  });

  test('remove shows Undo, Undo brings the widget back; reset returns the defaults', async ({ page }) => {
    const chooser = await openChooser(page);
    await chooser.getByRole('button', { name: 'More actions for Disk space' }).click();
    await page.getByRole('menuitem', { name: 'Remove' }).click();
    await expect(shown(chooser)).toHaveText(['Active alarms', 'CPU and memory']);
    await page.getByRole('button', { name: 'Undo' }).click();
    await expect(shown(chooser)).toHaveText(['Active alarms', 'Disk space', 'CPU and memory']);
    await chooser.getByRole('button', { name: 'Add Network' }).click();
    await expect(shown(chooser)).toHaveCount(4);
    await chooser.getByRole('button', { name: 'Reset to defaults' }).click();
    await expect(shown(chooser)).toHaveText(['Active alarms', 'Disk space', 'CPU and memory']);
  });

  test('the order survives a reload', async ({ page, context }) => {
    const chooser = await openChooser(page);
    await chooser.getByRole('button', { name: /^Reorder CPU and memory/ }).focus();
    await page.keyboard.press('Alt+Home');
    await expect(shown(chooser)).toHaveText(['CPU and memory', 'Active alarms', 'Disk space']);
    const p = await popup(context);
    await p.reload();
    await expect(p.locator('pd-widget-frame').first()).toContainText('CPU and memory');
    await page.reload();
    await expect(page.getByRole('heading', { level: 1, name: 'Dashboard' })).toBeVisible();
  });

  test('focus order equals the widget order', async ({ page }) => {
    await page.setViewportSize({ width: 360, height: 640 });
    await page.goto(POPUP('&popup=activeAlarms:ready'));
    await expect(page.locator('pd-widget-frame')).toHaveCount(3);
    const order = await page.evaluate(() => {
      const frames = [...document.querySelectorAll('pd-widget-frame')];
      const focusables = [...document.querySelectorAll<HTMLElement>('pd-widget-frame button, button[aria-label="More actions"], button.mat-mdc-unelevated-button')];
      return focusables.map((f) => frames.findIndex((fr) => fr.contains(f)));
    });
    const inFrames = order.filter((i) => i >= 0);
    expect(inFrames).toEqual([...inFrames].sort((a, b) => a - b));
  });

  test('every state through the mock switches, and the agent notice once', async ({ page }) => {
    await page.setViewportSize({ width: 360, height: 900 });
    await page.goto(POPUP('&popup=activeAlarms:empty,diskSpace:error'));
    const frames = page.locator('pd-widget-frame');
    await expect(frames.nth(0)).toContainText('No active alarms');
    await expect(frames.nth(1)).toContainText("Couldn't read volumes");
    await expect(frames.nth(1).getByRole('button', { name: 'Retry' })).toBeVisible();
    await page.goto(POPUP('&popup=diskSpace:loading,cpuMemory:stale&agent=stopped'));
    await expect(page.getByRole('alert')).toHaveCount(1);
    await expect(page.getByRole('button', { name: 'Restart agent' })).toBeVisible();
    await expect(page.locator('pd-widget-frame').filter({ hasText: 'CPU and memory' })).toContainText(/Updated|Paused/);
  });

  test('the footer menu icons are 16 px and sit on their text', async ({ page }) => {
    await page.setViewportSize({ width: 360, height: 640 });
    await page.goto(POPUP());
    await page.getByRole('button', { name: 'More actions' }).click();
    const items = page.getByRole('menuitem');
    await expect(items).toHaveCount(3);
    for (let i = 0; i < 3; i++) {
      const item = items.nth(i);
      const icon = await item.locator('mat-icon').first().boundingBox();
      const text = await item.locator('.mdc-list-item__primary-text').boundingBox();
      expect(icon, 'icon').toBeTruthy();
      expect(Math.round(icon!.width)).toBe(16);
      expect(Math.round(icon!.height)).toBe(16);
      // The icon's centre is on the text's centre line (within a pixel).
      expect(Math.abs(icon!.y + icon!.height / 2 - (text!.y + text!.height / 2))).toBeLessThanOrEqual(1.5);
    }
  });

  test('axe: the popup and the chooser, dark and light', async ({ page }) => {
    await page.setViewportSize({ width: 360, height: 900 });
    await page.goto(POPUP('&popup=activeAlarms:ready'));
    await expect(page.locator('pd-widget-frame')).toHaveCount(3);
    expect(await axeInThemes(page)).toEqual([]);
    const chooser = await openChooser(page);
    await expect(shown(chooser)).toHaveCount(3);
    expect(await axeInThemes(page, { include: ['dy-popup-widgets'] })).toEqual([]);
  });
});

test.describe('dashboard and menu-bar editors', () => {
  test('the dashboard chooser removes a widget, undoes it and applies a preset', async ({ page }) => {
    await page.goto('/?latency=0#/');
    await expect(page.getByRole('heading', { level: 1, name: 'Dashboard' })).toBeVisible();
    await page.getByRole('button', { name: 'Customize dashboard' }).click();
    const chooser = page.locator('dy-widget-board pd-widget-chooser');
    await expect(shown(chooser)).toHaveCount(15);
    await chooser.getByRole('button', { name: 'More actions for Health' }).click();
    await page.getByRole('menuitem', { name: 'Remove' }).click();
    await expect(shown(chooser)).toHaveCount(14);
    await page.getByRole('button', { name: 'Undo' }).click();
    await expect(shown(chooser)).toHaveCount(15);
    await chooser.getByRole('button', { name: 'Apply the Developer preset' }).click();
    await expect(shown(chooser)).toHaveCount(7);
  });

  test('the menu-bar editor adds an item up to four', async ({ page }) => {
    await page.goto('/?latency=0#/');
    await pressMod(page, 'Mod+Comma');
    const dialog = page.getByRole('dialog');
    await dialog.getByText('Dyno', { exact: true }).click();
    const editor = dialog.locator('dy-menu-bar-items');
    await editor.scrollIntoViewIfNeeded();
    await expect(shown(editor)).toHaveCount(2);
    await editor.getByRole('button', { name: 'Add Number' }).click();
    await expect(shown(editor)).toHaveCount(3);
  });
});

test.describe('screenshots for the independent review', () => {
  test.skip(!SHOTS, 'set WIDGET_SHOTS=<folder> to write the review screenshots');
  test.use({ deviceScaleFactor: 2 });
  for (const theme of ['light', 'dark'] as const) {
    test(`popup, states and chooser, ${theme}`, async ({ page, context }) => {
      mkdirSync(SHOTS!, { recursive: true });
      await page.emulateMedia({ colorScheme: theme, reducedMotion: 'reduce' });
      const snap = async (p: Page, name: string, locator?: Locator) => {
        await p.evaluate(() => document.fonts.ready);
        await (locator ?? p).screenshot({ path: join(SHOTS!, `${name}-${theme}.png`) });
      };
      const open = async (query: string, height = 900) => {
        const p = await context.newPage();
        await p.emulateMedia({ colorScheme: theme, reducedMotion: 'reduce' });
        await p.setViewportSize({ width: 392, height });
        await p.goto(POPUP(query));
        await expect(p.locator('pd-widget-frame').first()).toBeVisible();
        return p;
      };
      const p1 = await open('');
      await snap(p1, 'popup', p1.locator('pd-tray-popover'));
      const p2 = await open('&popup=activeAlarms:empty,diskSpace:error,cpuMemory:stale');
      await snap(p2, 'popup-states', p2.locator('pd-tray-popover'));
      const p3 = await open('&agent=stopped');
      await snap(p3, 'popup-agent', p3.locator('pd-tray-popover'));
      const p4 = await open('', 560);
      await p4.getByRole('button', { name: 'More actions' }).click();
      await snap(p4, 'popup-menu');
      await page.setViewportSize({ width: 1100, height: 2400 });
      const chooser = await openChooser(page);
      await chooser.getByRole('button', { name: 'Add Network' }).click();
      await expect(shown(chooser)).toHaveCount(4);
      // The whole Popup widgets section, from its heading: list, handles, volume select, Half/Full, counter, hint and the preview.
      await page.getByRole('heading', { name: 'Popup widgets' }).scrollIntoViewIfNeeded();
      const head = await page.getByRole('heading', { name: 'Popup widgets' }).boundingBox();
      const body = await chooser.boundingBox();
      await page.evaluate(() => document.fonts.ready);
      await page.screenshot({ path: join(SHOTS!, `chooser-${theme}.png`), clip: { x: head!.x - 16, y: head!.y - 8, width: Math.min(1100 - head!.x, body!.width + 32), height: body!.y + body!.height - head!.y + 24 } });
      await chooser.getByRole('button', { name: 'More actions for Network' }).click();
      await page.screenshot({ path: join(SHOTS!, `chooser-menu-${theme}.png`), clip: { x: head!.x - 16, y: head!.y - 8, width: Math.min(1100 - head!.x, body!.width + 32), height: body!.y + body!.height - head!.y + 24 } });
    });
  }
});
