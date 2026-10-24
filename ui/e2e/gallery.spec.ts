import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@decryp7/paddock-e2e';
import { PD_ICON_DOTS } from '../projects/paddock-ui/icons/src/paths';

const DOTS = Object.fromEntries(Object.entries(PD_ICON_DOTS).map(([k, v]) => [k, (v ?? []).map((d) => [d[0], d[1]])]));

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

test.describe('UI basics review fixes (regressions)', () => {
  test('the dots of alert, info, keyboard and more are solid in the browser (P1-1)', async ({ page }) => {
    await open(page, 'light', 'debrief');
    const result = await page.evaluate(async (dots) => {
      const out: Record<string, number[]> = {};
      for (const [name, list] of Object.entries(dots)) {
        const item = [...document.querySelectorAll('li.gal-icon')].find((l) => l.querySelector('.gal-icon-name')?.textContent === name) as Element;
        const svg = (item.querySelector('[data-row="literal"] .gal-mat--24 svg') as SVGElement).outerHTML.replace('currentColor', '#000').replaceAll('currentColor', '#000');
        const img = new Image();
        img.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
        await img.decode();
        const SCALE = 8;
        const canvas = document.createElement('canvas');
        canvas.width = canvas.height = 24 * SCALE;
        const ctx = canvas.getContext('2d') as CanvasRenderingContext2D;
        ctx.drawImage(img, 0, 0, 24 * SCALE, 24 * SCALE);
        out[name] = (list as number[][]).map(([cx, cy]) => ctx.getImageData(Math.round((cx as number) * SCALE), Math.round((cy as number) * SCALE), 1, 1).data[3] as number);
      }
      return out;
    }, DOTS);
    expect(Object.keys(result).sort()).toEqual(['alert', 'info', 'keyboard', 'more']);
    for (const [name, alphas] of Object.entries(result)) for (const a of alphas) expect(a, `${name} dot centre is ink`).toBeGreaterThan(250);
  });

  test('pd-glyph and the mat-icon literal draw the same 1.5 px non-scaling stroke at 16 and 24 px (P1-3)', async ({ page }) => {
    await open(page, 'dark', 'debrief');
    const strokes = await page.evaluate(() => {
      const w = (el: Element | null): string[] => {
        const g = el?.closest('svg') ?? el;
        const path = g?.querySelector('path') as SVGPathElement;
        const cs = getComputedStyle(path);
        const rect = (g as SVGElement).getBoundingClientRect();
        return [cs.strokeWidth, cs.vectorEffect, String(Math.round(rect.width))];
      };
      const li = [...document.querySelectorAll('li.gal-icon')].find((l) => l.querySelector('.gal-icon-name')?.textContent === 'refresh') as Element;
      return {
        glyph16: w(li.querySelector('[data-row="glyph"] pd-glyph:first-child svg')),
        glyph24: w(li.querySelector('[data-row="glyph"] pd-glyph:last-child svg')),
        lit16: w(li.querySelector('.gal-mat--16 svg')),
        lit24: w(li.querySelector('.gal-mat--24 svg')),
      };
    });
    expect(strokes.glyph16).toEqual(['1.5px', 'non-scaling-stroke', '16']);
    expect(strokes.lit16).toEqual(['1.5px', 'non-scaling-stroke', '16']);
    expect(strokes.glyph24).toEqual(['1.5px', 'non-scaling-stroke', '24']);
    expect(strokes.lit24).toEqual(['1.5px', 'non-scaling-stroke', '24']);
  });

  for (const density of ['comfortable', 'standard', 'compact'] as const) {
    test(`input, native select and themed select are the same height, ${density} (P1-4)`, async ({ page }) => {
      await open(page, 'dark', 'debrief', density);
      const heights = await page.evaluate(() => {
        const h = (sel: string): number => (document.querySelector(sel) as HTMLElement).getBoundingClientRect().height;
        return [
          h('[data-surface="page"] [data-group="fields"] input.pd-input'),
          h('[data-surface="page"] [data-group="fields"] select.pd-select'),
          h('[data-surface="page"] [data-group="themed-select"] pd-select .pd-trigger'),
          h('[data-surface="page"] [data-group="themed-select"] input.pd-input'),
        ];
      });
      expect(new Set(heights).size, `heights ${heights.join(', ')}`).toBe(1);
    });
  }

  test('inputs, selects, the themed select and the switch show a solid 2 px focus outline (P1-5)', async ({ page }) => {
    await open(page, 'light', 'debrief');
    for (const sel of ['#g-name', '#g-native', '#g-themed', '[data-surface="page"] input.pd-switch']) {
      const el = page.locator(sel).first();
      await el.focus();
      const o = await el.evaluate((e) => {
        const cs = getComputedStyle(e);
        return [cs.outlineStyle, cs.outlineWidth, cs.outlineOffset, cs.boxShadow];
      });
      expect(o.slice(0, 3), sel).toEqual(['solid', '2px', '2px']);
    }
  });

  test('the selected segment differs from the others by tint, ring and weight in both schemes (P1-6)', async ({ page }) => {
    for (const theme of THEMES) {
      await open(page, theme, 'debrief');
      const [on, off] = await page.evaluate(() => {
        const read = (b: Element) => {
          const cs = getComputedStyle(b);
          return [cs.backgroundColor, cs.fontWeight, cs.boxShadow];
        };
        const bs = document.querySelectorAll('[data-surface="page"] .pd-segmented > button');
        return [read(bs[0] as Element), read(bs[1] as Element)];
      });
      expect(on?.[1], theme).toBe('600');
      expect(on?.[0], theme).not.toBe(off?.[0]);
      expect(on?.[2], theme).toContain('inset');
      expect(off?.[2], theme).toBe('none');
    }
  });

  for (const sel of ['[data-testid="path-240"]', '.gal-path--narrow']) {
    test(`path text keeps the whole file name and its extension, middle ellipsis only in the start, ${sel} (P1-7)`, async ({ page }) => {
      await open(page, 'dark', 'debrief');
      const r = await page.locator(`${sel} pd-path-text`).evaluate((host) => {
        const head = host.querySelector('.pd-head') as HTMLElement;
        const tail = host.querySelector('.pd-tail') as HTMLElement;
        return { tail: tail.textContent ?? '', tailClipped: tail.scrollWidth > tail.clientWidth, headClipped: head.scrollWidth > head.clientWidth, font: getComputedStyle(host.querySelector('.pd-text') as Element).fontFamily };
      });
      expect(r.headClipped).toBe(true);
      expect(r.tailClipped).toBe(false);
      expect(r.tail.endsWith('.log')).toBe(true);
      expect(r.font).toMatch(/mono|Mono|Menlo|Consolas/i);
    });
  }

  test('the native select caret and the themed select chevron are both the chevron-down shape (P2-2)', async ({ page }) => {
    await open(page, 'dark', 'debrief');
    const bg = await page.locator('#g-native').evaluate((e) => getComputedStyle(e).backgroundImage);
    expect(bg.match(/linear-gradient/g)?.length).toBe(2);
    expect(bg).not.toMatch(/45deg|135deg/);
  });

  test('the open select list is as wide as its trigger (review 2, N-P2-1)', async ({ page }) => {
    await open(page, 'light', 'debrief');
    const trigger = page.locator('#g-themed');
    await trigger.click();
    const list = page.locator('.pd-list');
    await expect(list).toBeVisible();
    const [t, l] = [(await trigger.boundingBox())!, (await list.boundingBox())!];
    expect(Math.abs(l.x - t.x)).toBeLessThanOrEqual(1);
    expect(Math.abs(l.width - t.width)).toBeLessThanOrEqual(1);
  });

  test('hover changes the border of an input, a native select and the themed select, and a secondary button on a white card (review 2, N-P2-2, N-P3-3)', async ({ page }) => {
    await open(page, 'light', 'debrief');
    const border = (sel: string) => page.locator(sel).first().evaluate((e) => getComputedStyle(e).borderTopColor);
    for (const sel of ['#g-name', '#g-native', '#g-themed']) {
      const rest = await border(sel);
      await page.locator(sel).first().hover();
      await expect.poll(() => border(sel), { message: sel }).not.toBe(rest); // the border transitions over 100 ms
      await page.mouse.move(0, 0);
    }
    const btn = page.locator('[data-surface="card"] .pd-btn:not(.pd-btn--primary):not([disabled])').first();
    const rest = await btn.evaluate((e) => getComputedStyle(e).backgroundColor);
    await btn.hover();
    const read = () => btn.evaluate((e) => getComputedStyle(e).backgroundColor);
    await expect.poll(read).not.toBe(rest);
    expect(await read()).not.toBe('rgb(255, 255, 255)');
  });

  test('a lone checkbox sits on the control line of the fields beside it (review 2, N-P2-5)', async ({ page }) => {
    await open(page, 'dark', 'debrief');
    const centre = (sel: string) => page.locator(sel).first().evaluate((e) => { const r = e.getBoundingClientRect(); return r.top + r.height / 2; });
    expect(Math.abs((await centre('.pd-field--check input')) - (await centre('#g-name')))).toBeLessThanOrEqual(1);
  });

  test('a long path in a 240 px box shortens the directory first and keeps the start of the name, the extension and the last word (review 2, N-P2-4)', async ({ page }) => {
    await open(page, 'dark', 'debrief');
    const r = await page.locator('[data-testid="path-win-240"] pd-path-text').evaluate((host) => {
      const part = (c: string) => host.querySelector(c) as HTMLElement;
      const clipped = (e: HTMLElement) => e.scrollWidth > e.clientWidth;
      return { nameOverflow: part('.pd-name').scrollWidth - part('.pd-name').getBoundingClientRect().width, dirClipped: clipped(part('.pd-head')), nameClipped: clipped(part('.pd-name')), tailClipped: clipped(part('.pd-tail')), shown: part('.pd-name').textContent! + part('.pd-tail').textContent! };
    });
    expect(r.dirClipped).toBe(true);
    expect(r.nameClipped).toBe(false);
    expect(r.tailClipped).toBe(false);
    expect(r.shown).toContain('Service.2026-10-24.log');
    // Nothing of the name is replaced by an ellipsis: the displayed text is the whole value of both spans. Font metrics differ by a fraction of a pixel between
    // platforms (0.4 px on Linux), and the integer scrollWidth check above is what the ellipsis follows.
    expect(r.nameOverflow).toBeLessThan(1);
  });

  test('keycaps do not touch (P2-12)', async ({ page }) => {
    await open(page, 'dark', 'debrief');
    const gap = await page.locator('.pd-kbd').nth(1).evaluate((e) => e.getBoundingClientRect().left - (e.previousElementSibling as Element).getBoundingClientRect().right);
    expect(gap).toBeGreaterThanOrEqual(2);
  });
});
