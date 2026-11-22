import AxeBuilder from '@axe-core/playwright';
import type { Page } from '@playwright/test';

/** The WCAG rules every Paddock app is held to (2.2 AA). */
export const WCAG_TAGS: readonly string[] = ['wcag2a', 'wcag2aa', 'wcag22aa'];

/** Both themes, for `for (const colorScheme of THEMES)` loops (every screen is checked in each). */
export const THEMES = ['dark', 'light'] as const;
export type Theme = (typeof THEMES)[number];

export interface AxeOptions {
  /** Only these parts of the page (CSS selectors). */
  readonly include?: readonly string[];
  readonly exclude?: readonly string[];
  /** Report rule ids only, without the nodes. */
  readonly idsOnly?: boolean;
}

/**
 * Brings the page's finite animations and CSS transitions to their end state at once: axe reads the colours at the moment it
 * looks, and a theme switch is a colour transition (axe saw the old scheme's text colour on the new background). They are
 * finished, not waited for: a transition that restarts on every tick (a countdown bar's width) never ends by itself, and
 * waiting for it cost axe 1-23 s per theme. Endless animations (a spinner) are left running.
 */
export async function settleTransitions(page: Page): Promise<void> {
  await page.evaluate(() => {
    const finite = () => document.getAnimations().filter((a) => a.effect?.getComputedTiming().endTime !== Infinity && a.playState !== 'finished');
    for (let round = 0; round < 5 && finite().length; round++) for (const a of finite()) a.finish();
  });
}

/** Switches the page's colour scheme and waits until it is live (ER-33: scanning before that measures the old scheme). */
async function switchScheme(page: Page, scheme: Theme): Promise<void> {
  await page.emulateMedia({ colorScheme: scheme });
  // The page's matchMedia reports the scheme, then two animation frames pass, so the rendering update that applies it to the
  // styles (WebKit applies an emulated scheme there, not when `emulateMedia` returns) has run.
  await page.waitForFunction((s) => matchMedia(`(prefers-color-scheme: ${s})`).matches, scheme);
  await nextFrames(page);
}

/** Two animation frames of the page: a style change made before has been through a rendering update. */
async function nextFrames(page: Page): Promise<void> {
  await page.evaluate(() => new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
}

/**
 * The style that holds the page still while axe reads it, on every element and pseudo-element: no transitions, and every
 * finite animation jumped to its end (zero duration, started a moment ago), its fill kept. So an overlay that fades in with
 * `fill-mode: forwards` from opacity 0 (Material's tooltip, select panel and snack bar) is read open, not invisible. An
 * endless animation (a spinner) ends too, at its base style. Adopted by the document (a constructed stylesheet, which a CSP
 * without 'unsafe-inline' still allows) and removed again after the scan.
 */
const MOTION_OFF = '*, *::before, *::after { transition: none !important; animation-duration: 0s !important; animation-delay: -1ms !important; }';

/** Adopts the still style; returns whether the page already prefers reduced motion. */
async function addStillStyle(page: Page): Promise<boolean> {
  return page.evaluate((css) => {
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(css);
    (window as unknown as { __pdAxeStill?: CSSStyleSheet }).__pdAxeStill = sheet;
    document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
    return matchMedia('(prefers-reduced-motion: reduce)').matches;
  }, MOTION_OFF);
}

async function removeStillStyle(page: Page): Promise<void> {
  await page.evaluate(() => {
    const w = window as unknown as { __pdAxeStill?: CSSStyleSheet };
    document.adoptedStyleSheets = document.adoptedStyleSheets.filter((s) => s !== w.__pdAxeStill);
    delete w.__pdAxeStill;
  });
}

/**
 * Emulates `prefers-reduced-motion` and waits until the page sees it (WebKit applies an emulated media feature at its next
 * rendering update). Off is 'no-preference', not null: Linux WebKit (CI image) kept 'reduce' after a reset to null, every
 * time (macOS WebKit did not).
 */
async function reducedMotion(page: Page, on: boolean): Promise<void> {
  await page.emulateMedia({ reducedMotion: on ? 'reduce' : 'no-preference' });
  await page.waitForFunction((w) => matchMedia('(prefers-reduced-motion: reduce)').matches === w, on);
}

/**
 * Runs `scan` with the page held still, and returns its result.
 *
 * Colours read while they change are not a measurement (Pitwall run 8134 read a row half-way to the dark theme; Debrief's
 * runs 8297 to 8556 read the Inspector half-way through its slide-in; earlier a tooltip fading in). Common practice is to hold
 * the page still for the scan: the style above (no transitions, finite animations at their end) and
 * `prefers-reduced-motion: reduce`, two animation frames, the scan, and both undone, the media setting first. An error of the
 * undoing never replaces the scan's own; without one, it is thrown. Every axe scan of the family goes through here.
 */
export async function withPageStill<T>(page: Page, scan: () => Promise<T>): Promise<T> {
  let reducedBefore: boolean | undefined;
  let scanFailed = false;
  try {
    reducedBefore = await addStillStyle(page);
    if (!reducedBefore) await reducedMotion(page, true);
    await nextFrames(page);
    return await scan();
  } catch (e) {
    scanFailed = true;
    throw e;
  } finally {
    const undoErrors: unknown[] = [];
    if (reducedBefore === false) await reducedMotion(page, false).catch((e: unknown) => undoErrors.push(e));
    await removeStillStyle(page).catch((e: unknown) => undoErrors.push(e));
    // eslint-disable-next-line no-unsafe-finally -- a failed undo must not pass silently when the scan itself succeeded.
    if (!scanFailed && undoErrors.length) throw undoErrors[0];
  }
}

/** The page's axe violations as `rule: target, target` lines (empty: none), against `WCAG_TAGS`, scanned with the page held still. */
export async function axeViolations(page: Page, options: AxeOptions = {}): Promise<string[]> {
  // A click leaves the pointer on its target, and a hover tooltip (Material's opens after 500 ms) would appear while axe
  // scans. Moving the pointer away cancels the pending tooltip and closes an open one.
  await page.mouse.move(0, 0);
  let builder = new AxeBuilder({ page }).withTags([...WCAG_TAGS]);
  for (const s of options.include ?? []) builder = builder.include(s);
  for (const s of options.exclude ?? []) builder = builder.exclude(s);
  const results = await withPageStill(page, () => builder.analyze());
  return results.violations.map((v) => (options.idsOnly ? v.id : `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`));
}

/**
 * Calls `onScan` in the page at the start of each of axe's scans (its `runPartial`), with the scan's number, and counts them in
 * `window.axeScans`. For tests of what a scan sees: motion started from here begins inside the scan, so its order against the
 * helper's preparations is certain. Installed through the debugging protocol, so a CSP without 'unsafe-eval' does not stop it
 * (the Tauri apps' CSP); `onScan` is page code and closes over nothing of the test.
 */
export async function onEveryAxeScan(page: Page, onScan: (n: number) => void): Promise<void> {
  await page.evaluate(`window.__pdAxeOnScan = (${onScan.toString()});`);
  await page.evaluate(() => {
    type Run = (...a: unknown[]) => Promise<unknown>;
    const w = window as unknown as { axeScans: number; __pdAxeOnScan: (n: number) => void };
    w.axeScans = 0;
    let real: object | undefined;
    // axe-core assigns window.axe first and its runPartial later: wrap runPartial whenever it is assigned.
    Object.defineProperty(window, 'axe', {
      configurable: true,
      get: () => real,
      set: (axe: object) => {
        let run: Run | undefined;
        const wrapped: Run = async (...args) => {
          w.__pdAxeOnScan(++w.axeScans);
          return (run as Run)(...args);
        };
        Object.defineProperty(axe, 'runPartial', { configurable: true, get: () => (run ? wrapped : undefined), set: (f: Run) => (run = f) });
        real = axe;
      },
    });
  });
}

/** `axeViolations` in each theme (the page is switched with `emulateMedia`), as `theme: rule: targets` lines. */
export async function axeInThemes(page: Page, options: AxeOptions & { readonly themes?: readonly Theme[] } = {}): Promise<string[]> {
  const out: string[] = [];
  for (const theme of options.themes ?? THEMES) {
    await switchScheme(page, theme);
    for (const line of await axeViolations(page, options)) out.push(`${theme}: ${line}`);
  }
  return out;
}

/** One window or screen to check: a URL (relative to the base URL) and what to wait for before axe looks. */
export interface AxeTarget {
  /** A name for the report (a dialog id, a page title). */
  readonly name: string;
  readonly url: string;
  /** Resolves when the screen has finished loading (default: nothing beyond the page load). */
  readonly ready?: (page: Page) => Promise<void>;
}

/**
 * Opens each target in each theme and returns the violations by target name (a target with none is left out), so a spec
 * is `expect(await axeTargets(page, targets)).toEqual({})` and a failure names the window. Targets are visited in order
 * on one page.
 */
export async function axeTargets(page: Page, targets: readonly AxeTarget[], options: AxeOptions & { readonly themes?: readonly Theme[] } = {}): Promise<Record<string, string[]>> {
  const failures: Record<string, string[]> = {};
  for (const theme of options.themes ?? THEMES) {
    await switchScheme(page, theme);
    for (const target of targets) {
      // A fresh document each time: a change of the hash alone keeps the previous window (and its theme transition) alive.
      await page.goto('about:blank');
      await page.goto(target.url);
      await target.ready?.(page);
      const found = await axeViolations(page, options);
      if (found.length) failures[`${target.name} (${theme})`] = found;
    }
  }
  return failures;
}
