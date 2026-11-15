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
 * The style that holds the page still while axe reads it: no transition and no animation on any element or pseudo-element,
 * so every colour is its end state at once. Adopted by the document (a constructed stylesheet, which a CSP without
 * 'unsafe-inline' still allows) and removed again after the scan.
 */
const MOTION_OFF = '*, *::before, *::after { transition: none !important; animation: none !important; }';

/** Turns motion off for a scan (the style above plus `prefers-reduced-motion: reduce`); returns how to turn it on again. */
async function motionOff(page: Page): Promise<() => Promise<void>> {
  const reducedAlready = await page.evaluate((css) => {
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(css);
    (window as unknown as { __pdAxeStill?: CSSStyleSheet }).__pdAxeStill = sheet;
    document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
    return matchMedia('(prefers-reduced-motion: reduce)').matches;
  }, MOTION_OFF);
  // WebKit applies an emulated media feature at its next rendering update: wait until the page sees it, both ways.
  const reduced = (want: boolean) => page.waitForFunction((w) => matchMedia('(prefers-reduced-motion: reduce)').matches === w, want);
  if (!reducedAlready) {
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await reduced(true);
  }
  await nextFrames(page);
  return async () => {
    await page.evaluate(() => {
      const w = window as unknown as { __pdAxeStill?: CSSStyleSheet };
      document.adoptedStyleSheets = document.adoptedStyleSheets.filter((s) => s !== w.__pdAxeStill);
      delete w.__pdAxeStill;
    });
    if (!reducedAlready) {
      // 'no-preference', not null: Linux WebKit (CI image) kept 'reduce' after a reset to null, every time (macOS WebKit did not).
      await page.emulateMedia({ reducedMotion: 'no-preference' });
      await reduced(false);
    }
  };
}

/**
 * The page's axe violations as `rule: target, target` lines (empty: none), against `WCAG_TAGS`.
 *
 * Colours read while they change are not a measurement (Pitwall run 8134 read a row half-way to the dark theme; earlier a
 * tooltip fading in). Common practice is to hold the page still for the scan: transitions and animations are switched off
 * on every element and pseudo-element and `prefers-reduced-motion` is emulated, two animation frames pass, axe scans once,
 * and both are undone. An animation's end state is not applied by this: what axe reads is the element's own style.
 */
export async function axeViolations(page: Page, options: AxeOptions = {}): Promise<string[]> {
  // A click leaves the pointer on its target, and a hover tooltip (Material's opens after 500 ms) would appear while axe
  // scans. Moving the pointer away cancels the pending tooltip and closes an open one.
  await page.mouse.move(0, 0);
  let builder = new AxeBuilder({ page }).withTags([...WCAG_TAGS]);
  for (const s of options.include ?? []) builder = builder.include(s);
  for (const s of options.exclude ?? []) builder = builder.exclude(s);
  const motionOn = await motionOff(page);
  try {
    const results = await builder.analyze();
    return results.violations.map((v) => (options.idsOnly ? v.id : `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`));
  } finally {
    await motionOn();
  }
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
