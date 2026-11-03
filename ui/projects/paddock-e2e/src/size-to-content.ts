import { expect, type Browser } from '@playwright/test';
import { mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { dialogWindowUrl, type DialogContractEntry } from './shell.ts';

/** The scales of the size checks (ADR 0043 check 4): CSS rounding and re-wrap show at the fractional ones. */
export const SIZE_SCALES = [1, 1.25, 1.5] as const;

/**
 * The least a dialog window can be, logical px: `DIALOG_MIN` (320 x 200) for every kind, and the viewer's kind floor `VIEWER_MIN`
 * (680 x 560, which is its OS minimum, so a viewer opens at the larger of its content and 560). Rust applies them in
 * `size.rs`; the browser has no window, so the helper sizes the viewport the same way.
 */
export function kindFloor(kind: string): { width: number; height: number } {
  return kind === 'viewer' ? { width: 680, height: 560 } : { width: 320, height: 200 };
}

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
    // The window the frame would be given: its measure within the cap, never below the kind's floor; a tool keeps its default size.
    const windowHeight = (measured: number) => (contract.kind === 'tool' ? Math.min(contract.height ?? 680, cap) : Math.max(kindFloor(contract.kind).height, Math.min(measured, cap)));
    const shot = async (name: string, measured: number) => {
      const height = windowHeight(measured);
      expect(height, `${where}: a window below the 200 px floor`).toBeGreaterThanOrEqual(200);
      await page.setViewportSize({ width: contract.width, height });
      if (!shots) return;
      mkdirSync(shots, { recursive: true });
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
      const height = windowHeight(later.height);
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
    // A long error arrives late too: the loop follows it, and the review sees it.
    await page.getByTestId('fixture-error').click();
    const withError = await quiet();
    expect(withError.seq - later.seq, `${where}: an error sent ${withError.seq - later.seq} measures (at most two)`).toBeLessThanOrEqual(2);
    await shot('error', withError.height);
    // Content taller than the screen allows: the window stops at the cap, the body scrolls and the footer stays in view.
    if (contract.kind !== 'tool') {
      let tall = withError;
      for (let i = 0; i < 8 && tall.height <= cap; i++) {
        await page.getByTestId(fixture.addButton ?? 'fixture-add').click();
        tall = await quiet();
      }
      expect(tall.height, `${where}: could not make the content taller than the cap`).toBeGreaterThan(cap);
      await shot('capped', tall.height);
      const body = await page.evaluate(() => {
        const el = document.querySelector('[data-testid=dialog-body]');
        return el ? { scroll: el.scrollHeight, client: el.clientHeight } : null;
      });
      expect(body && body.scroll > body.client + 1, `${where}: at the cap the body must scroll`).toBe(true);
      const foot = await page.locator('[data-testid=dialog-footer] button').first().boundingBox();
      expect(foot && foot.y + foot.height <= cap + 0.5, `${where}: the footer is cut off at the cap`).toBe(true);
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

/**
 * ADR 0043 check 4 for a real dialog of any app, with no fixture: opened by URL at `scale` as its window, with the content
 * it has, the size loop goes quiet in a few sends (the first measure, each stage of late data, one follow-up: at most six), does not
 * log "did not settle", and sends in epoch 1; a reload sends in epoch 2. The page only (never Rust's conversion).
 */
export async function expectLoopSettles(browser: Browser, baseURL: string, id: string, contract: DialogContractEntry, scale: number, input: unknown = {}): Promise<void> {
  const where = `${id} at ${scale}x`;
  const context = await browser.newContext({ baseURL, deviceScaleFactor: scale, viewport: { width: contract.width, height: Math.floor(1080 * 0.75) } });
  try {
    const page = await context.newPage();
    const warnings: string[] = [];
    page.on('console', (m) => {
      if (m.text().includes('did not settle')) warnings.push(m.text());
    });
    const state = () =>
      page.evaluate(() => ({ seq: Number(document.documentElement.dataset['fitSeq'] ?? '0'), epoch: Number(document.documentElement.dataset['fitEpoch'] ?? '0') }));
    const settle = async () => {
      await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
      await page.locator('[data-testid=dialog-loading]').waitFor({ state: 'detached', timeout: 15_000 });
      await page.locator('html[data-fit-settled]').waitFor({ timeout: 15_000 });
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
    await page.goto(dialogWindowUrl(id, input, { title: id }));
    const first = await settle();
    expect(first.seq, `${where}: ${first.seq} measures sent before it settled (a dialog that loads data sends a few: the first, each stage of the data, one follow-up; at most six)`).toBeLessThanOrEqual(6);
    expect(first.epoch).toBe(1);
    await page.reload();
    const again = await settle();
    expect(again.epoch, `${where}: a reload sent the same epoch`).toBe(2);
    expect(warnings, `${where}: the size loop reported a loop`).toEqual([]);
  } finally {
    await context.close();
  }
}
