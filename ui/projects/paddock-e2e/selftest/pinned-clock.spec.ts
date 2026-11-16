import { test as base, expect } from '@playwright/test';
import { test as kit } from '../src/fixtures.ts';
import { clickWithLivePaused, newPinnedContext, pauseLive, pinnedClockTest, resumeLive, withLivePaused } from '../src/index.ts';

const test = pinnedClockTest(kit, { now: '2001-09-09T01:46:40Z', timezoneId: 'Asia/Singapore', locale: 'en-GB' });
// The same wrapper over plain Playwright, for the checks that need no kit fixture.
const plain = pinnedClockTest(base, { now: '2001-09-09T01:46:40Z', timezoneId: 'Asia/Singapore', locale: 'en-GB' });

plain('a page starts at the pinned instant in the pinned zone and locale, and the clock ticks', async ({ page }) => {
  await page.goto('data:text/html,<p>x</p>');
  const read = () => page.evaluate(() => ({ now: new Date().toISOString(), tz: Intl.DateTimeFormat().resolvedOptions().timeZone, lang: navigator.language }));
  const first = await read();
  expect(first.now.startsWith('2001-09-09T01:46:')).toBe(true);
  expect(first.tz).toBe('Asia/Singapore');
  expect(first.lang).toBe('en-GB');
  await page.clock.runFor(60_000);
  expect((await read()).now >= '2001-09-09T01:47:').toBe(true);
});

plain('a page opened from the context is pinned too', async ({ context }) => {
  const other = await context.newPage();
  expect(await other.evaluate(() => new Date().toISOString().slice(0, 13))).toBe('2001-09-09T01');
});

plain.describe('opted out', () => {
  // The spec needs the real time.
  plain.use({ pinnedClock: false });
  plain('keeps the real clock', async ({ page }) => {
    expect(Math.abs((await page.evaluate(() => Date.now())) - Date.now())).toBeLessThan(60_000);
  });
});

plain('a second install in the spec wins over the pin', async ({ page }) => {
  await page.clock.install({ time: new Date('2001-01-01T00:00:00Z') });
  expect(await page.evaluate(() => new Date().toISOString().slice(0, 10))).toBe('2001-01-01');
});

// The timers are the subject here, not waits, and C71 (rule 49) reads the call names: inside the page the timer functions are called
// through the window by name.
type Timers = Record<'setTimeout' | 'setInterval' | 'requestAnimationFrame', (cb: (...a: unknown[]) => void, ...rest: unknown[]) => unknown>;

// Playwright's fake clock catches an exception a timer callback throws; the fixture reports it the way a browser does.
test.describe('a throw from a timer callback reaches pageErrors and the window error event', () => {
  const plant = (page: import('@playwright/test').Page, body: () => void) => page.evaluate(body);

  test('setTimeout', async ({ page, pageErrors }) => {
    await page.goto('data:text/html,<p>x</p>');
    await plant(page, () => void (window as unknown as Timers)['setTimeout'](() => { throw new Error('boom-timeout'); }, 0));
    await expect.poll(() => pageErrors).toEqual(['boom-timeout']);
  });

  test('three in one batch: every one is reported, not only the first', async ({ page, pageErrors }) => {
    await page.goto('data:text/html,<p>x</p>');
    await plant(page, () => {
      for (const n of ['a', 'b', 'c']) (window as unknown as Timers)['setTimeout'](() => { throw new Error(`boom-${n}`); }, 0);
    });
    await expect.poll(() => [...pageErrors].sort()).toEqual(['boom-a', 'boom-b', 'boom-c']);
  });

  test('setInterval, requestAnimationFrame, requestIdleCallback, a string callback and the window error event', async ({ page, pageErrors }) => {
    await page.goto('data:text/html,<p>x</p>');
    await page.evaluate(() => {
      const w = window as unknown as { __seen: string[] };
      w.__seen = [];
      window.addEventListener('error', (e) => w.__seen.push((e.error as Error | null)?.message ?? e.message));
      const id = (window as unknown as Timers)['setInterval'](() => { clearInterval(id as number); throw new Error('boom-interval'); }, 5);
      (window as unknown as Timers)['requestAnimationFrame'](() => { throw new Error('boom-raf'); });
      // WebKit has no requestIdleCallback.
      (window as unknown as { requestIdleCallback?: (cb: () => void) => void }).requestIdleCallback?.(() => { throw new Error('boom-idle'); });
      (window as unknown as Timers)['setTimeout']('throw new Error("boom-string")' as never, 0);
    });
    const planted = ['boom-interval', 'boom-raf', 'boom-string', ...((await page.evaluate(() => 'requestIdleCallback' in window)) ? ['boom-idle'] : [])].sort();
    await expect.poll(() => [...pageErrors].sort()).toEqual(planted);
    // One window event per throw; WebKit hides the message of an error on a data: page ("Script error."), so count them.
    expect(await page.evaluate(() => (window as unknown as { __seen: string[] }).__seen.length)).toBe(planted.length);
  });

  test('ids, string-free arguments and runFor are unchanged', async ({ page, pageErrors }) => {
    await page.goto('data:text/html,<p>x</p>');
    const r = await page.evaluate(
      () =>
        new Promise<{ id: string; arg: number }>((resolve) => {
          const id = (window as unknown as Timers)['setTimeout']((a: unknown) => resolve({ id: typeof id, arg: a as number }), 50, 7);
        }),
    );
    expect(r).toEqual({ id: 'number', arg: 7 });
    await page.evaluate(() => { (window as unknown as { __n: number }).__n = 0; (window as unknown as Timers)['setTimeout'](() => { (window as unknown as { __n: number }).__n = 1; }, 10_000); });
    await page.clock.runFor(10_000);
    expect(await page.evaluate(() => (window as unknown as { __n: number }).__n)).toBe(1);
    expect(pageErrors).toEqual([]);
  });
});

// A context of a spec's own: pinned like the fixture's, with its own options kept and a timer throw reported.
test.describe('newPinnedContext', () => {
  test('is pinned, keeps its own options, and reports a timer throw', async ({ browser }) => {
    const context = await newPinnedContext(browser, { viewport: { width: 600, height: 400 }, pin: { now: '2001-09-09T01:46:40Z', timezoneId: 'Asia/Singapore', locale: 'en-GB' } });
    try {
      const page = await context.newPage();
      const errors: string[] = [];
      page.on('pageerror', (e) => errors.push(e.message));
      await page.goto('data:text/html,<p>x</p>');
      const read = await page.evaluate(() => ({ now: new Date().toISOString().slice(0, 13), tz: Intl.DateTimeFormat().resolvedOptions().timeZone, lang: navigator.language, w: innerWidth }));
      expect(read).toEqual({ now: '2001-09-09T01', tz: 'Asia/Singapore', lang: 'en-GB', w: 600 });
      await page.evaluate(() => void (window as unknown as Timers)['setTimeout'](() => { throw new Error('boom-context'); }, 0));
      await expect.poll(() => errors).toEqual(['boom-context']);
    } finally {
      await context.close();
    }
  });

  test('takes the wrapper defaults, and pin false is the plain context', async ({ browser }) => {
    const pinned = await newPinnedContext(browser);
    const plainContext = await newPinnedContext(browser, { pin: false });
    try {
      const a = await pinned.newPage();
      const b = await plainContext.newPage();
      expect(await a.evaluate(() => new Date().toISOString().slice(0, 16))).toBe('2026-10-22T14:30');
      expect(await a.evaluate(() => Intl.DateTimeFormat().resolvedOptions().timeZone)).toBe('UTC');
      expect(Math.abs((await b.evaluate(() => Date.now())) - Date.now())).toBeLessThan(60_000);
    } finally {
      await pinned.close();
      await plainContext.close();
    }
  });
});

// A live page: an interval refreshes the page every 20 ms; pauseLive stops it, actions still work, resume and runFor drive it again.
plain.describe('pauseLive', () => {
  const live = async (page: import('@playwright/test').Page) => {
    await page.goto('about:blank');
    await page.setContent('<button id="b">x</button><p id="n">0</p>');
    await page.evaluate(() => {
      let n = 0;
      (window as unknown as Timers)['setInterval'](() => { document.getElementById('n')!.textContent = String(++n); }, 20);
    });
  };
  const count = (page: import('@playwright/test').Page) => page.evaluate(() => Number(document.getElementById('n')!.textContent));

  plain('stops the updates, lets Playwright act, and resume continues them', async ({ page }) => {
    await live(page);
    await expect.poll(() => count(page)).toBeGreaterThan(2);
    await pauseLive(page);
    const frozen = await count(page);
    await page.getByRole('button', { name: 'x' }).click(); // an action under the paused clock does not hang
    // Real time passes while the clock is paused: round trips to the page until the host's own clock has moved 300 ms.
    for (const t0 = Date.now(); Date.now() - t0 < 300; ) await count(page);
    expect(await count(page)).toBe(frozen);
    await page.clock.runFor(200);
    expect(await count(page)).toBeGreaterThan(frozen);
    await resumeLive(page);
    const resumed = await count(page);
    await expect.poll(() => count(page)).toBeGreaterThan(resumed);
  });

  plain('clickWithLivePaused clicks with the updates stopped and resumes them', async ({ page }) => {
    await live(page);
    await page.evaluate(() => { document.getElementById('b')!.addEventListener('click', () => { (window as unknown as { __clicks: number }).__clicks = ((window as unknown as { __clicks?: number }).__clicks ?? 0) + 1; }); });
    await clickWithLivePaused(page, page.getByRole('button', { name: 'x' }));
    expect(await page.evaluate(() => (window as unknown as { __clicks: number }).__clicks)).toBe(1);
    const after = await count(page);
    await expect.poll(() => count(page)).toBeGreaterThan(after);
  });

  plain('a pause moves the clock about 250 ms and fires the timers due in it; repeated pauses add up', async ({ page }) => {
    await live(page);
    await expect.poll(() => count(page)).toBeGreaterThan(1);
    const at = () => page.evaluate(() => Date.now());
    const before = await at();
    await pauseLive(page);
    const first = await at();
    expect(first - before).toBeGreaterThanOrEqual(250);
    expect(first - before).toBeLessThan(1500); // the look-ahead, doubled at most twice, plus the real time since the read
    const ticks = await count(page);
    await pauseLive(page);
    const second = await at();
    expect(second - first).toBeGreaterThanOrEqual(250); // each pause moves the clock again
    expect(await count(page)).toBeGreaterThan(ticks); // the 20 ms interval fired on the way
    await resumeLive(page);
  });

  plain('withLivePaused resumes afterwards, also when its steps throw', async ({ page }) => {
    await live(page);
    await expect(withLivePaused(page, async () => { throw new Error('steps failed'); })).rejects.toThrow('steps failed');
    const after = await count(page);
    await expect.poll(() => count(page)).toBeGreaterThan(after);
  });
});
