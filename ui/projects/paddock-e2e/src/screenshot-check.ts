import { expect, type Page } from '@playwright/test';
import { textClippedWithoutEllipsis, textCutOffWithoutTooltip } from './layout.ts';
import { captureStable, type StableFrameOptions } from './screenshot.ts';

/**
 * The check an app's Playwright spec calls before (and after) it takes a screenshot, so a picture never records a defect
 * the page could have told the test about (ER-23: a screenshot is reviewed by someone else, but the obvious faults are
 * found by the machine first). It fails on:
 * - text cut by its box or past the window, unless the box is an accepted ellipsis (`layout.ts`: it says its full text and is not a
 *   control, tab, link or number);
 * - raw identifiers in visible text: `SNAKE_CASE` and `camelCase` tokens, `undefined`, `null`, `NaN`, `[object Object]`;
 * - mock, lorem ipsum, TODO and debug strings;
 * - a status chip (`pd-status-pill`, `.pd-pill`, `[data-status-chip]`) without a word or an icon (status is never colour alone).
 * Every decision is made in Node by the pure functions below; the page only reports facts.
 */

/** What the page reports of one visible text. */
export interface VisibleText {
  readonly text: string;
  /** Where it is, for the failure message (`tag.class`). */
  readonly where: string;
}

/** What the page reports of one status chip. */
export interface ChipProbe {
  readonly description: string;
  /** The chip's visible text, trimmed. */
  readonly text: string;
  /** It holds an `svg`, `img` or icon element. */
  readonly hasIcon: boolean;
}

export interface ScreenshotCheckOptions {
  /** Limit the check to this part of the page (a selector; default the whole body). */
  readonly within?: string;
  /** Elements whose text is code on purpose (default `code, pre, kbd, samp, [data-raw-ok]`); their text is not read. */
  readonly ignore?: string;
  /** Texts that are fine here, e.g. a product name written in camelCase: exact strings or patterns. */
  readonly allow?: readonly (string | RegExp)[];
  /** Skip the clipping check (a page that is meant to be cut, e.g. a clipped-box demo). */
  readonly skipClipping?: boolean;
}

export const DEFAULT_IGNORE = 'code, pre, kbd, samp, [data-raw-ok]';
export const STATUS_CHIPS = 'pd-status-pill, .pd-pill, [data-status-chip]';

interface Rule {
  readonly kind: string;
  readonly pattern: RegExp;
}

/**
 * Raw identifiers and placeholders a user should never read. Patterns see one visible text at a time; a camelCase token counts only
 * when it is the whole text or follows a `Label:` (a prose sentence may name `currentColor`).
 */
export const RAW_TEXT_RULES: readonly Rule[] = [
  { kind: 'raw identifier (SNAKE_CASE)', pattern: /\b[A-Z][A-Z0-9]*(?:_[A-Z0-9]+)+\b/ },
  { kind: 'raw identifier (snake_case)', pattern: /^[a-z][a-z0-9]*(?:_[a-z0-9]+)+$/ },
  { kind: 'raw identifier (camelCase)', pattern: /^(?:[^\n]{0,40}:\s*)?[a-z]{2,}(?:[A-Z][a-z0-9]+)+$/ },
  { kind: 'undefined, null or NaN', pattern: /\b(?:undefined|null|NaN)\b/ },
  { kind: '[object Object]', pattern: /\[object \w+\]/ },
  { kind: 'mock text', pattern: /\bmock(?:ed|s)?\b/i },
  { kind: 'lorem ipsum', pattern: /\blorem\b|\bipsum\b/i },
  { kind: 'TODO marker', pattern: /\b(?:TODO|FIXME|TBD|XXX)\b/ },
  { kind: 'debug text', pattern: /\[debug\]|\bDEBUG\b|\bdebug:/ },
];

const allowed = (text: string, allow: readonly (string | RegExp)[]): boolean => allow.some((a) => (typeof a === 'string' ? text.includes(a) : a.test(text)));

/** The failure messages for visible texts that read as raw identifiers, placeholders or debug strings. */
export function rawTextProblems(texts: readonly VisibleText[], allow: readonly (string | RegExp)[] = []): string[] {
  const out: string[] = [];
  for (const t of texts) {
    if (allowed(t.text, allow)) continue;
    for (const rule of RAW_TEXT_RULES) if (rule.pattern.test(t.text)) out.push(`${rule.kind} shown: "${t.text.slice(0, 60)}" in ${t.where}`);
  }
  return [...new Set(out)];
}

/** The failure messages for status chips that have neither a word nor an icon. */
export function statusChipProblems(chips: readonly ChipProbe[]): string[] {
  return chips.filter((c) => c.text === '' && !c.hasIcon).map((c) => `status chip without a word or icon (colour alone): ${c.description}`);
}

/** Reads the visible texts and status chips of the page (facts only, see the file comment). */
export async function readScreenshotFacts(page: Page, options: ScreenshotCheckOptions = {}): Promise<{ texts: VisibleText[]; chips: ChipProbe[] }> {
  return page.evaluate(
    ({ within, ignore, chips }) => {
      const root = (within ? document.querySelector(within) : document.body) ?? document.body;
      const texts: { text: string; where: string }[] = [];
      const hidden = (el: Element): boolean => {
        for (let e: Element | null = el; e; e = e.parentElement) {
          const s = getComputedStyle(e);
          if (s.display === 'none' || s.visibility === 'hidden' || e.hasAttribute('hidden')) return true;
          if (['SCRIPT', 'STYLE', 'TEMPLATE', 'NOSCRIPT'].includes(e.tagName)) return true;
          if (e.matches(ignore)) return true;
        }
        return false;
      };
      const describe = (el: Element) => `${el.tagName.toLowerCase()}${el.className && typeof el.className === 'string' ? `.${el.className.trim().split(/\s+/).slice(0, 3).join('.')}` : ''}`;
      const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
      for (let n = walker.nextNode(); n; n = walker.nextNode()) {
        const text = (n.textContent ?? '').replace(/\s+/g, ' ').trim();
        const parent = n.parentElement;
        if (text && parent && !hidden(parent)) texts.push({ text, where: describe(parent) });
      }
      for (const el of Array.from(root.querySelectorAll<HTMLInputElement>('input, textarea'))) {
        const v = (el.value || el.placeholder || '').trim();
        if (v && !hidden(el)) texts.push({ text: v, where: describe(el) });
      }
      const probes = Array.from(root.querySelectorAll(chips))
        .filter((c) => !hidden(c))
        .map((c) => ({
          description: `${describe(c)} in ${c.parentElement ? describe(c.parentElement) : 'body'}`,
          text: (c.textContent ?? '').replace(/\s+/g, ' ').trim(),
          hasIcon: c.querySelector('svg, img, mat-icon, .pd-icon') !== null,
        }));
      return { texts, chips: probes };
    },
    { within: options.within ?? '', ignore: options.ignore ?? DEFAULT_IGNORE, chips: STATUS_CHIPS },
  );
}

/** Every problem a screenshot of the page's current state would record (empty: none). */
export async function screenshotProblems(page: Page, options: ScreenshotCheckOptions = {}): Promise<string[]> {
  const facts = await readScreenshotFacts(page, options);
  const clips = options.skipClipping ? [] : [...(await textClippedWithoutEllipsis(page)), ...(await textCutOffWithoutTooltip(page))];
  return [...clips, ...rawTextProblems(facts.texts, options.allow), ...statusChipProblems(facts.chips)];
}

/** Fails the test, listing every problem, when the page is not fit to be photographed. Call it right before and after the screenshot. */
export async function expectScreenshotClean(page: Page, label = '', options: ScreenshotCheckOptions = {}): Promise<void> {
  expect(await screenshotProblems(page, options), `screenshot problems${label ? ` (${label})` : ''}`).toEqual([]);
}

/** {@link expectScreenshotClean}, then the settled screenshot of {@link captureStable}, then the check again (the frame may have moved). */
export async function captureChecked(page: Page, path: string, options: StableFrameOptions & ScreenshotCheckOptions & { readonly fullPage?: boolean } = {}): Promise<void> {
  await expectScreenshotClean(page, `before ${path}`, options);
  await captureStable(page, path, options);
  await expectScreenshotClean(page, `after ${path}`, options);
}
