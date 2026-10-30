import { mkdirSync } from 'node:fs';
import { dirname } from 'node:path';
import { expect, type Locator, type Page } from '@playwright/test';

/**
 * Screenshots a reviewer can trust (ER-21; docs/plans/shell-adoption-checklist.md): {@link captureScreen} refuses to
 * photograph a page that is still loading, is cut off mid-element, scrolled the frame instead of the page, or has an
 * opaque, empty cover over the app (it cannot be told to hide anything, but a determined author can still fool a
 * heuristic: the independent review of every image, ER-23, stays). {@link expectFullyVisible} and {@link expectPageFillsHost} are the layout
 * assertions behind it, usable on their own in a spec. Everything is polled from the test side, so it also works under
 * `page.clock.install()`.
 */

/** The shell's main scroller: the routed page scrolls here, never the frame. */
export const MAIN_SELECTOR = '[data-pd-region=main]';

/** Things that mean "still loading". */
export const BUSY_SELECTOR = 'pd-skeleton, .skeleton, [aria-busy=true]';

/** Parts that may legitimately float above the page (the shell's own hosts, dialogs, menus, tooltips). */
const OVERLAY_OK =
  'pd-dialog-host, pd-notification-host, pd-copy-menu, pd-title-bar, cdk-overlay-container, [role=dialog], [role=alertdialog], [role=menu], [role=listbox], [role=tooltip], [data-pd-overlay]';

/** The tallest window a whole-page shot may grow to (px). */
export const FRAME_MAX_HEIGHT = 4000;

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
  /** Elements that may be cut at the bottom edge (rare; say why in the spec). Never applies to covers. */
  readonly allow?: string;
  /**
   * Elements whose animation is ignored: an indeterminate spinner that IS the thing the image documents ("Still working").
   * Prefer `reducedMotion: 'reduce'` (`page.emulateMedia`), which the shell honours and which stops the spinner for good.
   */
  readonly animated?: string;
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
export function probe(args: { main: string; busy: string; allow: string; animated: string; overlayOk: string }): PageProbe {
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
  const vh = window.innerHeight;
  const vw = window.innerWidth;
  const undecoded = [...doc.images]
    .filter(visible)
    .filter((i) => {
      // A lazy image far below the window is not loaded and not in the shot; an SVG without an intrinsic size has width 0.
      if (i.loading === 'lazy' && i.getBoundingClientRect().top > vh + 200) return false;
      const svg = /^data:image\/svg|\.svg(?:[?#]|$)/i.test(i.currentSrc || i.src);
      return !(i.complete && (i.naturalWidth > 0 || svg));
    })
    .map(name);
  const animating = doc
    .getAnimations()
    .filter((a) => a.playState === 'running')
    .filter((a) => {
      const t = (a.effect as KeyframeEffect | null)?.target;
      return t instanceof Element && root.contains(t) && visible(t) && !(args.animated && t.closest(args.animated));
    })
    .map((a) => `${a.constructor.name} on ${name((a.effect as KeyframeEffect).target as Element)}`);

  const frameScrolled = (doc.scrollingElement?.scrollTop ?? 0) !== 0 || window.scrollY !== 0 || (isMain && doc.body.scrollTop !== 0);

  // The element's nearest scrolling ancestor below the root: rows cut by an inner scroller's edge (a virtual list) are its
  // business, not a cut of the shot.
  const innerScroller = (e: Element): boolean => {
    for (let a = e.parentElement; a && a !== root; a = a.parentElement) {
      const s = getComputedStyle(a);
      // Only a real scroller: overflow auto or scroll that has something to scroll. A card with overflow hidden or clip
      // cut at the shot's edge is cut content.
      if (/auto|scroll/.test(s.overflowY) && a.scrollHeight > a.clientHeight + 1) return true;
    }
    return false;
  };
  const rootRect = root.getBoundingClientRect();
  const edge = Math.min(vh, isMain ? rootRect.bottom : vh);
  const cutOff: string[] = [];
  for (const e of root.querySelectorAll('*')) {
    if (!visible(e) || allowed(e) || innerScroller(e)) continue;
    const r = e.getBoundingClientRect();
    if (!(r.top < edge - 1 && r.bottom > edge + 1)) continue;
    const media = /^(IMG|SVG|CANVAS|VIDEO|BUTTON|INPUT|SELECT|TEXTAREA)$/i.test(e.tagName);
    const ownText = [...e.childNodes].some((n) => n.nodeType === Node.TEXT_NODE && (n.textContent ?? '').trim() !== '');
    const row = /^(TR|LI)$/i.test(e.tagName) || e.getAttribute('role') === 'row' || e.getAttribute('role') === 'listitem';
    // A box taller than half the window is a container the page legitimately continues in; the things inside it are checked.
    if ((media || ownText || row) && r.height <= vh / 2) cutOff.push(name(e));
  }

  // Covers: an opaque, empty box (no text, no control) over the app. Any CSS colour syntax counts: the canvas parses it.
  const canvas = doc.createElement('canvas');
  canvas.width = canvas.height = 1;
  const g = canvas.getContext('2d', { willReadFrequently: true });
  const alphaOf = (color: string): number => {
    if (!g || !color || color === 'transparent') return 0;
    g.clearRect(0, 0, 1, 1);
    g.fillStyle = '#000';
    g.fillStyle = color;
    g.fillRect(0, 0, 1, 1);
    return g.getImageData(0, 0, 1, 1).data[3]! / 255;
  };
  const overlays: string[] = [];
  const consider = (e: Element, s: CSSStyleDeclaration, label: string, r: DOMRect, pseudo: boolean): void => {
    const insideRoot = root.contains(e) && e !== root;
    // Inside main, absolute is ordinary layout (split panes, badges); only fixed and sticky leave the flow.
    if (!(s.position === 'fixed' || s.position === 'sticky' || (s.position === 'absolute' && !insideRoot))) return;
    if (e.closest(args.overlayOk)) return;
    if (!(alphaOf(s.backgroundColor) >= 0.8 || /gradient/.test(s.backgroundImage))) return;
    const w = Math.max(0, Math.min(r.right, vw) - Math.max(r.left, 0));
    const h = Math.max(0, Math.min(r.bottom, vh) - Math.max(r.top, 0));
    const touchesEdge = r.bottom >= vh - 2 || r.top <= 2 || (insideRoot && r.bottom >= rootRect.bottom - 2);
    let hasText = false;
    if (!pseudo) {
      const walk = doc.createTreeWalker(e, NodeFilter.SHOW_TEXT);
      for (let n = walk.nextNode(); n && !hasText; n = walk.nextNode()) hasText = (n.textContent ?? '').trim() !== '' && !!n.parentElement && visible(n.parentElement);
    }
    const hasControl = !pseudo && [...e.querySelectorAll('button, a, input, select, textarea, [role=button]')].some(visible);
    if (w >= vw * 0.25 && h >= 8 && (touchesEdge || (w * h) / (vw * vh) >= 0.2) && !hasText && !hasControl && !/^(HEADER|FOOTER|NAV|ASIDE|MAIN)$/.test(e.tagName)) overlays.push(`${label} ${Math.round(w)}x${Math.round(h)}`);
  };
  for (const e of [doc.documentElement, doc.body, ...doc.body.querySelectorAll('*')]) {
    if (e !== doc.documentElement && visible(e)) consider(e, getComputedStyle(e), name(e), e.getBoundingClientRect(), false);
    for (const pseudo of ['::before', '::after'] as const) {
      const ps = getComputedStyle(e, pseudo);
      if (ps.content === 'none' || ps.content === 'normal' || ps.display === 'none') continue;
      const w = Number.parseFloat(ps.width);
      const h = Number.parseFloat(ps.height);
      if (!(w > 0 && h > 0)) continue;
      // The pseudo-element's box: fixed ones sit against the window, others against their element.
      const r = ps.position === 'fixed' ? new DOMRect(Number.parseFloat(ps.left) || 0, ps.bottom !== 'auto' ? vh - Number.parseFloat(ps.bottom) - h : Number.parseFloat(ps.top) || 0, w, h) : e.getBoundingClientRect();
      consider(e, ps, `${name(e)}${pseudo}`, r, true);
    }
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
  const p = await page.evaluate(probe, { main: options.main ?? MAIN_SELECTOR, busy, allow: options.allow ?? '', animated: options.animated ?? '', overlayOk: OVERLAY_OK });
  // An SVG without an intrinsic size has naturalWidth 0 like a broken one; only decode() tells them apart.
  const broken = await page.evaluate(async () => {
    const out: string[] = [];
    for (const i of document.images) {
      if (i.complete && i.naturalWidth === 0 && i.getBoundingClientRect().width > 0 && /^data:image\/svg|\.svg(?:[?#]|$)/i.test(i.currentSrc || i.src)) {
        try {
          await i.decode();
        } catch {
          out.push(`<img${i.id ? `#${i.id}` : ''}> (${(i.currentSrc || i.src).slice(0, 40)})`);
        }
      }
    }
    return out;
  });
  return broken.length ? { ...p, undecoded: [...p.undecoded, ...broken] } : p;
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
export async function captureScreen(page: Page, path: string, options: CaptureOptions = {}): Promise<Buffer> {
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
  let mode: 'viewport' | 'main' = 'viewport';
  const original = page.viewportSize();
  try {
    if (options.fullPage === 'main' && original) {
      const hidden = (await look(page, options)).hiddenHeight;
      // Never taller than FRAME_MAX_HEIGHT: a longer page keeps the cut-off check at the new bottom edge.
      grown = Math.max(0, Math.min(hidden, FRAME_MAX_HEIGHT - original.height));
      mode = grown >= hidden ? 'main' : 'viewport';
      if (grown > 0) await page.setViewportSize({ width: original.width, height: original.height + grown });
    }
    await waitForCapturable(page, { ...options, fullPage: mode });
    return await page.screenshot({ path });
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
  if (out.length > 0) return out;
  // Centre and the four corners. Hit tests respect border-radius, so a corner point 1 px inside the box of a rounded button
  // lands on the parent: each corner is inset by r * (1 - 1/sqrt 2) + 1 along both axes, r being that corner's radius
  // (capped at half the box). Note elementFromPoint skips pointer-events:none, so such a cover is not seen here.
  const at = (x: number, y: number) => document.elementFromPoint(Math.min(Math.max(x, 0), window.innerWidth - 1), Math.min(Math.max(y, 0), window.innerHeight - 1));
  const cs = getComputedStyle(el);
  const radius = (v: string): number => {
    const first = v.split(' ')[0] ?? '0';
    const n = Number.parseFloat(first);
    if (!Number.isFinite(n)) return 0;
    return Math.min(first.endsWith('%') ? (n / 100) * r.width : n, r.width / 2, r.height / 2);
  };
  const k = 1 - 1 / Math.SQRT2;
  const tl = radius(cs.borderTopLeftRadius) * k + 1;
  const tr = radius(cs.borderTopRightRadius) * k + 1;
  const bl = radius(cs.borderBottomLeftRadius) * k + 1;
  const br = radius(cs.borderBottomRightRadius) * k + 1;
  const points: [number, number][] = [[r.left + r.width / 2, r.top + r.height / 2], [r.left + tl, r.top + tl], [r.right - tr, r.top + tr], [r.left + bl, r.bottom - bl], [r.right - br, r.bottom - br]];
  for (const [x, y] of points) {
    const hit = at(x, y);
    if (hit && !el.contains(hit)) {
      out.push(`it is covered by <${hit.tagName.toLowerCase()}>`);
      break;
    }
  }
  return out;
}

/**
 * The element is wholly in the window, not cut by the overflow of any ancestor and not covered: use it for primary
 * actions (a dialog's OK, a wizard's Next). `toBeVisible` is not enough, it is true for a clipped button.
 */
export async function expectFullyVisible(locator: Locator, label = '', timeout = 5000): Promise<void> {
  await locator.waitFor({ state: 'attached', timeout });
  // Polled: a dialog that is still sliding in is measured again until it holds still, not failed on its first frame.
  await expect
    .poll(async () => (await locator.evaluate(visibilityProblems)).join('; '), { message: `${label || 'the element'} is fully visible`, timeout })
    .toBe('');
}

export interface FillsHostOptions {
  /** The host (default {@link MAIN_SELECTOR}). */
  readonly host?: string;
  /**
   * The routed page component inside the host (default: the first element child of the shell's loaded page box
   * `pd-part-outlet.pd-page`, else a `[data-page]` descendant, else the host's first child). Name your page's own selector
   * (`app-logs-page`) when the default is not it.
   */
  readonly page?: string;
  /**
   * `true` (default): the page component takes at least the host's height (a list, table or split page with `height: 100%`).
   * `false` for a short page that is meant to be content-high: only the collapsed-viewport check runs.
   */
  readonly fill?: boolean;
  readonly timeout?: number;
}

/** Runs in the page: what the host offers and what the page takes, plus every rendered virtual viewport and its height. */
export function fillMeasure(args: { host: string; page: string }): { host: number; page: number; viewports: { name: string; height: number }[] } | null {
  const host = document.querySelector(args.host) as HTMLElement | null;
  if (!host) return null;
  const outlet = host.querySelector('pd-part-outlet.pd-page');
  const real = outlet ? [...outlet.children].find((c) => !c.classList.contains('pd-loading') && c.tagName !== 'PD-ERROR-STATE') : null;
  const page = (args.page ? host.querySelector(args.page) : (real ?? host.querySelector('[data-page]') ?? host.firstElementChild)) as HTMLElement | null;
  // A viewport in an inactive tab or a closed panel is not rendered (no boxes): it has nothing to lose.
  const viewports = [...host.querySelectorAll('cdk-virtual-scroll-viewport')]
    .filter((v) => v.getClientRects().length > 0 && getComputedStyle(v).visibility !== 'hidden')
    .map((v) => ({ name: v.id || v.className || 'cdk-virtual-scroll-viewport', height: v.getBoundingClientRect().height }));
  return { host: host.clientHeight, page: page ? page.getBoundingClientRect().height : 0, viewports };
}

/**
 * The page under the shell host takes at least the host’s available height, and no rendered CDK virtual viewport has
 * collapsed to 0. Catches `height: 100%` in a page whose parent had no definite height. Polled, like a layout that is
 * still settling. Note that a page with `height: 100%` and `overflow: hidden` is now clipped to the host, not scrolled.
 */
export async function expectPageFillsHost(page: Page, options: FillsHostOptions = {}): Promise<void> {
  const host = options.host ?? MAIN_SELECTOR;
  await expect
    .poll(
      async () => {
        const m = await page.evaluate(fillMeasure, { host, page: options.page ?? '' });
        if (!m) return `the host ${host} does not exist`;
        const problems: string[] = [];
        if (options.fill !== false && m.page < m.host - 1) problems.push(`the page (${Math.round(m.page)} px) does not fill the host (${Math.round(m.host)} px)`);
        const collapsed = m.viewports.filter((v) => v.height < 1);
        if (collapsed.length) problems.push(`a virtual-scroll viewport has collapsed to 0 height: ${collapsed.map((v) => v.name).join(', ')}`);
        return problems.join('; ');
      },
      { message: 'the page fills the host', timeout: options.timeout ?? 5000 },
    )
    .toBe('');
}
