import AxeBuilder from '@axe-core/playwright';
import { expect, expectNoButtonLabelWraps, test, type Page } from '@decryp7/paddock-e2e';
import { mkdirSync } from 'node:fs';
import { join } from 'node:path';

const THEMES = ['dark', 'light'] as const;
/** Set MATERIAL_SHOTS to a folder to also write the review screenshots of the page there. */
const SHOTS = process.env['MATERIAL_SHOTS'];

async function open(page: Page, theme: string, accent = 'debrief', density = 'standard'): Promise<void> {
  await page.setViewportSize({ width: 1280, height: 4200 });
  await page.goto(`/?theme=${theme}&accent=${accent}&density=${density}`);
  // The section is deferred until it is near the viewport.
  await page.locator('gal-material, [data-testid="material-placeholder"]').first().scrollIntoViewIfNeeded();
  await page.locator('[data-testid="material"] .mat-mdc-unelevated-button').first().waitFor();
  // Let Material's enter animations and the layout settle: the page has no running animation.
  await settled(page);
}

const section = (page: Page) => page.getByTestId('material');
const pane = (page: Page) => page.locator('.cdk-overlay-pane').last();
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
const px = (page: Page, name: string) => page.evaluate((n) => parseFloat(getComputedStyle(document.documentElement).getPropertyValue(n)), name);
/** Waits for every finite animation (the indeterminate progress bar runs forever and is skipped). */
const settled = (page: Page) =>
  page.evaluate(() =>
    Promise.all(
      document
        .getAnimations()
        .filter((a) => Number.isFinite(a.effect?.getComputedTiming().endTime as number))
        .map((a) => a.finished.catch(() => undefined)),
    ),
  );
/** Closes whatever overlay is open and waits until it is gone. */
async function closeOverlays(page: Page): Promise<void> {
  await page.keyboard.press('Escape');
  await expect(page.locator('.cdk-overlay-pane')).toHaveCount(0);
}

for (const theme of THEMES) {
  test(`Material controls: axe finds no violations in the section (${theme})`, async ({ page }) => {
    await open(page, theme);
    const results = await new AxeBuilder({ page }).include('[data-testid="material"]').withTags(['wcag2a', 'wcag2aa', 'wcag22aa']).analyze();
    expect(results.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`)).toEqual([]);
  });

  test(`Material controls: buttons and fields follow the tokens (${theme})`, async ({ page }) => {
    await open(page, theme);
    expect(await bg(page, '[data-testid="material"] .mat-mdc-unelevated-button:not(.pd-button-danger)')).toBe(await token(page, '--pd-accent-fill'));
    expect(await bg(page, '[data-testid="material"] .mat-mdc-outlined-button:not(.pd-button-danger):not([disabled])')).toBe(await token(page, '--pd-surface-2'));
    expect(await bg(page, '[data-testid="material"] .mat-mdc-unelevated-button.pd-button-danger')).toBe(await token(page, '--pd-danger-fill'));
    const height = await section(page).locator('.mat-mdc-unelevated-button').first().evaluate((e) => e.getBoundingClientRect().height);
    expect(height).toBe(await px(page, '--pd-button-height'));
    // A disabled filled button is distinguishable from the page (not the page colour) and from an enabled one.
    const disabled = await bg(page, '[data-testid="material"] .mat-mdc-unelevated-button[disabled]');
    expect(disabled).not.toBe(await token(page, '--pd-bg'));
    expect(disabled).not.toBe(await token(page, '--pd-accent-fill'));
    const field = section(page).locator('mat-form-field').first();
    await expect(field).toHaveClass(/mat-form-field-appearance-outline/);
    await expect(field).toHaveClass(/mat-mdc-form-field-label-always-float/);
  });

  test(`Material controls: rendered sizes, type and fields match the guide (${theme})`, async ({ page }) => {
    await open(page, theme);
    const s = section(page);
    const body = await px(page, '--pd-text-label');
    // Floated label: RENDERED at the 12 px label size (Material scales a floated label by 0.75 on top of its font size), measured
    // against a 12 px span of the same text and font, not read from the computed font-size.
    const label = await s.locator('.mdc-floating-label').first().evaluate((e, size) => {
      const ref = document.createElement('span');
      ref.textContent = e.textContent;
      ref.style.position = 'absolute';
      ref.style.whiteSpace = 'nowrap';
      ref.style.fontFamily = getComputedStyle(e).fontFamily;
      ref.style.fontSize = `${size}px`;
      document.body.append(ref);
      const out = { rendered: e.getBoundingClientRect().width, reference: ref.getBoundingClientRect().width };
      ref.remove();
      return out;
    }, body);
    expect(label.rendered / label.reference).toBeGreaterThan(0.94);
    expect(label.rendered / label.reference).toBeLessThan(1.06);
    // Input text is vertically centred in the 28 px box (the two gaps differ by at most 1 px).
    const gaps = await s.locator('input.mat-mdc-input-element').first().evaluate((i) => {
      const w = i.closest('.mat-mdc-text-field-wrapper') as HTMLElement;
      const a = i.getBoundingClientRect();
      const b = w.getBoundingClientRect();
      return { top: a.top - b.top, bottom: b.bottom - a.bottom, box: b.height };
    });
    expect(Math.abs(gaps.top - gaps.bottom)).toBeLessThanOrEqual(1);
    expect(gaps.box).toBe(await px(page, '--pd-control-height'));
    // The suffix icon keeps clear of the border.
    const clear = await s.locator('mat-datepicker-toggle button').first().evaluate((b) => {
      const w = b.closest('.mat-mdc-text-field-wrapper') as HTMLElement;
      return w.getBoundingClientRect().right - b.getBoundingClientRect().right;
    });
    expect(clear).toBeGreaterThanOrEqual(4);
    // No letter-spacing and the family's line heights in Material text.
    expect(await s.locator('td.mat-mdc-cell').first().evaluate((e) => getComputedStyle(e).letterSpacing)).toMatch(/^(0px|normal)$/);
    expect(await s.locator('.mat-expansion-panel-body').first().evaluate((e) => getComputedStyle(e).letterSpacing)).toMatch(/^(0px|normal)$/);
    // Table: a 28 px header, right-aligned numbers.
    expect(await s.locator('tr.mat-mdc-header-row').first().evaluate((e) => e.getBoundingClientRect().height)).toBe(await px(page, '--pd-control-height'));
    expect(await s.locator('td.gal-num-col').first().evaluate((e) => getComputedStyle(e).textAlign)).toBe('right');
    // Hint text is muted.
    expect(await s.locator('.mat-mdc-form-field-hint').first().evaluate((e) => getComputedStyle(e).color)).toBe(
      await page.evaluate(() => {
        const p = document.createElement('span');
        p.style.color = 'var(--pd-text-muted)';
        document.body.append(p);
        const c = getComputedStyle(p).color;
        p.remove();
        return c;
      }),
    );
    // Calendar heading and first row are sentence case: Material upper-cases the heading text, the bridge lower-cases it and
    // capitalises the first letter; the first row takes `monthLabel` from providePaddockMaterial.
    const heading = s.locator('.mat-calendar-period-button .mdc-button__label > span').first();
    expect(await heading.evaluate((e) => getComputedStyle(e).textTransform)).toBe('lowercase');
    expect(await heading.evaluate((e) => getComputedStyle(e, '::first-letter').textTransform)).toBe('uppercase');
    expect(await s.locator('.mat-calendar-body-label').first().innerText()).toMatch(/^[A-Z][a-z]+$/);
    // The selected button toggle: 600 label and an accent ring.
    const toggle = s.locator('.mat-button-toggle-checked');
    expect(await toggle.locator('.mat-button-toggle-label-content').evaluate((e) => getComputedStyle(e).fontWeight)).toBe('600');
    expect(await toggle.evaluate((e) => getComputedStyle(e).boxShadow)).toContain('inset');
  });

  test(`Material controls: controls follow the density (${theme})`, async ({ page }) => {
    for (const density of ['compact', 'comfortable'] as const) {
      await open(page, theme, 'debrief', density);
      const control = await px(page, '--pd-control-height');
      const row = await px(page, '--pd-row-height');
      const heights = (selector: string) => section(page).locator(selector).evaluateAll((es) => es.map((e) => Math.round((e as HTMLElement).getBoundingClientRect().height)));
      expect(new Set(await heights('.mat-mdc-text-field-wrapper')), `${density} fields`).toEqual(new Set([control]));
      expect(await heights('.mat-mdc-paginator .mat-mdc-text-field-wrapper'), `${density} paginator select`).toEqual([control]);
      expect(new Set(await heights('.mat-mdc-icon-button')), `${density} icon buttons`).toEqual(new Set([control]));
      expect(await heights('tr.mat-mdc-header-row'), `${density} table header`).toEqual([control]);
      expect(new Set(await heights('.mat-tree-node')), `${density} tree nodes (the first one too)`).toEqual(new Set([row]));
    }
  });

  test(`Material controls: a hovered or focused icon button draws a translucent state layer (${theme})`, async ({ page }) => {
    await open(page, theme);
    const buttons = section(page).locator('.mat-mdc-icon-button:visible:not(.mat-mdc-button-disabled):not(.mat-mdc-button-disabled-interactive)'); // a disabled button has no state layer
    const count = await buttons.count();
    expect(count).toBeGreaterThan(0);
    const alpha = (colour: string) => {
      const slash = /\/\s*([\d.]+%?)\s*\)/.exec(colour);
      const comma = /^rgba\(([^)]*)\)$/.exec(colour);
      const raw = slash?.[1] ?? comma?.[1]?.split(',')[3]?.trim();
      return raw === undefined ? 1 : raw.endsWith('%') ? parseFloat(raw) / 100 : parseFloat(raw);
    };
    const hoverAlpha = alpha(await token(page, '--pd-hover'));
    const pressedAlpha = alpha(await token(page, '--pd-pressed'));
    expect(hoverAlpha).toBeLessThan(0.2);

    for (let n = 0; n < count; n++) {
      const button = buttons.nth(n);
      await button.scrollIntoViewIfNeeded();
      // The state layer is the ripple host's ::before: its fill is the token colour, shown at Material's state-layer opacity.
      const effective = () =>
        button.evaluate((e) => {
          const cs = getComputedStyle(e.querySelector('.mat-mdc-button-persistent-ripple') ?? e, '::before');
          return { colour: cs.backgroundColor, opacity: parseFloat(cs.opacity) };
        });
      const effectiveAlpha = async () => {
        const l = await effective();
        return alpha(l.colour) * l.opacity;
      };
      const shot = async (name: string) => {
        if (SHOTS) {
          mkdirSync(SHOTS, { recursive: true });
          await button.screenshot({ path: join(SHOTS, `icon-button-${n}-${name}-${theme}.png`) });
        }
      };
      // Rendered pixels: the corner of the button's box (inside the state layer, clear of the glyph) against the page just outside it
      // must differ by under 10 % per channel; a solid disc differs by far more.
      const pixelsDiffer = async () => {
        const box = (await button.boundingBox())!;
        const png = await page.screenshot({ clip: { x: box.x - 4, y: box.y - 4, width: box.width + 8, height: box.height + 8 } });
        return page.evaluate(async (b64) => {
          const bitmap = await createImageBitmap(await (await fetch(`data:image/png;base64,${b64}`)).blob());
          const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
          const ctx = canvas.getContext('2d')!;
          ctx.drawImage(bitmap, 0, 0);
          const at = (x: number, y: number) => Array.from(ctx.getImageData(x, y, 1, 1).data);
          const outside = at(1, 1);
          // The state layer is round: sample on the circle's inner ring (just inside its edge), clear of the glyph.
          const cx = bitmap.width / 2;
          const r = Math.min(bitmap.width, bitmap.height) / 2 - 6;
          const ring = at(Math.round(cx + r), Math.round(bitmap.height / 2));
          return Math.max(...[0, 1, 2].map((c) => Math.abs((ring[c] ?? 0) - (outside[c] ?? 0)) / 255));
        }, png.toString('base64'));
      };

      await button.hover();
      await settled(page);
      await shot('hover');
      expect(await effectiveAlpha(), `icon button ${n} hover`).toBeCloseTo(hoverAlpha, 2);
      expect(await pixelsDiffer(), `icon button ${n} hover pixels`).toBeLessThan(0.1);

      await page.mouse.move(0, 0);
      await page.keyboard.press('Shift');
      await button.focus();
      await expect(button).toBeFocused();
      await settled(page);
      await shot('focus');
      expect(await effectiveAlpha(), `icon button ${n} focus`).toBeCloseTo(hoverAlpha, 2);

      await button.hover();
      await page.mouse.down();
      await settled(page);
      expect(await effectiveAlpha(), `icon button ${n} pressed`).toBeCloseTo(pressedAlpha, 2);
      // Release away from the button: no click, so no popup opens or tree node toggles, and the next button stays reachable.
      await page.mouse.move(0, 0);
      await page.mouse.up();
    }
  });

  test(`Material controls: a glyph in a matButton is centred on the label with a gap (${theme})`, async ({ page }) => {
    await open(page, theme);
    for (const id of ['filled', 'outlined', 'text']) {
      const m = await page.getByTestId(`material-glyph-${id}`).evaluate((button) => {
        const glyph = button.querySelector('pd-glyph') as HTMLElement;
        const range = document.createRange();
        const text = [...(glyph.parentElement as HTMLElement).childNodes].find((n) => n.nodeType === Node.TEXT_NODE && n.textContent?.trim()) as Text;
        range.selectNodeContents(text);
        const t = range.getBoundingClientRect();
        const g = glyph.getBoundingClientRect();
        return { dy: Math.abs(g.top + g.height / 2 - (t.top + t.height / 2)), gap: t.left - g.right };
      });
      expect(m.dy, `${id} centre offset`).toBeLessThanOrEqual(1);
      expect(m.gap, `${id} gap`).toBeGreaterThanOrEqual(3);
      expect(m.gap, `${id} gap`).toBeLessThanOrEqual(8);
    }
  });

  test(`Material controls: a real select, menu and tooltip open themed and sized (${theme})`, async ({ page }) => {
    await open(page, theme);
    const rowH = await px(page, '--pd-control-height');
    const popover = await page.evaluate(() => {
      const p = document.createElement('div');
      p.style.borderTopLeftRadius = 'var(--pd-radius-popover)';
      document.body.append(p);
      const r = getComputedStyle(p).borderTopLeftRadius;
      p.remove();
      return r;
    });
    await section(page).getByRole('combobox', { name: 'Level' }).click();
    const select = pane(page).locator('.mat-mdc-select-panel');
    await expect(select).toBeVisible();
    await settled(page);
    expect(await select.evaluate((e) => getComputedStyle(e).backgroundColor)).toBe(await token(page, '--pd-surface-3'));
    const sel = await select.evaluate((e) => {
      const cs = getComputedStyle(e);
      return { radius: cs.borderBottomLeftRadius, top: cs.borderTopLeftRadius, pad: cs.paddingTop };
    });
    expect(sel.radius).toBe(popover);
    expect(sel.top).toBe(popover);
    expect(sel.pad).toBe('4px');
    expect(await select.locator('.mat-mdc-option').first().evaluate((e) => (e as HTMLElement).offsetHeight)).toBe(rowH);
    await closeOverlays(page);

    await page.getByTestId('material-menu').click();
    const menu = pane(page).locator('.mat-mdc-menu-panel');
    await expect(menu).toBeVisible();
    await settled(page);
    expect(await menu.evaluate((e) => getComputedStyle(e).backgroundColor)).toBe(await token(page, '--pd-surface-3'));
    expect(await menu.locator('.mat-mdc-menu-content').evaluate((e) => getComputedStyle(e).paddingTop)).toBe('4px');
    expect(await menu.locator('.mat-mdc-menu-item').first().evaluate((e) => (e as HTMLElement).offsetHeight)).toBe(rowH);
    await closeOverlays(page);

    await page.getByTestId('material-tooltip').hover();
    await expect(pane(page).locator('.mdc-tooltip__surface')).toBeVisible();
  });

  test(`Material controls: every kind of control shows a 2 px keyboard indicator at 3:1 (${theme})`, async ({ page }) => {
    await open(page, theme);
    const first = section(page).locator('.mat-mdc-unelevated-button').first();
    await first.focus();
    const seen = new Map<string, string>();
    const problems: string[] = [];
    for (let i = 0; i < 90; i++) {
      const info = await page.evaluate(() => {
        const el = document.activeElement as HTMLElement | null;
        if (!el || !el.closest('[data-testid="material"]')) return null;
        const lum = (c: string) => {
          const m = c.match(/[\d.]+/g)?.map(Number) ?? [0, 0, 0];
          const [r, g, b] = m.slice(0, 3).map((v) => {
            const x = v / 255;
            return x <= 0.03928 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4;
          });
          return 0.2126 * (r as number) + 0.7152 * (g as number) + 0.0722 * (b as number);
        };
        const ratio = (a: string, b: string) => {
          const x = lum(a);
          const y = lum(b);
          return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
        };
        const page = getComputedStyle(document.body).backgroundColor;
        // The element that carries the ring: the focused element, its focus-indicator host, or the visible box of a hidden native input.
        const hosts: (Element | null)[] = [
          el,
          el.closest('.mat-focus-indicator, .mat-mdc-focus-indicator'),
          el.querySelector('.mat-focus-indicator, .mat-mdc-focus-indicator'),
          el.closest('.mdc-checkbox')?.querySelector('.mdc-checkbox__background') ?? null,
          el.closest('.mdc-radio')?.querySelector('.mdc-radio__outer-circle') ?? null,
          el.closest('.mat-mdc-slider')?.querySelector('.mdc-slider__thumb-knob') ?? null,
          el.closest('.mat-mdc-slide-toggle')?.querySelector('.mdc-switch__track') ?? null,
          el.closest('.mat-mdc-form-field')?.querySelector('.mdc-notched-outline__leading') ?? null,
        ];
        let best: { how: string; width: number; ratio: number } | null = null;
        for (const h of hosts) {
          if (!h) continue;
          const cs = getComputedStyle(h);
          if (cs.outlineStyle === 'solid' && parseFloat(cs.outlineWidth) >= 2) {
            best = { how: 'outline', width: parseFloat(cs.outlineWidth), ratio: ratio(cs.outlineColor, page) };
            break;
          }
          if (h.classList.contains('mdc-notched-outline__leading') && parseFloat(cs.borderTopWidth) >= 2 && cs.borderTopStyle === 'solid') {
            best = { how: 'field outline', width: parseFloat(cs.borderTopWidth), ratio: ratio(cs.borderTopColor, page) };
            break;
          }
          const b = getComputedStyle(h, '::before');
          if (b.content !== 'none' && b.display !== 'none' && b.borderTopStyle === 'solid' && parseFloat(b.borderTopWidth) >= 2) {
            best = { how: 'before', width: parseFloat(b.borderTopWidth), ratio: ratio(b.borderTopColor, page) };
            break;
          }
        }
        const kind = el.closest('.mat-mdc-checkbox')
          ? 'checkbox'
          : el.closest('.mat-mdc-radio-button')
            ? 'radio'
            : el.closest('.mat-mdc-slide-toggle')
              ? 'slide-toggle'
              : el.closest('.mat-mdc-slider')
                ? 'slider'
                : el.closest('.mat-mdc-chip')
                  ? 'chip'
                  : el.closest('.mat-mdc-tab')
                    ? 'tab'
                    : el.closest('.mat-button-toggle')
                      ? 'button-toggle'
                      : el.closest('.mat-mdc-list-option')
                        ? 'list-option'
                        : el.closest('.mat-expansion-panel-header')
                          ? 'expansion'
                          : el.closest('.mat-mdc-paginator')
                            ? 'paginator'
                            : el.closest('.mat-sort-header')
                              ? 'sort'
                              : el.closest('.mat-datepicker-toggle, .mat-timepicker-toggle')
                                ? 'picker-toggle'
                                : el.closest('.mat-mdc-select')
                                  ? 'select'
                                  : el.matches('input')
                                    ? 'input'
                                    : el.closest('.mat-mdc-button-base')
                                      ? 'button'
                                      : el.tagName.toLowerCase();
        return { kind, label: `${kind}:${(el.getAttribute('aria-label') ?? el.textContent ?? '').trim().slice(0, 24)}`, best };
      });
      if (!info) break;
      if (!info.best) problems.push(`${info.label}: no visible indicator`);
      else if (info.best.ratio < 3) problems.push(`${info.label}: ring ${info.best.ratio.toFixed(2)}:1`);
      seen.set(info.kind, info.label);
      await page.keyboard.press('Tab');
    }
    for (const kind of ['button', 'input', 'select', 'checkbox', 'radio', 'slide-toggle', 'slider', 'chip', 'tab', 'button-toggle', 'list-option', 'expansion', 'paginator', 'sort', 'picker-toggle']) {
      expect([...seen.keys()], kind).toContain(kind);
    }
    expect(problems).toEqual([]);
  });

  test(`Material controls: screenshot (${theme})`, async ({ page }) => {
    test.skip(!SHOTS, 'set MATERIAL_SHOTS to a folder to write the review screenshots');
    mkdirSync(SHOTS as string, { recursive: true });
    await open(page, theme);
    await section(page).screenshot({ path: join(SHOTS as string, `material-controls-${theme}.png`) });
    // The live overlays, one at a time, in an ordinary window.
    await page.setViewportSize({ width: 1280, height: 900 });
    const shot = async (name: string) => {
      await settled(page);
      await page.screenshot({ path: join(SHOTS as string, `material-${name}-${theme}.png`) });
    };
    const level = section(page).getByRole('combobox', { name: 'Level' });
    await level.scrollIntoViewIfNeeded();
    await level.click();
    await expect(pane(page).locator('.mat-mdc-select-panel')).toBeVisible();
    await shot('select-open');
    await closeOverlays(page);
    await page.getByTestId('material-menu').click();
    await expect(pane(page).locator('.mat-mdc-menu-panel')).toBeVisible();
    await shot('menu-open');
    await closeOverlays(page);
    await page.getByTestId('material-tooltip').hover();
    await expect(pane(page).locator('.mdc-tooltip__surface')).toBeVisible();
    await shot('tooltip');
    await page.getByTestId('material-snack').click();
    await expect(page.locator('.mat-mdc-snack-bar-container')).toBeVisible();
    await shot('snack-bar');
    // The keyboard indicator on a button, a checkbox and a field.
    for (const [name, locator] of [
      ['focus-button', section(page).getByRole('button', { name: 'Cancel' })],
      ['focus-checkbox', section(page).getByRole('checkbox', { name: 'Include warnings' })],
      ['focus-field', section(page).getByRole('textbox', { name: 'Search events' })],
    ] as const) {
      await locator.scrollIntoViewIfNeeded();
      await page.keyboard.press('Escape');
      await locator.focus();
      await page.keyboard.press('Shift+Tab');
      await page.keyboard.press('Tab');
      await shot(name);
    }
  });
}

test('Material controls: tab nav bar links size to their label, so eight tabs fit one row', async ({ page }) => {
  await open(page, 'dark');
  const links = section(page).locator('.mat-mdc-tab-nav-bar .mat-mdc-tab-link');
  await expect(links).toHaveCount(8);
  const widths = await links.evaluateAll((els) => els.map((e) => ({ min: getComputedStyle(e).minWidth, width: e.getBoundingClientRect().width })));
  for (const w of widths) expect(w.min).toBe('0px');
  expect(Math.min(...widths.map((w) => w.width)), 'a short label is narrower than Material\'s own 160 px minimum').toBeLessThan(160);
});

test('Material: no button label wraps, even in a tight row', async ({ page }) => {
  await open(page, 'dark');
  await expectNoButtonLabelWraps(page, 'gallery');
});
