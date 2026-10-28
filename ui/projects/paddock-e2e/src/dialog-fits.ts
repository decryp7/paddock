import type { Browser } from '@playwright/test';
import { dialogWindowUrl, type DialogContractEntry } from './shell.ts';

/** The scales and work areas of ADR 0016 check 1 (ADR 0026 §6). */
export const FIT_SCALES = [1, 1.25, 1.5] as const;
export const FIT_WORK_AREAS = [
  { width: 1920, height: 1080 },
  { width: 1366, height: 768 },
] as const;
/** The largest share of the work area a dialog's preferred size takes (`DIALOG_CAP_FRACTION`). */
const CAP = 0.75;
/** The most space allowed below the footer's buttons. */
export const MAX_FOOTER_GAP = 24;

export interface DialogFitOptions {
  /** The app's base URL (the browser build). */
  readonly baseURL: string;
  /** Fixtures to open it with: one or more (a short and a long one). */
  readonly inputs?: readonly unknown[];
  readonly scales?: readonly number[];
  readonly workAreas?: readonly { readonly width: number; readonly height: number }[];
}

/**
 * ADR 0016 check 1 for dialog `id` (ADR 0026 §6): at every scale and work area, opened by URL as its window, the window
 * is given the size the frame asked for (the contract's width; a measured height capped to 75 % of the work area and
 * never below the minimum content; a tool's preferred size within the cap), and every footer button must lie inside it,
 * be on top (`elementFromPoint`) and leave at most 24 px below it. Returns the problems, empty when it fits.
 */
export async function dialogFitProblems(browser: Browser, id: string, contract: DialogContractEntry, options: DialogFitOptions): Promise<string[]> {
  const problems: string[] = [];
  for (const scale of options.scales ?? FIT_SCALES) {
    for (const area of options.workAreas ?? FIT_WORK_AREAS) {
      for (const input of options.inputs ?? [{}]) {
        const where = `${id} at ${scale}x on ${area.width}x${area.height}`;
        const capH = Math.floor(area.height * CAP);
        const width = Math.min(contract.width, Math.floor(area.width * CAP));
        const context = await browser.newContext({ baseURL: options.baseURL, deviceScaleFactor: scale, viewport: { width, height: capH } });
        try {
          const page = await context.newPage();
          await page.goto(dialogWindowUrl(id, input, { title: id }));
          await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
          const asked = await page.evaluate(() => ({ h: Number(document.documentElement.dataset['fittedHeight'] ?? '0'), min: document.documentElement.dataset['minContent'] ?? '0x0' }));
          const minH = Number(asked.min.split('x')[1] ?? '0');
          const height = contract.kind === 'tool' ? Math.min(contract.height ?? capH, capH) : Math.max(Math.min(asked.h, capH), minH);
          await page.setViewportSize({ width, height });
          const buttons = page.locator('[data-testid=dialog-footer] button');
          const n = await buttons.count();
          if (n === 0) problems.push(`${where}: no footer button`);
          for (let i = 0; i < n; i++) {
            const b = buttons.nth(i);
            const box = await b.boundingBox();
            const name = (await b.getAttribute('data-testid')) ?? `button ${i}`;
            if (!box) {
              problems.push(`${where}: ${name} is not drawn`);
              continue;
            }
            if (box.x < 0 || box.x + box.width > width + 0.5 || box.y < 0 || box.y + box.height > height + 0.5) problems.push(`${where}: ${name} lies outside the ${width}x${height} window`);
            const hit = await page.evaluate(([x, y]) => document.elementFromPoint(x!, y!)?.closest('button')?.getAttribute('data-testid') ?? null, [box.x + box.width / 2, box.y + box.height / 2]);
            if (hit !== name) problems.push(`${where}: ${name} is covered (${hit ?? 'nothing'} is on top)`);
            const gap = height - (box.y + box.height);
            if (gap > MAX_FOOTER_GAP + 0.5) problems.push(`${where}: ${Math.round(gap)} px below ${name} (at most ${MAX_FOOTER_GAP})`);
          }
        } finally {
          await context.close();
        }
      }
    }
  }
  return problems;
}
