import { expect, type Locator } from '@playwright/test';

/*
 * The "nothing covers an interactive element" check (ER-21: the same covering bug came back three times: grid cells over a
 * header, a timeline tick over chips, row tooltips over buttons). For every interactive element under `root` it asks the
 * browser what is on top (`document.elementFromPoint`) at the centre and four inset points of the part of the element that
 * is really visible, and reports the element when none of the points reaches it or one of its descendants.
 *
 * - Candidates: buttons, links, inputs, selects, textareas, `role` button/tab/menuitem/option/checkbox/switch/slider,
 *   `[tabindex>=0]` and an optional extra selector.
 * - Each candidate is cut to the visible rect of its scroll (overflow) ancestors and the window; a candidate that is fully
 *   clipped is skipped. A cover that is a sticky element of a scroller that has been scrolled, and does not contain the
 *   candidate, is a designed header over scrolled rows and is skipped.
 * - While probing, `* { pointer-events: auto !important }` is injected, so a cover that is painted but inert (a tooltip, a
 *   scrim, a pseudo-element with `pointer-events: none`) is caught too: the user sees the button under it covered. The style
 *   is removed afterwards, also on a failure.
 * - `allow` names designed overlaps as [cover selector, target selector] pairs.
 * - The probe runs at rest and after scrolling each scroller of the root (and the page) by a third, two thirds and to the
 *   end (the pattern of `expectHeaderOnTop`); the scroll positions are restored.
 */

/** A designed overlap: a cover that matches the first selector over a target that matches the second. */
export type AllowedOverlap = readonly [cover: string, target: string];

/**
 * Covers that are part of a component's own painting and never a bug: Material's ripple, state layer, focus overlay and form-field outline draw over their
 * control and are inert on purpose. Matched as a cover over any target.
 */
export const DESIGNED_OVERLAYS: readonly string[] = ['.mat-ripple', '.mat-mdc-button-persistent-ripple', '.mat-mdc-focus-indicator', '.mat-focus-indicator', '.mdc-notched-outline', '.mat-mdc-form-field-focus-overlay'];

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
  '[tabindex]',
].join(', ');

/** Turns a list of findings into the message of a failed {@link expectNothingCovers}; one line a finding. */
export function formatCovered(found: readonly CoveredElement[]): string {
  if (found.length === 0) return 'nothing covers an interactive element';
  return [
    `${found.length} interactive element${found.length === 1 ? ' is' : 's are'} covered:`,
    ...found.map((f) => `  - ${f.name ? `"${f.name}" ` : ''}${f.selector} is covered by ${f.coveredBy} (${f.at})`),
  ].join('\n');
}

/**
 * Finds interactive elements under `root` that another element sits on top of (see the file comment for the rules). Returns
 * the findings, without duplicates, at rest and, unless `scroll` is false, after scrolling each scroller.
 */
export async function coveredInteractive(root: Locator, opts: HitTestOptions = {}): Promise<readonly CoveredElement[]> {
  await root.first().scrollIntoViewIfNeeded();
  // One page function does all of it (the page's CSP forbids passing code in as text).
  return root.first().evaluate(
    async (rootEl, { selector, extra, allow, overlays, scroll, scrollers }) => {
      const style = document.createElement('style');
      style.setAttribute('data-pd-hit-test', '');
      style.textContent = '* { pointer-events: auto !important }';
      document.head.append(style);
      const frames = () => new Promise<void>((done) => requestAnimationFrame(() => requestAnimationFrame(() => done())));
      const seen = new Set<string>();
      const found: { name: string; selector: string; coveredBy: string; at: string }[] = [];
      const saved: { el: Element; top: number; left: number }[] = [];
      try {
        const describe = (e: Element) => {
          const id = e.id ? `#${e.id}` : '';
          const tid = e.getAttribute('data-testid');
          const cls = typeof e.className === 'string' && e.className.trim() ? '.' + e.className.trim().split(/\s+/).slice(0, 3).join('.') : '';
          const role = e.getAttribute('role');
          return `${e.tagName.toLowerCase()}${id}${tid ? `[data-testid=${tid}]` : ''}${cls}${role ? `[role=${role}]` : ''}`;
        };
        const nameOf = (e: Element) => {
          const text = [e.getAttribute('aria-label'), e.getAttribute('title'), (e as HTMLInputElement).value, e.getAttribute('placeholder'), e.textContent].find((t) => t && t.trim()) ?? '';
          return String(text).replace(/\s+/g, ' ').trim().slice(0, 40);
        };
        const tabStop = (e: Element) => {
          const t = e.getAttribute('tabindex');
          if (t === null || e.matches('button, a[href], input, select, textarea, [role]')) return true;
          const n = Number.parseInt(t, 10);
          return Number.isFinite(n) && n >= 0;
        };
        const canScroll = (e: Element) =>
          (e.scrollHeight > e.clientHeight + 1 && /(auto|scroll)/.test(getComputedStyle(e).overflowY)) ||
          (e.scrollWidth > e.clientWidth + 1 && /(auto|scroll)/.test(getComputedStyle(e).overflowX));
        const scrolledAncestors = (e: Element) => {
          const out: Element[] = [];
          for (let a = e.parentElement; a; a = a.parentElement) if (canScroll(a) && (a.scrollTop > 0 || a.scrollLeft > 0)) out.push(a);
          const doc = document.scrollingElement;
          if (doc && (doc.scrollTop > 0 || doc.scrollLeft > 0)) out.push(doc);
          return out;
        };
        /** The part of the element that shows: cut by every clipping ancestor (until a fixed one) and the window. */
        const visible = (e: Element) => {
          const r = e.getBoundingClientRect();
          let [l, t, rt, b] = [r.left, r.top, r.right, r.bottom];
          for (let cur: Element | null = e; cur; cur = cur.parentElement) {
            const cs = getComputedStyle(cur);
            if (cur !== e && (cs.overflowX !== 'visible' || cs.overflowY !== 'visible')) {
              const c = cur.getBoundingClientRect();
              if (cs.overflowX !== 'visible') [l, rt] = [Math.max(l, c.left + cur.clientLeft), Math.min(rt, c.left + cur.clientLeft + cur.clientWidth)];
              if (cs.overflowY !== 'visible') [t, b] = [Math.max(t, c.top + cur.clientTop), Math.min(b, c.top + cur.clientTop + cur.clientHeight)];
            }
            if (cs.position === 'fixed') break;
          }
          [l, t, rt, b] = [Math.max(l, 0), Math.max(t, 0), Math.min(rt, innerWidth), Math.min(b, innerHeight)];
          return { l, t, w: rt - l, h: b - t };
        };
        const allowed = (cover: Element, target: Element) => [...allow, ...overlays].some(([c, tg]) => !!cover.closest(c) && !!target.closest(tg));
        /** A sticky element of a scroller that has scrolled, which does not contain the target: a designed header over scrolled rows. */
        const stickyOverScrolled = (cover: Element, target: Element) => {
          const scrolled = scrolledAncestors(target);
          if (!scrolled.length) return false;
          for (let s: Element | null = cover; s; s = s.parentElement) {
            if (getComputedStyle(s).position !== 'sticky') continue;
            if (!s.contains(target) && scrolled.some((a) => a.contains(s))) return true;
          }
          return false;
        };
        const reaches = (hit: Element | null, e: Element) => {
          if (!hit) return false;
          if (e.contains(hit)) return true;
          // The label of a control (a custom checkbox drawn on its label) is the control's own hit area.
          const labels = (e as HTMLInputElement).labels;
          return !!labels && [...labels].some((l) => l.contains(hit));
        };

        const probe = (at: string) => {
          const all = [...rootEl.querySelectorAll(selector + (extra ? `, ${extra}` : '')), ...(rootEl.matches(selector) ? [rootEl] : [])];
          for (const e of all) {
            if (!tabStop(e)) continue;
            const cs = getComputedStyle(e);
            if (cs.visibility === 'hidden' || cs.display === 'none' || Number(cs.opacity) === 0) continue;
            const r = e.getBoundingClientRect();
            if (r.width <= 1 || r.height <= 1) continue; // visually hidden (a 1 px native input) or empty
            const v = visible(e);
            if (v.w < 2 || v.h < 2) continue; // fully clipped by a scroller or the window
            const insetX = Math.min(4, v.w / 4);
            const insetY = Math.min(4, v.h / 4);
            const points: [number, number][] = [
              [v.l + v.w / 2, v.t + v.h / 2],
              [v.l + insetX, v.t + insetY],
              [v.l + v.w - insetX, v.t + insetY],
              [v.l + insetX, v.t + v.h - insetY],
              [v.l + v.w - insetX, v.t + v.h - insetY],
            ];
            let first: Element | null | undefined;
            let ok = false;
            for (const [x, y] of points) {
              const hit = document.elementFromPoint(x, y);
              first ??= hit;
              if (reaches(hit, e) || (hit && (allowed(hit, e) || stickyOverScrolled(hit, e)))) {
                ok = true;
                break;
              }
            }
            if (ok) continue;
            const f = { name: nameOf(e), selector: describe(e), coveredBy: first ? describe(first) : '(nothing: outside the window)', at };
            const key = `${f.selector}|${f.coveredBy}`;
            if (seen.has(key)) continue;
            seen.add(key);
            found.push(f);
          }
        };

        await frames();
        probe('rest');
        if (scroll) {
          const list = new Set<Element>([...(rootEl.matches('*') && canScroll(rootEl) ? [rootEl] : []), ...[...rootEl.querySelectorAll('*')].filter(canScroll)]);
          if (scrollers) rootEl.querySelectorAll(scrollers).forEach((s) => list.add(s));
          const doc = document.scrollingElement;
          if (doc && doc.scrollHeight > doc.clientHeight + 1) list.add(doc);
          for (const s of list) {
            saved.push({ el: s, top: s.scrollTop, left: s.scrollLeft });
            const maxTop = s.scrollHeight - s.clientHeight;
            const maxLeft = s.scrollWidth - s.clientWidth;
            for (const share of [1 / 3, 2 / 3, 1]) {
              s.scrollTop = Math.round(maxTop * share);
              s.scrollLeft = Math.round(maxLeft * share);
              await frames();
              probe(`${s === doc ? 'page' : describe(s)} scrolled to ${Math.round(s.scrollTop)}/${Math.round(s.scrollLeft)}`);
            }
          }
        }
      } finally {
        for (const s of saved.reverse()) {
          s.el.scrollTop = s.top;
          s.el.scrollLeft = s.left;
        }
        style.remove();
      }
      return found;
    },
    { selector: INTERACTIVE_SELECTOR, extra: opts.extra ?? '', overlays: DESIGNED_OVERLAYS.map((c) => [c, '*'] as [string, string]), allow: (opts.allow ?? []).map((a) => [a[0], a[1]] as [string, string]), scroll: opts.scroll ?? true, scrollers: opts.scrollers ?? '' },
  );
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
