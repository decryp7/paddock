import { expect, type Locator } from '@playwright/test';

/*
 * The "nothing covers an interactive element" check (ER-21: the same covering bug came back three times: grid cells over a
 * header, a timeline tick over chips, row tooltips over buttons). For every interactive element under `root` it asks the
 * browser what is stacked at the centre and four inset points of the part of the element that is really visible
 * (`document.elementsFromPoint`, topmost first) and reports the element when, at ANY of the points, a layer that is not
 * allowed sits above it: a 2 px tick through a chip or a tooltip over three quarters of a button is a cover. A point that
 * lands on the element, its content, its own label element or (a rounded corner) an ancestor reaches it.
 *
 * - Candidates: buttons, links, inputs, selects, textareas, `role` button/tab/menuitem/option/checkbox/switch/slider,
 *   other tab stops (`[tabindex>=0]`; a control or role element with a negative tabindex is still a candidate) and an
 *   optional extra selector.
 * - Each candidate is cut to the visible rect of its scroll (overflow) ancestors and the window; a candidate that is fully
 *   clipped is skipped. A cover is skipped when it is (or sits in) a sticky element of a scroller that has scrolled, the
 *   candidate scrolls in that scroller (is neither fixed nor itself sticky there) and the sticky element does not contain it:
 *   a designed header over scrolled rows.
 * - While probing, `*, *::before, *::after { pointer-events: auto !important }` is injected, so a cover that is painted but
 *   inert (a tooltip, a scrim, a pseudo-element with `pointer-events: none`) is caught too: the user sees the button under
 *   it covered. A layer that was inert before the forcing and paints nothing of its own (no background, border, shadow,
 *   outline, filter, text, replaced content or drawn pseudo-element) covers nothing and is skipped. The style is removed
 *   afterwards, also on a failure.
 * - `allow` names designed overlaps as [cover selector, target selector] pairs.
 * - The probe runs at rest and after scrolling each scroller of the root (and the page) by a third, two thirds and to the
 *   end (the pattern of `expectHeaderOnTop`), each scroller on its own; the scroll positions are restored. Running
 *   animations are awaited and the pointer is moved away before probing.
 * - `clip-path` is treated as a clip to the ancestor's border box (its shape is not modelled).
 * - Out of scope: elements inside shadow roots and iframes (`elementsFromPoint` of the page sees only their host).
 */

/** A designed overlap: a cover that matches the first selector over a target that matches the second. */
export type AllowedOverlap = readonly [cover: string, target: string];

/**
 * Covers that are part of a component's own painting and never a bug: Material's ripple, state layer, focus overlay and form-field outline draw over their
 * control and are inert on purpose. Such an overlay is allowed only over the target held by its owning control
 * ({@link OVERLAY_OWNERS}): Material button B painted over button A is still a cover.
 */
export const DESIGNED_OVERLAYS: readonly string[] = ['.mat-ripple', '.mat-mdc-button-persistent-ripple', '.mat-mdc-focus-indicator', '.mat-focus-indicator', '.mdc-notched-outline', '.mat-mdc-form-field-focus-overlay'];

/** The controls that own a designed overlay: the closest of these around the overlay must contain the target. */
export const OVERLAY_OWNERS: readonly string[] = ['button', 'a[href]', '[role="button"]', '[role="tab"]', '[role="option"]', '.mat-mdc-form-field', 'mat-form-field', '.mat-button-toggle'];

/** Options of {@link coveredInteractive} and {@link expectNothingCovers}. */
export interface HitTestOptions {
  /** More selectors to treat as interactive (comma separated), beside the built-in list. */
  readonly extra?: string;
  /** Designed overlaps as [cover selector, target selector] pairs (a TODO naming the finding when it is a known bug). */
  readonly allow?: readonly AllowedOverlap[];
  /** Also probe after scrolling each scroller of the root and the page. Default true; false probes at rest only. */
  readonly scroll?: boolean;
  /** Extra scrollers to scroll, as a selector inside the root (the scrollers that can scroll are found anyway). */
  readonly scrollers?: string;
}

/** One interactive element that something else sits on top of. */
export interface CoveredElement {
  /** The accessible-ish name: aria-label, text, title, value or placeholder, cut to 40 characters. */
  readonly name: string;
  /** A short selector for the element (tag, `#id`, `data-testid`, classes, role). */
  readonly selector: string;
  /** A short selector for the element found on top at the centre. */
  readonly coveredBy: string;
  /** When it was seen: `rest`, or the scroller and its position. */
  readonly at: string;
  /** The viewport point where the cover was found (`x,y`). */
  readonly point: string;
}

/** The selector list of the built-in candidates (exported for the unit tests and the docs). */
export const INTERACTIVE_SELECTOR = [
  'button',
  'a[href]',
  'input:not([type="hidden"])',
  'select',
  'textarea',
  '[role="button"]',
  '[role="tab"]',
  '[role="menuitem"]',
  '[role="menuitemcheckbox"]',
  '[role="menuitemradio"]',
  '[role="option"]',
  '[role="checkbox"]',
  '[role="switch"]',
  '[role="slider"]',
  '[role="radio"]',
  '[role="link"]',
  '[role="treeitem"]',
  '[tabindex]',
].join(', ');

/** Turns a list of findings into the message of a failed {@link expectNothingCovers}; one line a finding. */
export function formatCovered(found: readonly CoveredElement[]): string {
  if (found.length === 0) return 'nothing covers an interactive element';
  return [
    `${found.length} interactive element${found.length === 1 ? ' is' : 's are'} covered:`,
    ...found.map((f) => `  - ${f.name ? `"${f.name}" ` : ''}${f.selector} is covered by ${f.coveredBy} at ${f.point} (${f.at})`),
  ].join('\n');
}

/**
 * Finds interactive elements under `root` that another element sits on top of (see the file comment for the rules). Returns
 * the findings at rest and, unless `scroll` is false, after scrolling each scroller.
 */
export async function coveredInteractive(root: Locator, opts: HitTestOptions = {}): Promise<readonly CoveredElement[]> {
  const page = root.page();
  // Scrolling the root into view moves the page: remember where it was and put it back afterwards.
  const pageScroll = await page.evaluate(() => [window.scrollX, window.scrollY] as const);
  try {
    await root.first().scrollIntoViewIfNeeded();
    // The pointer rests where it is over no candidate, so no hover state (a tooltip, a row action) is open while probing.
    const rest = await root.first().evaluate((rootEl, sel) => {
      for (const fy of [0.97, 0.5, 0.03, 0.75, 0.25]) {
        for (const fx of [0.97, 0.03, 0.5, 0.75, 0.25]) {
          const x = Math.round(innerWidth * fx);
          const y = Math.round(innerHeight * fy);
          const hit = document.elementFromPoint(x, y);
          // Over no candidate and nothing that opens a tooltip or hover card.
          if (hit && !hit.closest(sel) && !hit.closest('[tabindex], [title], [aria-describedby], [mattooltip], .mat-mdc-tooltip-trigger')) return [x, y] as const;
        }
      }
      return [0, 0] as const;
    }, INTERACTIVE_SELECTOR);
    await page.mouse.move(rest[0], rest[1]);
    // One page function does all of it (the page's CSP forbids passing code in as text).
    return await root.first().evaluate(
      async (rootEl, { selector, extra, allow, overlays, owners, scroll, scrollers }) => {
        const style = document.createElement('style');
        style.setAttribute('data-pd-hit-test', '');
        style.textContent = '*, *::before, *::after { pointer-events: auto !important }';
        document.head.append(style);
        const frames = () => new Promise<void>((done) => requestAnimationFrame(() => requestAnimationFrame(() => done())));
        const settle = async () => {
          const running = document.getAnimations().filter((a) => Number.isFinite(a.effect?.getComputedTiming().endTime ?? Number.NaN));
          await Promise.race([Promise.all(running.map((a) => a.finished.catch(() => undefined))), new Promise((done) => setTimeout(done, 5000))]);
          await frames();
        };
        const jump = (e: Element, top: number, left: number) => e.scrollTo({ top, left, behavior: 'instant' });
        const seen = new Set<string>();
        const found: { name: string; selector: string; coveredBy: string; at: string; point: string }[] = [];
        const saved: { el: Element; top: number; left: number }[] = [];
        try {
          const describe = (e: Element) => {
            const id = e.id ? `#${e.id}` : '';
            const tid = e.getAttribute('data-testid');
            const raw = e.getAttribute('class')?.trim(); // an SVG element's className is not a string
            const cls = raw ? '.' + raw.split(/\s+/).slice(0, 3).join('.') : '';
            const role = e.getAttribute('role');
            return `${e.tagName.toLowerCase()}${id}${tid ? `[data-testid=${tid}]` : ''}${cls}${role ? `[role=${role}]` : ''}`;
          };
          const nameOf = (e: Element) => {
            const text = [e.getAttribute('aria-label'), e.getAttribute('title'), (e as HTMLInputElement).value, e.getAttribute('placeholder'), e.textContent].find((t) => t && t.trim()) ?? '';
            return String(text).replace(/\s+/g, ' ').trim().slice(0, 40);
          };
          // Built-in controls and the interactive roles are always candidates (a roving tabindex of -1 is still a control);
          // any other `[tabindex]` element (a landmark, a tabpanel) is one only as a tab stop (tabindex >= 0).
          const INTERACTIVE_ROLE = /^(option|tab|menuitem|menuitemcheckbox|menuitemradio|treeitem|gridcell|radio|checkbox|switch|button|link|slider)$/;
          const isCandidate = (e: Element) => {
            const t = e.getAttribute('tabindex');
            if (e.matches('button, a[href], input, select, textarea') || t === null) return true;
            if (INTERACTIVE_ROLE.test(e.getAttribute('role') ?? '')) return true;
            const n = Number.parseInt(t, 10);
            return Number.isFinite(n) && n >= 0;
          };
          const doc = document.scrollingElement ?? document.documentElement;
          const canScroll = (e: Element) =>
            (e.scrollHeight > e.clientHeight + 1 && /(auto|scroll)/.test(getComputedStyle(e).overflowY)) ||
            (e.scrollWidth > e.clientWidth + 1 && /(auto|scroll)/.test(getComputedStyle(e).overflowX));
          /** The part of the element that shows: cut by every clipping ancestor (until a fixed one) and the window. */
          const visible = (e: Element) => {
            const r = e.getBoundingClientRect();
            let [l, t, rt, b] = [r.left, r.top, r.right, r.bottom];
            // An absolutely positioned element is clipped only from its containing block up: the overflow of a
            // non-positioned ancestor in between does not reach it.
            let skipTo: Element | null = null;
            for (let cur: Element | null = e; cur; cur = cur.parentElement) {
              if (skipTo && cur !== skipTo) continue;
              skipTo = null;
              const cs = getComputedStyle(cur);
              if (cur !== e) {
                const c = cur.getBoundingClientRect();
                if (cs.overflowX !== 'visible') [l, rt] = [Math.max(l, c.left + cur.clientLeft), Math.min(rt, c.left + cur.clientLeft + cur.clientWidth)];
                if (cs.overflowY !== 'visible') [t, b] = [Math.max(t, c.top + cur.clientTop), Math.min(b, c.top + cur.clientTop + cur.clientHeight)];
                // `contain: paint` (also in `strict` and `content`) and `clip-path` clip like overflow: to the box (clip-path: its box).
                if (/paint|strict|content/.test(cs.contain) || cs.clipPath !== 'none') [l, t, rt, b] = [Math.max(l, c.left), Math.max(t, c.top), Math.min(rt, c.right), Math.min(b, c.bottom)];
              }
              if (cs.position === 'fixed') break;
              if (cs.position === 'absolute') skipTo = (cur as HTMLElement).offsetParent ?? document.documentElement;
            }
            [l, t, rt, b] = [Math.max(l, 0), Math.max(t, 0), Math.min(rt, innerWidth), Math.min(b, innerHeight)];
            return { l, t, w: rt - l, h: b - t };
          };
          /** Centre and four points inset from the corners; a rounded corner pushes its point inwards so it lands on the shape. */
          const samplePoints = (e: Element, v: { l: number; t: number; w: number; h: number }): [number, number][] => {
            const cs = getComputedStyle(e);
            const radius = (value: string) => (value.endsWith('px') ? Number.parseFloat(value) || 0 : 0);
            const inset = (r: number, size: number) => Math.min(Math.max(Math.min(4, size / 4), r * 0.3 + 1), size / 2 - 0.5);
            const [tl, tr, br, bl] = [cs.borderTopLeftRadius, cs.borderTopRightRadius, cs.borderBottomRightRadius, cs.borderBottomLeftRadius].map(radius) as [number, number, number, number];
            return [
              [v.l + v.w / 2, v.t + v.h / 2],
              [v.l + inset(tl, v.w), v.t + inset(tl, v.h)],
              [v.l + v.w - inset(tr, v.w), v.t + inset(tr, v.h)],
              [v.l + inset(bl, v.w), v.t + v.h - inset(bl, v.h)],
              [v.l + v.w - inset(br, v.w), v.t + v.h - inset(br, v.h)],
            ];
          };

          // --- what a layer above the target may be without being a cover
          const inertCache = new Map<Element, boolean>();
          /** Whether the element had `pointer-events: none` before the probe forced it on. */
          const wasInert = (el: Element) => {
            const hit = inertCache.get(el);
            if (hit !== undefined) return hit;
            const sheet = style.sheet;
            if (sheet) sheet.disabled = true;
            const inert = getComputedStyle(el).pointerEvents === 'none';
            if (sheet) sheet.disabled = false;
            inertCache.set(el, inert);
            return inert;
          };
          const clear = (c: string) => c === 'transparent' || c === '' || /,\s*0\)$/.test(c) || /\/\s*0(%|\.0+)?\s*\)$/.test(c);
          const REPLACED = /^(img|canvas|video|picture|object|embed|iframe|input|textarea|select|button|audio|meter|progress)$/;
          /** Whether the element paints anything of its own (background, border, shadow, outline, filters, text, replaced content, a drawn pseudo-element). */
          const paints = (el: Element) => {
            const cs = getComputedStyle(el);
            // An SVG shape the stack returned is painted fill or stroke (the pointer rule of `auto` is visiblePainted); the svg root itself paints nothing.
            if (REPLACED.test(el.tagName.toLowerCase()) || el instanceof SVGGeometryElement || el instanceof SVGTextContentElement || el instanceof SVGImageElement) return true;
            if (!clear(cs.backgroundColor) || cs.backgroundImage !== 'none') return true;
            if (['Top', 'Right', 'Bottom', 'Left'].some((side) => Number.parseFloat(cs.getPropertyValue(`border-${side.toLowerCase()}-width`)) > 0 && !['none', 'hidden'].includes(cs.getPropertyValue(`border-${side.toLowerCase()}-style`)))) return true;
            if (cs.boxShadow !== 'none' || cs.filter !== 'none' || (cs.backdropFilter && cs.backdropFilter !== 'none')) return true;
            if (Number.parseFloat(cs.outlineWidth) > 0 && !['none', 'hidden'].includes(cs.outlineStyle)) return true;
            if ([...el.childNodes].some((n) => n.nodeType === Node.TEXT_NODE && (n.textContent ?? '').trim() !== '')) return true;
            for (const pe of ['::before', '::after']) {
              const p = getComputedStyle(el, pe);
              if (p.content === 'none' || p.content === 'normal') continue;
              if (!['""', "''"].includes(p.content)) return true;
              if (!clear(p.backgroundColor) || p.backgroundImage !== 'none' || p.boxShadow !== 'none' || Number.parseFloat(p.borderTopWidth) > 0 || Number.parseFloat(p.borderLeftWidth) > 0) return true;
            }
            return false;
          };
          const scrollerOf = (s: Element) => {
            for (let a = s.parentElement; a; a = a.parentElement) {
              const cs = getComputedStyle(a);
              if (cs.overflowX !== 'visible' || cs.overflowY !== 'visible') return a === document.body || a === document.documentElement ? doc : a;
            }
            return doc;
          };
          /** Whether the target moves with the scroller P: not fixed, and not itself sticky inside it. */
          const scrollsIn = (e: Element, p: Element) => {
            for (let x: Element | null = e; x && x !== p; x = x.parentElement) {
              const pos = getComputedStyle(x).position;
              if (pos === 'fixed' || pos === 'sticky') return false;
            }
            return true;
          };
          /** A sticky element of a scroller the target scrolls in, which has scrolled: a header over scrolled rows (a target inside a sticky element does not scroll in it). */
          const stickyOverScrolled = (layer: Element, e: Element) => {
            for (let s: Element | null = layer; s && s !== document.documentElement; s = s.parentElement) {
              if (getComputedStyle(s).position !== 'sticky') continue; // (a sticky ancestor of the target is caught by scrollsIn)
              const p = scrollerOf(s);
              if (p !== doc && !p.contains(e)) continue;
              if (!(p.scrollTop > 0 || p.scrollLeft > 0)) continue;
              if (scrollsIn(e, p)) return true;
            }
            return false;
          };
          const overlay = overlays.join(', ');
          const ownerSel = owners.join(', ');
          const skippable = (layer: Element, e: Element) => {
            if (allow.some(([c, t]) => !!layer.closest(c) && !!e.closest(t))) return true;
            // Material's ripple and state layer cover only their own control: its owner must hold the target.
            if (overlay && layer.closest(overlay)) {
              const owner = layer.closest(ownerSel);
              if (owner && owner.contains(e)) return true;
            }
            if (stickyOverScrolled(layer, e)) return true;
            // An empty, unpainted layer that never took a click (a transparent inert host) covers nothing.
            return wasInert(layer) && (Number(getComputedStyle(layer).opacity) === 0 || !paints(layer));
          };
          /** What sits on top of the target at a point: the first layer above it that is not a designed or empty one, or null. */
          const coverAt = (x: number, y: number, e: Element): Element | null => {
            const labels = (e as HTMLInputElement).labels ? [...(e as HTMLInputElement).labels!] : [];
            const stack = document.elementsFromPoint(x, y);
            // An ancestor reaches the target (a rounded corner) only when the target is not further down this stack: when it is,
            // the ancestor (or its ::before/::after scrim) paints over it.
            const targetDown = stack.some((l) => l === e || e.contains(l));
            for (const layer of stack) {
              // The target, its content or its own label element: the point reaches it.
              if (layer === e || e.contains(layer) || labels.includes(layer as HTMLLabelElement)) return null;
              if (layer.contains(e) && !targetDown) return null;
              if (skippable(layer, e)) continue;
              return layer;
            }
            return null;
          };

          const probe = (at: string) => {
            inertCache.clear();
            const all = new Set<Element>([...rootEl.querySelectorAll(selector + (extra ? `, ${extra}` : '')), ...(rootEl.matches(selector) ? [rootEl] : [])]);
            let index = 0;
            for (const e of all) {
              index++;
              if (!isCandidate(e)) continue;
              const cs = getComputedStyle(e);
              if (cs.visibility === 'hidden' || cs.display === 'none' || Number(cs.opacity) === 0) continue;
              const r = e.getBoundingClientRect();
              if (r.width <= 1 || r.height <= 1) continue; // visually hidden (a 1 px native input) or empty
              const v = visible(e);
              if (v.w < 2 || v.h < 2) continue; // fully clipped by a scroller or the window
              // Clipping left only a sliver (under 6 px): its points land on the neighbour's pixel, so it is not probed. An element
              // that is just small on its own (a 5 px band, a 4 px splitter) is probed at its centre only.
              const sliver = v.w < 6 || v.h < 6;
              const clipped = v.w < r.width - 0.5 || v.h < r.height - 0.5;
              if (sliver && clipped) continue;
              // The centre or any other sample point under a cover that is not allowed fails the element.
              let cover: Element | null = null;
              let point = '';
              for (const [x, y] of sliver ? [[v.l + v.w / 2, v.t + v.h / 2] as [number, number]] : samplePoints(e, v)) {
                cover = coverAt(x, y, e);
                if (cover) {
                  point = `${Math.round(x)},${Math.round(y)}`;
                  break;
                }
              }
              if (!cover) continue;
              const f = { name: nameOf(e), selector: describe(e), coveredBy: describe(cover), at, point };
              const key = `${index}|${f.selector}|${f.coveredBy}`;
              if (seen.has(key)) continue;
              seen.add(key);
              found.push(f);
            }
          };

          await settle();
          probe('rest');
          if (scroll) {
            const list = new Set<Element>([...(canScroll(rootEl) ? [rootEl] : []), ...[...rootEl.querySelectorAll('*')].filter(canScroll)]);
            if (scrollers) rootEl.querySelectorAll(scrollers).forEach((s) => list.add(s));
            if (doc.scrollHeight > doc.clientHeight + 1 || doc.scrollWidth > doc.clientWidth + 1) list.add(doc);
            for (const s of list) {
              const home = { el: s, top: s.scrollTop, left: s.scrollLeft };
              saved.push(home);
              const maxTop = s.scrollHeight - s.clientHeight;
              const maxLeft = s.scrollWidth - s.clientWidth;
              for (const share of [1 / 3, 2 / 3, 1]) {
                jump(s, Math.round(maxTop * share), Math.round(maxLeft * share));
                await settle();
                probe(`${s === doc ? 'page' : describe(s)} scrolled to ${Math.round(s.scrollTop)}/${Math.round(s.scrollLeft)}`);
              }
              // Back to where it was before the next scroller is probed, so each is probed on its own.
              jump(s, home.top, home.left);
              await settle();
            }
          }
        } finally {
          for (const s of saved.reverse()) jump(s.el, s.top, s.left);
          style.remove();
        }
        return found;
      },
      {
        selector: INTERACTIVE_SELECTOR,
        extra: opts.extra ?? '',
        allow: (opts.allow ?? []).map((a) => [a[0], a[1]] as [string, string]),
        overlays: [...DESIGNED_OVERLAYS],
        owners: [...OVERLAY_OWNERS],
        scroll: opts.scroll ?? true,
        scrollers: opts.scrollers ?? '',
      },
    );
  } finally {
    await page.evaluate(([x, y]) => window.scrollTo({ left: x, top: y, behavior: 'instant' }), pageScroll);
  }
}

/**
 * Fails when something sits on top of an interactive element under `root` (see {@link coveredInteractive}), listing each
 * element, what covers it and where it was seen.
 *
 * ```ts
 * await expectNothingCovers(page.getByTestId('events-grid'));
 * await expectNothingCovers(page.locator('main'), { allow: [['.hover-card', '.row-action']] });
 * ```
 */
export async function expectNothingCovers(root: Locator, opts: HitTestOptions = {}): Promise<void> {
  const found = await coveredInteractive(root, opts);
  expect(found.length, formatCovered(found)).toBe(0);
}
