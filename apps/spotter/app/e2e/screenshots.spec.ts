import { test, type Page } from '@playwright/test';

/**
 * Documentation screenshots (`SCREENSHOTS=1 npx playwright test --project=screenshots`): 1440x900 at 2x, both themes.
 * The clock is frozen so both themes show the same relative times, and long pages are framed so no section or card is
 * cut mid-element: a frame ends before the first row, card or section that does not fit, and the next frame starts at that section.
 */
const OUT = process.env['SHOTS_OUT'] ?? '../docs/screenshots';
const NOW = new Date('2026-10-22T12:00:00Z');
/** The units a frame is cut between. */
const BLOCKS = 'main header.page-head, main dl.strip, main section, main section > *, main .section-head, main li, main tbody tr, main footer.foot, main .body > *';

async function settle(page: Page, ms = 900): Promise<void> {
  await page.waitForTimeout(ms);
}

async function open(page: Page, url: string): Promise<void> {
  await page.clock.setFixedTime(NOW);
  await page.goto(url);
}

interface Frame {
  /** Content above this height (CSS px, from the top of the window) is kept; null keeps the whole window. */
  keep: number | null;
  /** How far to scroll before the next frame (null: this was the last one). */
  next: number | null;
}

/** Looks at the scrolled window and says where this frame ends and where the next one starts. */
async function measure(page: Page, blocks: string): Promise<Frame> {
  return page.evaluate((sel) => {
    const scroller = [...document.querySelectorAll('*')].find((e) => e.scrollHeight > e.clientHeight + 40 && getComputedStyle(e).overflowY !== 'visible');
    if (!scroller) return { keep: null, next: null };
    const box = scroller.getBoundingClientRect();
    const atEnd = scroller.scrollTop + scroller.clientHeight >= scroller.scrollHeight - 2;
    if (atEnd) return { keep: null, next: null };
    const all = [...scroller.querySelectorAll(sel)];
    const leaves = all.filter((e) => !all.some((o) => o !== e && e.contains(o)));
    const cut = leaves.find((e) => {
      const r = e.getBoundingClientRect();
      const straddles = r.top < box.bottom - 4 && r.bottom > box.bottom - 4;
      // A heading with only a sliver of its content in the window counts as cut too.
      const orphan = e.classList.contains('section-head') && r.top >= box.bottom - 120 && r.top < box.bottom;
      return straddles || orphan;
    });
    if (!cut) return { keep: null, next: scroller.clientHeight - 80 };
    const r = cut.getBoundingClientRect();
    // A unit taller than the window cannot be framed whole: scroll on by a page.
    if (r.top - box.top < 80) return { keep: null, next: scroller.clientHeight - 80 };
    // Never leave a heading alone at the foot of a frame: move the cut above it.
    let cutTop = r.top;
    let above = 0;
    for (let i = 0; i < 3; i++) {
      const prev = leaves.filter((e) => { const b = e.getBoundingClientRect(); return b.bottom <= cutTop + 2 && b.top < cutTop; }).sort((x, y) => y.getBoundingClientRect().bottom - x.getBoundingClientRect().bottom)[0];
      above = prev ? prev.getBoundingClientRect().bottom : 0;
      if (prev?.classList.contains('section-head')) cutTop = prev.getBoundingClientRect().top;
      else break;
    }
    // The next frame starts at the cut unit, with the gap that was above it (rows have none).
    const gap = Math.max(0, Math.min(24, cutTop - above) - 6);
    return { keep: Math.floor(Math.min(cutTop, above + 12)), next: Math.round(cutTop - box.top - gap) };
  }, blocks);
}

/** Settings are a stack of cards: a frame ends between cards, so no card is cut and none starts half-way. */
const CARDS = 'main section.group';

/** A table with its own scroll area would show a row cut at its foot: shorten the area to whole rows. */
async function trimInnerScrollers(page: Page): Promise<void> {
  await page.evaluate(() => {
    const main = document.querySelector('main');
    for (const el of main?.querySelectorAll<HTMLElement>('*') ?? []) {
      const overflow = getComputedStyle(el).overflowY;
      if ((overflow !== 'auto' && overflow !== 'scroll') || el.scrollHeight <= el.clientHeight + 4) continue;
      const box = el.getBoundingClientRect();
      const rows = [...el.querySelectorAll('tbody tr, li')].map((r) => r.getBoundingClientRect().bottom);
      const whole = rows.filter((b) => b <= box.bottom + 0.5);
      const last = whole[whole.length - 1];
      if (last !== undefined) el.style.maxHeight = `${Math.ceil(last - box.top + 1)}px`;
    }
  });
}

/** The wizard's footer sits over the page: hide the table rows and disclosures it would cut so only whole rows show above it. */
async function hideRowsUnderFooter(page: Page): Promise<void> {
  await page.evaluate(() => {
    const top = document.querySelector('footer.nav')?.getBoundingClientRect().top ?? Infinity;
    for (const row of document.querySelectorAll<HTMLElement>('main tbody tr, main details')) {
      if (row.getBoundingClientRect().bottom > top - 4) row.style.display = 'none';
    }
  });
}

/** Shoots the page as a series of whole-section frames: `name`, `name-2`, `name-3`. */
async function frames(page: Page, name: string, scheme: string, opts: { blocks?: string; max?: number } = {}): Promise<void> {
  let n = 1;
  await trimInnerScrollers(page);
  for (;;) {
    const frame = await measure(page, opts.blocks ?? BLOCKS);
    const file = `${OUT}/${name}${n > 1 ? `-${n}` : ''}-${scheme}.png`;
    const vp = page.viewportSize() ?? { width: 1440, height: 900 };
    await page.screenshot({ path: file, ...(frame.keep ? { clip: { x: 0, y: 0, width: vp.width, height: frame.keep } } : {}) });
    if (frame.next === null || n >= (opts.max ?? 3)) return;
    await page.evaluate((by) => {
      const sc = [...document.querySelectorAll('*')].find((e) => e.scrollHeight > e.clientHeight + 40 && getComputedStyle(e).overflowY !== 'visible');
      if (sc) sc.scrollTop += by;
    }, frame.next);
    await settle(page, 300);
    n++;
  }
}

/** Scrolls so `from` starts the window and shoots down to the bottom of `to` (a whole section or several). */
async function section(page: Page, from: string, to: string, file: string): Promise<void> {
  await page.locator(from).evaluate((e) => {
    const sec = e.closest('section') ?? e;
    sec.scrollIntoView({ block: 'start' });
  });
  // A little air above the heading.
  await page.evaluate(() => {
    const sc = [...document.querySelectorAll('*')].find((e) => e.scrollHeight > e.clientHeight + 40 && getComputedStyle(e).overflowY !== 'visible');
    if (sc) sc.scrollTop -= 16;
  });
  await settle(page, 500);
  const keep = await page.locator(to).evaluate((e) => Math.ceil((e.closest('section') ?? e).getBoundingClientRect().bottom + 16));
  const vp = page.viewportSize() ?? { width: 1440, height: 900 };
  await page.screenshot({ path: file, clip: { x: 0, y: 0, width: vp.width, height: Math.min(keep, vp.height) } });
}

for (const scheme of ['dark', 'light'] as const) {
  test.describe(scheme, () => {
    test.use({ colorScheme: scheme, timezoneId: 'UTC', locale: 'en-US' });
    const shots: [string, string][] = [
      ['dashboard', '/'],
      ['watches', '/#/watches'],
      ['watch-detail-products', '/?mock=products#/watches/w-9'],
    ];
    for (const [name, url] of shots) {
      test(`${name} ${scheme}`, async ({ page }) => {
        await open(page, url);
        await settle(page, 1400);
        await frames(page, name, scheme);
      });
    }
    // The detail page is a little taller than the window: a taller window shows it whole instead of cutting it in two.
    for (const [name, url] of [['watch-detail', '/#/watches/w-1'], ['watch-detail-upgrade', '/#/watches/w-6']] as const) {
      test(`${name} ${scheme}`, async ({ page }) => {
        await page.setViewportSize({ width: 1440, height: 1030 });
        await open(page, url);
        await settle(page, 1400);
        await trimInnerScrollers(page);
        await page.screenshot({ path: `${OUT}/${name}-${scheme}.png` });
      });
    }
    // The empty states are a heading and one message: no point in a window full of nothing.
    for (const [name, url] of [['dashboard-empty', '/?mock=empty'], ['watches-empty', '/?mock=empty#/watches']] as const) {
      test(`${name} ${scheme}`, async ({ page }) => {
        await open(page, url);
        await settle(page, 1000);
        await page.screenshot({ path: `${OUT}/${name}-${scheme}.png`, clip: { x: 0, y: 0, width: 1440, height: 560 } });
      });
    }
    test(`settings ${scheme}`, async ({ page }) => {
      await open(page, '/#/settings');
      await settle(page, 1400);
      await frames(page, 'settings', scheme, { blocks: CARDS, max: 4 });
    });
    test(`settings e-mail ${scheme}`, async ({ page }) => {
      await open(page, '/#/settings');
      await settle(page, 1000);
      const mail = page.locator('sp-mail-settings');
      await mail.getByLabel('Server').fill('smtp.example.org');
      await mail.getByLabel('From').fill('spotter@example.org');
      await mail.getByLabel('User name').fill('spotter-robot');
      await mail.getByLabel('Password').fill('example-password');
      await mail.getByRole('button', { name: 'Save' }).click();
      await mail.getByLabel('Send a test message to').fill('you@example.org');
      await mail.getByRole('button', { name: 'Send test e-mail' }).click();
      await settle(page, 500);
      await section(page, '#not-h', '#not-h', `${OUT}/settings-email-${scheme}.png`);
    });
    test(`cadence ${scheme}`, async ({ page }) => {
      await open(page, '/');
      await settle(page, 1400);
      await section(page, '#cad-h', '#cad-h', `${OUT}/dashboard-cadence-${scheme}.png`);
    });
    test(`release panel ${scheme}`, async ({ page }) => {
      await open(page, '/#/watches/w-2');
      await settle(page, 1200);
      await page.locator('tbody tr').nth(1).click();
      await settle(page, 400);
      await trimInnerScrollers(page);
      await page.screenshot({ path: `${OUT}/watch-detail-panel-${scheme}.png` });
    });
    test(`tray ${scheme}`, async ({ page }) => {
      // The shell sizes the popup window to its content; the browser build fits the viewport the same way.
      await page.setViewportSize({ width: 372, height: 600 });
      await page.clock.setFixedTime(NOW);
      await page.goto('/#/tray');
      await settle(page, 1200);
      const height = await page.locator('pd-tray-popover').evaluate((e) => Math.ceil(e.getBoundingClientRect().height));
      await page.setViewportSize({ width: 372, height: height + 12 });
      await settle(page, 300);
      await page.screenshot({ path: `${OUT}/tray-${scheme}.png` });
    });
    test(`charts ${scheme}`, async ({ page }) => {
      await open(page, '/');
      await settle(page, 1400);
      await section(page, '#chart-h', '#rel-h', `${OUT}/dashboard-charts-${scheme}.png`);
    });
    test(`row menu ${scheme}`, async ({ page }) => {
      await open(page, '/#/watches');
      await settle(page, 1200);
      await page.locator('tbody tr', { hasText: 'Orbit server' }).click({ button: 'right', position: { x: 160, y: 24 } });
      await settle(page, 400);
      await page.screenshot({ path: `${OUT}/watches-menu-${scheme}.png` });
    });
    test(`feed menu ${scheme}`, async ({ page }) => {
      await open(page, '/');
      await settle(page, 1200);
      await page.locator('sp-whats-new li.item').nth(1).click({ button: 'right', position: { x: 220, y: 24 } });
      await settle(page, 400);
      // Whole rows only: end the picture at the last feed row that fits in the window.
      const keep = await page.evaluate(() => {
        const win = window.innerHeight;
        const rows = [...document.querySelectorAll('sp-whats-new li.item')].map((e) => e.getBoundingClientRect().bottom).filter((b) => b <= win - 8);
        return Math.floor(Math.max(...rows) + 12);
      });
      await page.screenshot({ path: `${OUT}/dashboard-menu-${scheme}.png`, clip: { x: 0, y: 0, width: 1440, height: keep } });
    });
    test(`schedule ${scheme}`, async ({ page }) => {
      await open(page, '/#/watches/new');
      await page.getByRole('button', { name: /Node\.js/ }).click();
      await settle(page, 800);
      await page.getByRole('button', { name: 'Next' }).click();
      await page.getByRole('button', { name: 'Next' }).click();
      await page.getByRole('group', { name: 'How to schedule checks' }).getByRole('button', { name: 'Cron' }).click();
      await page.getByLabel('Cron expression').fill('0 * * * *');
      await settle(page, 1200);
      await trimInnerScrollers(page);
      await page.screenshot({ path: `${OUT}/wizard-schedule-${scheme}.png` });
    });
    test(`wizard ${scheme}`, async ({ page }) => {
      await open(page, '/#/watches/new');
      await settle(page, 700);
      await page.screenshot({ path: `${OUT}/wizard-source-${scheme}.png` });
      // The preview lists every release it reports ("12 releases found"): a taller window shows all of them above the footer.
      await page.setViewportSize({ width: 1440, height: 960 });
      await page.getByRole('button', { name: /Node\.js/ }).click();
      await settle(page, 1600);
      await trimInnerScrollers(page);
      await hideRowsUnderFooter(page);
      await page.screenshot({ path: `${OUT}/wizard-configure-${scheme}.png` });
    });
  });
}
