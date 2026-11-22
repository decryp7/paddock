import { expect, test } from '@playwright/test';
import type { Page } from '@playwright/test';
import { DOCK_LOOP_STEPS, expectDockPlanned } from '../src/index.ts';

// `expectDockPlanned` waits for `data-dock-planned`. A dock that keeps taking steps (`data-dock-progress`: plans, resizes) is not stuck, however
// long it takes: on loaded WebKit runners a fixed load budget ran out while the dock was still working, in both tries (debrief.yml run 8636 job
// 45078, ai-vision.e2e.ts:96; run 8559, ai-pitwall.e2e.ts:68). A dock that stands still, or one that never stops re-planning, fails and says which.

/** One wait of the tests below: 20 steps of 50 ms fit in it, so a step always lands inside a wait however the runner schedules them. */
const WAIT_MS = 1_000;

// The timers are the subject here, not waits, and C71 (rule 49) reads the call names: inside the page they are called through the window by name.
type Timers = Record<'setInterval' | 'clearInterval' | 'setTimeout', (...a: unknown[]) => unknown>;

/** A dock host that takes a step every `everyMs` and is planned after `steps` steps (never, when `steps` is null). */
async function dockHost(page: Page, everyMs: number, steps: number | null, plannedAfterMs?: number): Promise<void> {
  await page.setContent('<main><div class="dock" data-dock-progress="0"></div></main>');
  if (plannedAfterMs !== undefined) {
    // Planned after a quiet stretch with no step at all (whenSized waiting for its observer, the release frames).
    await page.evaluate((ms) => {
      const timers = window as unknown as Timers;
      timers.setTimeout(() => document.querySelector('.dock')!.setAttribute('data-dock-planned', ''), ms);
    }, plannedAfterMs);
    return;
  }
  await page.evaluate(
    ([everyMs, steps]) => {
      const host = document.querySelector('.dock')!;
      const timers = window as unknown as Timers;
      let taken = 0;
      const id = timers.setInterval(() => {
        if (steps !== null && taken === steps) {
          host.setAttribute('data-dock-planned', '');
          timers.clearInterval(id);
          return;
        }
        host.setAttribute('data-dock-progress', String(++taken));
      }, everyMs);
    },
    [everyMs, steps] as const,
  );
}

test('a dock that keeps working for longer than one wait is waited for until it has planned', async ({ page }) => {
  // 60 steps 50 ms apart: about 3 s, three waits of 1 s.
  await dockHost(page, 50, 60);
  await expectDockPlanned(page, { timeout: WAIT_MS });
  await expect(page.locator('[data-dock-planned]')).toBeAttached({ timeout: 1 });
});

test('the first wait lasts the caller\'s load budget, so a quiet start shorter than it is not called stuck', async ({ page }) => {
  // No step for 1.5 s, then planned: the old fixed budget (6.2 to 7.1 s left in runs 8559 and 8636) covered such a stretch, and the first wait keeps it.
  await dockHost(page, 60_000, null, 1_500);
  await expectDockPlanned(page, { timeout: WAIT_MS, firstTimeout: 3_000 });
});

test('a page with no dock host fails after one wait and says there is none', async ({ page }) => {
  await page.setContent('<main>no dock here</main>');
  const failure = await expectDockPlanned(page, { timeout: WAIT_MS }).then(
    () => null,
    (e: unknown) => e,
  );
  expect(failure).toBeInstanceOf(Error);
  expect((failure as Error).message).toContain('not shown: no dock host');
});

test('a dock that stands still fails after one wait and says it is stuck', async ({ page }) => {
  await dockHost(page, 60_000, null);
  const failure = await expectDockPlanned(page, { timeout: WAIT_MS }).then(
    () => null,
    (e: unknown) => e,
  );
  expect(failure, 'a dock that never moves is not waited for').toBeInstanceOf(Error);
  expect((failure as Error).message).toContain('The dock is stuck: it took no step (plan, resize, whenSized, release frame) during wait 1');
});

test('a dock that keeps re-planning fails at the loop limit and says so', async ({ page }) => {
  // A step every 5 ms never settles: the limit is reached within a few waits.
  await dockHost(page, 5, null);
  const failure = await expectDockPlanned(page, { timeout: WAIT_MS }).then(
    () => null,
    (e: unknown) => e,
  );
  expect(failure, 'a loop is not waited for until the test times out').toBeInstanceOf(Error);
  expect((failure as Error).message).toContain(`The dock keeps re-planning`);
  expect(DOCK_LOOP_STEPS).toBeGreaterThan(5);
});
