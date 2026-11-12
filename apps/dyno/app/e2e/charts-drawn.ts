import type { Page } from '@decryp7/paddock-e2e';

/**
 * Every chart has drawn and finished its entry animation: each dy-chart has a canvas, and the painted pixels of all canvases are the same
 * in two checks a frame apart (the first draw loads ECharts on demand and animates, so a capture before that shows empty axes).
 * `minPainted` (device pixels, default 0) also needs every canvas to hold that many painted pixels, so a chart that is up but has no
 * bars or axis yet does not count as drawn. A condition, never a delay (ER-12).
 */
export async function chartsDrawn(page: Page, minPainted = 0): Promise<void> {
  await page.waitForFunction((min) => {
    const charts = [...document.querySelectorAll('dy-chart')];
    if (charts.length === 0) return true;
    if (charts.some((c) => !c.querySelector('canvas'))) return false;
    const counts = [...document.querySelectorAll<HTMLCanvasElement>('dy-chart canvas')].map((cv) => {
      const d = cv.getContext('2d')!.getImageData(0, 0, cv.width, cv.height).data;
      let n = 0;
      for (let i = 3; i < d.length; i += 4) if (d[i]! > 0) n++;
      return n;
    });
    if (counts.some((n) => n < min)) return false;
    const sig = counts.join(',');
    const w = window as unknown as { __sig?: string; __same?: number };
    w.__same = w.__sig === sig ? (w.__same ?? 0) + 1 : 0;
    w.__sig = sig;
    return w.__same >= 8;
  }, minPainted, { timeout: 30_000, polling: 'raf' });
}

