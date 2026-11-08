import { expect, type Locator, type Page } from '@playwright/test';

/**
 * Where to press the mouse on an element, once it has stopped moving: waits (on the condition, never a sleep) until the element's
 * box is the same on two animation frames, then checks that the point lies over the element it means to grab
 * (`elementFromPoint(...).closest(within)`), so a press aimed at coordinates read earlier fails with a clear message instead of
 * landing on whatever moved there. `dy` presses that many pixels below the element's top instead of its middle.
 */
export async function stablePressPoint(page: Page, el: Locator, within: string, dy?: number): Promise<{ x: number; y: number }> {
  const settled = (): Promise<string> =>
    el.evaluate(
      (e) =>
        new Promise<string>((done) =>
          requestAnimationFrame(() => {
            const r = e.getBoundingClientRect();
            requestAnimationFrame(() => {
              const s = e.getBoundingClientRect();
              done(r.x === s.x && r.y === s.y && r.width === s.width && r.height === s.height ? `${s.x},${s.y},${s.width},${s.height}` : '');
            });
          }),
        ),
    );
  await expect.poll(settled, { message: 'the element to press keeps still', timeout: 10_000 }).not.toBe('');
  const b = await el.boundingBox();
  if (!b) throw new Error('the element to press has no box');
  const x = b.x + b.width / 2;
  const y = dy === undefined ? b.y + b.height / 2 : b.y + dy;
  const hit = await page.evaluate(({ x, y, within }) => document.elementFromPoint(x, y)?.closest(within) != null, { x, y, within });
  expect(hit, `the pointer at ${x},${y} is not over ${within}: the element moved before the press`).toBe(true);
  return { x, y };
}
