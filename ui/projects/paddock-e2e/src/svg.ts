import type { Locator, Page } from '@playwright/test';

/** A point in page (client) coordinates. */
export interface PagePoint {
  readonly x: number;
  readonly y: number;
}

/** How far (CSS px) around a point the shape must still be under the pointer for the point to count as safely on it. */
export const POINT_CUSHION_PX = 2;

/** A probe of the shape that is on top, nearest the box centre first, and whether the shape is also on top all around it. */
export interface ShapeProbe extends PagePoint {
  /** True when the shape is the element under the pointer at every point {@link POINT_CUSHION_PX} px away (the 8 neighbours). */
  readonly cushioned: boolean;
}

/**
 * The point to use among probes ordered nearest the box centre first: the first with room around it, else the first on the shape.
 * A point on the very edge of the fill (the nearest to the centre of a half ring often is) passes `elementFromPoint` but a real click
 * rounds to the device pixel and WebKit then lands beside the shape (Dyno's disk map, a 50% segment, 2026-11-17).
 */
export function pickProbe(probes: readonly ShapeProbe[]): PagePoint | null {
  const best = probes.find((p) => p.cushioned) ?? probes[0];
  return best ? { x: best.x, y: best.y } : null;
}

/**
 * A point that is really on an SVG shape, in page coordinates. The middle of the bounding box of an annular segment (a ring
 * sector, a donut slice) is usually in the hole, so a plain `locator.click()` lands on whatever is there, or hangs on "intercepts
 * pointer events". The point is the one nearest the box centre among a grid of probes that `isPointInFill` accepts and that
 * `elementFromPoint` confirms is the shape itself (nothing on top of it), preferring one with room all around it ({@link pickProbe}).
 * It is measured again for up to a second while the page settles (a shape re-created by a render, a layout that is still moving).
 * Throws when no probe is on the shape.
 */
export async function pointInShape(shape: Locator): Promise<PagePoint> {
  const measure = (): Promise<PagePoint | null> =>
    shape
      .evaluate(
        async (el, cushion) => {
          const g = el as unknown as SVGGeometryElement;
          // Scrolled by the page itself: Playwright's own scroll waits for a stable box, which a shape in a fading group never has.
          g.scrollIntoView({ block: 'center', inline: 'center' });
          await new Promise<void>((done) => requestAnimationFrame(() => requestAnimationFrame(() => done())));
          const box = g.getBBox();
          const ctm = g.getScreenCTM();
          if (!ctm) return [];
          const steps = 24;
          const probes: { x: number; y: number; d: number }[] = [];
          for (let i = 0; i <= steps; i += 1) {
            for (let j = 0; j <= steps; j += 1) {
              const p = new DOMPoint(box.x + (box.width * i) / steps, box.y + (box.height * j) / steps);
              if (g.isPointInFill(p)) probes.push({ x: p.x, y: p.y, d: Math.hypot(p.x - (box.x + box.width / 2), p.y - (box.y + box.height / 2)) });
            }
          }
          probes.sort((a, b) => a.d - b.d);
          const out: { x: number; y: number; cushioned: boolean }[] = [];
          for (const probe of probes) {
            const at = new DOMPoint(probe.x, probe.y).matrixTransform(ctm);
            if (document.elementFromPoint(at.x, at.y) !== el) continue;
            let cushioned = true;
            for (const [dx, dy] of [[-1, -1], [0, -1], [1, -1], [-1, 0], [1, 0], [-1, 1], [0, 1], [1, 1]] as const) {
              if (document.elementFromPoint(at.x + dx * cushion, at.y + dy * cushion) !== el) cushioned = false;
            }
            out.push({ x: at.x, y: at.y, cushioned });
            // The nearest cushioned one is the answer: no need to look further.
            if (cushioned) break;
          }
          return out;
        },
        POINT_CUSHION_PX,
      )
      .then(pickProbe);
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
