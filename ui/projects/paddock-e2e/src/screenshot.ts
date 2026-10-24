import { mkdirSync } from 'node:fs';
import { dirname } from 'node:path';
import { expect, type Page } from '@playwright/test';

/**
 * Screenshots of a settled frame: the fonts are loaded, no animation is running and the layout of the watched parts did
 * not change between two looks. Polled from the test side, so it also works under `page.clock.install()` (which stops
 * the page's own timers and frames).
 */

export interface StableFrameOptions {
  /** The parts whose layout must hold still (default: every element with a `data-status-item`, the frame's regions and headings). */
  readonly selector?: string;
}

const DEFAULT_WATCH = '[data-status-item], [data-pd-region], h1, h2, [role=dialog]';

/** Resolves once the frame has settled (see the file's comment). */
export async function waitForStableFrame(page: Page, options: StableFrameOptions = {}): Promise<void> {
  await page.evaluate(() => document.fonts.ready.then(() => undefined));
  const selector = options.selector ?? DEFAULT_WATCH;
  const look = () =>
    page.evaluate((sel) => {
      const running = document.getAnimations().filter((a) => a.playState === 'running').length;
      const layout = [...document.querySelectorAll(sel)]
        .map((e) => {
          const r = e.getBoundingClientRect();
          return `${e.tagName}.${e.className}:${Math.round(r.x)},${Math.round(r.y)},${Math.round(r.width)}x${Math.round(r.height)}`;
        })
        .join('|');
      return `${running}#${layout}`;
    }, selector);
  let last = '';
  await expect
    .poll(async () => {
      const now = await look();
      const settled = now === last && now.startsWith('0#');
      last = now;
      return settled;
    })
    .toBe(true);
}

/** Waits for a settled frame and writes the screenshot to `path` (its folder is made). */
export async function captureStable(page: Page, path: string, options: StableFrameOptions & { readonly fullPage?: boolean } = {}): Promise<void> {
  mkdirSync(dirname(path), { recursive: true });
  await waitForStableFrame(page, options);
  await page.screenshot({ path, fullPage: options.fullPage ?? false });
}
