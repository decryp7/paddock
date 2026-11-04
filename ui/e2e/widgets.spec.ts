import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@decryp7/paddock-e2e';
import { mkdirSync } from 'node:fs';
import { join } from 'node:path';

/** Set WIDGETS_SHOTS to a folder to also write the review screenshots there (light and dark). */
const SHOTS = process.env['WIDGETS_SHOTS'];
const THEMES = ['dark', 'light'] as const;
const KINDS = ['alarms', 'disk', 'cpu', 'net', 'backup', 'apps'] as const;

async function open(page: Page, theme: string, contrast: 'off' | 'more' = 'off'): Promise<void> {
  await page.setViewportSize({ width: 1280, height: 1800 });
  await page.goto(`/?theme=${theme}&accent=dyno`);
  await page.getByTestId('widgets').waitFor();
  await page.getByTestId('widgets').scrollIntoViewIfNeeded();
  await page.evaluate((c) => document.documentElement.setAttribute('data-contrast', c), contrast);
  // Wait for every finite animation (the popover's enter, the shimmer runs forever and is skipped).
  await page.evaluate(() => Promise.all(document.getAnimations().filter((a) => Number.isFinite(a.effect?.getComputedTiming().endTime as number)).map((a) => a.finished.catch(() => undefined))));
}

/** Waits for every finite animation (the shimmer and the indeterminate bar run forever and are skipped). */
const animationsDone = (page: Page) => page.evaluate(() => Promise.all(document.getAnimations().filter((a) => Number.isFinite(a.effect?.getComputedTiming().endTime as number)).map((a) => a.finished.catch(() => undefined))));
const widgets = (page: Page) => page.getByTestId('widgets');
const cell = (page: Page, kind: string, state: string) => page.getByTestId(`wgt-${kind}-${state}`);
const chooser = (page: Page) => page.getByTestId('wgt-chooser');
const grips = (page: Page) => chooser(page).locator('.pd-wc-grip');
const shownTitles = (page: Page) => chooser(page).locator('ul[role=list]').first().locator('.pd-wc-txt b').allTextContents();
const height = (page: Page, kind: string, state: string) => cell(page, kind, state).locator('section.pd-w').evaluate((e) => e.getBoundingClientRect().height);

for (const theme of THEMES) {
  test(`Widgets: axe finds no violations (${theme})`, async ({ page }) => {
    await open(page, theme);
    const results = await new AxeBuilder({ page }).include('[data-testid="widgets"]').withTags(['wcag2a', 'wcag2aa', 'wcag22aa']).analyze();
    expect(results.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`)).toEqual([]);
  });

  test(`Widgets: axe finds no violations in high contrast with the popup stopped (${theme})`, async ({ page }) => {
    await open(page, theme, 'more');
    await page.getByTestId('wgt-stop').click();
    const results = await new AxeBuilder({ page }).include('[data-testid="widgets"]').withTags(['wcag2a', 'wcag2aa', 'wcag22aa']).analyze();
    expect(results.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`)).toEqual([]);
    // The frame is outlined in the strong border colour there.
    const outline = await cell(page, 'cpu', 'ready').locator('section.pd-w').evaluate((e) => {
      const s = getComputedStyle(e);
      return { style: s.outlineStyle, width: s.outlineWidth, color: s.outlineColor };
    });
    expect(outline.style).toBe('solid');
    expect(outline.width).toBe('1px');
    expect(outline.color).not.toBe('rgba(0, 0, 0, 0)');
  });

  test.describe(`review screenshots (${theme})`, () => {
    test.use({ deviceScaleFactor: 2 });
    test(`Widgets: review screenshots (${theme})`, async ({ page }) => {
      test.skip(!SHOTS, 'set WIDGETS_SHOTS to a folder');
      const shot = (loc: ReturnType<Page['locator']>, name: string) => loc.screenshot({ path: join(SHOTS as string, `widgets-${name}-${theme}.png`) });
      mkdirSync(SHOTS as string, { recursive: true });
      await open(page, theme);
      await shot(widgets(page).locator('.wg-sheet'), 'states');
      await page.getByTestId('wgt-stop').click();
      await shot(page.getByTestId('wgt-popup'), 'popup-agent');
      await page.getByTestId('wgt-stop').click();
      await shot(page.getByTestId('wgt-popup'), 'popup');
      await page.getByTestId('wgt-load').click();
      await expect(page.getByTestId('wgt-popup').locator('section.pd-w[aria-busy=true]')).toHaveCount(5);
      await shot(page.getByTestId('wgt-popup'), 'popup-loading');
      await page.getByTestId('wgt-load').click();
      await page.getByTestId('wgt-empty').click();
      await expect(page.getByTestId('wgt-popup').locator('.pd-wa-et')).toBeVisible();
      await shot(page.getByTestId('wgt-popup'), 'popup-empty');
      await page.getByTestId('wgt-empty').click();
      await page.getByTestId('wgt-popup').getByRole('button', { name: 'More actions' }).click();
      await expect(page.getByRole('menuitem', { name: 'Settings…' })).toBeVisible();
      await animationsDone(page);
      await shot(page.getByTestId('wgt-popup'), 'popup-menu');
      await page.keyboard.press('Escape');
      await shot(chooser(page), 'chooser');
      await chooser(page).getByLabel('Preset name').fill('Work');
      await chooser(page).getByRole('button', { name: 'Save as preset' }).click();
      await expect(chooser(page).getByRole('button', { name: 'Delete the Work preset' })).toBeVisible();
      await shot(chooser(page), 'chooser-presets');
      await chooser(page).locator('.pd-wc-more').nth(2).click();
      await expect(page.locator('.pd-wc-menu [role=menuitem]').first()).toBeVisible();
      await animationsDone(page);
      await shot(chooser(page), 'chooser-menu');
      await page.keyboard.press('Escape');
      await chooser(page).getByRole('button', { name: 'Add Top apps' }).click();
      await expect(chooser(page).locator('.pd-wc-limit').first()).toBeVisible();
      await shot(chooser(page), 'chooser-limit');
      await page.emulateMedia({ reducedMotion: 'reduce' });
      await page.evaluate(() => document.documentElement.setAttribute('data-contrast', 'more'));
      await shot(widgets(page).locator('.wg-sheet'), 'states-contrast');
    });
  });
}

test('Widgets: loading is as tall as ready for every kind', async ({ page }) => {
  await open(page, 'dark');
  for (const kind of KINDS) {
    expect(await height(page, kind, 'loading'), kind).toBe(await height(page, kind, 'ready'));
  }
});

test('Widgets: the popup grid is two equal 159 px columns with an 8 px gap and the notice sits above the widgets', async ({ page }) => {
  await open(page, 'dark');
  const popup = page.getByTestId('wgt-popup');
  const frames = popup.locator('pd-widget-frame');
  const rects = await frames.evaluateAll((els) => els.map((e) => e.getBoundingClientRect()).map((r) => ({ x: r.x, y: r.y, w: r.width })));
  const full = rects[0] as { x: number; y: number; w: number };
  expect(full.w).toBeCloseTo(326, 0);
  const [net, backup] = rects.slice(3) as { x: number; y: number; w: number }[];
  expect(net?.w).toBeCloseTo(159, 0);
  expect(backup?.w).toBeCloseTo(159, 0);
  expect((backup?.x ?? 0) - (net?.x ?? 0) - (net?.w ?? 0)).toBeCloseTo(8, 0);
  await page.getByTestId('wgt-stop').click();
  const alert = popup.locator('[role=alert]');
  await expect(alert).toHaveCount(1);
  await expect(alert).toContainText('Agent stopped');
  const a = await alert.boundingBox();
  const first = await frames.first().boundingBox();
  expect((a?.y ?? 0) + (a?.height ?? 0)).toBeLessThanOrEqual(first?.y ?? 0);
  await expect(popup.locator('.pd-w-meta--stale').first()).toContainText('Updated 4 min ago');
  await alert.getByRole('button', { name: 'Restart agent' }).click();
  await expect(page.getByTestId('wgt-acted')).toContainText('retry');
  await page.getByTestId('wgt-empty').click();
  await popup.getByRole('button', { name: 'Choose widgets' }).click();
  await expect(page.getByTestId('wgt-acted')).toContainText('customize');
});

test('Widgets: the chooser reorders with Alt+arrows, keeps focus on the moved handle and announces it', async ({ page }) => {
  await open(page, 'dark');
  await expect.poll(() => shownTitles(page)).toEqual(['Active alarms', 'Disk space', 'CPU and memory', 'Network', 'Backup']);
  // Focus order: grip, then the row's controls, then the next grip.
  await grips(page).first().focus();
  await page.keyboard.press('Tab');
  await expect(chooser(page).locator('.pd-wc-more').first()).toBeFocused();
  await page.keyboard.press('Tab');
  await expect(grips(page).nth(1)).toBeFocused();
  await page.keyboard.press('Alt+ArrowUp');
  await expect.poll(() => shownTitles(page)).toEqual(['Disk space', 'Active alarms', 'CPU and memory', 'Network', 'Backup']);
  await expect(grips(page).first()).toBeFocused();
  await expect(grips(page).first()).toHaveAttribute('aria-label', 'Reorder Disk space, position 1 of 5');
  await expect(page.locator('.cdk-live-announcer-element').first()).toContainText('Disk space moved to position 1 of 5');
  await page.keyboard.press('Alt+End');
  await expect.poll(async () => (await shownTitles(page)).at(-1)).toBe('Disk space');
  await expect(grips(page).nth(4)).toBeFocused();
  await page.keyboard.press('Alt+Home');
  await expect.poll(async () => (await shownTitles(page))[0]).toBe('Disk space');
  await page.keyboard.press('Alt+ArrowDown');
  await expect.poll(async () => (await shownTitles(page))[1]).toBe('Disk space');
  // The popup preview follows the same order.
  await expect(chooser(page).locator('.pd-wc-preview .pd-w-title').nth(1)).toHaveText('Disk space');
});

test('Widgets: dragging a handle moves the row', async ({ page }) => {
  await open(page, 'dark');
  const from = await grips(page).nth(0).boundingBox();
  const to = await grips(page).nth(2).boundingBox();
  await page.mouse.move((from?.x ?? 0) + 10, (from?.y ?? 0) + 10);
  await page.mouse.down();
  await page.mouse.move((from?.x ?? 0) + 10, (from?.y ?? 0) + 40, { steps: 5 });
  await page.mouse.move((to?.x ?? 0) + 10, (to?.y ?? 0) + 30, { steps: 10 });
  await page.mouse.up();
  await expect.poll(async () => (await shownTitles(page))[0]).not.toBe('Active alarms');
});

test('Widgets: the row menu opens beside its button and never covers its focus ring', async ({ page }) => {
  await open(page, 'light');
  for (const index of [0, 2, 4]) {
    const trigger = chooser(page).locator('.pd-wc-more').nth(index);
    await trigger.scrollIntoViewIfNeeded();
    await trigger.focus();
    await page.keyboard.press('Enter');
    const menu = page.locator('.pd-wc-menu');
    await expect(menu).toBeVisible();
    await expect(menu.locator('[role=menuitem]:not([disabled])').first()).toBeFocused();
    await page.evaluate(() => Promise.all(document.getAnimations().filter((a) => Number.isFinite(a.effect?.getComputedTiming().endTime as number)).map((a) => a.finished.catch(() => undefined))));
    const t = await trigger.boundingBox();
    const m = await menu.boundingBox();
    const ring = 4; // the 2 px ring and its 2 px offset
    const overlap = !((m?.x ?? 0) >= (t?.x ?? 0) + (t?.width ?? 0) + ring || (m?.x ?? 0) + (m?.width ?? 0) <= (t?.x ?? 0) - ring || (m?.y ?? 0) >= (t?.y ?? 0) + (t?.height ?? 0) + ring || (m?.y ?? 0) + (m?.height ?? 0) <= (t?.y ?? 0) - ring);
    expect(overlap, `menu ${index} covers the button`).toBe(false);
    await page.keyboard.press('Escape');
    await expect(menu).toHaveCount(0);
    await expect(trigger).toBeFocused();
  }
});

test('Widgets: removing a widget shows the Undo toast, Undo brings it back, the limit disables Add and says why', async ({ page }) => {
  await open(page, 'dark');
  await chooser(page).locator('.pd-wc-more').nth(4).click();
  await page.getByRole('menuitem', { name: 'Remove' }).click();
  await expect.poll(() => shownTitles(page)).toEqual(['Active alarms', 'Disk space', 'CPU and memory', 'Network']);
  const toast = page.locator('.mat-mdc-snack-bar-container');
  await expect(toast).toContainText('Backup removed from the popup.');
  await toast.getByRole('button', { name: 'Undo' }).click();
  await expect.poll(() => shownTitles(page)).toEqual(['Active alarms', 'Disk space', 'CPU and memory', 'Network', 'Backup']);
  await expect(toast).toHaveCount(0);
  // 5 of 6: add the last kind, then the limit shows
  await chooser(page).getByRole('button', { name: 'Add Top apps' }).click();
  await expect(chooser(page).locator('.pd-wc-count').first()).toHaveText('6 of 6');
  await expect(chooser(page).locator('.pd-wc-limit')).toContainText('The popup holds 6 widgets. Remove one to add another.');
  await expect(chooser(page).getByRole('button', { name: 'Add volume Disk space' })).toBeDisabled();
  await expect(chooser(page).locator('.pd-wc-addedlabel')).toHaveCount(5);
});

test('Widgets: Half and Full change a width and the volume select is the app control', async ({ page }) => {
  await open(page, 'dark');
  const net = chooser(page).locator('li.pd-wc-item', { hasText: 'Network' }).first();
  await net.getByRole('radio', { name: 'Full' }).click();
  await expect(chooser(page).locator('.pd-wc-preview pd-widget-frame').nth(3)).toHaveCSS('grid-column-start', 'span 2');
  await chooser(page).getByRole('combobox', { name: /Volumes shown by disk/ }).click();
  await page.getByRole('option', { name: 'Projects' }).click();
  await expect(chooser(page).getByRole('combobox', { name: /Volumes shown by disk/ })).toContainText('Projects');
});

test('Widgets: a scrolling popup with nothing focusable in it is still a named, focusable region (axe)', async ({ page }) => {
  await open(page, 'dark');
  await page.getByTestId('wgt-load').click();
  const popup = page.getByTestId('wgt-popup');
  await expect(popup.locator('section.pd-w[aria-busy=true]')).toHaveCount(5);
  expect(await popup.locator('.pd-content button, .pd-content a, .pd-content input').count()).toBe(0);
  const region = popup.locator('.pd-content');
  await expect(region).toHaveAttribute('tabindex', '0');
  await expect(region).toHaveAttribute('role', 'region');
  await expect(region).toHaveAttribute('aria-label', 'Dyno content');
  const results = await new AxeBuilder({ page }).include('[data-testid="wgt-popup"]').withTags(['wcag2a', 'wcag2aa', 'wcag22aa']).analyze();
  expect(results.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`)).toEqual([]);
  // A popup that fits is not a focus stop.
  await page.getByTestId('wgt-empty').click();
  await expect(region).not.toHaveAttribute('tabindex', /.*/);
});

test('Widgets: presets are applied, saved and deleted, with Undo for an apply', async ({ page }) => {
  await open(page, 'dark');
  const c = chooser(page);
  await c.getByRole('button', { name: 'Apply the Minimal preset' }).click();
  await expect.poll(() => shownTitles(page)).toEqual(['Active alarms', 'CPU and memory']);
  await page.locator('.mat-mdc-snack-bar-container').getByRole('button', { name: 'Undo' }).click();
  await expect.poll(() => shownTitles(page)).toHaveLength(5);
  await c.getByLabel('Preset name').fill('Work');
  await c.getByRole('button', { name: 'Save as preset' }).click();
  await expect(c.getByRole('button', { name: 'Delete the Work preset' })).toBeVisible();
  await expect(c.getByLabel('Preset name')).toHaveValue('');
  await c.getByRole('button', { name: 'Delete the Work preset' }).click();
  await c.getByRole('button', { name: 'Delete the Work preset' }).click();
  await expect(c.getByRole('button', { name: 'Delete the Work preset' })).toHaveCount(0);
});

test('Widgets: kind icons are 14 px in a frame head and 16 px in a chooser row (Material draws 24 px otherwise)', async ({ page }) => {
  await open(page, 'dark');
  const sizes = (sel: string) => page.locator(sel).evaluateAll((els) => els.map((e) => [e.getBoundingClientRect().width, e.getBoundingClientRect().height]));
  for (const [w, h] of await sizes('[data-testid="wgt-popup"] .pd-w-icon')) expect([w, h]).toEqual([14, 14]);
  for (const [w, h] of await sizes('[data-testid="wgt-chooser"] .pd-wc-kicon')) expect([w, h]).toEqual([16, 16]);
  expect((await sizes('[data-testid="wgt-chooser"] .pd-wc-kicon')).length).toBeGreaterThan(5);
});

for (const density of ['compact', 'comfortable'] as const) {
  test(`Widgets: loading is as tall as ready, and nothing overflows, at ${density} density`, async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 1800 });
    await page.goto(`/?theme=dark&accent=dyno&density=${density}`);
    await page.getByTestId('widgets').waitFor();
    // Text a few per cent wider than this machine's font (a CI runner's font is): a row with no slack here overflows there.
    await page.addStyleTag({ content: '*{letter-spacing:0.03em !important}' });
    for (const kind of KINDS) expect(await height(page, kind, 'loading'), kind).toBe(await height(page, kind, 'ready'));
    // Each frame that is wider inside than outside, named with its widths and the part that sticks out.
    const overflow = await page.getByTestId('wgt-popup').locator('pd-widget-frame').evaluateAll((els) =>
      els
        .filter((e) => e.scrollWidth > e.clientWidth + 1)
        .map((e) => {
          const box = e.getBoundingClientRect();
          const wide = [...e.querySelectorAll<HTMLElement>('*')].filter((c) => c.getBoundingClientRect().right > box.right + 1).map((c) => `${c.tagName.toLowerCase()}.${c.className}`.slice(0, 80));
          return `${(e.textContent ?? '').trim().slice(0, 40)}: ${e.scrollWidth} in ${e.clientWidth}; past the edge: ${wide.join(', ')}`;
        }),
    );
    expect(overflow).toEqual([]);
  });
}

test('Widgets: a scrolling popup shows a fade at its foot until the end', async ({ page }) => {
  await open(page, 'dark');
  const popup = page.getByTestId('wgt-popup');
  await expect(popup.locator('.pd-fade--on')).toHaveCount(1);
  await popup.locator('.pd-content').evaluate((e) => e.scrollTo(0, e.scrollHeight));
  await expect(popup.locator('.pd-fade--on')).toHaveCount(0);
});

test('Widgets: deleting a preset asks first and puts focus on the next preset', async ({ page }) => {
  await open(page, 'dark');
  const c = chooser(page);
  await c.getByLabel('Preset name').fill('Work');
  await c.getByRole('button', { name: 'Save as preset' }).click();
  await c.getByRole('button', { name: 'Delete the Work preset' }).click();
  await expect(c.getByText('Delete this preset?')).toBeVisible();
  await c.getByRole('button', { name: 'Delete the Work preset' }).click();
  await expect(c.getByRole('button', { name: 'Apply the Minimal preset' })).toBeFocused();
});

test('Widgets: the popup footer menu opens inside the popover, not past its edge', async ({ page }) => {
  await open(page, 'dark');
  const popup = page.getByTestId('wgt-popup').locator('pd-tray-popover');
  await page.getByTestId('wgt-popup').getByRole('button', { name: 'More actions' }).click();
  const menu = page.locator('.mat-mdc-menu-panel[role=menu]').last();
  await expect(menu).toBeVisible();
  await animationsDone(page);
  const p = (await popup.boundingBox())!;
  const m = (await menu.boundingBox())!;
  expect(m.x).toBeGreaterThanOrEqual(p.x);
  expect(m.x + m.width).toBeLessThanOrEqual(p.x + p.width);
  expect(m.y).toBeGreaterThanOrEqual(p.y);
  expect(m.y + m.height).toBeLessThanOrEqual(p.y + p.height);
});

test('Widgets: Escape cancels the preset delete confirm', async ({ page }) => {
  await open(page, 'dark');
  const c = chooser(page);
  await c.getByLabel('Preset name').fill('Work');
  await c.getByRole('button', { name: 'Save as preset' }).click();
  await c.getByRole('button', { name: 'Delete the Work preset' }).click();
  await expect(c.getByText('Delete this preset?')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(c.getByText('Delete this preset?')).toHaveCount(0);
  await expect(c.getByRole('button', { name: 'Delete the Work preset' })).toBeFocused();
});

test('Widgets: the preset name field has 13 px text and the height of its Save button', async ({ page }) => {
  await open(page, 'dark');
  const c = chooser(page);
  const field = await c.locator('.pd-wc-save .mat-mdc-text-field-wrapper').boundingBox();
  const button = await c.getByRole('button', { name: 'Save as preset' }).boundingBox();
  expect(Math.round(field!.height)).toBe(Math.round(button!.height));
  expect(await c.locator('.pd-wc-save input').evaluate((e) => getComputedStyle(e).fontSize)).toBe('13px');
});

test('Widgets: a card whose content does not fit scrolls inside its body, reaches its last item, keeps the head and chains the wheel', async ({ page }) => {
  await open(page, 'dark');
  const frame = page.getByTestId('wgt-scroll-cell').locator('.pd-w');
  const body = frame.locator('.pd-w-body');
  await expect(body).toHaveCount(1);
  const m = await body.evaluate((e) => ({ scrolls: e.scrollHeight > e.clientHeight, overflowY: getComputedStyle(e).overflowY, chain: getComputedStyle(e).overscrollBehaviorY, tab: e.tabIndex }));
  expect(m.scrolls).toBe(true);
  expect(m.overflowY).toBe('auto');
  expect(m.chain).toBe('auto');
  // Keyboard focus can scroll it, and a screen reader hears what it is.
  expect(m.tab).toBe(0);
  expect(await body.getAttribute('role')).toBe('group');
  expect(await body.getAttribute('aria-label')).toBe('What holds it back');
  // The head stays where it is while the body scrolls, and the last item becomes reachable.
  const head = frame.locator('.pd-w-head');
  const before = (await head.boundingBox())!.y - (await frame.boundingBox())!.y;
  await body.focus();
  await page.keyboard.press('End');
  const last = body.getByText('Reason 12 of 12');
  await expect(last).toBeInViewport();
  const box = (await last.boundingBox())!;
  const cellBox = (await frame.boundingBox())!;
  expect(box.y + box.height).toBeLessThanOrEqual(cellBox.y + cellBox.height);
  expect((await head.boundingBox())!.y - (await frame.boundingBox())!.y).toBe(before);
});

test('Widgets: a card whose content fits has no scrollable body and no extra tab stop', async ({ page }) => {
  await open(page, 'dark');
  const bodies = page.getByTestId('widgets').locator('.pd-w-body');
  expect(await bodies.count()).toBeGreaterThan(0);
  const offenders = await bodies.evaluateAll((els) =>
    els.filter((e) => e.closest('[data-testid^="wgt-scroll"]') === null && (e.scrollHeight > e.clientHeight + 1 || e.getAttribute('tabindex') !== null)).map((e) => `${e.closest('pd-widget-frame')?.querySelector('.pd-w-title')?.textContent}: ${e.scrollHeight} > ${e.clientHeight}`),
  );
  expect(offenders).toEqual([]);
});

test('Widgets: a popup tile taller than its cap scrolls inside its body and its last item is reachable', async ({ page }) => {
  await open(page, 'light');
  const body = page.getByTestId('wgt-scroll-tile').locator('.pd-w-body');
  const m = await body.evaluate((e) => ({ scrolls: e.scrollHeight > e.clientHeight, overflowY: getComputedStyle(e).overflowY, chain: getComputedStyle(e).overscrollBehaviorY, tab: e.tabIndex }));
  expect(m).toEqual({ scrolls: true, overflowY: 'auto', chain: 'auto', tab: 0 });
  await body.focus();
  await page.keyboard.press('End');
  await expect(body.getByText('Reason 12 of 12')).toBeInViewport();
});
