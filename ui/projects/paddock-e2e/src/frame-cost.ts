import type { Page } from '@playwright/test';

/**
 * Frame cost of a page while something happens (a drag): per frame, the time from its `requestAnimationFrame` callback to the
 * first task after rendering (style, layout and paint included). This measures frames; it is not a wait, so it lives in the kit
 * and not in a spec (conformance C71 flags animation-frame waits in specs).
 */
export async function startFrameCost(page: Page): Promise<void> {
  await page.evaluate(() => {
    const w = window as unknown as { __frames: number[]; __stop: boolean };
    w.__frames = [];
    w.__stop = false;
    const tick = () => {
      if (w.__stop) return;
      const t0 = performance.now();
      const ch = new MessageChannel();
      ch.port1.onmessage = () => w.__frames.push(performance.now() - t0);
      ch.port2.postMessage(0);
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
}

/** Stops the sampler [`startFrameCost`] started and returns the frame costs in milliseconds, in frame order. */
export async function stopFrameCost(page: Page): Promise<number[]> {
  return page.evaluate(() => {
    const w = window as unknown as { __frames: number[]; __stop: boolean };
    w.__stop = true;
    return w.__frames;
  });
}
