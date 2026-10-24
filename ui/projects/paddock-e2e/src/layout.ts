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
