import { expect, type Browser } from '@playwright/test';
import { mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { dialogWindowUrl, type DialogContractEntry } from './shell.ts';

/** The scales of the size checks (ADR 0043 check 4): CSS rounding and re-wrap show at the fractional ones. */
export const SIZE_SCALES = [1, 1.25, 1.5] as const;

/** The fixture a dialog offers: the button that adds content after the first measure. */
export interface SizeFixture {
  /** The dialog id (a fixture of one kind). */
  readonly id: string;
  /** Its contract (kind and width). */
  readonly contract: DialogContractEntry;
  /** The `data-testid` of the button that adds content (default `fixture-add`). */
  readonly addButton?: string;
}

/**
 * ADR 0043 check 4 for one fixture dialog at one scale, in the browser build (this exercises the page only: the loop, its
 * baseline, the epoch; never Rust's conversion). The window is as wide as its contract; after the content arrives late:
 * the last measure sent is the one for the new content, the body does not scroll in a window as tall as the measure (below
 * the cap), the footer buttons are inside the viewport and on top, one change caused at most two sends, and a reload sends
 * with a new epoch. Waits are on state (`data-fit-seq` quiet for two frames), never on time (rule 60).
 */
export async function expectSizeToContent(browser: Browser, baseURL: string, fixture: SizeFixture, scale: number, shots?: string): Promise<void> {
  const { id, contract } = fixture;
  const where = `${id} at ${scale}x`;
  const cap = Math.floor(1080 * 0.75);
  const context = await browser.newContext({ baseURL, deviceScaleFactor: scale, viewport: { width: contract.width, height: cap } });
  try {
    const page = await context.newPage();
    const state = () =>
      page.evaluate(() => ({
        seq: Number(document.documentElement.dataset['fitSeq'] ?? '0'),
        epoch: Number(document.documentElement.dataset['fitEpoch'] ?? '0'),
        height: Number(document.documentElement.dataset['fittedHeight'] ?? '0'),
      }));
    // No more sends while two frames pass: the loop is quiet.
    const quiet = async (): Promise<ReturnType<typeof state>> => {
      let before = await state();
      await expect
        .poll(async () => {
          await page.evaluate(() => new Promise<void>((r) => requestAnimationFrame(() => requestAnimationFrame(() => r()))));
          const now = await state();
          const same = now.seq === before.seq;
          before = now;
          return same;
        }, { message: `${where}: the size loop never went quiet` })
        .toBe(true);
      return before;
    };
    await page.goto(dialogWindowUrl(id, {}, { title: id }));
    await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
    await page.locator('html[data-fit-settled]').waitFor({ timeout: 15_000 });
    const first = await quiet();
    // For the independent review (ER-23): the window as the frame sized it, before and after the late content.
    const shot = async (name: string, height: number) => {
      if (!shots) return;
      mkdirSync(shots, { recursive: true });
      await page.setViewportSize({ width: contract.width, height: contract.kind === 'tool' ? cap : Math.min(height, cap) });
      await page.screenshot({ path: join(shots, `${id}-${scale}x-${name}.png`) });
    };
    await shot('initial', first.height);
    await page.getByTestId(fixture.addButton ?? 'fixture-add').click();
    const later = await quiet();
    await shot('late', later.height);
    // A tool's frame fills its window (its size is the user's), so its measure does not follow the content; every other kind does.
    if (contract.kind !== 'tool') expect(later.height, `${where}: the content grew, the measure must too`).toBeGreaterThan(first.height);
    expect(later.seq - first.seq, `${where}: one content change sent ${later.seq - first.seq} measures (at most two)`).toBeLessThanOrEqual(2);
    expect(later.epoch).toBe(first.epoch);
    if (contract.kind !== 'tool') {
      // A window as tall as the last measure (within the cap) shows all of it, and its footer.
      const height = Math.min(later.height, cap);
      await page.setViewportSize({ width: contract.width, height });
      const body = await page.evaluate(() => {
        const el = document.querySelector('[data-testid=dialog-body]');
        return el ? { scroll: el.scrollHeight, client: el.clientHeight } : null;
      });
      if (later.height <= cap && body) expect(body.scroll, `${where}: the body scrolls in a window as tall as its measure`).toBeLessThanOrEqual(body.client + 1);
      const buttons = page.locator('[data-testid=dialog-footer] button');
      const n = await buttons.count();
      expect(n, `${where}: no footer button`).toBeGreaterThan(0);
      for (let i = 0; i < n; i++) {
        const box = await buttons.nth(i).boundingBox();
        expect(box, `${where}: footer button ${i} is not drawn`).not.toBeNull();
        expect(box!.y + box!.height, `${where}: footer button ${i} is below the window`).toBeLessThanOrEqual(height + 0.5);
        const hit = await page.evaluate(([x, y]) => document.elementFromPoint(x!, y!)?.closest('button') !== null, [box!.x + box!.width / 2, box!.y + box!.height / 2]);
        expect(hit, `${where}: footer button ${i} is covered`).toBe(true);
      }
    }
    // A reload is a new page load: the next measure carries a new epoch, from seq 1.
    await page.reload();
    await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
    await page.locator('html[data-fit-settled]').waitFor({ timeout: 15_000 });
    const reloaded = await quiet();
    expect(reloaded.epoch, `${where}: a reload sent the same epoch`).toBe(first.epoch + 1);
  } finally {
    await context.close();
  }
}
