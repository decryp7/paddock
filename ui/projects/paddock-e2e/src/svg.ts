import type { Locator, Page } from '@playwright/test';

/** A point in page (client) coordinates. */
export interface PagePoint {
  readonly x: number;
  readonly y: number;
}

/**
 * A point that is really on an SVG shape, in page coordinates. The middle of the bounding box of an annular segment (a ring
 * sector, a donut slice) is usually in the hole, so a plain `locator.click()` lands on whatever is there, or hangs on "intercepts
 * pointer events". The point is the one nearest the box centre among a grid of probes that `isPointInFill` accepts and that
 * `elementFromPoint` confirms is the shape itself (nothing on top of it). It is measured again for up to a second while the page
 * settles (a shape re-created by a render, a layout that is still moving). Throws when no probe is on the shape.
 */
export async function pointInShape(shape: Locator): Promise<PagePoint> {
  const measure = (): Promise<PagePoint | null> =>
    shape.evaluate(async (el) => {
      const g = el as unknown as SVGGeometryElement;
      // Scrolled by the page itself: Playwright's own scroll waits for a stable box, which a shape in a fading group never has.
      g.scrollIntoView({ block: 'center', inline: 'center' });
      await new Promise<void>((done) => requestAnimationFrame(() => requestAnimationFrame(() => done())));
      const box = g.getBBox();
      const ctm = g.getScreenCTM();
      if (!ctm) return null;
      const steps = 24;
      const probes: { x: number; y: number; d: number }[] = [];
      for (let i = 0; i <= steps; i += 1) {
        for (let j = 0; j <= steps; j += 1) {
          const p = new DOMPoint(box.x + (box.width * i) / steps, box.y + (box.height * j) / steps);
          if (g.isPointInFill(p)) probes.push({ x: p.x, y: p.y, d: Math.hypot(p.x - (box.x + box.width / 2), p.y - (box.y + box.height / 2)) });
        }
      }
      probes.sort((a, b) => a.d - b.d);
      for (const probe of probes) {
        const out = new DOMPoint(probe.x, probe.y).matrixTransform(ctm);
        if (document.elementFromPoint(out.x, out.y) === el) return { x: out.x, y: out.y };
      }
      return null;
    });
  for (let attempt = 0; attempt < 20; attempt += 1) {
    const point = await measure().catch(() => null);
    if (point) return point;
  }
  throw new Error('pointInShape: no point of the shape is both inside its fill and on top (is it drawn and visible?)');
}

/** A real pointer click inside an SVG shape (see {@link pointInShape}). */
export async function clickShape(page: Page, shape: Locator, options: { button?: 'left' | 'right' } = {}): Promise<void> {
  const p = await pointInShape(shape);
  await page.mouse.click(p.x, p.y, { button: options.button ?? 'left' });
}

/** A real pointer move onto an SVG shape (see {@link pointInShape}). */
export async function hoverShape(page: Page, shape: Locator): Promise<void> {
  const p = await pointInShape(shape);
  await page.mouse.move(p.x, p.y);
}
