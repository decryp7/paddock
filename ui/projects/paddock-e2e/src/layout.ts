import { expect, type Page } from '@playwright/test';

/**
 * Layout scenarios (rule 18): the widths every frame is checked at (the narrowest supported, the phone widths, the
 * rail threshold and its neighbours, a small laptop), and the checks that go with them.
 */
export const LAYOUT_WIDTHS: readonly number[] = [320, 360, 400, 640, 775, 776, 1024];

/** The named window sizes of the screenshots and the layout checks. */
export const WINDOW_SIZES = {
  wide: { width: 1440, height: 900 },
  laptop: { width: 1280, height: 800 },
  rail: { width: 640, height: 800 },
  narrow: { width: 320, height: 640 },
} as const;

/** How far the page scrolls sideways (0 or less: not at all). */
export function horizontalOverflow(page: Page): Promise<number> {
  return page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
}

/** Fails when the page scrolls sideways (rule 18: no horizontal scroll at any width of 320 px and up). */
export async function expectNoHorizontalScroll(page: Page, label = ''): Promise<void> {
  expect(await horizontalOverflow(page), `horizontal scroll${label ? ` (${label})` : ''}`).toBeLessThanOrEqual(0);
}

/** Fails when the element's right edge is beyond `width` (it would be cut off). */
export async function expectWithinWidth(page: Page, selector: string, width: number): Promise<void> {
  const box = await page.locator(selector).boundingBox();
  expect(box && box.x + box.width, `${selector} within ${width} px`).toBeLessThanOrEqual(width);
}

/** Resizes the viewport (a window resize); height defaults to 700. */
export async function resize(page: Page, width: number, height = 700): Promise<void> {
  await page.setViewportSize({ width, height });
}

/** What the check reads of one element (see {@link isSilentlyClipped}). */
export interface ClipProbe {
  readonly clientWidth: number;
  readonly scrollWidth: number;
  readonly hasOwnText: boolean;
  readonly overflowX: string;
  readonly textOverflow: string;
  readonly positionAbsolute: boolean;
}

/**
 * Whether an element cuts its own text without saying so (ER-21: text clipped without an ellipsis kept coming back): it holds
 * text, hides its overflow, is narrower than its text and has no `text-overflow: ellipsis`. Screen-reader-only text (absolute, one
 * pixel wide) and scrollable boxes are not clipped.
 */
export function isSilentlyClipped(p: ClipProbe): boolean {
  if (p.clientWidth === 0 || p.scrollWidth <= p.clientWidth + 1 || !p.hasOwnText) return false;
  if (p.overflowX === 'visible' || p.overflowX === 'auto' || p.overflowX === 'scroll') return false;
  if (p.positionAbsolute && p.clientWidth <= 1) return false;
  return p.textOverflow !== 'ellipsis';
}

/** Every element of the page that cuts its own text without an ellipsis, described for the failure message. */
export function textClippedWithoutEllipsis(page: Page): Promise<string[]> {
  return page.evaluate((decide) => {
    const silently = new Function(`return ${decide}`)() as (p: ClipProbe) => boolean;
    const out: string[] = [];
    for (const el of Array.from(document.querySelectorAll<HTMLElement>('body *'))) {
      const css = getComputedStyle(el);
      const probe: ClipProbe = {
        clientWidth: el.clientWidth,
        scrollWidth: el.scrollWidth,
        hasOwnText: Array.from(el.childNodes).some((n) => n.nodeType === Node.TEXT_NODE && (n.textContent ?? '').trim() !== ''),
        overflowX: css.overflowX,
        textOverflow: css.textOverflow,
        positionAbsolute: css.position === 'absolute',
      };
      if (!silently(probe)) continue;
      const name = `${el.tagName.toLowerCase()}.${String(el.className).trim().split(/\s+/).join('.')}`;
      out.push(`${name} "${(el.textContent ?? '').trim().slice(0, 40)}": text needs ${el.scrollWidth}px, has ${el.clientWidth}px, no ellipsis`);
    }
    return out;
  }, isSilentlyClipped.toString());
}

/** Fails when any element of the page cuts its own text without an ellipsis. */
export async function expectNoSilentClipping(page: Page, label = ''): Promise<void> {
  expect(await textClippedWithoutEllipsis(page), `text cut without an ellipsis${label ? ` (${label})` : ''}`).toEqual([]);
}
