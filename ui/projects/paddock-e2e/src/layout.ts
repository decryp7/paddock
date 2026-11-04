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
  /** The element says its full text: a non-empty `data-full-text` (set by the app's full-text tip), title or aria-label. */
  readonly says: boolean;
  /** The text is in something that must never be cut: a control, a link, a tab, a number. */
  readonly never: boolean;
}

/** Text in these (the element or an ancestor) is never accepted with an ellipsis: a control's name, a tab, a link. */
export const ELLIPSIS_NEVER_WITHIN = 'button, a, [role="tab"], .mdc-button__label';
/** Text in these (the element itself) is never accepted with an ellipsis: a number cut short is a wrong number. */
export const ELLIPSIS_NEVER_SELF = '.num';

/**
 * Whether a truncation with `text-overflow: ellipsis` is an acceptable decision: the clipping element says its full text
 * (a non-empty `data-full-text`, title, aria-label or Material tooltip message; Material adds its tooltip class even for an empty
 * message, so that class alone counts for nothing) and it is not a control, a tab, a link or a number.
 */
export function isAcceptedEllipsis(p: { readonly textOverflow: string; readonly says: boolean; readonly never: boolean }): boolean {
  return p.textOverflow === 'ellipsis' && p.says && !p.never;
}

/**
 * Whether an element cuts its own text without saying so (ER-21: text clipped without an ellipsis kept coming back): it holds
 * text, hides its overflow, is narrower than its text and is not an {@link isAcceptedEllipsis accepted ellipsis}. Screen-reader-only
 * text (absolute, one pixel wide) and scrollable boxes are not clipped.
 */
export function isSilentlyClipped(p: ClipProbe): boolean {
  if (p.clientWidth === 0 || p.scrollWidth <= p.clientWidth + 1 || !p.hasOwnText) return false;
  if (p.overflowX === 'visible' || p.overflowX === 'auto' || p.overflowX === 'scroll') return false;
  if (p.positionAbsolute && p.clientWidth <= 1) return false;
  return !isAcceptedEllipsis(p);
}

/** One text of the page that sits beyond the box that clips it (or beyond the window): what the page reports, decided in Node. */
export interface TextClip {
  /** The text and the element it is in, for the failure message. */
  readonly description: string;
  /** The text ends beyond the window's right edge. */
  readonly pastWindowEdge: boolean;
  /** The text ends beyond the box that clips it. */
  readonly beyondClipper: boolean;
  readonly textOverflow: string;
  readonly says: boolean;
  readonly never: boolean;
}

/** The failure messages for the {@link TextClip}s that are not an accepted ellipsis. */
export function unacceptedTextClips(clips: readonly TextClip[]): string[] {
  const out: string[] = [];
  for (const c of clips) {
    if (isAcceptedEllipsis(c)) continue;
    if (c.pastWindowEdge) out.push(`text past the window edge: ${c.description}`);
    if (c.beyondClipper) out.push(`text cut by its box without a tooltipped ellipsis: ${c.description}`);
  }
  return out;
}

// The page-side code below cannot call the functions above (a page.evaluate callback is serialised on its own), so it only reads
// facts into plain data; every decision is made in Node by the pure functions above.

/** Every element of the page that cuts its own text without an accepted ellipsis, described for the failure message. */
export async function textClippedWithoutEllipsis(page: Page): Promise<string[]> {
  const found = await page.evaluate(
    ({ within, self }) => {
      const out: { description: string; probe: ClipProbe }[] = [];
      for (const el of Array.from(document.querySelectorAll<HTMLElement>('body *'))) {
        const css = getComputedStyle(el);
        const described = (el.getAttribute('aria-describedby') ?? '').split(/\s+/).some((id) => id !== '' && (document.getElementById(id)?.textContent ?? '').trim() !== '');
        const direct = (el.getAttribute('data-full-text') ?? el.getAttribute('title') ?? el.getAttribute('aria-label') ?? '').trim() !== '';
        const name = `${el.tagName.toLowerCase()}.${String(el.className).trim().split(/\s+/).join('.')}`;
        out.push({
          description: `${name} "${(el.textContent ?? '').trim().slice(0, 40)}": text needs ${el.scrollWidth}px, has ${el.clientWidth}px, no accepted ellipsis`,
          probe: {
            clientWidth: el.clientWidth,
            scrollWidth: el.scrollWidth,
            hasOwnText: Array.from(el.childNodes).some((n) => n.nodeType === Node.TEXT_NODE && (n.textContent ?? '').trim() !== ''),
            overflowX: css.overflowX,
            textOverflow: css.textOverflow,
            positionAbsolute: css.position === 'absolute',
            says: direct || described,
            never: el.closest(within) !== null || el.matches(self),
          },
        });
      }
      return out.filter((e) => e.probe.scrollWidth > e.probe.clientWidth + 1);
    },
    { within: ELLIPSIS_NEVER_WITHIN, self: ELLIPSIS_NEVER_SELF },
  );
  return found.filter((e) => isSilentlyClipped(e.probe)).map((e) => e.description);
}

/**
 * Every text of the page that is cut by the box that clips it, or ends past the window, unless that box is an accepted
 * ellipsis. Measures the text itself (a range), so it also finds a child that overflows a parent that hides it.
 */
export async function textCutOffWithoutTooltip(page: Page): Promise<string[]> {
  const clips = await page.evaluate(
    ({ within, self }) => {
      const vw = document.documentElement.clientWidth;
      const out: TextClip[] = [];
      const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
      for (let n = walker.nextNode(); n; n = walker.nextNode()) {
        const text = (n.textContent ?? '').trim();
        const parent = n.parentElement;
        if (!text || !parent) continue;
        const ps = getComputedStyle(parent);
        if (ps.visibility === 'hidden' || ps.display === 'none') continue;
        const range = document.createRange();
        range.selectNodeContents(n);
        const tr = range.getBoundingClientRect();
        if (tr.width === 0 || tr.height === 0) continue;
        let clip: Element | null = null;
        for (let a: Element | null = parent; a && a !== document.body; a = a.parentElement) {
          if (getComputedStyle(a).overflowX !== 'visible') {
            clip = a;
            break;
          }
        }
        const cr = clip?.getBoundingClientRect() ?? null;
        if (cr && (cr.width <= 2 || cr.height <= 2)) continue; // visually hidden text
        const pastWindowEdge = tr.right > vw + 1 && tr.left < vw;
        const beyondClipper = cr !== null && (tr.right > cr.right + 1 || tr.left < cr.left - 1);
        if (!pastWindowEdge && !beyondClipper) continue;
        let never = parent.closest(within) !== null;
        for (let e: Element | null = parent; e && e !== document.body; e = e.parentElement) {
          if (e.matches(self)) never = true;
          if (e === clip) break;
        }
        const said = clip
          ? (clip.getAttribute('aria-describedby') ?? '').split(/\s+/).some((id) => id !== '' && (document.getElementById(id)?.textContent ?? '').trim() !== '') ||
            (clip.getAttribute('data-full-text') ?? clip.getAttribute('title') ?? clip.getAttribute('aria-label') ?? '').trim() !== ''
          : false;
        out.push({
          description: `"${text.slice(0, 50)}" in ${parent.tagName.toLowerCase()}.${String(parent.className).trim().split(/\s+/).slice(0, 3).join('.')}`,
          pastWindowEdge,
          beyondClipper,
          textOverflow: clip ? getComputedStyle(clip).textOverflow : '',
          says: said,
          never,
        });
      }
      return out;
    },
    { within: ELLIPSIS_NEVER_WITHIN, self: ELLIPSIS_NEVER_SELF },
  );
  return [...new Set(unacceptedTextClips(clips))];
}

/** Fails when any element of the page cuts its own text without an ellipsis. */
export async function expectNoSilentClipping(page: Page, label = ''): Promise<void> {
  expect(await textClippedWithoutEllipsis(page), `text cut without an ellipsis${label ? ` (${label})` : ''}`).toEqual([]);
}

/** The number of text lines a label occupies, from the tops of its client rects (rects within 2 px of each other share a line). */
export function lineCount(tops: readonly number[]): number {
  const lines: number[] = [];
  for (const t of [...tops].sort((a, b) => a - b)) if (lines.length === 0 || t - lines[lines.length - 1]! > 2) lines.push(t);
  return lines.length;
}

/** The selector of everything that looks like a button: its label must stay on one line. A clickable card (`pd-card`) holds paragraphs by design and is not a label button. */
export const BUTTON_SELECTOR = 'button, [role="button"]:not(pd-card), a.pd-btn, a[matButton], a.mat-mdc-button-base';

/**
 * The drawn buttons with a text that wraps onto more than one line (a button grows to fit its label or the layout gives it room; it
 * never wraps it, and truncation is only for a declared full-text tooltip). Each text node is measured on its own, so a row button
 * that lays several texts side by side (a badge, a name, a time) is not a wrapped label, and neither is an icon beside the label.
 */
export async function buttonLabelsThatWrap(page: Page): Promise<string[]> {
  const found = await page.evaluate((selector) => {
    const out: { name: string; tops: number[][] }[] = [];
    for (const el of Array.from(document.querySelectorAll<HTMLElement>(selector))) {
      if (el.getClientRects().length === 0) continue;
      const perText: number[][] = [];
      const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
      for (let n = walker.nextNode(); n; n = walker.nextNode()) {
        if ((n.textContent ?? '').trim() === '') continue;
        const range = document.createRange();
        range.selectNodeContents(n);
        perText.push(Array.from(range.getClientRects()).filter((r) => r.width > 0).map((r) => Math.round(r.top * 10) / 10));
      }
      out.push({ name: `${el.tagName.toLowerCase()} "${(el.textContent ?? '').trim().slice(0, 40)}"`, tops: perText });
    }
    return out;
  }, BUTTON_SELECTOR);
  return found
    .map((b) => ({ name: b.name, lines: Math.max(0, ...b.tops.map((t) => lineCount(t))) }))
    .filter((b) => b.lines > 1)
    .map((b) => `${b.name}: ${b.lines} lines`);
}

/** Fails when any button label of the page wraps. */
export async function expectNoButtonLabelWraps(page: Page, label = ''): Promise<void> {
  expect(await buttonLabelsThatWrap(page), `button label wraps${label ? ` (${label})` : ''}`).toEqual([]);
}
