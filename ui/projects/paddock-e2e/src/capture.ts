import { mkdirSync } from 'node:fs';
import { dirname } from 'node:path';
import { expect, type Locator, type Page } from '@playwright/test';

/**
 * Screenshots a reviewer can trust (ER-21; docs/plans/shell-adoption-checklist.md): {@link captureScreen} refuses to
 * photograph a page that is still loading, is cut off mid-element, scrolled the frame instead of the page, or carries an
 * overlay that was not part of the app. {@link expectFullyVisible} and {@link expectPageFillsHost} are the layout
 * assertions behind it, usable on their own in a spec. Everything is polled from the test side, so it also works under
 * `page.clock.install()`.
 */

/** The shell's main scroller: the routed page scrolls here, never the frame. */
export const MAIN_SELECTOR = '[data-pd-region=main]';

/** Things that mean "still loading". */
export const BUSY_SELECTOR = 'pd-skeleton, .skeleton, [aria-busy=true]';

/** Parts that may legitimately float above the page (the shell's own hosts, dialogs, menus, tooltips). */
const OVERLAY_OK =
  'pd-dialog-host, pd-notification-host, pd-copy-menu, pd-title-bar, cdk-overlay-container, [role=dialog], [role=alertdialog], [role=menu], [role=listbox], [role=tooltip], [role=status], [role=alert], [data-pd-overlay]';

export interface CaptureOptions {
  /** Scroll container to use (default {@link MAIN_SELECTOR}, then the document). */
  readonly main?: string;
  /** Scroll the main scroller to this offset first (default 0). The frame is never scrolled. */
  readonly scrollTop?: number;
  /**
   * `'viewport'` (default): the window as it is; the bottom edge must not cut an element. `'main'`: the window grows by
   * exactly the main scroller's hidden height for the shot (and goes back), so the whole page is shown.
   */
  readonly fullPage?: 'viewport' | 'main';
  /** Elements that are allowed to be cut at the bottom edge or to float (rare; say why in the spec). */
  readonly allow?: string;
  /** Extra things that count as loading. */
  readonly busy?: string;
  /** Timeout of the settle poll in ms (default 10000). */
  readonly timeout?: number;
}

/** What one look at the page found. All lists are short human descriptions of the offending elements. */
export interface PageProbe {
  readonly busy: string[];
  readonly undecoded: string[];
  readonly animating: string[];
  readonly frameScrolled: boolean;
  readonly cutOff: string[];
  readonly overlays: string[];
  /** Layout signature of the visible content, to compare two looks. */
  readonly layout: string;
  readonly hiddenHeight: number;
}

/** Runs in the page. Plain function with serialisable arguments: it must not close over anything. */
export function probe(args: { main: string; busy: string; allow: string; overlayOk: string }): PageProbe {
  const doc = document;
  const root = (doc.querySelector(args.main) as HTMLElement | null) ?? (doc.scrollingElement as HTMLElement);
  const isMain = root !== doc.scrollingElement;
  const name = (e: Element): string => {
    const id = e.id ? `#${e.id}` : '';
    const cls = typeof e.className === 'string' && e.className ? `.${e.className.trim().split(/\s+/).slice(0, 2).join('.')}` : '';
    const text = (e.textContent ?? '').trim().replace(/\s+/g, ' ').slice(0, 30);
    return `<${e.tagName.toLowerCase()}${id}${cls}>${text ? ` "${text}"` : ''}`;
  };
  const visible = (e: Element): boolean => {
    const r = e.getBoundingClientRect();
    if (r.width === 0 || r.height === 0) return false;
    const s = getComputedStyle(e);
    return s.visibility !== 'hidden' && s.display !== 'none' && Number(s.opacity) > 0;
  };
  const allowed = (e: Element): boolean => !!args.allow && !!e.closest(args.allow);

  const busy = [...doc.querySelectorAll(`${args.busy}`)].filter(visible).map(name);
  const undecoded = [...doc.images]
    .filter(visible)
    .filter((i) => !(i.complete && i.naturalWidth > 0))
    .map(name);
  const animating = doc
    .getAnimations()
    .filter((a) => a.playState === 'running')
    .filter((a) => {
      const t = (a.effect as KeyframeEffect | null)?.target;
      return t instanceof Element && root.contains(t) && visible(t);
    })
    .map((a) => `${a.constructor.name} on ${name((a.effect as KeyframeEffect).target as Element)}`);

  const frameScrolled = (doc.scrollingElement?.scrollTop ?? 0) !== 0 || window.scrollY !== 0 || (isMain && doc.body.scrollTop !== 0);

  const rootRect = root.getBoundingClientRect();
  const edge = Math.min(window.innerHeight, isMain ? rootRect.bottom : window.innerHeight);
  const cutOff: string[] = [];
  const content = root.querySelectorAll('*');
  for (const e of content) {
    if (!visible(e) || allowed(e)) continue;
    const r = e.getBoundingClientRect();
    if (!(r.top < edge - 1 && r.bottom > edge + 1)) continue;
    const media = /^(IMG|SVG|CANVAS|VIDEO|BUTTON|INPUT|SELECT|TEXTAREA)$/i.test(e.tagName);
    const ownText = [...e.childNodes].some((n) => n.nodeType === Node.TEXT_NODE && (n.textContent ?? '').trim() !== '');
    const row = /^(TR|LI)$/i.test(e.tagName) || e.getAttribute('role') === 'row' || e.getAttribute('role') === 'listitem';
    // A box taller than half the window is a container the page legitimately continues in; the things inside it are checked.
    if ((media || ownText || row) && r.height <= window.innerHeight / 2) cutOff.push(name(e));
  }

  const overlays: string[] = [];
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  for (const e of doc.body.querySelectorAll('*')) {
    const s = getComputedStyle(e);
    if (s.position !== 'fixed' && s.position !== 'absolute') continue;
    if (!visible(e) || e.closest(args.overlayOk) || allowed(e)) continue;
    // Only things that paint opaque over the app: a doctored cover hides cut-off content. Transparent hit-areas are fine.
    const bg = /rgba?\(([^)]+)\)/.exec(s.backgroundColor)?.[1]?.split(',').map((x) => Number.parseFloat(x));
    const opaque = !!bg && (bg.length < 4 || (bg[3] ?? 1) >= 0.9) && s.backgroundColor !== 'rgba(0, 0, 0, 0)';
    const gradient = s.backgroundImage.includes('gradient');
    if (!opaque && !gradient) continue;
    const r = e.getBoundingClientRect();
    const w = Math.max(0, Math.min(r.right, vw) - Math.max(r.left, 0));
    const h = Math.max(0, Math.min(r.bottom, vh) - Math.max(r.top, 0));
    const wide = w >= vw * 0.25;
    const touchesEdge = r.bottom >= vh - 2 || r.top <= 2;
    // Real app chrome (a sticky footer bar with controls) holds controls or is a landmark; a cover is empty.
    const chrome = !!e.querySelector('button, a, input, select, textarea, [role=button]') || /^(HEADER|FOOTER|NAV|ASIDE|MAIN)$/.test(e.tagName);
    if (wide && h >= 8 && (touchesEdge || (w * h) / (vw * vh) >= 0.2) && !chrome) overlays.push(`${name(e)} ${Math.round(w)}x${Math.round(h)}`);
  }

  const layout = [...root.querySelectorAll('h1, h2, h3, button, a, img, tr, li, [data-pd-region], [data-testid]')]
    .filter(visible)
    .slice(0, 400)
    .map((e) => {
      const r = e.getBoundingClientRect();
      return `${e.tagName}:${Math.round(r.x)},${Math.round(r.y)},${Math.round(r.width)}x${Math.round(r.height)}`;
    })
    .join('|');
  return { busy, undecoded, animating, frameScrolled, cutOff, overlays, layout, hiddenHeight: Math.max(0, root.scrollHeight - root.clientHeight) };
}

/** Why a look is not a screenshot yet (empty: it is). Pure, so it is unit-tested without a browser. */
export function problemsOf(p: PageProbe, fullPage: 'viewport' | 'main'): string[] {
  const out: string[] = [];
  if (p.busy.length) out.push(`still loading: ${p.busy.join(', ')}`);
  if (p.undecoded.length) out.push(`images not decoded: ${p.undecoded.join(', ')}`);
  if (p.animating.length) out.push(`running animations or transitions in main: ${p.animating.join(', ')}`);
  if (p.frameScrolled) out.push('the frame was scrolled; only the page’s main scroller may scroll');
  if (fullPage === 'viewport' && p.cutOff.length) out.push(`cut off mid-element at the bottom edge: ${p.cutOff.join(', ')}`);
  if (p.overlays.length) out.push(`an opaque overlay covers the app (screenshots are never doctored): ${p.overlays.join(', ')}`);
  return out;
}

async function look(page: Page, options: CaptureOptions): Promise<PageProbe> {
  await page.evaluate(() => document.fonts.ready.then(() => undefined));
  const busy = [BUSY_SELECTOR, options.busy].filter(Boolean).join(', ');
  return page.evaluate(probe, { main: options.main ?? MAIN_SELECTOR, busy, allow: options.allow ?? '', overlayOk: OVERLAY_OK });
}

/**
 * Waits until the page is settled: nothing loading, every image decoded, fonts ready, no animation in main, and the layout
 * the same in two looks in a row. Fails with the reasons when it never is (or when the frame itself scrolled, or an overlay
 * covers the app: those never fix themselves).
 */
export async function waitForCapturable(page: Page, options: CaptureOptions = {}): Promise<PageProbe> {
  const fullPage = options.fullPage ?? 'viewport';
  let last = '';
  let latest: PageProbe | undefined;
  await expect
    .poll(
      async () => {
        latest = await look(page, options);
        const problems = problemsOf(latest, fullPage);
        const stable = latest.layout === last;
        last = latest.layout;
        return problems.length === 0 && stable ? 'ready' : [...problems, stable ? '' : 'layout still moving'].filter(Boolean).join('; ');
      },
      { message: 'the page is not ready to be photographed', timeout: options.timeout ?? 10_000 },
    )
    .toBe('ready');
  return latest as PageProbe;
}

/**
 * The one way to take a screenshot of an app page: scrolls only the main scroller to `scrollTop`, waits until
 * {@link waitForCapturable}, and writes the PNG to `path` (its folder is made). It cannot be told to hide anything.
 */
export async function captureScreen(page: Page, path: string, options: CaptureOptions = {}): Promise<void> {
  mkdirSync(dirname(path), { recursive: true });
  await page.evaluate(
    ({ main, top }) => {
      const el = document.querySelector(main) as HTMLElement | null;
      if (el) el.scrollTop = top;
      else document.scrollingElement?.scrollTo(0, top);
    },
    { main: options.main ?? MAIN_SELECTOR, top: options.scrollTop ?? 0 },
  );
  let grown = 0;
  const original = page.viewportSize();
  try {
    if (options.fullPage === 'main') {
      grown = (await look(page, options)).hiddenHeight;
      if (grown > 0 && original) await page.setViewportSize({ width: original.width, height: original.height + grown });
    }
    await waitForCapturable(page, options);
    await page.screenshot({ path });
  } finally {
    if (grown > 0 && original) await page.setViewportSize(original);
  }
}

/** Where a locator's element is clipped or covered; empty means fully visible. Runs in the page. */
export function visibilityProblems(el: Element): string[] {
  const out: string[] = [];
  const r = el.getBoundingClientRect();
  if (r.width === 0 || r.height === 0) return ['it has no size'];
  const eps = 0.5;
  if (r.left < -eps || r.top < -eps || r.right > window.innerWidth + eps || r.bottom > window.innerHeight + eps) out.push('it is outside the window');
  for (let a = el.parentElement; a; a = a.parentElement) {
    const s = getComputedStyle(a);
    const clips = [s.overflowX, s.overflowY, s.overflow].some((o) => o !== 'visible');
    if (!clips) continue;
    const c = a.getBoundingClientRect();
    const x = s.overflowX !== 'visible';
    const y = s.overflowY !== 'visible';
    if ((x && (r.left < c.left - eps || r.right > c.right + eps)) || (y && (r.top < c.top - eps || r.bottom > c.bottom + eps))) {
      out.push(`it is clipped by ${a.tagName.toLowerCase()}${a.id ? `#${a.id}` : ''}${typeof a.className === 'string' && a.className ? `.${a.className.trim().split(/\s+/)[0]}` : ''}`);
      break;
    }
  }
  const hit = document.elementFromPoint(Math.min(Math.max(r.left + r.width / 2, 0), window.innerWidth - 1), Math.min(Math.max(r.top + r.height / 2, 0), window.innerHeight - 1));
  if (out.length === 0 && hit && !el.contains(hit)) out.push(`it is covered by <${hit.tagName.toLowerCase()}>`);
  return out;
}

/**
 * The element is wholly in the window, not cut by the overflow of any ancestor and not covered: use it for primary
 * actions (a dialog's OK, a wizard's Next). `toBeVisible` is not enough, it is true for a clipped button.
 */
export async function expectFullyVisible(locator: Locator, label = ''): Promise<void> {
  await locator.waitFor({ state: 'attached' });
  const problems = await locator.evaluate(visibilityProblems);
  expect(problems, `${label || 'the element'} is fully visible`).toEqual([]);
}

export interface FillsHostOptions {
  /** The host (default {@link MAIN_SELECTOR}). */
  readonly host?: string;
  /** The routed page inside the host (default: the host's first element child, or its `[data-page]` descendant). */
  readonly page?: string;
}

/** Runs in the page: what the host offers and what the page takes, plus every virtual viewport and its height. */
export function fillMeasure(args: { host: string; page: string }): { host: number; page: number; viewports: { name: string; height: number }[] } | null {
  const host = document.querySelector(args.host) as HTMLElement | null;
  if (!host) return null;
  const page = (args.page ? host.querySelector(args.page) : (host.querySelector('[data-page]') ?? host.firstElementChild)) as HTMLElement | null;
  const viewports = [...host.querySelectorAll('cdk-virtual-scroll-viewport')].map((v) => ({ name: v.id || v.className || 'cdk-virtual-scroll-viewport', height: v.getBoundingClientRect().height }));
  return { host: host.clientHeight, page: page ? page.getBoundingClientRect().height : 0, viewports };
}

/**
 * The page under the shell host takes at least the host’s available height, and no CDK virtual viewport has collapsed to 0.
 * Catches `height: 100%` in a page whose parent had no definite height.
 */
export async function expectPageFillsHost(page: Page, options: FillsHostOptions = {}): Promise<void> {
  const m = await page.evaluate(fillMeasure, { host: options.host ?? MAIN_SELECTOR, page: options.page ?? '' });
  expect(m, `the host ${options.host ?? MAIN_SELECTOR} exists`).not.toBeNull();
  if (!m) return;
  expect(m.page, `the page (${Math.round(m.page)} px) fills the host (${Math.round(m.host)} px)`).toBeGreaterThanOrEqual(m.host - 1);
  expect(
    m.viewports.filter((v) => v.height < 1),
    'no virtual-scroll viewport has collapsed to 0 height',
  ).toEqual([]);
}
