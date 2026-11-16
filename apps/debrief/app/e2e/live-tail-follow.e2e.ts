import { expect, Page, test } from '@playwright/test';
import { openAppWith } from './mock-url';
import { viewport } from './mock-url';
import { gridRows } from './app';

// ADR 0072: the live-tail grid follows the newest events, holds on request, and never takes the view from the user. Each case runs in
// both sort orders and in Chromium and WebKit (branch CI runs Chromium only; WebKit runs locally and in the full run), on Playwright's
// pinned clock: `mockPageStaggerMs` answers the pages of one request at different times and the live ticks (every 50 ms) land between.
const URL = '?mockTotal=5000&mockLive=1&mockLiveRate=20&mockLiveMs=50&mockLiveRotateMs=0&mockLiveAlertMs=0&mockPageStaggerMs=10';
const SCALED = '?mockTotal=3000000&mockLive=1&mockLiveRate=20&mockLiveMs=50&mockLiveRotateMs=0&mockLiveAlertMs=0&mockPageStaggerMs=10';

// Headless browsers hide the scrollbar: the real-mouse scrollbar drag needs it drawn (`--hide-scrollbars` is a default argument of headless Chromium).
test.use({ launchOptions: { ignoreDefaultArgs: ['--hide-scrollbars'] } });

type Order = 'oldest first' | 'newest first';
const ORDERS: Order[] = ['oldest first', 'newest first'];

// The grid's log (ER-27, category ui.grid: scroll writes, origins, arrivals, growth) is attached when a test fails.
const gridLogs = new WeakMap<Page, string[]>();
test.afterEach(async ({ page }, testInfo) => {
  if (testInfo.status !== testInfo.expectedStatus) await testInfo.attach('grid-log', { body: (gridLogs.get(page) ?? []).join('\n'), contentType: 'text/plain' });
});

async function open(page: Page, order: Order, search = URL): Promise<void> {
  const log: string[] = [];
  gridLogs.set(page, log);
  page.on('console', (m) => void (m.text().startsWith('[ui.grid]') && log.push(m.text())));
  await page.clock.install();
  await openAppWith(page, search);
  if (order === 'newest first') await page.getByRole('button', { name: /Oldest first/ }).click();
  await page.locator('debrief-status-bar').getByRole('button', { name: 'Live tail', exact: true }).click();
  await expect(page.locator('debrief-status-bar').getByRole('button', { name: 'Live tail', exact: true })).toHaveAttribute('aria-pressed', 'true');
}

const follow = (page: Page) => page.getByTestId('follow-toggle');
const metrics = (page: Page) => viewport(page).evaluate((el) => ({ gap: el.scrollHeight - el.clientHeight - el.scrollTop, top: el.scrollTop, max: el.scrollHeight - el.clientHeight }));

/** Moves the pinned clock on in 20 ms steps until `done()` holds, at most `maxMs` of page time (virtual time, not wall time). */
async function runUntil(page: Page, done: () => Promise<boolean>, maxMs: number): Promise<boolean> {
  for (let spent = 0; spent <= maxMs; spent += 20) {
    if (await done()) return true;
    await page.clock.runFor(20);
  }
  return done();
}

/** The event id of the row at the top of the view (null while a placeholder is there). */
const topEventId = (page: Page) =>
  viewport(page).evaluate((el) => {
    const r = el.getBoundingClientRect();
    const header = el.querySelector('.header')?.getBoundingClientRect().height ?? 0;
    const row = document.elementFromPoint(r.left + 60, r.top + header + 6)?.closest('.row');
    const m = /Synthetic event (\d+)/.exec(row?.textContent ?? '');
    return m ? Number(m[1]) : null;
  });

/** The top event id once two reads in a row agree (rows are replaced while pages load: one read can catch a placeholder). */
async function stableTopEventId(page: Page): Promise<number> {
  let last: number | null = null;
  let id: number | null = null;
  const ok = await runUntil(
    page,
    async () => {
      id = await topEventId(page);
      const same = id !== null && id === last;
      last = id;
      return same;
    },
    2_000,
  );
  expect(ok, 'the top row settles on an event').toBe(true);
  return id as unknown as number;
}

/** Presses the scrollbar thumb with the real mouse and drags it by `dy` px (a user scroll that no script produces). */
async function dragScrollbar(page: Page, dy: number): Promise<void> {
  const box = await viewport(page).boundingBox();
  const m = await viewport(page).evaluate((el) => ({ top: el.scrollTop, height: el.scrollHeight, client: el.clientHeight, bar: el.offsetWidth - el.clientWidth }));
  const thumb = Math.max(20, (m.client / m.height) * m.client);
  const x = (box?.x ?? 0) + (box?.width ?? 0) - Math.max(4, m.bar / 2);
  const y = (box?.y ?? 0) + (m.top / Math.max(1, m.height - m.client)) * (m.client - thumb) + thumb / 2;
  await page.mouse.move(x - 2, y - 2); // hover first: an overlay scrollbar is only drawn (and pressable) under the pointer
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x, y + dy / 2, { steps: 5 });
  await page.mouse.move(x, y + dy, { steps: 5 });
  await page.mouse.up();
}

/** On the newest end: the last row oldest first, the first row newest first. */
async function onNewest(page: Page, order: Order): Promise<boolean> {
  const m = await metrics(page);
  return order === 'oldest first' ? m.gap <= 2 : m.top <= 2;
}

/** Goes to the newest end with the keyboard (a user move: it pins the view there). */
async function goNewest(page: Page, order: Order): Promise<void> {
  await viewport(page).focus();
  await page.keyboard.press(order === 'oldest first' ? 'End' : 'Home');
  expect(await runUntil(page, () => onNewest(page, order), 2_000), 'the view reaches the newest end').toBe(true);
}

/** The user moves the view away from the newest end: with the wheel, a key or by dragging the real scrollbar. */
async function leaveNewest(page: Page, order: Order, how: 'wheel' | 'key' | 'scrollbar drag'): Promise<void> {
  sentAt.set(page, takeOvers(page));
  const box = await viewport(page).boundingBox();
  if (how === 'wheel') {
    await page.mouse.move((box?.x ?? 0) + 200, (box?.y ?? 0) + 150);
    await page.mouse.wheel(0, order === 'oldest first' ? -900 : 900);
  } else if (how === 'key') {
    await viewport(page).focus();
    await page.keyboard.press(order === 'oldest first' ? 'PageUp' : 'PageDown');
    await page.keyboard.press(order === 'oldest first' ? 'PageUp' : 'PageDown');
  } else {
    await dragScrollbar(page, order === 'oldest first' ? -300 : 300);
  }
}

/** The number of take-overs the grid has logged on this page so far. */
const takeOvers = (page: Page) => (gridLogs.get(page) ?? []).filter((l) => l.includes('User took over')).length;
/** The number of take-overs logged when the last input was sent (a wheel or a scrollbar press). */
const sentAt = new WeakMap<Page, number>();

/**
 * The view left the newest end after an input. A wheel or scrollbar scroll is applied by the browser's scrolling machinery in real time, not
 * in page time (the pinned clock does not move it), and headless WebKit sometimes drops a wheel gesture altogether (measured: the grid's log
 * then shows no take-over). So this waits on the condition, polling in real time with page time moving beside it, and sends the input again
 * only when the grid's log shows no take-over since the last send, at most twice. When the grid did take over and the view still never left
 * (the browser delivered the event and did not scroll) the wait ends at its timeout and says so. The resends are attached to the test.
 */
async function expectLeftNewest(page: Page, order: Order, how: 'wheel' | 'key' | 'scrollbar drag' | 'wheel (scaled)'): Promise<void> {
  let resends = 0;
  let waited = 0;
  await expect
    .poll(
      async () => {
        await page.clock.runFor(20);
        waited += 1;
        if (!(await onNewest(page, order))) return 'left';
        if (resends < 2 && waited >= 3 && takeOvers(page) <= (sentAt.get(page) ?? 0)) {
          resends += 1;
          waited = 0;
          await leaveNewest(page, order, how === 'wheel (scaled)' ? 'wheel' : how);
        }
        return takeOvers(page) > (sentAt.get(page) ?? 0) ? 'taken over, the view never left' : 'waiting';
      },
      { message: `${how}: the view left the newest end` },
    )
    .toBe('left');
  test.info().annotations.push({ type: 'input-resends', description: `${how}: ${resends}` });
}

async function holdWithEvents(page: Page): Promise<void> {
  await follow(page).click();
  await expect(follow(page)).toHaveAttribute('aria-pressed', 'false');
  await page.clock.runFor(1_000);
  await expect(page.getByTestId('new-events')).toBeVisible();
}

for (const order of ORDERS) {
  test.describe(`live tail grid, ${order}`, () => {
    test('reveal, with events pushed between its pages, ends on the newest event', async ({ page }) => {
      await open(page, order);
      await holdWithEvents(page);
      await page.getByTestId('new-events').click();
      expect(await runUntil(page, () => onNewest(page, order), 3_000), 'the view reaches the newest end').toBe(true);
      await page.clock.runFor(1_000);
      expect(await runUntil(page, () => onNewest(page, order), 1_000), 'and stays there').toBe(true);
    });

    test('following over 20 pushes: pinned stays on the newest; not pinned keeps its events in view', async ({ page }) => {
      await open(page, order);
      await goNewest(page, order);
      await page.clock.runFor(1_000);
      expect(await runUntil(page, () => onNewest(page, order), 1_000), 'pinned: stays on the newest end').toBe(true);
      // Away from the newest end (a user key move): the same events stay in view while others arrive.
      await leaveNewest(page, order, 'key');
      await expectLeftNewest(page, order, 'key');
      const before = await stableTopEventId(page);
      await page.clock.runFor(1_200);
      expect(await runUntil(page, async () => (await topEventId(page)) === before, 1_000), `the top row is the same event (${before})`).toBe(true);
    });

    // Each way the user can scroll away, in the middle of a reveal (the first page of its round has been answered, later ones not yet).
    for (const how of ['wheel', 'key', 'scrollbar drag'] as const) {
      test(`${how} during a reveal leaves the view where the user put it`, async ({ page }) => {
        await open(page, order);
        await holdWithEvents(page);
        await page.getByTestId('new-events').click();
        await page.clock.runFor(40);
        await leaveNewest(page, order, how);
        await expectLeftNewest(page, order, how);
        const left = await stableTopEventId(page);
        await page.clock.runFor(1_500);
        expect(await onNewest(page, order), `${how}: and was not pulled back`).toBe(false);
        expect(await runUntil(page, async () => (await topEventId(page)) === left, 1_000), `${how}: and the same event (${left}) is at the top`).toBe(true);
      });
    }

    // The reviewer's probe: a real scrollbar drag over several refresh rounds while not pinned (newest first: each round shifts the rows).
    test('a scrollbar drag across refresh rounds ends where the user dropped it', async ({ page }) => {
      await open(page, order);
      await goNewest(page, order);
      await viewport(page).focus();
      for (let i = 0; i < 6; i++) await page.keyboard.press(order === 'oldest first' ? 'PageUp' : 'PageDown'); // not pinned, still following: rounds keep arriving
      await page.clock.runFor(300);
      const box = await viewport(page).boundingBox();
      const m = await viewport(page).evaluate((el) => ({ top: el.scrollTop, height: el.scrollHeight, client: el.clientHeight, bar: el.offsetWidth - el.clientWidth }));
      const thumb = Math.max(20, (m.client / m.height) * m.client);
      const x = (box?.x ?? 0) + (box?.width ?? 0) - Math.max(4, m.bar / 2);
      let y = (box?.y ?? 0) + (m.top / Math.max(1, m.height - m.client)) * (m.client - thumb) + thumb / 2;
      await page.mouse.move(x - 2, y - 2);
      await page.mouse.move(x, y);
      await page.mouse.down();
      const start = (await metrics(page)).top;
      // A slow drag: 180 px of thumb travel over about a second of page time, so several live rounds (a refresh each 250 ms) land in between.
      const toward = order === 'oldest first' ? -1 : 1;
      for (let i = 0; i < 12; i++) {
        y += toward * 15;
        await page.mouse.move(x, y, { steps: 3 });
        await page.clock.runFor(80);
      }
      await page.mouse.up();
      await page.clock.runFor(300);
      const dropped = (await metrics(page)).top;
      // The scrolling machinery applies the last mouse moves a little later, so the position is read again after more page time.
      await page.clock.runFor(1_500);
      const later = (await metrics(page)).top;
      expect(Math.abs(dropped - start), `the drag moved the view and it was not reverted (${start} to ${dropped})`).toBeGreaterThan(2_000);
      expect(Math.abs(later - dropped), `and it stays where it was dropped (${dropped} then ${later})`).toBeLessThanOrEqual(order === 'oldest first' ? 48 : 24 * 120);
    });

    test('a wheel at once after the reveal click (before its first page answers) is not pulled back', async ({ page }) => {
      await open(page, order);
      await holdWithEvents(page);
      // Away from the newest end first, then the reveal, then a wheel away without waiting.
      await page.getByTestId('new-events').click();
      await leaveNewest(page, order, 'wheel');
      await expectLeftNewest(page, order, 'wheel');
      await page.clock.runFor(1_500);
      expect(await onNewest(page, order), 'not pulled back to the newest end').toBe(false);
    });

    test('a wheel toward the newest end while on it is a no-op: following goes on', async ({ page }) => {
      await open(page, order);
      await goNewest(page, order);
      const box = await viewport(page).boundingBox();
      await page.mouse.move((box?.x ?? 0) + 200, (box?.y ?? 0) + 150);
      await page.mouse.wheel(0, order === 'oldest first' ? 600 : -600);
      await page.clock.runFor(1_200);
      expect(await runUntil(page, () => onNewest(page, order), 1_000), 'still following').toBe(true);
    });

    test('a wheel in the scaled scrollbar mode leaves the view where the user put it', async ({ page }) => {
      await open(page, order, SCALED);
      await holdWithEvents(page);
      await page.getByTestId('new-events').click();
      await page.clock.runFor(40);
      const box = await viewport(page).boundingBox();
      await page.mouse.move((box?.x ?? 0) + 200, (box?.y ?? 0) + 150);
      await page.mouse.wheel(0, order === 'oldest first' ? -1_200 : 1_200);
      await expectLeftNewest(page, order, 'wheel (scaled)');
      await page.clock.runFor(1_500);
      expect(await onNewest(page, order), 'not pulled back to the newest end').toBe(false);
    });

    test('resize smaller and larger while pinned: the view stays on the newest end and keeps following', async ({ page }) => {
      await open(page, order);
      await goNewest(page, order);
      for (const height of [620, 900, 720]) {
        await page.setViewportSize({ width: 1440, height });
        await page.clock.runFor(200);
        expect(await runUntil(page, () => onNewest(page, order), 1_000), `${height} px: still on the newest end`).toBe(true);
      }
      await page.clock.runFor(1_000);
      expect(await runUntil(page, () => onNewest(page, order), 1_000), 'and still following').toBe(true);
    });

    test('resize while not pinned keeps the top row', async ({ page }) => {
      await open(page, order);
      await goNewest(page, order);
      await leaveNewest(page, order, 'key');
      await expectLeftNewest(page, order, 'key');
      const before = await stableTopEventId(page);
      for (const height of [620, 900]) {
        await page.setViewportSize({ width: 1440, height });
        await page.clock.runFor(300);
        expect(await runUntil(page, async () => (await topEventId(page)) === before, 1_000), `${height} px: the top row is kept (${before})`).toBe(true);
      }
    });

    test('Follow off counts the new events; Follow on shows the newest', async ({ page }) => {
      await open(page, order);
      await goNewest(page, order);
      await holdWithEvents(page);
      await expect(page.getByTestId('new-events')).toHaveText(/\d[\d,]* new events? — show/);
      await follow(page).click();
      await expect(follow(page)).toHaveAttribute('aria-pressed', 'true');
      expect(await runUntil(page, () => onNewest(page, order), 3_000), 'following again: the newest end').toBe(true);
      await expect(page.getByTestId('new-events')).toHaveCount(0);
    });
  });
}

// Find in page and assistive technology scroll with no input event: the grid keeps that position (it only undoes the browser's own
// adjustments while the layout changes, ADR 0072 decision 2).
test('a scroll with no input event is kept while live tail adds events (oldest first, keep mode)', async ({ page }) => {
  await open(page, 'oldest first');
  await expect(gridRows(page).first()).toBeVisible();
  await page.clock.runFor(500);
  await viewport(page).evaluate((el) => {
    el.scrollTop = 30_000;
  });
  await page.clock.runFor(1_500);
  expect((await metrics(page)).top, 'kept where the scroll put it, though events arrived').toBeGreaterThan(29_000);
});
